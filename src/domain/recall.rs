use super::*;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecallStatus {
    Found,
    NoHit,
    Incomplete,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Coverage {
    pub complete: bool,
    pub gaps: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Truncation {
    pub original_bytes: u64,
    pub reason: String,
}

/// Actual serialized response bytes and server-collected retrieval context.
#[derive(Clone, Debug, Serialize)]
pub struct RecallDraft {
    pub request_id: String,
    pub output: String,
    pub scope: Scope,
    pub read_seq: u64,
    pub retrieval_mode: RetrievalMode,
    pub selected: Vec<RevisionRef>,
    pub sources: Vec<SourceSpan>,
    pub status: RecallStatus,
    pub coverage: Coverage,
    pub truncation: Option<Truncation>,
    pub config_generation: u64,
    pub index_generation: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
pub enum DeliveryPhase {
    Prepared,
    Delivered { request_id: String },
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecallRecord {
    pub schema_version: u32,
    pub receipt_id: String,
    pub request_sha256: String,
    pub principal: String,
    pub binding: String,
    pub run_id: String,
    pub prepared_seq: u64,
    pub read_seq: u64,
    pub validated_at_seq: u64,
    pub permission_revision: u64,
    pub retrieval_mode: RetrievalMode,
    pub scope: Scope,
    pub selected: Vec<RevisionRef>,
    pub sources: Vec<SourceSpan>,
    pub status: RecallStatus,
    pub coverage: Coverage,
    pub truncation: Option<Truncation>,
    pub config_generation: u64,
    pub index_generation: Option<String>,
    pub storage_format: u8,
    pub storage_engine: String,
    pub output_sha256: String,
    pub output_bytes: usize,
    pub delivery: DeliveryPhase,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RecallReceipt {
    pub record: RecallRecord,
    pub output: String,
    pub revision: u64,
    pub recorded_at_ms: u64,
}

const ENGINE: &str = "surrealkv-1.0.0@4338a790c8edc56ef5ea1e46157427af4815be3c";

fn recall_key(receipt: &str) -> RecordKey {
    RecordKey {
        kind: RecordKind::State,
        id: format!("recall/{}", hash(receipt.as_bytes())),
    }
}

fn body_key(receipt: &str) -> RecordKey {
    RecordKey {
        kind: RecordKind::Evidence,
        id: format!("recall-body/{}", hash(receipt.as_bytes())),
    }
}

impl RecallDraft {
    fn validate(&self) -> Result<()> {
        self.scope.validate()?;
        match self.retrieval_mode {
            RetrievalMode::HistoricalSemantic => {
                return Err(error("HISTORICAL_SEMANTIC_UNSUPPORTED"));
            }
            RetrievalMode::ReceiptReplay => return Err(error("INVALID_RECALL_MODE")),
            _ => {}
        }
        let mut selected = std::collections::BTreeSet::new();
        if !id(&self.request_id)
            || self.output.len() > 128 * 1024
            || self.selected.len() > 32
            || self.sources.len() > 64
            || self.sources.iter().any(|s| {
                !id(&s.event_id)
                    || s.revision == 0
                    || s.start >= s.end
                    || s.content_sha256.len() != 64
                    || !s.content_sha256.bytes().all(|b| b.is_ascii_hexdigit())
            })
            || self.config_generation == 0
            || self.index_generation.as_ref().is_some_and(|v| !id(v))
            || self.coverage.gaps.len() > 32
            || self.coverage.gaps.iter().any(|v| !id(v))
            || self.selected.iter().any(|r| {
                r.key.kind != RecordKind::Memory
                    || !id(&r.key.id)
                    || r.commit_seq == 0
                    || !selected.insert((&r.key, r.commit_seq))
            })
            || self
                .truncation
                .as_ref()
                .is_some_and(|v| v.original_bytes <= self.output.len() as u64 || !id(&v.reason))
            || self.coverage.complete
                != (self.coverage.gaps.is_empty() && self.truncation.is_none())
            || (self.status == RecallStatus::Incomplete) == self.coverage.complete
            || (self.status == RecallStatus::NoHit
                && (!self.selected.is_empty() || !self.sources.is_empty()))
            || (self.status == RecallStatus::Found
                && self.selected.is_empty()
                && self.sources.is_empty())
        {
            return Err(error("INVALID_RECALL"));
        }
        Ok(())
    }
}

impl Domain<'_> {
    fn recall_selection(
        &self,
        context: &ReadContext,
        draft: &RecallDraft,
        validated: u64,
    ) -> Result<Vec<SourceSpan>> {
        self.storage.check_point(draft.read_seq)?;
        let mut sources = Vec::new();
        let now = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| error("CLOCK_INVALID"))?
                .as_millis(),
        )
        .map_err(|_| error("CLOCK_INVALID"))?;
        for reference in &draft.selected {
            let memory = self
                .revision(&reference.key.id, reference.commit_seq)?
                .ok_or_else(|| error("REVISION_NOT_FOUND"))?;
            let applicable_rule = draft.retrieval_mode == RetrievalMode::CurrentState
                && memory.record.is_rule()
                && memory.record.draft.scope.contains(&draft.scope);
            if !draft.scope.contains(&memory.record.draft.scope) && !applicable_rule {
                return Err(error("SCOPE_DENIED"));
            }
            self.permission(context, Some(&memory.record.draft.scope))?;
            if draft.retrieval_mode == RetrievalMode::CurrentState
                && !memory.record.draft.valid_at(now)
            {
                return Err(error("STALE_RECALL"));
            }
            let at = if draft.retrieval_mode == RetrievalMode::CurrentState {
                validated
            } else {
                draft.read_seq
            };
            if reference.commit_seq > at {
                return Err(error("STALE_RECALL"));
            }
            if draft.retrieval_mode != RetrievalMode::RevisionHistory {
                let active = self.adopted_at(&reference.key.id, at)?;
                if !active.is_some_and(|v| {
                    v.record.lifecycle == Lifecycle::Active && v.revision == *reference
                }) {
                    return Err(error("STALE_RECALL"));
                }
            }
            for span in memory
                .record
                .draft
                .source_spans
                .iter()
                .chain(memory.record.approval.iter().map(|a| &a.source))
            {
                let source = self.span(span)?;
                self.permission(context, Some(&source.payload.scope))?;
                if !sources.contains(span) {
                    sources.push(span.clone());
                }
                if sources.len() > 64 {
                    return Err(error("RESULT_TOO_LARGE"));
                }
            }
        }
        for span in &draft.sources {
            let source = self.span(span)?;
            if !draft.scope.contains(&source.payload.scope) {
                return Err(error("SCOPE_DENIED"));
            }
            self.permission(context, Some(&source.payload.scope))?;
            if span.revision > draft.read_seq && draft.retrieval_mode != RetrievalMode::CurrentState
            {
                return Err(error("STALE_RECALL"));
            }
            if !sources.contains(span) {
                sources.push(span.clone());
            }
        }
        if sources.len() > 64 {
            return Err(error("RESULT_TOO_LARGE"));
        }
        sources.sort_by(|a, b| {
            (&a.event_id, a.revision, a.start, a.end, &a.content_sha256).cmp(&(
                &b.event_id,
                b.revision,
                b.start,
                b.end,
                &b.content_sha256,
            ))
        });
        Ok(sources)
    }

    fn read_recall(&self, context: &ReadContext, receipt: &str) -> Result<RecallReceipt> {
        if !id(receipt) {
            return Err(error("INVALID_RECALL"));
        }
        let revision = self
            .storage
            .current(&recall_key(receipt))?
            .ok_or_else(|| error("RECEIPT_NOT_FOUND"))?;
        let record: RecallRecord = serde_json::from_str(&revision.content)?;
        if record.principal != context.principal
            || record.binding != context.binding
            || record.run_id != context.run_id
        {
            return Err(error("READ_DENIED"));
        }
        if record.schema_version != 1
            || record.receipt_id != receipt
            || record.prepared_seq == 0
            || record.prepared_seq > revision.commit_seq
            || record.validated_at_seq.checked_add(1) != Some(record.prepared_seq)
            || record.read_seq > record.validated_at_seq
            || record.permission_revision == 0
            || record.permission_revision > record.validated_at_seq
            || record.storage_format != crate::storage::FORMAT_VERSION
            || record.storage_engine != ENGINE
            || record.scope.validate().is_err()
        {
            return Err(error("RECALL_CORRUPT"));
        }
        match &record.delivery {
            DeliveryPhase::Prepared
                if revision.commit_seq != record.prepared_seq || revision.previous.is_some() =>
            {
                return Err(error("RECALL_CORRUPT"));
            }
            DeliveryPhase::Delivered { request_id }
                if !id(request_id) || revision.previous != Some(record.prepared_seq) =>
            {
                return Err(error("RECALL_CORRUPT"));
            }
            _ => {}
        }
        if matches!(record.delivery, DeliveryPhase::Delivered { .. }) {
            let original = self
                .storage
                .revision(&recall_key(receipt), record.prepared_seq)?
                .ok_or_else(|| error("RECALL_CORRUPT"))?;
            let prepared: RecallRecord = serde_json::from_str(&original.content)?;
            let mut expected = record.clone();
            expected.delivery = DeliveryPhase::Prepared;
            if prepared != expected || original.previous.is_some() {
                return Err(error("RECALL_CORRUPT"));
            }
        }
        let body = self
            .storage
            .revision(&body_key(receipt), record.prepared_seq)?
            .ok_or_else(|| error("RECALL_CORRUPT"))?;
        if record.output_sha256 != body.content_sha256
            || record.output_bytes != body.content.len()
            || body.previous.is_some()
        {
            return Err(error("RECALL_CORRUPT"));
        }
        Ok(RecallReceipt {
            record,
            output: body.content,
            revision: revision.commit_seq,
            recorded_at_ms: self.storage.recorded_at_ms(revision.commit_seq)?,
        })
    }

    /// Persist these exact response bytes immediately before returning them to the caller.
    pub async fn prepare_recall(
        &self,
        context: &ReadContext,
        draft: &RecallDraft,
    ) -> Result<CommitReceipt> {
        draft.validate()?;
        let fingerprint = hash(&serde_json::to_vec(&(context, draft))?);
        self.storage
            .commit_checked(|| {
                let permission_revision = self.permission(context, Some(&draft.scope))?;
                let validated = self.storage.sequence()?;
                let sources = self.recall_selection(context, draft, validated)?;
                let (record, output) = if self
                    .storage
                    .current(&recall_key(&draft.request_id))?
                    .is_some()
                {
                    let existing = self.read_recall(context, &draft.request_id)?;
                    if existing.record.request_sha256 != fingerprint {
                        return Err(error("IDEMPOTENCY_CONFLICT"));
                    }
                    let original = self
                        .storage
                        .revision(&recall_key(&draft.request_id), existing.record.prepared_seq)?
                        .ok_or_else(|| error("RECALL_CORRUPT"))?;
                    (original.content, existing.output)
                } else {
                    let record = RecallRecord {
                        schema_version: 1,
                        receipt_id: draft.request_id.clone(),
                        request_sha256: fingerprint,
                        principal: context.principal.clone(),
                        binding: context.binding.clone(),
                        run_id: context.run_id.clone(),
                        prepared_seq: validated + 1,
                        read_seq: draft.read_seq,
                        validated_at_seq: validated,
                        permission_revision,
                        retrieval_mode: draft.retrieval_mode,
                        scope: draft.scope.clone(),
                        selected: draft.selected.clone(),
                        sources,
                        status: draft.status,
                        coverage: draft.coverage.clone(),
                        truncation: draft.truncation.clone(),
                        config_generation: draft.config_generation,
                        index_generation: draft.index_generation.clone(),
                        storage_format: crate::storage::FORMAT_VERSION,
                        storage_engine: ENGINE.into(),
                        output_sha256: hash(draft.output.as_bytes()),
                        output_bytes: draft.output.len(),
                        delivery: DeliveryPhase::Prepared,
                    };
                    (serde_json::to_string(&record)?, draft.output.clone())
                };
                Ok(CommitRequest {
                    request_id: draft.request_id.clone(),
                    config_generation: draft.config_generation,
                    policy_revision: None,
                    changes: vec![
                        Change {
                            key: body_key(&draft.request_id),
                            expected_revision: None,
                            content: output,
                        },
                        Change {
                            key: recall_key(&draft.request_id),
                            expected_revision: None,
                            content: record,
                        },
                    ],
                })
            })
            .await
    }

    /// An archive replay, including superseded material; it is never a current recommendation.
    pub async fn recall_receipt(
        &self,
        context: &ReadContext,
        receipt: &str,
    ) -> Result<ReadResult<RecallReceipt>> {
        self.storage
            .serialized_read(|| {
                self.permission(context, None)?;
                let data = self.read_recall(context, receipt)?;
                self.permission(context, Some(&data.record.scope))?;
                for reference in &data.record.selected {
                    let memory = self
                        .revision(&reference.key.id, reference.commit_seq)?
                        .ok_or_else(|| error("REVISION_NOT_FOUND"))?;
                    self.permission(context, Some(&memory.record.draft.scope))?;
                }
                for span in &data.record.sources {
                    self.permission(context, Some(&self.span(span)?.payload.scope))?;
                }
                Ok(ReadResult {
                    read_seq: data.record.read_seq,
                    validated_at_seq: self.storage.sequence()?,
                    retrieval_mode: RetrievalMode::ReceiptReplay,
                    data,
                })
            })
            .await
    }

    /// Call only after the transport acknowledges every byte. This records an observation, not a send.
    pub async fn record_delivery(
        &self,
        context: &ReadContext,
        receipt: &str,
        request: &str,
        output_sha256: &str,
    ) -> Result<CommitReceipt> {
        if !id(request) {
            return Err(error("INVALID_RECALL"));
        }
        self.storage
            .commit_checked(|| {
                let mut existing = self.read_recall(context, receipt)?;
                if existing.record.output_sha256 != output_sha256 {
                    return Err(error("DELIVERY_HASH_MISMATCH"));
                }
                if let DeliveryPhase::Delivered { request_id } = &existing.record.delivery
                    && request_id != request
                {
                    return Err(error("DELIVERY_CONFLICT"));
                }
                existing.record.delivery = DeliveryPhase::Delivered {
                    request_id: request.into(),
                };
                Ok(CommitRequest {
                    request_id: request.into(),
                    config_generation: existing.record.config_generation,
                    policy_revision: None,
                    changes: vec![Change {
                        key: recall_key(receipt),
                        expected_revision: Some(existing.record.prepared_seq),
                        content: serde_json::to_string(&existing.record)?,
                    }],
                })
            })
            .await
    }
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct OriginalRange {
    pub event_id: String,
    pub revision: u64,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Serialize)]
pub struct OriginalRead {
    pub receipt_id: String,
    pub output: String,
    pub output_sha256: String,
    pub reference: SourceSpan,
    pub scope: Scope,
    pub provenance: Provenance,
    pub start: usize,
    pub end: usize,
    pub requested_end: usize,
    pub total_bytes: usize,
    pub truncated: bool,
}

impl Domain<'_> {
    pub async fn original_range(
        &self,
        context: &ReadContext,
        range: &OriginalRange,
        request_id: &str,
        max_chars: usize,
        config_generation: u64,
    ) -> Result<OriginalRead> {
        self.permission(context, None)?;
        let source = self.source(&range.event_id, range.revision)?;
        self.permission(context, Some(&source.payload.scope))?;
        let text = &source.payload.text;
        if max_chars == 0 || range.start >= range.end {
            return Err(error("INVALID_RANGE"));
        }
        let requested = text
            .get(range.start..range.end)
            .ok_or_else(|| error("INVALID_RANGE"))?;
        let length = requested
            .char_indices()
            .nth(max_chars)
            .map_or(requested.len(), |(offset, _)| offset);
        let output = requested[..length].to_owned();
        let end = range.start + length;
        let truncated = end != range.end;
        let reference = SourceSpan {
            event_id: range.event_id.clone(),
            revision: range.revision,
            content_sha256: source.content_sha256,
            start: range.start,
            end,
        };
        self.prepare_recall(
            context,
            &RecallDraft {
                request_id: request_id.into(),
                output: output.clone(),
                scope: source.payload.scope.clone(),
                read_seq: self.storage.sequence()?,
                retrieval_mode: RetrievalMode::CurrentState,
                selected: vec![],
                sources: vec![reference.clone()],
                status: if truncated {
                    RecallStatus::Incomplete
                } else {
                    RecallStatus::Found
                },
                coverage: Coverage {
                    complete: !truncated,
                    gaps: vec![],
                },
                truncation: truncated.then(|| Truncation {
                    original_bytes: requested.len() as u64,
                    reason: "character_budget".into(),
                }),
                config_generation,
                index_generation: None,
            },
        )
        .await?;
        Ok(OriginalRead {
            receipt_id: request_id.into(),
            output_sha256: hash(output.as_bytes()),
            output,
            reference,
            scope: source.payload.scope,
            provenance: source.provenance,
            start: range.start,
            end,
            requested_end: range.end,
            total_bytes: text.len(),
            truncated,
        })
    }
}
