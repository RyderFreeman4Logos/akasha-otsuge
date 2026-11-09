use super::*;
use crate::adapter::Invocation;
use crate::domain::{DeliveryPhase, RecallReceipt};
use crate::outcome::{self, Event, Relation};
use crate::storage::CommitPlan;

fn outcome_key(id: &str) -> Result<RecordKey> {
    if !ipc::identifier(id) {
        return Err(Error("INVALID_OUTCOME"));
    }
    Ok(RecordKey {
        kind: RecordKind::Evidence,
        id: format!("outcome/{}", hash(id.as_bytes())),
    })
}

impl Service {
    fn outcome_binding(&self, id: &str) -> Result<Invocation> {
        let invocation = Invocation::load_verified(&self.storage, id, &principal())?;
        let binding = invocation.diagnostic();
        Domain::new(&self.storage).permission(&invocation.read_context()?, Some(&binding.scope))?;
        if self.allowed_path(&binding.workspace)? != binding.workspace {
            return Err(Error("SCOPE_DENIED"));
        }
        Ok(invocation)
    }

    fn outcome_invocation(&self, event: &Event) -> Result<Invocation> {
        let invocation = self.outcome_binding(&event.invocation_id)?;
        let binding = invocation.diagnostic();
        if let Some(execution) = &event.execution
            && (execution.workspace != binding.workspace
                || self.allowed_path(&execution.workspace)? != binding.workspace)
        {
            return Err(Error("SCOPE_DENIED"));
        }
        Ok(invocation)
    }

    async fn outcome_receipts(
        &self,
        jobs: &[String],
        invocation: &Invocation,
    ) -> Result<Vec<RecallReceipt>> {
        let mut receipts = Vec::new();
        for id in jobs {
            let (_, job) = self.job(id)?;
            let plan = job
                .consult_plan
                .as_ref()
                .ok_or(Error("INVALID_OUTCOME_RECEIPT"))?;
            if self.consult_invocation(plan)?.id() != invocation.id() || job.state != "succeeded" {
                return Err(Error("INVALID_OUTCOME_RECEIPT"));
            }
            self.authorize_job(&job)?;
            let result = job.result.as_ref().ok_or(Error("JOB_CORRUPT"))?;
            self.authorize_consult_result(result)?;
            let id = result["receipt_id"]
                .as_str()
                .ok_or(Error("INVALID_OUTCOME_RECEIPT"))?;
            let receipt = Domain::new(&self.storage)
                .recall_receipt(&self.job_read_context(&job)?, id)
                .await?
                .data;
            if receipt.record.binding != invocation.id() || receipt.record.run_id != job.id {
                return Err(Error("INVALID_OUTCOME_RECEIPT"));
            }
            receipts.push(receipt);
        }
        Ok(receipts)
    }

    fn outcome_claims(
        &self,
        event: &Event,
        invocation: &Invocation,
        receipts: &[RecallReceipt],
    ) -> Result<()> {
        let domain = Domain::new(&self.storage);
        for claim in &event.claims {
            let index = event
                .receipt_jobs
                .iter()
                .position(|id| id == &claim.job_id)
                .ok_or(Error("INVALID_OUTCOME_CLAIM"))?;
            if !receipts[index].record.selected.contains(&claim.memory) {
                return Err(Error("INVALID_OUTCOME_CLAIM"));
            }
            for span in &claim.evidence {
                let source = domain.span(span)?;
                let scope = &invocation.diagnostic().scope;
                if !scope.contains(&source.payload.scope) && !source.payload.scope.contains(scope) {
                    return Err(Error("SCOPE_DENIED"));
                }
                domain.permission(&invocation.read_context()?, Some(&source.payload.scope))?;
            }
        }
        Ok(())
    }

