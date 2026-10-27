use super::*;
use crate::config::deadline::Deadline;
use crate::storage::RevisionRef;

// Include bounded profile names and per-stage traces alongside the response bytes.
const MAX_RESULT: usize =
    crate::provider::MAX_RESPONSE_BYTES * crate::provider::MAX_CALLS + 64 * 1024;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ResultArchive {
    bytes: usize,
    sha256: String,
    parts: Vec<RevisionRef>,
}

async fn save_result(storage: &Storage, value: &Value, generation: u64) -> Result<Value> {
    let content = serde_json::to_string(value)?;
    if content.len() > MAX_RESULT {
        return Err(Error("PROVIDER_OUTPUT_TOO_LARGE"));
    }
    let mut parts = Vec::new();
    let mut start = 0;
    while start < content.len() {
        let mut end = (start + 16384).min(content.len());
        while !content.is_char_boundary(end) {
            end -= 1;
        }
        let chunk = &content[start..end];
        let digest = hash(chunk.as_bytes());
        let key = RecordKey {
            kind: RecordKind::Evidence,
            id: format!("model-blob/{digest}"),
        };
        let commit_seq = if let Some(existing) = storage.current(&key)? {
            if existing.previous.is_some() || existing.content != chunk {
                return Err(Error("JOB_CORRUPT"));
            }
            existing.commit_seq
        } else {
            storage
                .commit(&CommitRequest {
                    request_id: format!("model-blob/{digest}"),
                    config_generation: generation,
                    policy_revision: None,
                    changes: vec![Change {
                        key: key.clone(),
                        expected_revision: None,
                        content: chunk.into(),
                    }],
                })
                .await?
                .commit_seq
        };
        parts.push(RevisionRef { key, commit_seq });
        start = end;
    }
    Ok(serde_json::to_value(ResultArchive {
        bytes: content.len(),
        sha256: hash(content.as_bytes()),
        parts,
    })?)
}

pub(super) fn load_result(storage: &Storage, pointer: &Value, point: u64) -> Result<Value> {
    let pointer: ResultArchive =
        serde_json::from_value(pointer.clone()).map_err(|_| Error("JOB_CORRUPT"))?;
    if pointer.bytes > MAX_RESULT || pointer.parts.len() > MAX_RESULT / 16384 + 1 {
        return Err(Error("JOB_CORRUPT"));
    }
    let mut content = String::new();
    for part in pointer.parts {
        if part.key.kind != RecordKind::Evidence || part.commit_seq > point {
            return Err(Error("JOB_CORRUPT"));
        }
        let chunk = storage
            .revision(&part.key, part.commit_seq)?
            .ok_or(Error("JOB_CORRUPT"))?;
        if chunk.previous.is_some()
            || part.key.id != format!("model-blob/{}", hash(chunk.content.as_bytes()))
            || content.len() + chunk.content.len() > pointer.bytes
        {
            return Err(Error("JOB_CORRUPT"));
        }
        content.push_str(&chunk.content);
    }
    if content.len() != pointer.bytes || hash(content.as_bytes()) != pointer.sha256 {
        return Err(Error("JOB_CORRUPT"));
    }
    serde_json::from_str(&content).map_err(|_| Error("JOB_CORRUPT"))
}

impl Service {
    pub(super) async fn model_cancelled(&self, id: &str) -> &'static str {
        loop {
            let changed = self.work.notified();
            tokio::pin!(changed);
            changed.as_mut().enable();
            {
                let _guard = self.admission.lock().await;
                if self.stopping.load(std::sync::atomic::Ordering::Acquire) {
                    return "PROVIDER_INTERRUPTED";
                }
                match self.job(id) {
                    Ok((_, job)) if job.cancel_requested => return "CANCELLED",
                    Err(_) => return "JOB_CORRUPT",
                    _ => (),
                }
            }
            changed.await;
        }
    }

    pub(super) async fn execute_model(
        self: &Arc<Self>,
        id: String,
        workflow: Deadline,
    ) -> Result<()> {
        let (plan, config) = {
            let _guard = self.admission.lock().await;
            let (_, job) = self.job(&id)?;
            let checkpoint = configuration::load(
                &self.storage,
                &format!("daemon/config-generation/{:020}", job.config_generation),
            )?
            .ok_or(Error("CONFIG_CORRUPT"))?;
            let restored = crate::config::ConfigManager::restore(checkpoint, |_| Ok(()))?;
            if restored.pin().resolved().digest() != job.config_digest {
                return Err(Error("CONFIG_CORRUPT"));
            }
            (job.model_plan.ok_or(Error("JOB_CORRUPT"))?, restored.pin())
        };
        let mut calls = Vec::new();
        for call in &plan.calls {
            {
                let _guard = self.admission.lock().await;
                let (revision, job) = self.job(&id)?;
                if workflow.expired().is_some() {
                    return self
                        .finish_job(job, revision, "failed", Some("WORKFLOW_TIMEOUT"))
                        .await;
                }
                if let Err(error) = self.authorize_job(&job) {
                    return self
                        .finish_job(job, revision, "failed", Some(error.0))
                        .await;
                }
            }
            let outcome = self
                .providers
                .call(
                    config.resolved(),
                    call,
                    &workflow,
                    self.model_cancelled(&id),
                )
                .await;
            let failure = outcome.error.clone();
            calls.push(serde_json::to_value(outcome)?);
            let _guard = self.admission.lock().await;
            let (revision, mut job) = self.job(&id)?;
            if job.state != "running" || job.lease_owner.as_deref() != Some(&self.instance) {
                return Err(Error("LEASE_LOST"));
            }
            let result = save_result(
                &self.storage,
                &json!({"calls":calls}),
                job.config_generation,
            )
            .await;
            job.result = Some(match result {
                Ok(result) => result,
                Err(Error("PROVIDER_OUTPUT_TOO_LARGE")) => {
                    return self
                        .finish_job(job, revision, "failed", Some("PROVIDER_OUTPUT_TOO_LARGE"))
                        .await;
                }
                Err(error) => return Err(error),
            });
            if job.cancel_requested {
                return self
                    .finish_job(job, revision, "cancelled", Some("CANCELLED"))
                    .await;
            }
            if let Some(reason) = failure {
                return self
                    .finish_job(job, revision, "failed", Some(&reason))
                    .await;
            }
            if workflow.expired().is_some() {
                return self
                    .finish_job(job, revision, "failed", Some("WORKFLOW_TIMEOUT"))
                    .await;
            }
            if calls.len() == plan.calls.len() {
                return self.finish_job(job, revision, "succeeded", None).await;
            }
            job.lease_until_ms = Some(ipc::now_ms()?.saturating_add(30_000));
            self.save_job(&job, Some(revision), vec![]).await?;
        }
        Err(Error("JOB_CORRUPT"))
    }
}
