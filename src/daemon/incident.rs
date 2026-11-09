use super::*;
use crate::config::IncidentTrigger;
use crate::domain::ReviewState;
use crate::incident::{Event, Investigation, Priority, Record};

fn digest_id(id: &str) -> bool {
    id.len() == 64
        && id
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn key(id: &str) -> Result<RecordKey> {
    if !digest_id(id) {
        return Err(Error("INVALID_INCIDENT"));
    }
    Ok(RecordKey {
        kind: RecordKind::State,
        id: format!("incident/{id}"),
    })
}

fn event_key(id: &str, event: &str) -> RecordKey {
    RecordKey {
        kind: RecordKind::Evidence,
        id: format!("incident-event/{id}/{event}"),
    }
}

fn pending_key() -> RecordKey {
    RecordKey {
        kind: RecordKind::State,
        id: "daemon/incidents".into(),
    }
}

impl Service {
    fn pending_incidents(&self) -> Result<(Option<u64>, Vec<String>)> {
        let Some(row) = self.storage.current(&pending_key())? else {
            return Ok((None, vec![]));
        };
        let ids: Vec<String> = serde_json::from_str(&row.content)?;
        if ids.len() > MAX_ACTIVE || ids.iter().any(|id| !digest_id(id)) {
            return Err(Error("INCIDENT_CORRUPT"));
        }
        Ok((Some(row.commit_seq), ids))
    }

    fn incident_row(&self, id: &str) -> Result<(u64, Record)> {
        let row = self
            .storage
            .current(&key(id)?)?
            .ok_or(Error("INCIDENT_NOT_FOUND"))?;
        let record: Record = serde_json::from_str(&row.content)?;
        if record.schema_version != 1 || record.id != id || record.occurrences == 0 {
            return Err(Error("INCIDENT_CORRUPT"));
        }
        record.signal.validate()?;
        Ok((row.commit_seq, record))
    }

    pub(in crate::daemon::jobs) fn incident_record(&self, id: &str) -> Result<(u64, Record)> {
        let (revision, record) = self.incident_row(id)?;
        let invocation = self.outcome_binding(&record.invocation_id)?;
        if record.principal != principal() || record.scope != invocation.diagnostic().scope {
            return Err(Error("READ_DENIED"));
        }
        Ok((revision, record))
    }

    pub(in crate::daemon::jobs) fn authorize_incident_execution(&self, id: &str) -> Result<Record> {
        let record = self.incident_record(id)?.1;
        if !matches!(record.state, ReviewState::Open | ReviewState::Investigating) {
            return Err(Error("CANCELLED"));
        }
        if !self
            .config
            .pin()
            .resolved()
            .config()
            .incidents
            .triggers
            .contains(&record.signal.kind)
        {
            return Err(Error("INCIDENT_DISABLED"));
        }
        if record.expires_at_ms <= ipc::now_ms()? {
            return Err(Error("PATCH_EXPIRED"));
        }
        Ok(record)
    }

    pub(super) fn incident_changes(
        &self,
        outcome: &mut crate::outcome::Record,
    ) -> Result<Vec<Change>> {
        let Some(mut signal) = outcome.event.incident.clone() else {
            return Ok(vec![]);
        };
        let snapshot = self.config.pin();
        let policy = &snapshot.resolved().config().incidents;
        if !policy.triggers.contains(&signal.kind) {
            return Ok(vec![]);
        }
        if signal.targets.is_empty() {
            for link in &outcome.links {
                let target = crate::editor::Target {
                    memory_id: link.memory.key.id.clone(),
                    expected_revision: link.memory.commit_seq,
                };
                if !signal.targets.contains(&target) {
                    signal.targets.push(target);
                }
            }
        }
        // Oversized exposure sets remain reviewable without choosing an arbitrary patch target.
        let target_limit = signal.targets.len() > 8;
        if target_limit {
            signal.targets.clear();
        }
        signal.targets.sort_by(|a, b| {
            a.memory_id
                .cmp(&b.memory_id)
                .then(a.expected_revision.cmp(&b.expected_revision))
        });
        let invocation = self.outcome_binding(&outcome.run_id)?;
        let domain = Domain::new(&self.storage);
        for target in &signal.targets {
            let memory = domain
                .revision(&target.memory_id, target.expected_revision)?
                .ok_or(Error("REVISION_NOT_FOUND"))?;
            domain.permission(
                &invocation.read_context()?,
                Some(&memory.record.draft.scope),
            )?;
            if !outcome.scope.contains(&memory.record.draft.scope) {
                return Err(Error("SCOPE_DENIED"));
            }
        }
        let id = hash(&serde_json::to_vec(&(
            principal(),
            &outcome.scope,
            &outcome.event.source,
            signal.kind,
            &signal.key,
            &signal.targets,
        ))?);
        let id = if policy.deduplicate {
            id
        } else {
            hash(&serde_json::to_vec(&(id, &outcome.event.event_id))?)
        };
        let k = key(&id)?;
        let old = self.storage.current(&k)?;
        let mut changes = vec![];
        let now = ipc::now_ms()?;
        let mut record: Record = match &old {
            Some(row) => {
                let mut record: Record = serde_json::from_str(&row.content)?;
                if record.schema_version != 1 || record.id != id || record.scope != outcome.scope {
                    return Err(Error("INCIDENT_CORRUPT"));
                }
                record.occurrences = record
                    .occurrences
                    .checked_add(1)
                    .ok_or(Error("INCIDENT_CORRUPT"))?;
                record.last_outcome = outcome.event.event_id.clone();
                record.last_report_at_ms = now;
                record
            }
            None => {
                let (pending_revision, mut pending) = self.pending_incidents()?;
                let error = if target_limit {
                    Some("TARGET_LIMIT")
                } else if policy.executor.is_none() {
                    Some("EXECUTOR_UNAVAILABLE")
                } else if pending.len() >= MAX_ACTIVE {
                    Some("RESOURCE_BUSY")
                } else {
                    None
                };
                if error.is_none() {
                    pending.push(id.clone());
                    changes.push(Change {
                        key: pending_key(),
                        expected_revision: pending_revision,
                        content: serde_json::to_string(&pending)?,
                    });
                }
                let expiry = snapshot
                    .resolved()
                    .timeouts()
                    .queue
                    .checked_add(snapshot.resolved().timeouts().workflow)
                    .and_then(|d| u64::try_from(d.as_millis()).ok())
                    .and_then(|d| now.checked_add(d))
                    .ok_or(Error("CONFIG_INVALID"))?;
                Record {
                    schema_version: 1,
                    id: id.clone(),
                    principal: principal(),
                    scope: outcome.scope.clone(),
                    invocation_id: outcome.run_id.clone(),
                    signal: *signal,
                    first_outcome: outcome.event.event_id.clone(),
                    last_outcome: outcome.event.event_id.clone(),
                    occurrences: 1,
                    first_report_at_ms: now,
                    last_report_at_ms: now,
                    expires_at_ms: expiry,
                    priority: Priority::Normal,
                    priority_reasons: vec![],
                    state: ReviewState::Open,
                    config_generation: snapshot.generation(),
                    investigation: if error.is_none() {
                        Investigation::Pending
                    } else {
                        Investigation::Finished
                    },
                    job_id: None,
                    error: error.map(str::to_owned),
                    classification: None,
                    operator_reason: None,
                }
            }
        };
        let (priority, reason) = if let Some(priority) = policy.priorities.get(&record.signal.kind)
        {
            (*priority, "configured_trigger")
        } else if record.signal.kind == IncidentTrigger::UserCorrection {
            (Priority::Urgent, "reported_correction")
        } else if matches!(
            record.signal.kind,
            IncidentTrigger::ReproducibleFailure | IncidentTrigger::Contradiction
        ) {
            (Priority::High, "reported_conflict")
        } else {
            (Priority::Normal, "maintenance")
        };
        for (priority, reason) in std::iter::once((priority, reason))
            .chain(
                (outcome.event.severity == crate::outcome::Severity::Critical)
                    .then_some((Priority::Urgent, "critical_signal")),
            )
            .chain(
                (record.occurrences >= policy.escalate_after)
                    .then_some((Priority::High, "recurrence")),
            )
        {
            record.priority = record.priority.max(priority);
            if !record.priority_reasons.iter().any(|r| r == reason) {
                record.priority_reasons.push(reason.into());
            }
        }
        changes.push(Change {
            key: k,
            expected_revision: old.map(|r| r.commit_seq),
            content: serde_json::to_string(&record)?,
        });
        changes.push(Change {
            key: event_key(&id, &hash(outcome.event.event_id.as_bytes())),
            expected_revision: None,
            content: serde_json::to_string(&Event {
                event_id: outcome.event.event_id.clone(),
                invocation_id: outcome.run_id.clone(),
            })?,
        });
        outcome.incident = Some(id);
        Ok(changes)
    }

    async fn save_incident(&self, revision: u64, record: &Record) -> Result<()> {
        let mut changes = vec![Change {
            key: key(&record.id)?,
            expected_revision: Some(revision),
            content: serde_json::to_string(record)?,
        }];
        if record.investigation == Investigation::Finished {
            let (pending_revision, mut pending) = self.pending_incidents()?;
            pending.retain(|id| id != &record.id);
            changes.push(Change {
                key: pending_key(),
                expected_revision: pending_revision,
                content: serde_json::to_string(&pending)?,
            });
        }
        self.storage
            .commit(&CommitRequest {
                request_id: format!("incident-state/{}/{revision}", record.id),
                config_generation: self.config.pin().generation(),
                policy_revision: None,
                changes,
            })
            .await?;
        Ok(())
    }

    pub(super) async fn advance_incidents(self: &Arc<Self>) -> Result<()> {
        let ids = {
            let _guard = self.admission.lock().await;
            self.pending_incidents()?.1
        };
        for id in ids {
            let record = {
                let _guard = self.admission.lock().await;
                self.incident_row(&id)?.1
            };
            if record.job_id.is_none() {
                let submitted = async {
                    self.authorize_incident_execution(&id)?;
                    let config = self.retrieval_snapshot(record.config_generation)?;
                    let queue = u64::try_from(config.resolved().timeouts().queue.as_millis())
                        .map_err(|_| Error("CONFIG_INVALID"))?;
                    if ipc::now_ms()? >= record.first_report_at_ms.saturating_add(queue) {
                        return Err(Error("QUEUE_TIMEOUT"));
                    }
                    self.submit_incident_editor(&record).await
                }
                .await;
                if matches!(submitted, Err(Error("RESOURCE_BUSY"))) {
                    continue;
                }
                let _guard = self.admission.lock().await;
                let (revision, mut current) = self.incident_row(&id)?;
                match submitted {
                    Ok(job) => {
                        current.job_id =
                            Some(job["id"].as_str().ok_or(Error("JOB_CORRUPT"))?.into());
                        current.investigation = Investigation::Running;
                        if current.state == ReviewState::Open {
                            current.state = ReviewState::Investigating;
                        }
                    }
                    Err(error) => {
                        current.investigation = Investigation::Finished;
                        current.error = Some(error.0.into());
                    }
                }
                self.save_incident(revision, &current).await?;
                continue;
            }
            let _guard = self.admission.lock().await;
            let (revision, mut current) = self.incident_row(&id)?;
            let (_, job) = self.job(current.job_id.as_deref().ok_or(Error("INCIDENT_CORRUPT"))?)?;
            if job
                .consult_plan
                .as_ref()
                .and_then(|p| p.incident.as_deref())
                != Some(&id)
            {
                return Err(Error("INCIDENT_CORRUPT"));
            }
            if !job.terminal() {
                continue;
            }
            current.investigation = Investigation::Finished;
            let mut state = ReviewState::Open;
            if job.state == "succeeded" {
                let result = job.result.as_ref().ok_or(Error("JOB_CORRUPT"))?;
                let report: crate::editor::Report =
                    serde_json::from_value(result["editor"].clone())?;
                current.classification = Some(report.classification.clone());
                let complete = result["status"] == "found"
                    && result["gaps"].as_array().is_some_and(Vec::is_empty)
                    && result["conflicts"].as_array().is_some_and(Vec::is_empty)
                    && result["evidence"].as_array().is_some_and(|e| !e.is_empty());
                if complete {
                    if let Some(patch) = report.patch {
                        let seq = result["patch_receipt"]["commit_seq"]
                            .as_u64()
                            .ok_or(Error("JOB_CORRUPT"))?;
                        let domain = Domain::new(&self.storage);
                        let active = patch
                            .changes
                            .iter()
                            .map(|change| domain.active(&change.memory_id))
                            .collect::<std::result::Result<Vec<_>, _>>()?;
                        if active.iter().all(|memory| {
                            memory
                                .as_ref()
                                .is_some_and(|m| m.revision.commit_seq == seq)
                        }) {
                            state = ReviewState::Resolved;
                        }
                    } else if matches!(
                        report.classification,
                        crate::editor::Classification::Environment
                            | crate::editor::Classification::ExecutionError
                            | crate::editor::Classification::RequirementsChanged
                    ) {
                        state = ReviewState::Dismissed;
                    }
                }
            } else {
                current.error = job.error.clone();
            }
            if matches!(
                current.state,
                ReviewState::Open | ReviewState::Investigating
            ) {
                current.state = state;
            }
            self.save_incident(revision, &current).await?;
        }
        Ok(())
    }

    pub(in crate::daemon) async fn show_incident(
        &self,
        id: &str,
        after: Option<&str>,
    ) -> Result<Value> {
        let _guard = self.admission.lock().await;
        let (revision, record) = self.incident_record(id)?;
        if after.is_some_and(|cursor| !digest_id(cursor)) {
            return Err(Error("INVALID_INCIDENT"));
        }
        let after = after.map(|cursor| event_key(id, cursor));
        let keys = self.storage.head_keys(
            &event_key(id, &"0".repeat(64)),
            &event_key(id, &"g".repeat(64)),
            after.as_ref().map(|k| k.id.as_str()),
            32,
        )?;
        let more = keys.len() > 32;
        let mut events = vec![];
        let mut next = None;
        for k in keys.into_iter().take(32) {
            let row = self.storage.current(&k)?.ok_or(Error("INCIDENT_CORRUPT"))?;
            let event: Event = serde_json::from_str(&row.content)?;
            let outcome = self.read_outcome(&event.event_id).await?;
            if outcome.incident.as_deref() != Some(id) || outcome.run_id != event.invocation_id {
                return Err(Error("INCIDENT_CORRUPT"));
            }
            next = k.id.rsplit('/').next().map(str::to_owned);
            events.push(event);
        }
        Ok(
            json!({"revision":revision,"record":record,"events":events,"next":if more { next } else { None }}),
        )
    }

    pub(in crate::daemon) async fn list_incidents(
        &self,
        scope: &Scope,
        after: Option<&str>,
    ) -> Result<Value> {
        let _guard = self.admission.lock().await;
        Domain::new(&self.storage).permission(&context()?, Some(scope))?;
        let after = after.map(key).transpose()?;
        let keys = self.storage.head_keys(
            &key(&"0".repeat(64))?,
            &RecordKey {
                kind: RecordKind::State,
                id: format!("incident/{}", "g".repeat(64)),
            },
            after.as_ref().map(|k| k.id.as_str()),
            32,
        )?;
        let more = keys.len() > 32;
        let mut items = vec![];
        let mut next = None;
        for k in keys.into_iter().take(32) {
            let id =
                k.id.strip_prefix("incident/")
                    .ok_or(Error("INCIDENT_CORRUPT"))?;
            match self.incident_record(id) {
                Ok((revision, record)) if scope.contains(&record.scope) => {
                    items.push(json!({"revision":revision,"record":record}))
                }
                Ok(_) | Err(Error("READ_DENIED" | "SCOPE_DENIED")) => (),
                Err(error) => return Err(error),
            }
            next = Some(id.to_owned());
        }
        Ok(json!({"items":items,"next":if more { next } else { None }}))
    }

    pub(in crate::daemon) async fn update_incident(
        self: &Arc<Self>,
        request: String,
        update: crate::domain::ReviewUpdate,
    ) -> Result<Value> {
        if !ipc::identifier(&request)
            || update.reason.trim().is_empty()
            || update.reason.len() > 4096
        {
            return Err(Error("INVALID_INCIDENT"));
        }
        let service = self.clone();
        self.operation(async move {
            let _guard = service.admission.lock().await;
            service.incident_record(&update.id)?;
            let k = key(&update.id)?;
            let request_id = format!("incident-update/{}", hash(request.as_bytes()));
            let receipt = service
                .storage
                .commit_checked(|| {
                    let row = service
                        .storage
                        .revision(&k, update.expected_revision)?
                        .ok_or(crate::storage::StorageError::new("INCIDENT_NOT_FOUND"))?;
                    let mut record: Record = serde_json::from_str(&row.content)?;
                    if record.state == update.state
                        || matches!(record.state, ReviewState::Resolved | ReviewState::Dismissed)
                            && update.state != ReviewState::Open
                    {
                        return Err(crate::storage::StorageError::new(
                            "INVALID_REVIEW_TRANSITION",
                        ));
                    }
                    record.state = update.state;
                    record.operator_reason = Some(update.reason.clone());
                    Ok(CommitRequest {
                        request_id: request_id.clone(),
                        config_generation: service
                            .storage
                            .receipt(&request_id)?
                            .map_or(service.config.pin().generation(), |r| r.config_generation),
                        policy_revision: None,
                        changes: vec![Change {
                            key: k.clone(),
                            expected_revision: Some(update.expected_revision),
                            content: serde_json::to_string(&record)?,
                        }],
                    })
                })
                .await?;
            service.wake_jobs();
            Ok(serde_json::to_value(receipt)?)
        })
        .await
    }
}