    fn outcome_log_events(
        &self,
        event: &Event,
        invocation: &Invocation,
    ) -> Result<Vec<crate::capture::EventView>> {
        let capture = Capture::new(&self.storage);
        let binding = invocation.capture_binding()?;
        let mut seen = BTreeSet::new();
        event
            .logs
            .iter()
            .map(|log| {
                if !seen.insert(serde_json::to_string(&log.reference)?) {
                    return Err(Error("INVALID_OUTCOME"));
                }
                let view = capture
                    .event(&binding, &log.reference, self.storage.sequence()?)?
                    .ok_or(Error("SOURCE_UNAVAILABLE"))?;
                Domain::new(&self.storage)
                    .permission(&invocation.read_context()?, Some(&view.event.scope))?;
                Ok(view)
            })
            .collect()
    }

    pub(in crate::daemon) async fn submit_outcome(self: &Arc<Self>, event: Event) -> Result<Value> {
        event.validate()?;
        let service = self.clone();
        self.operation(async move {
            let _guard = service.admission.lock().await;
            let invocation = service.outcome_invocation(&event)?;
            let receipts = service
                .outcome_receipts(&event.receipt_jobs, &invocation)
                .await?;
            service.outcome_claims(&event, &invocation, &receipts)?;
            let log_events = service.outcome_log_events(&event, &invocation)?;
            let key = outcome_key(&event.event_id)?;
            if let Some(old) = service.storage.current(&key)? {
                let record: outcome::Record = serde_json::from_str(&old.content)?;
                if record.schema_version != 1 || record.event != event {
                    return Err(Error("IDEMPOTENCY_CONFLICT"));
                }
                return Ok(serde_json::to_value(
                    service
                        .storage
                        .receipt(&key.id)?
                        .ok_or(Error("OUTCOME_CORRUPT"))?,
                )?);
            }
            let mut links = Vec::new();
            for (id, receipt) in event.receipt_jobs.iter().zip(&receipts) {
                for memory in &receipt.record.selected {
                    links.push(outcome::Link {
                        job_id: id.clone(),
                        receipt_id: receipt.record.receipt_id.clone(),
                        memory: memory.clone(),
                        relation: Relation::Attached,
                    });
                    if matches!(receipt.record.delivery, DeliveryPhase::Delivered { .. }) {
                        links.push(outcome::Link {
                            job_id: id.clone(),
                            receipt_id: receipt.record.receipt_id.clone(),
                            memory: memory.clone(),
                            relation: Relation::Returned,
                        });
                    }
                }
            }
            for claim in &event.claims {
                let index = event
                    .receipt_jobs
                    .iter()
                    .position(|id| id == &claim.job_id)
                    .ok_or(Error("INVALID_OUTCOME_CLAIM"))?;
                links.push(outcome::Link {
                    job_id: claim.job_id.clone(),
                    receipt_id: receipts[index].record.receipt_id.clone(),
                    memory: claim.memory.clone(),
                    relation: claim.relation,
                });
            }
            let mut logs = Vec::new();
            for (source, view) in event.logs.iter().zip(&log_events) {
                let raw = Capture::new(&service.storage).raw(view)?;
                let mut preview =
                    String::from_utf8_lossy(&raw[..raw.len().min(outcome::PREVIEW_BYTES)])
                        .into_owned();
                let mut end = preview.len().min(outcome::PREVIEW_BYTES);
                while !preview.is_char_boundary(end) {
                    end -= 1;
                }
                let truncated = raw.len() > outcome::PREVIEW_BYTES || end < preview.len();
                preview.truncate(end);
                logs.push(outcome::Log {
                    source: source.clone(),
                    revision: view.revision,
                    bytes: raw.len(),
                    sha256: view.event.raw_sha256.clone(),
                    preview,
                    preview_truncated: truncated,
                    preview_encoding: "utf8_lossy".into(),
                    coverage: Some(view.event.coverage),
                    completeness: source.completeness.with_coverage(view.event.coverage),
                });
            }
            let record = outcome::Record {
                schema_version: 1,
                run_id: invocation.id().into(),
                caller: invocation.diagnostic().caller.clone(),
                scope: invocation.diagnostic().scope.clone(),
                invocation_revision: invocation.revision(),
                config_generation: service.config.pin().generation(),
                receipts: event
                    .receipt_jobs
                    .iter()
                    .zip(&receipts)
                    .map(|(id, r)| outcome::Receipt {
                        job_id: id.clone(),
                        receipt_id: r.record.receipt_id.clone(),
                        revision: r.revision,
                        output_sha256: r.record.output_sha256.clone(),
                        delivery: r.record.delivery.clone(),
                    })
                    .collect(),
                event,
                links,
                logs,
            };
            let content = serde_json::to_string(&record)?;
            if content.len() > ipc::MAX_MESSAGE / 2 {
                return Err(Error("RESULT_TOO_LARGE"));
            }
            let receipt = service
                .storage
                .commit_or_reuse(|| {
                    Domain::new(&service.storage)
                        .permission(&invocation.read_context()?, Some(&record.scope))?;
                    Ok(CommitPlan::Write(CommitRequest {
                        request_id: key.id.clone(),
                        config_generation: record.config_generation,
                        policy_revision: None,
                        changes: vec![Change {
                            key: key.clone(),
                            expected_revision: None,
                            content: content.clone(),
                        }],
                    }))
                })
                .await?;
            Ok(serde_json::to_value(receipt)?)
        })
        .await
    }

