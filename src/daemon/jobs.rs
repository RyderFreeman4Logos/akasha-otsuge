use super::*;
use crate::capture::{Capture, PendingReason, ReconcileReport};
use crate::domain::{Domain, ReadContext, Scope};
use crate::ipc::CaptureInput;
use crate::storage::{Change, CommitRequest, RecordKey, RecordKind, hash};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;

const MAX_ACTIVE: usize = 256;
const MAX_RUNNING: usize = 16;
const MAX_ATTEMPTS: u32 = 3;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Job {
    schema_version: u32,
    id: String,
    request_sha256: String,
    binding: CaptureInput,
    path: PathBuf,
    config_generation: u64,
    config_digest: String,
    accepted_at_ms: u64,
    queue_deadline_ms: u64,
    execution_budget_ms: u64,
    started_at_ms: Option<u64>,
    execution_deadline_ms: Option<u64>,
    attempt: u32,
    retry_at_ms: u64,
    lease_owner: Option<String>,
    lease_until_ms: Option<u64>,
    state: String,
    cancel_requested: bool,
    error: Option<String>,
    result: Option<ReconcileReport>,
}
impl Job {
    fn terminal(&self) -> bool {
        matches!(self.state.as_str(), "succeeded" | "failed" | "cancelled")
    }
}
fn job_key(id: &str) -> RecordKey {
    RecordKey {
        kind: RecordKind::Job,
        id: format!("daemon-job/{}", hash(id.as_bytes())),
    }
}
fn active_key() -> RecordKey {
    RecordKey {
        kind: RecordKind::State,
        id: "daemon/active-jobs".into(),
    }
}
fn principal() -> String {
    format!("uid:{}", rustix::process::geteuid().as_raw())
}
pub(super) fn context() -> Result<ReadContext> {
    Ok(ReadContext::from_verified_binding(
        &principal(),
        "local-management",
        "local-management",
    )?)
}
pub(super) fn owner_scope() -> Scope {
    Scope {
        user: principal(),
        project: None,
        branch_or_version: None,
        session: None,
    }
}

