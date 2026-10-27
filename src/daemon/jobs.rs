use super::*;
#[cfg(test)]
use crate::capture::ReconcileReport;
use crate::capture::{Capture, PendingReason};
use crate::domain::{Domain, ReadContext, Scope};
use crate::ipc::CaptureInput;
use crate::storage::{Change, CommitRequest, RecordKey, RecordKind, hash};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeSet;
#[path = "models.rs"]
mod models;

const MAX_ACTIVE: usize = 256;
const MAX_RUNNING: usize = 16;
const MAX_ATTEMPTS: u32 = 3;

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixture(PathBuf);
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    async fn fixture(name: &str, extra: &str) -> (Fixture, Arc<Service>, CaptureInput) {
        let fixture =
            Fixture(std::env::temp_dir().join(format!("akasha-job-{name}-{}", std::process::id())));
        private_dir(&fixture.0).unwrap();
        let input = format!("[access]\nread_roots=[{:?}]\n{extra}", fixture.0);
        std::fs::write(fixture.0.join("config.toml"), &input).unwrap();
        let initial = crate::config::resolve(
            &input,
            None,
            "",
            &crate::config::Paths {
                data_dir: fixture.0.join("data"),
                socket_path: fixture.0.join("run/socket"),
            },
        )
        .unwrap();
        let storage = Arc::new(Storage::open(&initial).await.unwrap());
        let manager = configuration::open(&storage, initial).await.unwrap();
        let service = Arc::new(Service {
            storage,
            config: manager,
            config_path: fixture.0.join("config.toml"),
            profile: None,
            admission: Mutex::new(()),
            operations: Mutex::new(JoinSet::new()),
            last_config: Mutex::new(None),
            stop: Notify::new(),
            work: Notify::new(),
            stopping: std::sync::atomic::AtomicBool::new(false),
            instance: "new-daemon".into(),
            providers: crate::provider::Client::new().unwrap(),
            batch_gate: Mutex::new(None),
        });
        let scope = owner_scope();
        Domain::new(&service.storage)
            .ensure_local_reader(&scope.user, &scope, 1)
            .await
            .unwrap();
        let binding: CaptureInput = serde_json::from_value(json!({
            "source":{"adapter":"fixture","profile":"test","session":"session","thread":"thread","agent":"agent"},
            "scope":scope,"stream":"stream","coverage":"available_payloads"
        })).unwrap();
        (fixture, service, binding)
    }

    #[tokio::test]
    async fn serve_accept_error_stops_and_closes_storage() {
        let (_fixture, service, _) = fixture("accept-error", "").await;
        let error = serve(service.clone(), || async {
            Err(std::io::Error::other("accept failure"))
        })
        .await
        .unwrap_err();
        let stopped = service.stopping.load(std::sync::atomic::Ordering::Acquire);
        let write_after_return = service
            .storage
            .commit(&CommitRequest {
                request_id: "after-return".into(),
                config_generation: 1,
                policy_revision: None,
                changes: vec![Change {
                    key: job_key("after-return"),
                    expected_revision: None,
                    content: "probe".into(),
                }],
            })
            .await;
        // Collect the old detached scheduler even when the RED assertion below fails.
        service
            .stopping
            .store(true, std::sync::atomic::Ordering::Release);
        service.wake_jobs();
        tokio::time::timeout(Duration::from_secs(2), async {
            while Arc::strong_count(&service) > 1 {
                tokio::task::yield_now().await;
            }
        })
        .await
        .unwrap();
        let _ = service.storage.close().await;
        assert_eq!(error.0, "IO_ERROR");
        assert!(stopped, "accept error bypassed daemon drain");
        assert!(
            write_after_return.is_err(),
            "Storage was still writable after run returned"
        );
    }

    #[tokio::test]
    async fn disconnected_mutation_owners_are_bounded_and_drained() {
        let (fixture, service, binding) = fixture("owner-cap", "").await;
        let guard = service.admission.lock().await;
        for n in 0..16 {
            let mut request = Box::pin(service.submit(
                format!("owner-{n}"),
                binding.clone(),
                fixture.0.join("missing.jsonl"),
            ));
            std::future::poll_fn(|cx| {
                assert!(std::future::Future::poll(request.as_mut(), cx).is_pending());
                std::task::Poll::Ready(())
            })
            .await;
            drop(request);
        }
        assert_eq!(
            service
                .submit("over-cap".into(), binding, fixture.0.join("missing.jsonl"))
                .await
                .unwrap_err()
                .0,
            "RESOURCE_BUSY"
        );
        let mut closing = Box::pin(service.close(Ok(())));
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(closing.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        drop(guard);
        closing.await.unwrap();
        assert!(service.operations.lock().await.is_empty());
    }

    #[tokio::test]
    async fn shutdown_collects_reload_after_its_client_timeout() {
        let (fixture, service, _) = fixture("reload-owner", "").await;
        std::fs::write(
            &service.config_path,
            format!(
                "[access]\nread_roots=[{:?}]\n[timeouts]\nrunner='2200s'\n",
                fixture.0
            ),
        )
        .unwrap();
        let guard = service.admission.lock().await;
        assert!(
            tokio::time::timeout(Duration::from_millis(10), service.reload())
                .await
                .is_err()
        );
        let mut closing = Box::pin(service.close(Err(Error("PRIMARY_ERROR"))));
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(closing.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        assert_eq!(service.config.pin().generation(), 1);
        assert!(service.stopping.load(std::sync::atomic::Ordering::Acquire));
        drop(guard);
        assert_eq!(closing.await.unwrap_err().0, "PRIMARY_ERROR");
        assert!(service.operation(async { Ok(json!(null)) }).await.is_err());
        assert_eq!(service.config.pin().generation(), 2);
        let initial = service.config.pin().resolved().clone();
        drop(service);
        let storage = Storage::open(&initial).await.unwrap();
        let saved = configuration::load(&storage, "daemon/config")
            .unwrap()
            .unwrap();
        assert_eq!(saved["generation"], 2);
        let restored = crate::config::ConfigManager::restore(saved, |_| Ok(())).unwrap();
        assert_eq!(
            restored.pin().resolved().timeouts().runner,
            Duration::from_secs(2200)
        );
        storage.close().await.unwrap();
    }

    #[tokio::test]
    async fn scheduler_error_collects_a_sibling_physical_batch() {
        let (fixture, service, binding) = fixture("sibling-owner", "").await;
        let path = fixture.0.join("capture.jsonl");
        std::fs::write(&path, "").unwrap();
        service
            .submit("physical".into(), binding, path)
            .await
            .unwrap();
        assert!(service.claim("physical", 0).await.unwrap());
        let (reached, ready) = tokio::sync::oneshot::channel();
        let (resume, release) = tokio::sync::oneshot::channel();
        *service.batch_gate.lock().await = Some(BatchGate {
            reached,
            resume: release,
        });
        let mut running = JoinSet::new();
        let worker = service.clone();
        running.spawn(async move {
            (
                "physical".into(),
                worker.execute_job("physical".into()).await,
            )
        });
        ready.await.unwrap();
        running.spawn(async { ("failed".into(), Err(Error("PRIMARY_ERROR"))) });
        let (_, failure) = running.join_next().await.unwrap().unwrap();
        running.spawn(async { ("another".into(), Err(Error("SECONDARY_ERROR"))) });
        let mut draining = Box::pin(service.drain_jobs(running, failure));
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(draining.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        assert_eq!(service.job("physical").unwrap().1.state, "running");
        resume.send(()).unwrap();
        assert_eq!(draining.await.unwrap_err().0, "PRIMARY_ERROR");
        let terminal = service.job("physical").unwrap().1;
        assert_eq!(terminal.state, "succeeded");
        assert!(terminal.lease_owner.is_none());
        assert!(terminal.result.is_some());
        assert!(service.active().unwrap().1.is_empty());
        service.close(Ok(())).await.unwrap();
    }

    #[tokio::test]
    async fn completed_scheduler_error_is_collected_before_storage_close() {
        let (_fixture, service, _) = fixture("scheduler-error", "").await;
        service
            .storage
            .commit(&CommitRequest {
                request_id: "corrupt-active".into(),
                config_generation: 1,
                policy_revision: None,
                changes: vec![Change {
                    key: active_key(),
                    expected_revision: None,
                    content: "[\"missing\"]".into(),
                }],
            })
            .await
            .unwrap();
        let error = serve(service.clone(), std::future::pending)
            .await
            .unwrap_err();
        assert_eq!(error.0, "JOB_NOT_FOUND");
        assert!(service.stopping.load(std::sync::atomic::Ordering::Acquire));
        assert!(
            service
                .storage
                .commit(&CommitRequest {
                    request_id: "after-close".into(),
                    config_generation: 1,
                    policy_revision: None,
                    changes: vec![Change {
                        key: job_key("after-close"),
                        expected_revision: None,
                        content: "probe".into()
                    }],
                })
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn claim_checks_reloaded_cap_after_waiting_for_admission() {
        let (fixture, service, binding) =
            fixture("cap", "[runtime]\nmax_concurrent_jobs=2\n").await;
        for id in ["owned", "waiting"] {
            service
                .submit(id.into(), binding.clone(), fixture.0.join("missing.jsonl"))
                .await
                .unwrap();
        }
        assert!(service.claim("owned", 0).await.unwrap());
        let guard = service.admission.lock().await;
        let old_permission = 1 < service
            .config
            .pin()
            .resolved()
            .config()
            .runtime
            .max_concurrent_jobs;
        assert!(old_permission);
        let mut claimant = Box::pin(service.claim("waiting", 1));
        std::future::poll_fn(|cx| {
            assert!(std::future::Future::poll(claimant.as_mut(), cx).is_pending());
            std::task::Poll::Ready(())
        })
        .await;
        let candidate = format!(
            "[access]\nread_roots=[{:?}]\n[runtime]\nmax_concurrent_jobs=1\n",
            fixture.0
        );
        service.config.reload(&candidate, None, |_| Ok(())).unwrap();
        configuration::save(&service.storage, &service.config.checkpoint().unwrap())
            .await
            .unwrap();
        drop(guard);
        assert!(
            !claimant.await.unwrap(),
            "claim used the old concurrency cap"
        );
        let waiting = service.job("waiting").unwrap().1;
        assert_eq!(waiting.state, "queued");
        assert!(waiting.started_at_ms.is_none());
        assert_eq!(waiting.config_generation, 1);
        let (revision, owned) = service.job("owned").unwrap();
        service
            .finish_job(owned, revision, "cancelled", Some("CANCELLED"))
            .await
            .unwrap();
        assert!(service.claim("waiting", 0).await.unwrap());
        service.storage.close().await.unwrap();
    }

    #[tokio::test]
    async fn recovered_jobs_keep_retry_exhaustion_and_original_deadlines() {
        let (fixture, service, binding) = fixture("recovery", "").await;
        for (id, expected) in [("attempts", "RETRY_EXHAUSTED"), ("deadline", "JOB_TIMEOUT")] {
            service
                .submit(id.into(), binding.clone(), fixture.0.join("missing.jsonl"))
                .await
                .unwrap();
            assert!(service.claim(id, 0).await.unwrap());
            let (revision, mut interrupted) = service.job(id).unwrap();
            if id == "attempts" {
                interrupted.attempt = MAX_ATTEMPTS;
            } else {
                interrupted.started_at_ms = Some(1);
                interrupted.execution_deadline_ms = Some(1 + interrupted.execution_budget_ms);
            }
            interrupted.lease_owner = Some("previous-daemon".into());
            service
                .save_job(&interrupted, Some(revision), vec![])
                .await
                .unwrap();
            service.recover().await.unwrap();
            let recovered = service.job(id).unwrap().1;
            assert_eq!(recovered.attempt, interrupted.attempt);
            assert_eq!(
                recovered.execution_deadline_ms,
                interrupted.execution_deadline_ms
            );
            assert_eq!(recovered.started_at_ms, interrupted.started_at_ms);
            assert!(recovered.lease_owner.is_none());
            assert!(!service.claim(id, 0).await.unwrap());
            let terminal = service.job(id).unwrap().1;
            assert_eq!(terminal.state, "failed");
            assert_eq!(terminal.error.as_deref(), Some(expected));
            assert!(service.active().unwrap().1.is_empty());
        }
        service.storage.close().await.unwrap();
    }

    #[tokio::test(start_paused = true)]
    async fn final_physical_batch_obeys_the_original_execution_deadline() {
        let (fixture, service, binding) = fixture("final-deadline", "").await;
        let path = fixture.0.join("capture.jsonl");
        std::fs::write(&path, "").unwrap();
        for (id, expired) in [("before", false), ("after", true)] {
            let mut binding = binding.clone();
            binding.stream = id.into();
            service
                .submit(id.into(), binding.clone(), path.clone())
                .await
                .unwrap();
            assert!(service.claim(id, 0).await.unwrap());
            let original = service.job(id).unwrap().1;
            let (reached, ready) = tokio::sync::oneshot::channel();
            let (resume, release) = tokio::sync::oneshot::channel();
            *service.batch_gate.lock().await = Some(BatchGate {
                reached,
                resume: release,
            });
            let worker = tokio::spawn(service.clone().execute_job(id.into()));
            let end = ready.await.unwrap();
            let until_end = end.saturating_duration_since(tokio::time::Instant::now());
            assert!(until_end > Duration::from_millis(1));
            tokio::time::advance(if expired {
                until_end + Duration::from_millis(1)
            } else {
                until_end - Duration::from_millis(1)
            })
            .await;
            assert!(!worker.is_finished());
            resume.send(()).unwrap();
            worker.await.unwrap().unwrap();
            let terminal = service.job(id).unwrap().1;
            assert_eq!(terminal.state, if expired { "failed" } else { "succeeded" });
            assert_eq!(terminal.error.as_deref(), expired.then_some("JOB_TIMEOUT"));
            assert_eq!(
                terminal.execution_deadline_ms,
                original.execution_deadline_ms
            );
            assert_eq!(terminal.started_at_ms, original.started_at_ms);
            let report: ReconcileReport = serde_json::from_value(terminal.result.unwrap()).unwrap();
            assert_ne!(
                report.checkpoint.checkpoint.pending,
                Some(PendingReason::Backlog)
            );
            assert_eq!(
                Capture::new(&service.storage)
                    .checkpoint(&binding.binding().unwrap())
                    .unwrap()
                    .unwrap()
                    .revision,
                report.checkpoint.revision
            );
            assert!(service.active().unwrap().1.is_empty());
        }
        service.storage.close().await.unwrap();
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Job {
    schema_version: u32,
    id: String,
    request_sha256: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    binding: Option<CaptureInput>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    path: Option<PathBuf>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    model_plan: Option<crate::provider::Plan>,
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
    result: Option<Value>,
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
            || !matches!(
                (&job.binding, &job.path, &job.model_plan),
                (Some(_), Some(_), None) | (None, None, Some(_))
            )
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
        let scope = job
            .binding
            .as_ref()
            .map(|b| b.scope.clone())
            .unwrap_or_else(owner_scope);
        Domain::new(&self.storage).permission(&context()?, Some(&scope))?;
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
        self.submit_job(id, Some(binding), Some(path), None).await
    }
    pub(super) async fn submit_model(
        self: &Arc<Self>,
        id: String,
        plan: crate::provider::Plan,
    ) -> Result<Value> {
        self.submit_job(id, None, None, Some(plan)).await
    }
    async fn submit_job(
        self: &Arc<Self>,
        id: String,
        binding: Option<CaptureInput>,
        path: Option<PathBuf>,
        model_plan: Option<crate::provider::Plan>,
    ) -> Result<Value> {
        let service = self.clone();
        self.operation(async move {
            let _guard = service.admission.lock().await;
            if !ipc::identifier(&id) {
                return Err(Error("INVALID_REQUEST"));
            }
            let path = path
                .as_deref()
                .map(|p| service.allowed_path(p))
                .transpose()?;
            if let Some(binding) = &binding {
                binding.binding()?;
            }
            let scope = binding
                .as_ref()
                .map(|b| b.scope.clone())
                .unwrap_or_else(owner_scope);
            Domain::new(&service.storage).permission(&context()?, Some(&scope))?;
            let fingerprint = if let Some(plan) = &model_plan {
                hash(&serde_json::to_vec(&("model", plan))?)
            } else {
                hash(&serde_json::to_vec(&(&binding, &path))?)
            };
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
            let budget = match &model_plan {
                Some(plan) => plan.validate(snapshot.resolved()).map_err(|e| Error(e.0))?,
                None => snapshot.resolved().timeouts().tools,
            };
            let execution_budget_ms =
                u64::try_from(budget.as_millis()).map_err(|_| Error("CONFIG_INVALID"))?;
            let job = Job {
                schema_version: 1,
                id: id.clone(),
                request_sha256: fingerprint,
                binding,
                path,
                model_plan,
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
            service.wake_jobs();
            Ok(serde_json::to_value(job)?)
        })
        .await
    }
    pub(super) async fn show_job(&self, id: &str) -> Result<Value> {
        let _guard = self.admission.lock().await;
        let (revision, job) = self.job(id)?;
        self.authorize_job(&job)?;
        let mut value = serde_json::to_value(&job)?;
        if job.model_plan.is_some()
            && let Some(result) = &job.result
        {
            value["result"] = models::load_result(&self.storage, result, revision)?;
        }
        Ok(value)
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
        self.operation(async move {
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
                service.wake_jobs();
            }
            Ok(serde_json::to_value(job)?)
        })
        .await
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
        self.wake_jobs();
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
                if job.model_plan.is_some() {
                    self.finish_job(job, revision, "failed", Some("PROVIDER_INTERRUPTED"))
                        .await?;
                    continue;
                }
                job.state = "queued".into();
                job.lease_owner = None;
                job.lease_until_ms = None;
                self.save_job(&job, Some(revision), vec![]).await?;
            }
        }
        Ok(())
    }
    async fn claim(&self, id: &str, running: usize) -> Result<bool> {
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
        let snapshot = self.config.pin();
        let runtime = &snapshot.resolved().config().runtime;
        if running >= runtime.max_concurrent_jobs.min(MAX_RUNNING)
            || runtime.paused
            || self.stopping.load(std::sync::atomic::Ordering::Acquire)
            || now < job.retry_at_ms
        {
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
        if self.job(&id)?.1.model_plan.is_some() {
            return self.execute_model(id, remaining).await;
        }
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
                .and_then(|_| self.allowed_path(job.path.as_deref().ok_or(Error("JOB_CORRUPT"))?));
            if let Err(error) = checked {
                return self
                    .finish_job(job, revision, "failed", Some(error.0))
                    .await;
            }
            let storage = self.storage.clone();
            let binding = job
                .binding
                .as_ref()
                .ok_or(Error("JOB_CORRUPT"))?
                .binding()?;
            let path = job.path.clone().ok_or(Error("JOB_CORRUPT"))?;
            let generation = job.config_generation;
            let runtime = tokio::runtime::Handle::current();
            #[cfg(test)]
            let gate = self.batch_gate.lock().await.take();
            let result = tokio::task::spawn_blocking(move || {
                let result = runtime.block_on(
                    Capture::new(&storage).reconcile_jsonl(&binding, &path, 1, generation),
                );
                #[cfg(test)]
                if let Some(gate) = gate {
                    gate.reached.send(end).unwrap();
                    gate.resume.blocking_recv().unwrap();
                }
                result
            })
            .await
            .map_err(|_| Error("JOB_ERROR"))?;
            match result {
                Ok(report) => {
                    let more = report.checkpoint.checkpoint.pending == Some(PendingReason::Backlog);
                    job.result = Some(serde_json::to_value(report)?);
                    if tokio::time::Instant::now() >= end {
                        return self
                            .finish_job(job, revision, "failed", Some("JOB_TIMEOUT"))
                            .await;
                    }
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
                    self.wake_jobs();
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
        let outcome = async {
        loop {
            if !self.stopping.load(std::sync::atomic::Ordering::Acquire) {
                let ids = {
                    let _guard = self.admission.lock().await;
                    self.active()?.1
                };
                for id in ids {
                    if !owned.contains(&id) && self.claim(&id, owned.len()).await? {
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
        }.await;
        self.drain_jobs(running, outcome).await
    }

    async fn drain_jobs(
        &self,
        mut running: JoinSet<(String, Result<()>)>,
        mut outcome: Result<()>,
    ) -> Result<()> {
        if outcome.is_err() {
            self.stopping
                .store(true, std::sync::atomic::Ordering::Release);
            self.stop.notify_one();
            self.wake_jobs();
        }
        while let Some(result) = running.join_next().await {
            let result = result
                .map_err(|_| Error("JOB_ERROR"))
                .and_then(|(_, result)| result);
            outcome = outcome.and(result);
        }
        outcome
    }
}
