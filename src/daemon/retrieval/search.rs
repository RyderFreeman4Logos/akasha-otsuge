use super::*;
use crate::domain::{Coverage, MemoryState, RecallDraft, RecallStatus, RetrievalMode, Truncation};

impl Service {
    pub(in crate::daemon::jobs) async fn search(
        &self,
        id: &str,
        workflow: &Deadline,
        index: Arc<Index>,
        config: Arc<crate::config::Snapshot<()>>,
        vector_generation: Option<u64>,
        plan: RetrievalRequest,
    ) -> Result<Value> {
        let RetrievalRequest::Search { query, scope } = plan else {
            return Err(Error("JOB_CORRUPT"));
        };
        let domain = Domain::new(&self.storage);
        let (context, consult) = {
            let _guard = self.admission.lock().await;
            let (_, job) = self.job(id)?;
            self.authorize_job(&job)?;
            (self.job_read_context(&job)?, job.consult_plan.is_some())
        };
        domain.permission(&context, Some(&scope))?;
        let index_epoch = self.index_epoch.load(Ordering::Acquire);
        let read_seq = self.storage.sequence()?;
        let mut progress = Index::status(&self.storage, config.resolved())?;
        if progress.lexical_watermark == 0 {
            return Err(Error("INDEX_NOT_READY"));
        }
        let lexical = index
            .keyword(
                &query,
                &scope,
                config.resolved().config().retrieval.max_candidates,
            )
            .await?;
        let mut model_calls = Vec::new();
        let mut gaps = Vec::new();
        let mut mode = "keyword";
        let mut ranked = std::collections::BTreeMap::new();
        for (rank, key) in lexical.into_iter().enumerate() {
            ranked.insert(key, 1.0 / (60.0 + rank as f64));
        }
        let vector_config = match vector_generation {
            Some(generation) if generation != config.generation() => {
                self.retrieval_snapshot(generation)?
            }
            _ => config.clone(),
        };
        let pinned_vector = Index::status(&self.storage, vector_config.resolved())?.vector;
        if progress.vector.as_ref().map(|v| &v.generation)
            != pinned_vector.as_ref().map(|v| &v.generation)
        {
            gaps.push("generation_pending".into());
        }
        progress.vector = pinned_vector;
        if let Some(vector) = &progress.vector {
            if vector.watermark == 0 {
                gaps.push("vector_generation_pending".to_owned());
            } else {
                let outcome = self
                    .providers
                    .call_guarded(
                        vector_config.resolved(),
                        &crate::provider::Call {
                            profile: vector_config
                                .resolved()
                                .config()
                                .retrieval
                                .embedding
                                .clone()
                                .unwrap(),
                            input: crate::provider::Input::Embedding {
                                texts: vec![query.clone()],
                            },
                        },
                        workflow,
                        self.model_cancelled(id),
                        self.authorize_model(id),
                    )
                    .await;
                model_calls.push(json!({"profile":outcome.profile,"identity":outcome.identity,"error":outcome.error,"trace":outcome.trace}));
                if let Some(code) = outcome.error {
                    if matches!(
                        code.as_str(),
                        "CANCELLED" | "WORKFLOW_TIMEOUT" | "PROVIDER_INTERRUPTED"
                    ) {
                        return Err(Error(crate::retrieval::model_error(&code)));
                    }
                    gaps.push("embedding_unavailable".to_owned());
                } else {
                    let output = outcome.output.ok_or(Error("PROVIDER_OUTPUT_INVALID"))?;
                    let query_vector: Vec<f32> = output["vectors"][0]
                        .as_array()
                        .ok_or(Error("PROVIDER_OUTPUT_INVALID"))?
                        .iter()
                        .map(|v| {
                            v.as_f64()
                                .map(|v| v as f32)
                                .ok_or(Error("PROVIDER_OUTPUT_INVALID"))
                        })
                        .collect::<Result<_>>()?;
                    if query_vector.len() != vector.dimension as usize
                        || query_vector.iter().any(|v| !v.is_finite())
                        || !query_vector.iter().any(|v| *v != 0.0)
                    {
                        return Err(Error("PROVIDER_OUTPUT_INVALID"));
                    }
                    let semantic = index
                        .vector_index(&self.storage, vector_config.resolved())
                        .await?
                        .search(
                            &query_vector,
                            &scope,
                            config.resolved().config().retrieval.max_candidates,
                        )
                        .await?;
                    for (rank, key) in semantic.into_iter().enumerate() {
                        *ranked.entry(key).or_default() += 1.0 / (60.0 + rank as f64);
                    }
                    mode = "hybrid";
                }
            }
        }
        let mut ranked: Vec<_> = ranked.into_iter().collect();
        ranked.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        let candidates = ranked
            .into_iter()
            .take(config.resolved().config().retrieval.max_candidates)
            .map(|(key, _)| key);
        let mut items = Vec::new();
        let now = i64::try_from(ipc::now_ms()?).map_err(|_| Error("CLOCK_INVALID"))?;
        let (mut rules, rules_complete) = domain
            .applicable_rules(
                &context,
                &scope,
                read_seq,
                now,
                config.resolved().config().retrieval.max_candidates,
            )
            .await?;
        let mut selected: Vec<_> = rules.iter().map(|r| r.memory.revision.clone()).collect();
        for (memory_id, revision) in candidates {
            if let MemoryState::Active(timed) = domain.get(&context, &memory_id, None).await?.data
                && timed.memory.revision.commit_seq == revision
                && revision <= read_seq
                && scope.contains(&timed.memory.record.draft.scope)
                && timed.memory.record.draft.valid_at(now)
            {
                if selected.contains(&timed.memory.revision) {
                    continue;
                }
                selected.push(timed.memory.revision.clone());
                items.push(timed);
            }
        }
        if let Some(profile) = &config.resolved().config().retrieval.reranker {
            let mut scores = Vec::new();
            let mut offset = 0;
            while offset < items.len() {
                let documents: Vec<_> = items[offset..items.len().min(offset + 8)]
                    .iter()
                    .map(|v| {
                        let claim = &v.memory.record.draft.claim;
                        let mut end = claim.len().min(4096);
                        while !claim.is_char_boundary(end) {
                            end -= 1;
                        }
                        if end < claim.len()
                            && !gaps.iter().any(|g| g == "reranker_input_truncated")
                        {
                            gaps.push("reranker_input_truncated".into());
                        }
                        claim[..end].to_owned()
                    })
                    .collect();
                let mut call = crate::provider::Call {
                    profile: profile.clone(),
                    input: crate::provider::Input::Reranker {
                        query: query.clone(),
                        documents,
                    },
                };
                while call.request(config.resolved()).is_err() {
                    let crate::provider::Input::Reranker { documents, .. } = &mut call.input else {
                        unreachable!()
                    };
                    if documents.len() > 1 {
                        documents.pop();
                        continue;
                    }
                    let doc = &mut documents[0];
                    if doc.len() <= 1 {
                        break;
                    }
                    let mut end = doc.len() / 2;
                    while !doc.is_char_boundary(end) {
                        end -= 1;
                    }
                    if end == 0 {
                        break;
                    }
                    doc.truncate(end);
                    if !gaps.iter().any(|g| g == "reranker_input_truncated") {
                        gaps.push("reranker_input_truncated".into());
                    }
                }
                let crate::provider::Input::Reranker { documents, .. } = &call.input else {
                    unreachable!()
                };
                let batch_size = documents.len();
                let outcome = self
                    .providers
                    .call_guarded(
                        config.resolved(),
                        &call,
                        workflow,
                        self.model_cancelled(id),
                        self.authorize_model(id),
                    )
                    .await;
                model_calls.push(json!({"profile":outcome.profile,"identity":outcome.identity,"error":outcome.error,"trace":outcome.trace}));
                if let Some(code) = outcome.error {
                    if matches!(
                        code.as_str(),
                        "CANCELLED" | "WORKFLOW_TIMEOUT" | "PROVIDER_INTERRUPTED"
                    ) {
                        return Err(Error(crate::retrieval::model_error(&code)));
                    }
                    gaps.push("reranker_unavailable".to_owned());
                    scores.clear();
                    break;
                }
                let output = outcome.output.ok_or(Error("PROVIDER_OUTPUT_INVALID"))?;
                for score in output["results"]
                    .as_array()
                    .ok_or(Error("PROVIDER_OUTPUT_INVALID"))?
                {
                    scores.push((
                        score["relevance_score"]
                            .as_f64()
                            .ok_or(Error("PROVIDER_OUTPUT_INVALID"))?,
                        offset
                            + score["index"]
                                .as_u64()
                                .ok_or(Error("PROVIDER_OUTPUT_INVALID"))?
                                as usize,
                    ));
                }
                offset += batch_size;
            }
            if !scores.is_empty() {
                scores.sort_by(|a, b| b.0.total_cmp(&a.0).then(a.1.cmp(&b.1)));
                items = scores.into_iter().map(|(_, i)| items[i].clone()).collect();
                mode = if mode == "hybrid" {
                    "hybrid_reranked"
                } else {
                    "keyword_reranked"
                };
            }
        }
        let top_k = config.resolved().config().retrieval.top_k;
        items.truncate(top_k.saturating_sub(rules.len()));
        if !rules_complete {
            gaps.push("rule_scan_budget".to_owned());
        }
        // A bounded tail check can prove that later job/receipt metadata did not change memories.
        let watermark = progress
            .vector
            .as_ref()
            .map_or(progress.lexical_watermark, |v| {
                v.watermark.min(progress.lexical_watermark)
            });
        if read_seq.saturating_sub(watermark) > 256 {
            gaps.push("index_lag".to_owned());
        } else {
            for seq in watermark + 1..=read_seq {
                if self
                    .storage
                    .outbox(seq)?
                    .ok_or(Error("OUTBOX_GAP"))?
                    .revisions
                    .iter()
                    .any(|r| r.key.kind == RecordKind::Memory)
                {
                    gaps.push("index_lag".to_owned());
                    break;
                }
            }
        }
        if index_epoch % 2 == 1 || self.index_epoch.load(Ordering::Acquire) != index_epoch {
            gaps.push("index_building".into());
        }
        let index_metadata = match domain.permission(&context, Some(&owner_scope())) {
            Ok(_) => Some(&progress),
            Err(e) if e.code == "READ_DENIED" => None,
            Err(e) => return Err(e.into()),
        };
        let render = |items: &Vec<crate::domain::TimedMemory>,
                      rules: &Vec<crate::domain::TimedMemory>,
                      gaps: &Vec<String>|
         -> Result<_> {
            let coverage = Coverage {
                complete: gaps.is_empty(),
                gaps: gaps.clone(),
            };
            let status = if !coverage.complete {
                RecallStatus::Incomplete
            } else if items.is_empty() && rules.is_empty() {
                RecallStatus::NoHit
            } else {
                RecallStatus::Found
            };
            Ok((
                serde_json::to_string(
                    &json!({"query":query,"scope":scope,"items":items,"rules":rules,"index":index_metadata,"searched_content":"active_memories","mode":mode,"model_calls":model_calls,"status":status,"coverage":coverage}),
                )?,
                status,
                coverage,
            ))
        };
        let original_bytes = render(&items, &rules, &gaps)?.0.len();
        let mut truncated = false;
        if rules.len() > top_k {
            rules.truncate(top_k);
            gaps.push("rule_budget".into());
            truncated = true;
        }
        let mut remaining = config.resolved().config().retrieval.max_original_chars;
        let mut source_spans = Vec::new();
        let mut source_truncated = false;
        let mut chars_truncated = false;
        let mut accept = |item: &crate::domain::TimedMemory| {
            let draft = &item.memory.record.draft;
            let chars = draft.claim.chars().count()
                + draft.rationale.chars().count()
                + draft
                    .conditions
                    .iter()
                    .chain(&draft.assumptions)
                    .map(|s| s.chars().count())
                    .sum::<usize>();
            let mut candidate_sources = source_spans.clone();
            for span in draft
                .source_spans
                .iter()
                .chain(item.memory.record.approval.iter().map(|a| &a.source))
            {
                if !candidate_sources.contains(span) {
                    candidate_sources.push(span.clone());
                }
            }
            if candidate_sources.len() > 64 {
                source_truncated = true;
                truncated = true;
                false
            } else if chars > remaining {
                chars_truncated = true;
                truncated = true;
                false
            } else {
                source_spans = candidate_sources;
                remaining -= chars;
                true
            }
        };
        rules.retain(&mut accept);
        items.retain(&mut accept);
        if chars_truncated {
            gaps.push("character_budget".into());
        }
        if source_truncated {
            gaps.push("source_budget".into());
        }
        let (output, status, coverage) = loop {
            let value = render(&items, &rules, &gaps)?;
            if value.0.len() <= 128 * 1024 {
                break value;
            }
            if items.pop().or_else(|| rules.pop()).is_none() {
                return Err(Error("RESULT_TOO_LARGE"));
            }
            truncated = true;
            if !gaps.iter().any(|g| g == "output_bytes") {
                gaps.push("output_bytes".into());
            }
        };
        selected = rules
            .iter()
            .chain(&items)
            .map(|r| r.memory.revision.clone())
            .collect();
        let truncation = truncated.then(|| Truncation {
            original_bytes: original_bytes as u64,
            reason: "retrieval_budget".into(),
        });
        let receipt_id = if consult {
            format!(
                "consult-search/{}",
                hash(&serde_json::to_vec(&(id, &query, read_seq))?)
            )
        } else {
            format!("search/{}", hash(id.as_bytes()))
        };
        drop(index);
        let _guard = self.admission.lock().await;
        let (_, job) = self.job(id)?;
        self.authorize_job(&job)?;
        if index_metadata.is_some() {
            domain.permission(&context, Some(&owner_scope()))?;
        }
        if job.cancel_requested {
            return Err(Error("CANCELLED"));
        }
        if workflow.expired().is_some() {
            return Err(Error("WORKFLOW_TIMEOUT"));
        }
        domain
            .prepare_recall(
                &context,
                &RecallDraft {
                    request_id: receipt_id.clone(),
                    output,
                    scope: if index_metadata.is_some() {
                        owner_scope()
                    } else {
                        scope
                    },
                    read_seq,
                    retrieval_mode: RetrievalMode::CurrentState,
                    selected,
                    sources: vec![],
                    status,
                    coverage,
                    truncation,
                    config_generation: config.generation(),
                    index_generation: Some(format!(
                        "{}+{}",
                        progress.generation,
                        progress
                            .vector
                            .as_ref()
                            .map(|v| v.generation.as_str())
                            .unwrap_or("none")
                    )),
                },
            )
            .await?;
        Ok(json!({"receipt_id":receipt_id}))
    }
}