    async fn read_outcome(&self, id: &str) -> Result<outcome::Record> {
        let row = self
            .storage
            .current(&outcome_key(id)?)?
            .ok_or(Error("OUTCOME_NOT_FOUND"))?;
        let mut record: outcome::Record = serde_json::from_str(&row.content)?;
        if record.schema_version != 1
            || record.event.event_id != id
            || record.run_id != record.event.invocation_id
        {
            return Err(Error("OUTCOME_CORRUPT"));
        }
        record.event.validate()?;
        let invocation = self.outcome_invocation(&record.event)?;
        if record.invocation_revision != invocation.revision()
            || record.scope != invocation.diagnostic().scope
            || record.caller != invocation.diagnostic().caller
        {
            return Err(Error("OUTCOME_CORRUPT"));
        }
        let receipts = self
            .outcome_receipts(&record.event.receipt_jobs, &invocation)
            .await?;
        self.outcome_claims(&record.event, &invocation, &receipts)?;
        let events = self.outcome_log_events(&record.event, &invocation)?;
        if events.len() != record.logs.len() {
            return Err(Error("OUTCOME_CORRUPT"));
        }
        for ((source, view), log) in record.event.logs.iter().zip(&events).zip(&mut record.logs) {
            let completeness = source.completeness.with_coverage(view.event.coverage);
            if log.source != *source
                || view.revision != log.revision
                || view.event.raw_sha256 != log.sha256
                || view.event.raw_bytes != log.bytes
                || log
                    .coverage
                    .is_some_and(|c| c != view.event.coverage || log.completeness != completeness)
            {
                return Err(Error("OUTCOME_CORRUPT"));
            }
            // Older records never asserted completeness; preserve their uncertainty on read.
            log.coverage = Some(view.event.coverage);
            log.completeness = completeness;
        }
        Ok(record)
    }

    pub(in crate::daemon) async fn show_outcome(&self, id: &str) -> Result<Value> {
        let _guard = self.admission.lock().await;
        Ok(serde_json::to_value(self.read_outcome(id).await?)?)
    }

    pub(in crate::daemon) async fn outcome_context(&self, id: &str) -> Result<Value> {
        let _guard = self.admission.lock().await;
        let invocation = self.outcome_binding(id)?;
        let job_id = format!("consult/{}", hash(id.as_bytes()));
        let receipt_jobs = match self.job(&job_id) {
            Ok((_, job)) if job.state == "succeeded" => vec![job_id],
            Ok(_) | Err(Error("JOB_NOT_FOUND")) => Vec::new(),
            Err(error) => return Err(error),
        };
        self.outcome_receipts(&receipt_jobs, &invocation).await?;
        Ok(serde_json::to_value(outcome::RunContext {
            run_id: id.into(),
            workspace: invocation.diagnostic().workspace.clone(),
            data_dir: self.config.pin().resolved().config().paths.data_dir.clone(),
            receipt_jobs,
        })?)
    }

