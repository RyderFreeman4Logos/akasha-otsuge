use super::*;
use crate::adapter::ContextText;

#[derive(serde::Serialize)]
pub(super) struct Fragment {
    start: usize,
    end: usize,
    source: SourceSpan,
}

#[derive(serde::Serialize)]
pub(super) struct ReadEvidence {
    total_bytes: usize,
    sha256: String,
    fragments: Vec<Fragment>,
}

impl ReadEvidence {
    pub(super) fn complete(&self) -> bool {
        let mut ranges: Vec<_> = self.fragments.iter().map(|f| (f.start, f.end)).collect();
        if ranges.is_empty() {
            return false;
        }
        ranges.sort_unstable();
        let mut end = 0;
        for (start, next) in ranges {
            if start > end {
                return false;
            }
            end = end.max(next);
        }
        end == self.total_bytes
    }
}

impl Research<'_> {
    // Both callers hold admission through permission checks, reads and evidence capture.
    async fn incident_view(
        &mut self,
        lease: &RunLease,
    ) -> Result<(crate::incident::Record, crate::outcome::Record)> {
        let id = self.incident.as_deref().ok_or(Error("READ_DENIED"))?;
        let incident = self.service.authorize_incident_execution(id)?;
        let outcome = self.service.read_outcome(&incident.first_outcome).await?;
        lease.charge_read(serde_json::to_vec(&outcome)?.len() as u64)?;
        for (index, log) in outcome.logs.iter().enumerate() {
            self.incident_expected
                .insert(format!("incident:log:{index}"));
            if log.completeness != crate::outcome::LogCompleteness::Complete {
                self.gap("outcome_log_incomplete");
            }
        }
        if let Some(execution) = &outcome.event.execution {
            if !execution.before.gaps.is_empty() || !execution.after.gaps.is_empty() {
                self.gap("outcome_workspace_gap");
            }
            if !execution.artifact_gaps.is_empty() {
                self.gap("outcome_artifact_gap");
            }
        } else if incident.signal.kind == crate::config::IncidentTrigger::ReproducibleFailure {
            self.gap("outcome_execution_unavailable");
        }
        Ok((incident, outcome))
    }

    pub(super) async fn incident_description(
        &mut self,
        lease: &RunLease,
    ) -> Result<crate::incident::Context> {
        let service = self.service;
        let _guard = service.admission.lock().await;
        let (incident, outcome) = self.incident_view(lease).await?;
        Ok(crate::incident::Context {
            incident_id: incident.id,
            outcome: "incident:outcome".into(),
            logs: outcome
                .logs
                .into_iter()
                .enumerate()
                .map(|(index, log)| crate::incident::LogContext {
                    handle: format!("incident:log:{index}"),
                    reference: log.source.reference,
                    raw_bytes: log.bytes,
                    sha256: log.sha256,
                    coverage: log.coverage,
                    completeness: log.completeness,
                })
                .collect(),
        })
    }

    pub(super) async fn incident_text(
        &mut self,
        lease: &RunLease,
        handle: &str,
        start: usize,
        max_bytes: usize,
    ) -> Result<ContextText> {
        if max_bytes == 0 || max_bytes > 16 * 1024 {
            return Err(Error("INVALID_ARGUMENT"));
        }
        let service = self.service;
        let _guard = service.admission.lock().await;
        let (incident, outcome) = self.incident_view(lease).await?;
        if !self.incident_expected.contains(handle) {
            return Err(Error("READ_DENIED"));
        }
        let text = if handle == "incident:outcome" {
            serde_json::to_string(&outcome)?
        } else {
            let index: usize = handle
                .strip_prefix("incident:log:")
                .ok_or(Error("READ_DENIED"))?
                .parse()
                .map_err(|_| Error("READ_DENIED"))?;
            let log = outcome.logs.get(index).ok_or(Error("READ_DENIED"))?;
            lease.charge_read(log.bytes as u64)?;
            let raw = service.outcome_raw(&outcome, index)?;
            let mut value = json!({"schema_version":1,"outcome_event_id":outcome.event.event_id,
                "index":index,"reference":log.source.reference,"revision":log.revision,
                "raw_sha256":log.sha256,"coverage":log.coverage,"completeness":log.completeness});
            match String::from_utf8(raw) {
                Ok(text) => {
                    value["encoding"] = json!("utf8");
                    value["text"] = json!(text);
                }
                Err(error) => {
                    value["encoding"] = json!("hex");
                    value["raw_hex"] = json!(crate::capture::encode_hex(error.as_bytes()));
                }
            }
            serde_json::to_string(&value)?
        };
        if start > text.len() || !text.is_char_boundary(start) {
            return Err(Error("INVALID_ARGUMENT"));
        }
        let mut end = start.saturating_add(max_bytes).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        if end == start {
            return Err(Error("INVALID_ARGUMENT"));
        }
        let sha256 = hash(text.as_bytes());
        let event_id = format!(
            "incident-source/{}",
            hash(&serde_json::to_vec(&(
                self.id, handle, &sha256, start, end
            ))?)
        );
        let chunk = &text[start..end];
        let draft = SourceDraft {
            event_id: event_id.clone(),
            scope: outcome.scope.clone(),
            text: chunk.into(),
            reported_role: None,
            event_time_ms: None,
            observed_time_ms: incident
                .first_report_at_ms
                .try_into()
                .map_err(|_| Error("CLOCK_INVALID"))?,
        };
        let ingress = Ingress::tool("incident.read", self.invocation.id(), outcome.scope)?;
        if self.sources.len() == 64 {
            return Err(Error("BUDGET_EXHAUSTED"));
        }
        lease.check()?;
        let receipt = Domain::new(&service.storage)
            .record_source(&event_id, &draft, &ingress, self.config.generation())
            .await?;
        let source = SourceSpan {
            event_id,
            revision: receipt.commit_seq,
            content_sha256: hash(chunk.as_bytes()),
            start: 0,
            end: chunk.len(),
        };
        if !self.sources.contains(&source) {
            self.sources.push(source.clone());
        }
        self.read_seq = self.read_seq.max(receipt.commit_seq);
        let evidence = self
            .incident_reads
            .entry(handle.into())
            .or_insert_with(|| ReadEvidence {
                total_bytes: text.len(),
                sha256: sha256.clone(),
                fragments: vec![],
            });
        if evidence.sha256 != sha256 || evidence.total_bytes != text.len() {
            return Err(Error("SOURCE_CHANGED"));
        }
        if !evidence
            .fragments
            .iter()
            .any(|f| f.start == start && f.end == end)
        {
            evidence.fragments.push(Fragment {
                start,
                end,
                source: source.clone(),
            });
        }
        Ok(ContextText {
            text: chunk.into(),
            start,
            end,
            total_bytes: text.len(),
            sha256,
            truncated: end < text.len(),
            sources: vec![source],
        })
    }
}
