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
}

pub(in crate::daemon) struct Dispatch {
    id: String,
    channel: String,
    scope: Scope,
    route: Notifier,
    pub(super) delivery: Delivery,
    permit: Arc<OwnedSemaphorePermit>,
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
        let route_sha256 = hash(&serde_json::to_vec(&route)?);
        let previous = item
            .deliveries
            .get(channel)
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
                        .checked_add(30_000)
                        .ok_or(Error("CLOCK_ERROR"))?,
                    envelope,
                    capabilities: None,
                    error: None,
                }),
            },
            permit: Arc::new(permit),
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
        if hash(&serde_json::to_vec(&route)?) != dispatch.attempt()?.route_sha256 {
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
        let remaining = attempt
            .deadline_ms
            .checked_sub(ipc::now_ms()?)
            .filter(|n| *n > 0)
            .ok_or(Error("NOTIFIER_TIMEOUT"))?;
        let deadline = tokio::time::Instant::now() + Duration::from_millis(remaining);
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
            let caps: Capabilities = serde_json::from_slice(&self.call_notifier(&dispatch, json!({"schema_version":1,"op":"capabilities"})).await?)?;
            if caps.schema_version != 1 || !caps.send { return Err(Error("NOTIFIER_UNSUPPORTED")); }
            let attempt = dispatch.delivery.attempt.as_mut().ok_or(Error("NOTIFICATION_CORRUPT"))?;
            if !caps.update { attempt.envelope.action = "create".into(); attempt.envelope.external_message_id = None; }
            attempt.capabilities = Some(caps);
            dispatch.delivery.status = "sending".into();
            self.save_notification_dispatch(&dispatch).await?;
            let result: Delivered = serde_json::from_slice(&self.call_notifier(&dispatch, json!({"schema_version":1,"op":"deliver","notification":dispatch.attempt()?.envelope})).await?)?;
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
                dispatch.delivery.status = if matches!(
                    error.0,
                    "CANCELLED" | "CHANNEL_DISABLED" | "NOTIFIER_ROUTE_CHANGED" | "READ_DENIED"
                ) {
                    "blocked"
                } else {
                    "failed"
                }
                .into();
                dispatch
                    .delivery
                    .attempt
                    .as_mut()
                    .ok_or(Error("NOTIFICATION_CORRUPT"))?
                    .error = Some(error.0.into());
            }
        }
        self.save_notification_dispatch(&dispatch).await?;
        self.wake_jobs();
        Ok(())
    }
}