    pub(in crate::daemon) async fn write_outcome_log(
        self: &Arc<Self>,
        chunk: outcome::LogChunk,
    ) -> Result<Value> {
        use crate::capture::{CaptureBinding, CoverageLevel, EventDraft, EventKind, Visibility};
        if !ipc::identifier(&chunk.event_id)
            || chunk.part >= 64
            || chunk.observed_at_ms == 0
            || chunk.observed_at_ms > i64::MAX as u64
        {
            return Err(Error("INVALID_OUTCOME"));
        }
        let raw = crate::capture::decode_hex(&chunk.raw_hex, outcome::LOG_CHUNK_BYTES)?;
        let service = self.clone();
        self.operation(async move {
            let _guard = service.admission.lock().await;
            let invocation = service.outcome_binding(&chunk.invocation_id)?;
            let mut source = invocation.diagnostic().caller.clone();
            source.adapter = "outcome-wrapper".into();
            source.thread =
                hash(serde_json::to_string(&(&chunk.invocation_id, &chunk.event_id))?.as_bytes());
            let scope = invocation.diagnostic().scope.clone();
            let binding = CaptureBinding::from_source(
                source,
                scope.clone(),
                "execution",
                CoverageLevel::AvailablePayloads,
            )?;
            let event = EventDraft {
                event_id: format!("{:?}/{}", chunk.stream, chunk.part),
                source_version: "akasha-outcome-v1".into(),
                runtime_instance: chunk.invocation_id.clone(),
                branch: chunk.event_id.clone(),
                parent: None,
                fork_cutoff: None,
                event_time_ms: Some(chunk.observed_at_ms as i64),
                observed_time_ms: chunk.observed_at_ms as i64,
                visibility: Visibility::Tool,
                kind: EventKind::Execution,
                text: String::from_utf8_lossy(&raw[..raw.len().min(outcome::PREVIEW_BYTES)])
                    .into_owned(),
            };
            let view = Capture::new(&service.storage)
                .ingest(
                    &binding,
                    &event,
                    &raw,
                    &crate::domain::Ingress::tool("outcome-wrapper", invocation.id(), scope)?,
                    None,
                    service.config.pin().generation(),
                )
                .await?;
            Ok(serde_json::to_value(view.event.reference)?)
        })
        .await
    }

    pub(in crate::daemon) async fn outcome_log(
        &self,
        id: &str,
        index: usize,
        start: usize,
    ) -> Result<Value> {
        let _guard = self.admission.lock().await;
        let record = self.read_outcome(id).await?;
        let log = record.logs.get(index).ok_or(Error("INVALID_ARGUMENT"))?;
        let invocation = self.outcome_invocation(&record.event)?;
        let capture = Capture::new(&self.storage);
        let event = capture
            .event(
                &invocation.capture_binding()?,
                &log.source.reference,
                log.revision,
            )?
            .ok_or(Error("SOURCE_UNAVAILABLE"))?;
        let raw = capture.raw(&event)?;
        if start > raw.len() {
            return Err(Error("INVALID_ARGUMENT"));
        }
        let end = start.saturating_add(32 * 1024).min(raw.len());
        Ok(
            json!({"source":log.source,"revision":log.revision,"sha256":log.sha256,
            "total_bytes":raw.len(),"coverage":log.coverage,"completeness":log.completeness,
            "start":start,"raw_hex":crate::capture::encode_hex(&raw[start..end]),
            "next_start":(end < raw.len()).then_some(end)}),
        )
    }
}
