use super::*;
use crate::adapter::native;
use crate::config::Notifier;
use std::os::unix::process::CommandExt;
use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::sync::OwnedSemaphorePermit;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    notification_id: String,
    idempotency_key: String,
    review_id: String,
    priority: Priority,
    summary: String,
    evidence_locator: String,
    action: String,
    external_message_id: Option<String>,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Capabilities {
    schema_version: u32,
    send: bool,
    #[serde(default)]
    update: bool,
    #[serde(default)]
    idempotency: bool,
    #[serde(default)]
    ack_callback: bool,
    #[serde(default)]
    rich_text: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Delivered {
    schema_version: u32,
    external_message_id: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Attempt {
    owner: String,
    route_sha256: String,
    started_at_ms: u64,
    deadline_ms: u64,
    envelope: Envelope,
    capabilities: Option<Capabilities>,
    error: Option<String>,
    #[serde(default = "first_attempt")]
    number: u32,
    #[serde(default)]
    first_started_at_ms: u64,
    #[serde(default)]
    next_retry_at_ms: Option<u64>,
    #[serde(default)]
    duplicate_risk: bool,
    #[serde(default)]
    transient_failures: Option<u32>,
}

fn first_attempt() -> u32 {
    1
}

fn policy_blocked(code: &str) -> bool {
    matches!(
        code,
        "CANCELLED" | "CHANNEL_DISABLED" | "NOTIFIER_ROUTE_CHANGED" | "READ_DENIED"
    )
}

// Delivery policy changes must not change a destination's idempotency identity.
fn route_hash(route: &Notifier) -> Result<String> {
    #[derive(Serialize)]
    struct Destination<'a> {
        kind: NotifierKind,
        enabled: bool,
        command: &'a [String],
        credential_ref: &'a Option<String>,
    }
    Ok(hash(&serde_json::to_vec(&Destination {
        kind: route.kind,
        enabled: true,
        command: &route.command,
        credential_ref: &route.credential_ref,
    })?))
}

#[test]
fn policy_does_not_change_existing_destination_fingerprints() {
    let mut route: Notifier = serde_json::from_value(
        json!({"kind":"stdio","command":["plugin"],"credential_ref":"env:EXAMPLE"}),
    )
    .unwrap();
    let original = hash(&serde_json::to_vec(&route).unwrap());
    assert_eq!(route_hash(&route).unwrap(), original);
    route.min_priority = Priority::Urgent;
    route.timeout = "2s".into();
    route.digest_seconds = 60;
    route.quiet_hours_utc = Some([22, 7]);
    assert_eq!(route_hash(&route).unwrap(), original);
}

#[test]
fn policy_cancellation_keeps_failure_budget_and_recovers_legacy_terminal_controls() {
    let original = json!({"status":"sending","generation":1,"config_generation":1,"external_message_id":"",
        "attempt":{"owner":"old","route_sha256":"route","started_at_ms":1,"deadline_ms":30001,
        "number":5,"first_started_at_ms":1,"transient_failures":4,"capabilities":null,"error":null,
        "envelope":{"notification_id":"notice","idempotency_key":"key","review_id":"review","priority":"normal",
        "summary":"review is open","evidence_locator":"review show review","action":"create","external_message_id":null}}});
    for code in [
        "CANCELLED",
        "CHANNEL_DISABLED",
        "NOTIFIER_ROUTE_CHANGED",
        "READ_DENIED",
    ] {
        let mut delivery: Delivery = serde_json::from_value(original.clone()).unwrap();
        delivery.fail(code, 100).unwrap();
        assert_eq!(delivery.status, "blocked");
        let attempt = delivery.attempt.as_mut().unwrap();
        assert_eq!(attempt.transient_failures, Some(4));
        assert!(attempt.next_retry_at_ms.is_none());
        attempt.number = 6;
        delivery.status = "sending".into();
        delivery.fail("NOTIFIER_FAILED", 200).unwrap();
        assert_eq!(delivery.status, "failed");
        assert_eq!(
            delivery.attempt.as_ref().unwrap().transient_failures,
            Some(5)
        );
        assert!(!delivery.retry_pending(200));

        let mut legacy = original.clone();
        legacy["status"] = json!("failed");
        legacy["attempt"]["error"] = json!(code);
        legacy["attempt"]
            .as_object_mut()
            .unwrap()
            .remove("transient_failures");
        let mut delivery: Delivery = serde_json::from_value(legacy).unwrap();
        assert!(delivery.recover_interrupted("new", 200).unwrap());
        assert_eq!(delivery.status, "blocked");
        assert_eq!(
            delivery.attempt.as_ref().unwrap().transient_failures,
            Some(4)
        );
    }
}

impl Delivery {
    fn fail(&mut self, code: &str, now: u64) -> Result<()> {
        let attempt = self.attempt.as_mut().ok_or(Error("NOTIFICATION_CORRUPT"))?;
        if self.status == "sending" && !attempt.capabilities.as_ref().is_some_and(|c| c.idempotency)
        {
            attempt.duplicate_risk = true;
        }
        let blocked = policy_blocked(code);
        let retryable = matches!(
            code,
            "NOTIFIER_FAILED"
                | "NOTIFIER_TIMEOUT"
                | "NOTIFIER_UNAVAILABLE"
                | "NOTIFIER_INTERRUPTED"
                | "IO_ERROR"
        );
        let failures = attempt
            .transient_failures
            .get_or_insert(attempt.number.saturating_sub(1));
        if retryable {
            *failures = failures.saturating_add(1);
        }
        attempt.next_retry_at_ms = if retryable && *failures < 5 {
            Some(now.saturating_add(1000 * (1 << failures.saturating_sub(1))))
        } else {
            None
        };
        self.status = if blocked {
            "blocked"
        } else if attempt.next_retry_at_ms.is_some() {
            "retry_wait"
        } else {
            "failed"
        }
        .into();
        attempt.error = Some(code.into());
        Ok(())
    }

    pub(super) fn recover_interrupted(&mut self, owner: &str, now: u64) -> Result<bool> {
        if self.status == "failed"
            && let Some(attempt) = self.attempt.as_mut()
            && attempt.transient_failures.is_none()
            && attempt.error.as_deref().is_some_and(policy_blocked)
        {
            attempt.transient_failures = Some(attempt.number.saturating_sub(1));
            self.status = "blocked".into();
            return Ok(true);
        }
        if matches!(self.status.as_str(), "preparing" | "sending")
            && self.attempt.as_ref().is_some_and(|a| a.owner != owner)
        {
            self.fail("NOTIFIER_INTERRUPTED", now)?;
            return Ok(true);
        }
        Ok(false)
    }

    pub(super) fn retry_pending(&self, now: u64) -> bool {
        self.status == "retry_wait"
            && self
                .attempt
                .as_ref()
                .and_then(|a| a.next_retry_at_ms)
                .is_some_and(|at| now < at)
    }
}

pub(in crate::daemon) struct Dispatch {
    id: String,
    channel: String,
    scope: Scope,
    route: Notifier,
    pub(super) delivery: Delivery,
    permit: Arc<OwnedSemaphorePermit>,
    deadline: tokio::time::Instant,
}

impl Dispatch {
    pub(super) fn claim(
        item: &Notification,
        channel: &str,
        config_generation: u64,
        route: Notifier,
        owner: &str,
        permit: OwnedSemaphorePermit,
    ) -> Result<Self> {
        let route_sha256 = route_hash(&route)?;
        let timeout_ms = route.timeout_ms()?;
        let deadline = tokio::time::Instant::now()
            .checked_add(Duration::from_millis(timeout_ms))
            .ok_or(Error("CLOCK_ERROR"))?;
        let previous = item
            .deliveries
            .get(channel)
            .filter(|d| {
                d.attempt
                    .as_ref()
                    .is_some_and(|a| a.route_sha256 == route_sha256)
            })
            .map(|d| d.external_message_id.clone())
            .filter(|s| !s.is_empty());
        let envelope = Envelope {
            notification_id: item.notification_id.clone(),
            idempotency_key: format!(
                "{}:{}:{}",
                item.review_id,
                item.generation,
                hash(format!("{channel}/{route_sha256}").as_bytes())
            ),
            review_id: item.review_id.clone(),
            priority: item.priority,
            summary: item.summary.clone(),
            evidence_locator: item.evidence_locator.clone(),
            action: if previous.is_some() {
                "update"
            } else {
                "create"
            }
            .into(),
            external_message_id: previous.clone(),
        };
        let started_at_ms = ipc::now_ms()?;
        if let Some(previous) = item.deliveries.get(channel)
            && matches!(previous.status.as_str(), "retry_wait" | "blocked")
            && let Some(prior) = &previous.attempt
            && prior.route_sha256 == route_sha256
        {
            let mut delivery = previous.clone();
            let attempt = delivery
                .attempt
                .as_mut()
                .ok_or(Error("NOTIFICATION_CORRUPT"))?;
            // Replay the accepted envelope even if the source has advanced meanwhile.
            attempt
                .transient_failures
                .get_or_insert(if previous.status == "retry_wait" {
                    attempt.number
                } else {
                    attempt.number.saturating_sub(1)
                });
            attempt.number = attempt.number.saturating_add(1);
            if attempt.first_started_at_ms == 0 {
                attempt.first_started_at_ms = attempt.started_at_ms;
            }
            attempt.owner = owner.into();
            attempt.started_at_ms = started_at_ms;
            attempt.deadline_ms = started_at_ms
                .checked_add(timeout_ms)
                .ok_or(Error("CLOCK_ERROR"))?;
            attempt.next_retry_at_ms = None;
            attempt.error = None;
            delivery.status = "preparing".into();
            delivery.config_generation = config_generation;
            return Ok(Self {
                id: item.notification_id.clone(),
                channel: channel.into(),
                scope: item.scope.clone(),
                route,
                delivery,
                permit: Arc::new(permit),
                deadline,
            });
        }
        let duplicate_risk = item
            .deliveries
            .get(channel)
            .and_then(|d| {
                d.attempt
                    .as_ref()
                    .map(|a| a.duplicate_risk || d.status != "sent")
            })
            .unwrap_or(false);
        Ok(Self {
            id: item.notification_id.clone(),
            channel: channel.into(),
            scope: item.scope.clone(),
            route,
            delivery: Delivery {
                status: "preparing".into(),
                generation: item.generation,
                config_generation,
                external_message_id: previous.unwrap_or_default(),
                attempt: Some(Attempt {
                    owner: owner.into(),
                    route_sha256,
                    started_at_ms,
                    deadline_ms: started_at_ms
                        .checked_add(timeout_ms)
                        .ok_or(Error("CLOCK_ERROR"))?,
                    envelope,
                    capabilities: None,
                    error: None,
                    number: 1,
                    first_started_at_ms: started_at_ms,
                    next_retry_at_ms: None,
                    duplicate_risk,
                    transient_failures: Some(0),
                }),
            },
            permit: Arc::new(permit),
            deadline,
        })
    }

    fn attempt(&self) -> Result<&Attempt> {
        self.delivery
            .attempt
            .as_ref()
            .ok_or(Error("NOTIFICATION_CORRUPT"))
    }
}

impl Service {
    async fn check_notification_dispatch(&self, dispatch: &Dispatch) -> Result<()> {
        let _guard = self.admission.lock().await;
        if self.stopping.load(std::sync::atomic::Ordering::Acquire) {
            return Err(Error("CANCELLED"));
        }
        Domain::new(&self.storage).permission(&jobs::context()?, Some(&dispatch.scope))?;
        let (_, route) = self
            .config
            .delivery_route(&dispatch.channel)
            .ok_or(Error("CHANNEL_DISABLED"))?;
        if route_hash(&route)? != dispatch.attempt()?.route_sha256 {
            return Err(Error("NOTIFIER_ROUTE_CHANGED"));
        }
        Ok(())
    }

    async fn save_notification_dispatch(&self, dispatch: &Dispatch) -> Result<()> {
        let _guard = self.admission.lock().await;
        let (revision, mut item) = self.notification(&dispatch.id)?;
        let existing = item
            .deliveries
            .get(&dispatch.channel)
            .and_then(|d| d.attempt.as_ref())
            .ok_or(Error("NOTIFICATION_CORRUPT"))?;
        if existing.owner != dispatch.attempt()?.owner
            || existing.envelope.idempotency_key != dispatch.attempt()?.envelope.idempotency_key
        {
            return Err(Error("NOTIFICATION_CORRUPT"));
        }
        item.deliveries
            .insert(dispatch.channel.clone(), dispatch.delivery.clone());
        self.save_notification(&item, Some(revision)).await
    }

    async fn call_notifier(&self, dispatch: &Dispatch, request: Value) -> Result<Vec<u8>> {
        self.check_notification_dispatch(dispatch).await?;
        let attempt = dispatch.attempt()?;
        let deadline = dispatch.deadline;
        if tokio::time::Instant::now() >= deadline {
            return Err(Error("NOTIFIER_TIMEOUT"));
        }
        let binary = native::resolve(Path::new(
            dispatch
                .route
                .command
                .first()
                .ok_or(Error("CONFIG_INVALID"))?,
        ))?;
        let credential = dispatch
            .route
            .credential_ref
            .as_ref()
            .map(|reference| {
                let name = reference
                    .strip_prefix("env:")
                    .ok_or(Error("CONFIG_INVALID"))?;
                std::env::var(name)
                    .ok()
                    .filter(|s| !s.is_empty())
                    .ok_or(Error("NOTIFIER_CREDENTIAL_UNAVAILABLE"))
            })
            .transpose()?;
        let mut bytes = serde_json::to_vec(&request)?;
        bytes.push(b'\n');
        if bytes.len() > 8192 {
            return Err(Error("NOTIFIER_REQUEST_TOO_LARGE"));
        }
        let directory = std::env::temp_dir().join(format!(
            "akasha-run-{}",
            hash(&serde_json::to_vec(&(
                &attempt.owner,
                &attempt.envelope.idempotency_key,
                &request
            ))?)
        ));
        fs::DirBuilder::new().mode(0o700).create(&directory)?;
        let mut command =
            match crate::adapter::guarded_command(Path::new("/proc/self/exe"), &binary, &directory)
            {
                Ok(command) => command,
                Err(error) => {
                    let _ = fs::remove_dir(&directory);
                    return Err(error.into());
                }
            };
        command
            .args(&dispatch.route.command[1..])
            .current_dir(&directory)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .process_group(0)
            .env_remove("AKASHA_NOTIFIER_CREDENTIAL");
        if let Some(secret) = credential {
            command.env("AKASHA_NOTIFIER_CREDENTIAL", secret);
        }
        let mut child = match tokio::process::Command::from(command)
            .kill_on_drop(false)
            .spawn()
        {
            Ok(child) => child,
            Err(_) => {
                let _ = fs::remove_dir(&directory);
                return Err(Error("NOTIFIER_UNAVAILABLE"));
            }
        };
        let group = native::ProcessGroup(
            rustix::process::Pid::from_raw(child.id().ok_or(Error("NOTIFIER_FAILED"))? as i32)
                .ok_or(Error("NOTIFIER_FAILED"))?,
            true,
        );
        let mut input = child.stdin.take().ok_or(Error("NOTIFIER_FAILED"))?;
        let stdout = child.stdout.take().ok_or(Error("NOTIFIER_FAILED"))?;
        let stderr = child.stderr.take().ok_or(Error("NOTIFIER_FAILED"))?;
        let response = {
            let exchange = async {
                let (_, out, _, ()) = tokio::try_join!(
                    async move {
                        input.write_all(&bytes).await?;
                        input.shutdown().await?;
                        drop(input);
                        Ok::<_, Error>(())
                    },
                    async { native::bounded(stdout).await.map_err(Error::from) },
                    async { native::bounded(stderr).await.map_err(Error::from) },
                    async { native::wait_for_exit(&child).await.map_err(Error::from) },
                )?;
                Ok::<_, Error>(out)
            };
            tokio::pin!(exchange);
            let mut tick = tokio::time::interval(Duration::from_millis(50));
            loop {
                tokio::select! {
                    result = &mut exchange => break result,
                    _ = tokio::time::sleep_until(deadline) => break Err(Error("NOTIFIER_TIMEOUT")),
                    _ = tick.tick() => if let Err(error) = self.check_notification_dispatch(dispatch).await { break Err(error); },
                }
            }
        };
        drop(group);
        let permit = dispatch.permit.clone();
        let status = native::reap(child, true, move || {
            let _ = fs::remove_dir_all(directory);
            drop(permit);
        })
        .wait()
        .await;
        let bytes = response?;
        if !status.is_some_and(|s| s.success()) {
            return Err(Error("NOTIFIER_FAILED"));
        }
        Ok(bytes)
    }

    pub(in crate::daemon) async fn deliver_notification(
        &self,
        mut dispatch: Dispatch,
    ) -> Result<()> {
        let sent = async {
            let caps: Capabilities = if let Some(caps) = dispatch.attempt()?.capabilities.clone() { caps } else { serde_json::from_slice(&self.call_notifier(&dispatch, json!({"schema_version":1,"op":"capabilities"})).await?).map_err(|_| Error("NOTIFIER_PROTOCOL_INVALID"))? };
            if caps.schema_version != 1 || !caps.send { return Err(Error("NOTIFIER_UNSUPPORTED")); }
            let attempt = dispatch.delivery.attempt.as_mut().ok_or(Error("NOTIFICATION_CORRUPT"))?;
            if !caps.update { attempt.envelope.action = "create".into(); attempt.envelope.external_message_id = None; }
            attempt.capabilities = Some(caps);
            dispatch.delivery.status = "sending".into();
            self.save_notification_dispatch(&dispatch).await?;
            let result: Delivered = serde_json::from_slice(&self.call_notifier(&dispatch, json!({"schema_version":1,"op":"deliver","notification":dispatch.attempt()?.envelope})).await?).map_err(|_| Error("NOTIFIER_PROTOCOL_INVALID"))?;
            if result.schema_version != 1 || !ipc::identifier(&result.external_message_id) {
                return Err(Error("NOTIFIER_PROTOCOL_INVALID"));
            }
            Ok(result.external_message_id)
        }.await;
        match sent {
            Ok(id) => {
                dispatch.delivery.status = "sent".into();
                dispatch.delivery.external_message_id = id;
            }
            Err(error) => {
                dispatch.delivery.fail(error.0, ipc::now_ms()?)?;
            }
        }
        self.save_notification_dispatch(&dispatch).await?;
        self.wake_jobs();
        Ok(())
    }
}