impl Service {
    fn job(&self, id: &str) -> Result<(u64, Job)> {
        if !ipc::identifier(id) {
            return Err(Error("INVALID_REQUEST"));
        }
        let revision = self
            .storage
            .current(&job_key(id))?
            .ok_or(Error("JOB_NOT_FOUND"))?;
        let job: Job = serde_json::from_str(&revision.content)?;
        if job.schema_version != 1
            || job.id != id
            || job.attempt > MAX_ATTEMPTS
            || !["queued", "running", "succeeded", "failed", "cancelled"]
                .contains(&job.state.as_str())
        {
            return Err(Error("JOB_CORRUPT"));
        }
        Ok((revision.commit_seq, job))
    }
    fn active(&self) -> Result<(Option<u64>, Vec<String>)> {
        let Some(revision) = self.storage.current(&active_key())? else {
            return Ok((None, Vec::new()));
        };
        let ids: Vec<String> = serde_json::from_str(&revision.content)?;
        if ids.len() > MAX_ACTIVE
            || ids.iter().any(|s| !ipc::identifier(s))
            || ids.iter().collect::<BTreeSet<_>>().len() != ids.len()
        {
            return Err(Error("JOB_CORRUPT"));
        }
        Ok((Some(revision.commit_seq), ids))
    }
    async fn save_job(
        &self,
        job: &Job,
        previous: Option<u64>,
        mut changes: Vec<Change>,
    ) -> Result<()> {
        changes.push(Change {
            key: job_key(&job.id),
            expected_revision: previous,
            content: serde_json::to_string(job)?,
        });
        self.storage
            .commit(&CommitRequest {
                request_id: format!("job/{}/{}", hash(job.id.as_bytes()), previous.unwrap_or(0)),
                config_generation: job.config_generation,
                policy_revision: None,
                changes,
            })
            .await?;
        Ok(())
    }
    fn authorize_job(&self, job: &Job) -> Result<()> {
        Domain::new(&self.storage).permission(&context()?, Some(&job.binding.scope))?;
        Ok(())
    }
    fn allowed_path(&self, path: &Path) -> Result<PathBuf> {
        if !path.is_absolute() {
            return Err(Error("PATH_DENIED"));
        }
        let canonical = match path.canonicalize() {
            Ok(p) => p,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => path
                .parent()
                .ok_or(Error("PATH_DENIED"))?
                .canonicalize()?
                .join(path.file_name().ok_or(Error("PATH_DENIED"))?),
            Err(_) => return Err(Error("PATH_DENIED")),
        };
        let snapshot = self.config.pin();
        if !snapshot
            .resolved()
            .config()
            .access
            .read_roots
            .iter()
            .filter_map(|p| p.canonicalize().ok())
            .any(|root| canonical.starts_with(root))
        {
            return Err(Error("PATH_DENIED"));
        }
        Ok(canonical)
    }
    pub(super) async fn submit(
        self: &Arc<Self>,
        id: String,
        binding: CaptureInput,
        path: PathBuf,
    ) -> Result<Value> {
        let service = self.clone();
        tokio::spawn(async move {
            let _guard = service.admission.lock().await;
            if !ipc::identifier(&id) {
                return Err(Error("INVALID_REQUEST"));
            }
            binding.binding()?;
            Domain::new(&service.storage).permission(&context()?, Some(&binding.scope))?;
            let path = service.allowed_path(&path)?;
            let fingerprint = hash(&serde_json::to_vec(&(&binding, &path))?);
            if service.storage.current(&job_key(&id))?.is_some() {
                let (_, job) = service.job(&id)?;
                if job.request_sha256 != fingerprint {
                    return Err(Error("IDEMPOTENCY_CONFLICT"));
                }
                return Ok(serde_json::to_value(job)?);
            }
            let snapshot = service.config.pin();
            let runtime = &snapshot.resolved().config().runtime;
            let (previous, mut ids) = service.active()?;
            let mut queued = 0;
            for id in &ids {
                if service.job(id)?.1.state == "queued" {
                    queued += 1;
                }
            }
            if service.stopping.load(std::sync::atomic::Ordering::Acquire)
                || runtime.paused
                || ids.len() >= MAX_ACTIVE
                || queued >= runtime.max_queued_jobs
            {
                return Err(Error("RESOURCE_BUSY"));
            }
            let now = ipc::now_ms()?;
            let queue_ms = u64::try_from(snapshot.resolved().timeouts().queue.as_millis())
                .map_err(|_| Error("CONFIG_INVALID"))?;
            let execution_budget_ms =
                u64::try_from(snapshot.resolved().timeouts().tools.as_millis())
                    .map_err(|_| Error("CONFIG_INVALID"))?;
            let job = Job {
                schema_version: 1,
                id: id.clone(),
                request_sha256: fingerprint,
                binding,
                path,
                config_generation: snapshot.generation(),
                config_digest: snapshot.resolved().digest().into(),
                accepted_at_ms: now,
                queue_deadline_ms: now.checked_add(queue_ms).ok_or(Error("CONFIG_INVALID"))?,
                execution_budget_ms,
                started_at_ms: None,
                execution_deadline_ms: None,
                attempt: 0,
                retry_at_ms: now,
                lease_owner: None,
                lease_until_ms: None,
                state: "queued".into(),
                cancel_requested: false,
                error: None,
                result: None,
            };
            ids.push(id);
            service
                .save_job(
                    &job,
                    None,
                    vec![Change {
                        key: active_key(),
                        expected_revision: previous,
                        content: serde_json::to_string(&ids)?,
                    }],
                )
                .await?;
            service.work.notify_one();
            Ok(serde_json::to_value(job)?)
        })
        .await
        .map_err(|_| Error("JOB_ERROR"))?
    }
    pub(super) async fn show_job(&self, id: &str) -> Result<Value> {
        let _guard = self.admission.lock().await;
        let (_, job) = self.job(id)?;
        self.authorize_job(&job)?;
        Ok(serde_json::to_value(job)?)
    }
    pub(super) async fn list_jobs(&self, after: Option<&str>) -> Result<Value> {
        let _guard = self.admission.lock().await;
        let keys = self.storage.head_keys(
            &RecordKey {
                kind: RecordKind::Job,
                id: format!("daemon-job/{}", "0".repeat(64)),
            },
            &RecordKey {
                kind: RecordKind::Job,
                id: format!("daemon-job/{}", "g".repeat(64)),
            },
            after,
            32,
        )?;
        let mut jobs = Vec::new();
        for key in keys.iter().take(32) {
            let row = self.storage.current(key)?.ok_or(Error("JOB_CORRUPT"))?;
            let job: Job = serde_json::from_str(&row.content)?;
            self.authorize_job(&job)?;
            jobs.push(job);
        }
        Ok(json!({"items":jobs,"next":if keys.len()>32 {Some(&keys[31].id)}else{None}}))
    }
    pub(super) async fn cancel_job(self: &Arc<Self>, id: String) -> Result<Value> {
        let service = self.clone();
        tokio::spawn(async move {
            let _guard = service.admission.lock().await;
            let (revision, mut job) = service.job(&id)?;
            service.authorize_job(&job)?;
            if !job.terminal() && !job.cancel_requested {
                job.cancel_requested = true;
                if job.state == "queued" {
                    service
                        .finish_job(job, revision, "cancelled", Some("CANCELLED"))
                        .await?;
                    job = service.job(&id)?.1;
                } else {
                    service.save_job(&job, Some(revision), vec![]).await?;
                }
                service.work.notify_one();
            }
            Ok(serde_json::to_value(job)?)
        })
        .await
        .map_err(|_| Error("JOB_ERROR"))?
    }
    async fn finish_job(
        &self,
        mut job: Job,
        revision: u64,
        state: &str,
        error: Option<&str>,
    ) -> Result<()> {
        job.state = state.into();
        job.error = error.map(str::to_owned);
        job.lease_owner = None;
        job.lease_until_ms = None;
        let (previous, mut ids) = self.active()?;
        ids.retain(|id| id != &job.id);
        self.save_job(
            &job,
            Some(revision),
            vec![Change {
                key: active_key(),
                expected_revision: previous,
                content: serde_json::to_string(&ids)?,
            }],
        )
        .await?;
        self.work.notify_one();
        Ok(())
    }
    pub(super) async fn recover(&self) -> Result<()> {
        let _guard = self.admission.lock().await;
        for id in self.active()?.1 {
            let (revision, mut job) = self.job(&id)?;
            if job.terminal() {
                return Err(Error("JOB_CORRUPT"));
            }
            let checkpoint = configuration::load(
                &self.storage,
                &format!("daemon/config-generation/{:020}", job.config_generation),
            )?
            .ok_or(Error("CONFIG_CORRUPT"))?;
            let config = crate::config::ConfigManager::restore(checkpoint, |_| Ok(()))?;
            if config.pin().resolved().digest() != job.config_digest {
                return Err(Error("CONFIG_CORRUPT"));
            }
            if job.state == "running" {
                job.state = "queued".into();
                job.lease_owner = None;
                job.lease_until_ms = None;
                self.save_job(&job, Some(revision), vec![]).await?;
            }
        }
        Ok(())
    }
    async fn claim(&self, id: &str) -> Result<bool> {
        let _guard = self.admission.lock().await;
        let (revision, mut job) = self.job(id)?;
        if job.state != "queued" {
            return Ok(false);
        }
        let now = ipc::now_ms()?;
        let reason = if job.cancel_requested {
            Some(("cancelled", "CANCELLED"))
        } else if job.started_at_ms.is_none() && now >= job.queue_deadline_ms {
            Some(("failed", "QUEUE_TIMEOUT"))
        } else if job.execution_deadline_ms.is_some_and(|d| now >= d) {
            Some(("failed", "JOB_TIMEOUT"))
        } else if job.attempt >= MAX_ATTEMPTS {
            Some(("failed", "RETRY_EXHAUSTED"))
        } else {
            None
        };
        if let Some((state, reason)) = reason {
            self.finish_job(job, revision, state, Some(reason)).await?;
            return Ok(false);
        }
        if self.config.pin().resolved().config().runtime.paused || now < job.retry_at_ms {
            return Ok(false);
        }
        job.state = "running".into();
        job.attempt += 1;
        if job.started_at_ms.is_none() {
            job.started_at_ms = Some(now);
            job.execution_deadline_ms = Some(
                now.checked_add(job.execution_budget_ms)
                    .ok_or(Error("CONFIG_INVALID"))?,
            );
        }
        job.lease_owner = Some(self.instance.clone());
        job.lease_until_ms = Some(now.saturating_add(30_000));
        self.save_job(&job, Some(revision), vec![]).await?;
        Ok(true)
    }
    async fn execute_job(self: Arc<Self>, id: String) -> Result<()> {
        let deadline = {
            let _guard = self.admission.lock().await;
            self.job(&id)?
                .1
                .execution_deadline_ms
                .ok_or(Error("JOB_CORRUPT"))?
        };
        let remaining = Duration::from_millis(deadline.saturating_sub(ipc::now_ms()?));
        let end = tokio::time::Instant::now() + remaining;
        loop {
            let _guard = self.admission.lock().await;
            let (revision, mut job) = self.job(&id)?;
            if job.state != "running" || job.lease_owner.as_deref() != Some(&self.instance) {
                return Err(Error("LEASE_LOST"));
            }
            if job.cancel_requested {
                return self
                    .finish_job(job, revision, "cancelled", Some("CANCELLED"))
                    .await;
            }
            if tokio::time::Instant::now() >= end {
                return self
                    .finish_job(job, revision, "failed", Some("JOB_TIMEOUT"))
                    .await;
            }
            if self.stopping.load(std::sync::atomic::Ordering::Acquire) {
                job.state = "queued".into();
                job.lease_owner = None;
                job.lease_until_ms = None;
                return self.save_job(&job, Some(revision), vec![]).await;
            }
            let checked = self
                .authorize_job(&job)
                .and_then(|_| self.allowed_path(&job.path));
            if let Err(error) = checked {
                return self
                    .finish_job(job, revision, "failed", Some(error.0))
                    .await;
            }
            let storage = self.storage.clone();
            let binding = job.binding.binding()?;
            let path = job.path.clone();
            let generation = job.config_generation;
            let runtime = tokio::runtime::Handle::current();
            let result = tokio::task::spawn_blocking(move || {
                runtime.block_on(
                    Capture::new(&storage).reconcile_jsonl(&binding, &path, 1, generation),
                )
            })
            .await
            .map_err(|_| Error("JOB_ERROR"))?;
            match result {
                Ok(report) => {
                    let more = report.checkpoint.checkpoint.pending == Some(PendingReason::Backlog);
                    job.result = Some(report);
                    if !more {
                        return self.finish_job(job, revision, "succeeded", None).await;
                    }
                    job.lease_until_ms = Some(ipc::now_ms()?.saturating_add(30_000));
                    self.save_job(&job, Some(revision), vec![]).await?;
                }
                Err(error)
                    if matches!(error.code, "CAPTURE_SOURCE_CHANGED" | "REVISION_CONFLICT")
                        && job.attempt < MAX_ATTEMPTS =>
                {
                    job.state = "queued".into();
                    job.retry_at_ms = ipc::now_ms()?.saturating_add(100 * (1 << job.attempt));
                    job.lease_owner = None;
                    job.lease_until_ms = None;
                    self.save_job(&job, Some(revision), vec![]).await?;
                    self.work.notify_one();
                    return Ok(());
                }
                Err(error) => {
                    return self
                        .finish_job(job, revision, "failed", Some(error.code))
                        .await;
                }
            }
        }
    }
    pub(super) async fn jobs(self: Arc<Self>) -> Result<()> {
        let mut running = JoinSet::new();
        let mut owned = BTreeSet::new();
        let mut tick = tokio::time::interval(Duration::from_secs(1));
        loop {
            if !self.stopping.load(std::sync::atomic::Ordering::Acquire) {
                let ids = {
                    let _guard = self.admission.lock().await;
                    self.active()?.1
                };
                for id in ids {
                    if owned.len()
                        >= self
                            .config
                            .pin()
                            .resolved()
                            .config()
                            .runtime
                            .max_concurrent_jobs
                            .min(MAX_RUNNING)
                    {
                        break;
                    }
                    if !owned.contains(&id) && self.claim(&id).await? {
                        owned.insert(id.clone());
                        let service = self.clone();
                        running.spawn(async move {
                            let result = service.execute_job(id.clone()).await;
                            (id, result)
                        });
                    }
                }
            } else if running.is_empty() {
                break;
            }
            tokio::select! {
                Some(result)=running.join_next(),if !running.is_empty()=>{let (id,result)=result.map_err(|_|Error("JOB_ERROR"))?;owned.remove(&id);result?;},
                _=self.work.notified()=>{},
                _=tick.tick()=>{},
            }
        }
        Ok(())
    }
}
