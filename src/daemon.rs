use std::fs::{self, File, OpenOptions};
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use serde_json::json;
use tokio::net::{UnixListener, UnixStream};
use tokio::sync::{Mutex, Notify, Semaphore};
use tokio::task::JoinSet;

use crate::config::ResolvedConfig;
use crate::ipc::{self, Command, Error, Request, Result};
use crate::storage::Storage;
mod codex;
mod configuration;
mod jobs;

pub(crate) fn private_dir(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|p| matches!(p, std::path::Component::ParentDir))
    {
        return Err(Error("UNSAFE_PATH"));
    }
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || meta.uid() != rustix::process::geteuid().as_raw()
        || meta.mode() & 0o777 != 0o700
    {
        return Err(Error("UNSAFE_PATH"));
    }
    Ok(())
}

struct SocketOwner {
    path: PathBuf,
    dev: u64,
    ino: u64,
    _lock: File,
}
impl Drop for SocketOwner {
    fn drop(&mut self) {
        if fs::symlink_metadata(&self.path)
            .is_ok_and(|m| m.dev() == self.dev && m.ino() == self.ino)
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}
fn listen(path: &Path) -> Result<(UnixListener, SocketOwner)> {
    let parent = path.parent().ok_or(Error("UNSAFE_PATH"))?;
    private_dir(parent)?;
    let lock = OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(parent.join("daemon.lock"))?;
    let meta = lock.metadata()?;
    if !meta.is_file()
        || meta.uid() != rustix::process::geteuid().as_raw()
        || meta.mode() & 0o777 != 0o600
        || meta.nlink() != 1
    {
        return Err(Error("UNSAFE_PATH"));
    }
    lock.try_lock()
        .map_err(|_| Error("DAEMON_ALREADY_RUNNING"))?;
    match fs::symlink_metadata(path) {
        Ok(meta)
            if meta.file_type().is_socket()
                && meta.uid() == rustix::process::geteuid().as_raw() =>
        {
            fs::remove_file(path)?
        }
        Ok(_) => return Err(Error("UNSAFE_PATH")),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => (),
        Err(e) => return Err(e.into()),
    }
    let listener = UnixListener::bind(path)?;
    let meta = fs::symlink_metadata(path)?;
    let owner = SocketOwner {
        path: path.into(),
        dev: meta.dev(),
        ino: meta.ino(),
        _lock: lock,
    };
    fs::set_permissions(path, fs::Permissions::from_mode(0o600))?;
    Ok((listener, owner))
}

#[cfg(test)]
struct BatchGate {
    reached: tokio::sync::oneshot::Sender<tokio::time::Instant>,
    resume: tokio::sync::oneshot::Receiver<()>,
}

struct Service {
    storage: Arc<Storage>,
    config: crate::config::ConfigManager<()>,
    config_path: PathBuf,
    profile: Option<String>,
    admission: Mutex<()>,
    operations: Mutex<JoinSet<()>>,
    last_config: Mutex<Option<(String, serde_json::Value)>>,
    stop: Notify,
    work: Notify,
    stopping: std::sync::atomic::AtomicBool,
    instance: String,
    providers: crate::provider::Client,
    search_index: Mutex<Option<Arc<crate::retrieval::Index>>>,
    index_builder: Mutex<()>,
    index_epoch: std::sync::atomic::AtomicU64,
    #[cfg(test)]
    batch_gate: Mutex<Option<BatchGate>>,
}
impl Service {
    fn wake_jobs(&self) {
        self.work.notify_waiters();
        self.work.notify_one();
    }
    async fn operation(
        &self,
        future: impl std::future::Future<Output = Result<serde_json::Value>> + Send + 'static,
    ) -> Result<serde_json::Value> {
        let (send, receive) = tokio::sync::oneshot::channel();
        {
            let mut owners = self.operations.lock().await;
            while let Some(result) = owners.try_join_next() {
                result.map_err(|_| Error("JOB_ERROR"))?;
            }
            if self.stopping.load(std::sync::atomic::Ordering::Acquire) || owners.len() >= 16 {
                return Err(Error("RESOURCE_BUSY"));
            }
            owners.spawn(async move {
                let _ = send.send(future.await);
            });
        }
        receive.await.map_err(|_| Error("JOB_ERROR"))?
    }

    async fn close(&self, mut outcome: Result<()>) -> Result<()> {
        self.stopping
            .store(true, std::sync::atomic::Ordering::Release);
        self.wake_jobs();
        let mut owners = self.operations.lock().await;
        while let Some(result) = owners.join_next().await {
            outcome = outcome.and(result.map_err(|_| Error("JOB_ERROR")));
        }
        self.search_index.lock().await.take();
        let closed = self.storage.close().await.map_err(Error::from);
        outcome.and(closed)
    }

    async fn dispatch(
        self: &Arc<Self>,
        request_id: &str,
        command: Command,
    ) -> Result<serde_json::Value> {
        match command {
            Command::Codex { event } => self.codex(event).await,
            Command::Status => Ok(
                json!({"state":"running","pid":std::process::id(),"storage_seq":self.storage.sequence()?}),
            ),
            Command::Doctor => Ok(
                json!({"transport":"unix","peer_uid":rustix::process::geteuid().as_raw(),"storage_seq":self.storage.sequence()?,"local_long_timeout_verified":false}),
            ),
            Command::Stop => {
                self.stopping
                    .store(true, std::sync::atomic::Ordering::Release);
                self.stop.notify_one();
                Ok(json!({"state":"draining"}))
            }
            Command::ConfigStatus => self.config_status().await,
            Command::ConfigShow => Ok(self.config_status().await?["effective"].clone()),
            Command::ConfigValidate => self.validate(),
            Command::ConfigReload => self.reload().await,
            Command::Reconcile {
                binding,
                path,
                idempotency_key,
            } => self.submit(idempotency_key, binding, path).await,
            Command::ModelRun {
                plan,
                idempotency_key,
            } => self.submit_model(idempotency_key, plan).await,
            Command::RetrievalRun {
                plan,
                idempotency_key,
            } => self.submit_retrieval(idempotency_key, plan).await,
            Command::OriginalShow { range } => {
                let service = self.clone();
                let request_id = request_id.to_owned();
                self.operation(async move {
                    let _guard = service.admission.lock().await;
                    let config = service.config.pin();
                    Ok(serde_json::to_value(
                        crate::domain::Domain::new(&service.storage)
                            .original_range(
                                &jobs::context()?,
                                &range,
                                &request_id,
                                config.resolved().config().retrieval.max_original_chars,
                                config.generation(),
                            )
                            .await?,
                    )?)
                })
                .await
            }
            Command::IndexStatus => {
                crate::domain::Domain::new(&self.storage)
                    .permission(&jobs::context()?, Some(&jobs::owner_scope()))?;
                let mut value = serde_json::to_value(crate::retrieval::Index::status(
                    &self.storage,
                    self.config.pin().resolved(),
                )?)?;
                value["building"] =
                    json!(self.index_epoch.load(std::sync::atomic::Ordering::Acquire) % 2 == 1);
                value["active_vector"] =
                    serde_json::to_value(crate::retrieval::active_vector(&self.storage)?)?;
                value["searched_content"] = json!("active_memories");
                Ok(value)
            }
            Command::JobShow { id } => self.show_job(&id).await,
            Command::JobCancel { id } => self.cancel_job(id).await,
            Command::JobList { after } => self.list_jobs(after.as_deref()).await,
            Command::MemoryShow { id, at } => Ok(serde_json::to_value(
                crate::domain::Domain::new(&self.storage)
                    .get(&jobs::context()?, &id, at)
                    .await?,
            )?),
            Command::MemoryHistory {
                id,
                from,
                to,
                cursor,
            } => Ok(serde_json::to_value(
                crate::domain::Domain::new(&self.storage)
                    .history(
                        &jobs::context()?,
                        &id,
                        from..=to.unwrap_or(self.storage.sequence()?),
                        cursor.as_ref(),
                        read_budget(),
                    )
                    .await?,
            )?),
            Command::StateShow { scope, at, cursor } => Ok(serde_json::to_value(
                crate::domain::Domain::new(&self.storage)
                    .state(
                        &jobs::context()?,
                        &scope,
                        at,
                        cursor.as_ref(),
                        read_budget(),
                    )
                    .await?,
            )?),
            Command::StateDiff {
                scope,
                from,
                to,
                cursor,
            } => Ok(serde_json::to_value(
                crate::domain::Domain::new(&self.storage)
                    .diff(
                        &jobs::context()?,
                        &scope,
                        from,
                        to,
                        cursor.as_ref(),
                        read_budget(),
                    )
                    .await?,
            )?),
            Command::ReceiptShow { id } => Ok(serde_json::to_value(
                crate::domain::Domain::new(&self.storage)
                    .recall_receipt(&jobs::context()?, &id)
                    .await?,
            )?),
            Command::ReceiptReplay { id } => {
                let receipt = crate::domain::Domain::new(&self.storage)
                    .recall_receipt(&jobs::context()?, &id)
                    .await?
                    .data;
                Ok(
                    json!({"receipt_id":id,"output":receipt.output,"output_sha256":receipt.record.output_sha256,"already_delivered":matches!(receipt.record.delivery,crate::domain::DeliveryPhase::Delivered {..})}),
                )
            }
            Command::AccessSet { scope } => {
                let service = self.clone();
                let request_id = request_id.to_owned();
                self.operation(async move {
                    let _guard = service.admission.lock().await;
                    if scope
                        .as_ref()
                        .is_some_and(|scope| !jobs::owner_scope().contains(scope))
                    {
                        Err(Error("SCOPE_DENIED"))
                    } else {
                        let changed = serde_json::to_value(
                            crate::domain::Domain::new(&service.storage)
                                .change_local_reader(
                                    &request_id,
                                    &jobs::owner_scope().user,
                                    scope.as_ref(),
                                    service.config.pin().generation(),
                                )
                                .await?,
                        )?;
                        service.wake_jobs();
                        Ok(changed)
                    }
                })
                .await
            }
        }
    }
    async fn connection(self: &Arc<Self>, stream: &mut UnixStream) -> Result<()> {
        if stream.peer_cred()?.uid() != rustix::process::geteuid().as_raw() {
            return Err(Error("PEER_DENIED"));
        }
        ipc::write(stream, &ipc::hello()).await?;
        let request: Request = match ipc::read(stream).await {
            Ok(r) => r,
            Err(e) => {
                ipc::write(stream, &ipc::response(None, Err(e))).await?;
                return Ok(());
            }
        };
        let replay = match &request.command {
            Command::ReceiptReplay { id } => Some(id.clone()),
            _ => None,
        };
        let result = if request.protocol_version != ipc::VERSION {
            Err(Error("PROTOCOL_VERSION"))
        } else if !ipc::identifier(&request.request_id) {
            Err(Error("INVALID_REQUEST"))
        } else if request
            .deadline_ms
            .is_some_and(|d| ipc::now_ms().map_or(true, |now| d <= now))
        {
            Err(Error("REQUEST_EXPIRED"))
        } else {
            self.dispatch(&request.request_id, request.command).await
        };
        let reply = ipc::response(Some(&request.request_id), result);
        match ipc::write(stream, &reply).await {
            Err(Error("RESULT_TOO_LARGE")) => {
                return ipc::write(
                    stream,
                    &ipc::response(Some(&request.request_id), Err(Error("RESULT_TOO_LARGE"))),
                )
                .await;
            }
            result => result?,
        }
        if let Some(receipt_id) = replay
            && reply.get("error").is_none()
        {
            let delivered = async {
                let ack: ipc::DeliveryAck = ipc::read(stream).await?;
                if ack.protocol_version != ipc::VERSION
                    || ack.receipt_id != receipt_id
                    || reply["data"]["output_sha256"] != ack.output_sha256
                {
                    return Err(Error("DELIVERY_HASH_MISMATCH"));
                }
                let service = self.clone();
                let delivered = reply["data"]["already_delivered"] == true;
                self.operation(async move {
                    let _guard = service.admission.lock().await;
                    if !delivered {
                        crate::domain::Domain::new(&service.storage)
                            .record_delivery(
                                &jobs::context()?,
                                &receipt_id,
                                &format!(
                                    "ipc-delivery/{}",
                                    crate::storage::hash(receipt_id.as_bytes())
                                ),
                                &ack.output_sha256,
                            )
                            .await?;
                    }
                    Ok(json!({"receipt_id":receipt_id,"phase":"delivered"}))
                })
                .await
            }
            .await;
            let mut acknowledgement = ipc::response(Some(&request.request_id), delivered);
            acknowledgement["sequence"] = json!(2);
            ipc::write(stream, &acknowledgement).await?;
        }
        Ok(())
    }
}

fn read_budget() -> crate::domain::PageBudget {
    crate::domain::PageBudget {
        limit: 32,
        max_bytes: 1024 * 1024,
    }
}

/// Explicit lifecycle command. GNU nohup preserves the daemon when its CLI exits.
pub async fn start(
    config: &ResolvedConfig,
    args: &[std::ffi::OsString],
) -> Result<serde_json::Value> {
    use std::os::unix::process::CommandExt;
    use std::process::{Command as Process, Stdio};
    let path = &config.config().paths.socket_path;
    let parent = path.parent().ok_or(Error("UNSAFE_PATH"))?;
    private_dir(parent)?;
    let request = Request {
        protocol_version: ipc::VERSION,
        request_id: format!("start/{}/{}", std::process::id(), ipc::now_ms()?),
        deadline_ms: None,
        command: Command::Status,
    };
    if let Ok(response) = ipc::call(path, &request).await {
        if response.get("error").is_some() {
            return Err(Error("RESOURCE_BUSY"));
        }
        return Ok(response);
    }
    let log = OpenOptions::new()
        .append(true)
        .create(true)
        .mode(0o600)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(parent.join("daemon.log"))?;
    let metadata = log.metadata()?;
    if !metadata.is_file()
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o600
        || metadata.nlink() != 1
    {
        return Err(Error("UNSAFE_PATH"));
    }
    let mut child_args = args.to_vec();
    *child_args.last_mut().ok_or(Error("INVALID_ARGUMENT"))? = "run".into();
    let mut child = Process::new("/usr/bin/nohup")
        .arg("--")
        .arg(std::env::current_exe()?)
        .args(child_args)
        .stdin(Stdio::null())
        .stdout(log.try_clone()?)
        .stderr(log)
        .process_group(0)
        .spawn()?;
    let end = tokio::time::Instant::now() + config.timeouts().startup;
    loop {
        if child.try_wait()?.is_some() {
            return Err(Error("DAEMON_START_FAILED"));
        }
        if let Ok(response) = ipc::call(path, &request).await
            && response["data"]["pid"] == child.id()
        {
            return Ok(response);
        }
        if tokio::time::Instant::now() >= end {
            let _ = child.kill();
            let _ = child.wait();
            return Err(Error("STARTUP_TIMEOUT"));
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}

pub async fn run(
    config: ResolvedConfig,
    config_path: PathBuf,
    profile: Option<String>,
) -> Result<()> {
    let providers = crate::provider::Client::new().map_err(|e| Error(e.0))?;
    private_dir(&config.config().paths.data_dir)?;
    let (listener, _owner) = listen(&config.config().paths.socket_path)?;
    let storage = Arc::new(Storage::open(&config).await?);
    let manager = match configuration::open(&storage, config).await {
        Ok(manager) => manager,
        Err(error) => {
            let _ = storage.close().await;
            return Err(error);
        }
    };
    let instance = match ipc::now_ms() {
        Ok(now) => format!("{}/{now}", std::process::id()),
        Err(error) => {
            let _ = storage.close().await;
            return Err(error);
        }
    };
    let service = Arc::new(Service {
        storage,
        config: manager,
        config_path,
        profile,
        admission: Mutex::new(()),
        operations: Mutex::new(JoinSet::new()),
        last_config: Mutex::new(None),
        stop: Notify::new(),
        work: Notify::new(),
        stopping: std::sync::atomic::AtomicBool::new(false),
        instance,
        providers,
        search_index: Mutex::new(None),
        index_builder: Mutex::new(()),
        index_epoch: std::sync::atomic::AtomicU64::new(0),
        #[cfg(test)]
        batch_gate: Mutex::new(None),
    });
    let initialized = async {
    // Reconcile startup input through the same pipeline, without resetting durable generations.
    if let Ok(input) = crate::config::read_file(&service.config_path) {
        let active = service.config.pin();
        let candidate = crate::config::resolve(
            &input,
            service.profile.as_deref(),
            "",
            active.resolved().base_paths(),
        )?;
        if candidate.digest() == active.resolved().digest() {
            *service.last_config.lock().await = Some((
                crate::storage::hash(input.as_bytes()),
                json!({"status":"applied","active_generation":active.generation(),"unchanged":true}),
            ));
        } else {
            service.reload().await?;
        }
    }
    let scope = jobs::owner_scope();
    crate::domain::Domain::new(&service.storage)
        .ensure_local_reader(&scope.user, &scope, service.config.pin().generation())
        .await?;
    service.recover().await?;
    Ok(())
    }.await;
    if let Err(error) = initialized {
        return service.close(Err(error)).await;
    }
    serve(service, || listener.accept()).await
}

async fn serve<A, F>(service: Arc<Service>, mut accept: A) -> Result<()>
where
    A: FnMut() -> F,
    F: std::future::Future<Output = std::io::Result<(UnixStream, tokio::net::unix::SocketAddr)>>,
{
    let signals = (|| -> std::io::Result<_> {
        Ok((
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::terminate())?,
            tokio::signal::unix::signal(tokio::signal::unix::SignalKind::interrupt())?,
        ))
    })();
    let (mut terminate, mut interrupt) = match signals {
        Ok(signals) => signals,
        Err(error) => return service.close(Err(error.into())).await,
    };
    let mut scheduler = tokio::spawn(service.clone().jobs());
    let mut scheduler_collected = false;
    let slots = Arc::new(Semaphore::new(16));
    let mut tasks = JoinSet::new();
    let mut watcher = tokio::time::interval(Duration::from_millis(500));
    watcher.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut observed = None;
    let mut outcome = loop {
        tokio::select! {
            _ = service.stop.notified() => break Ok(()),
            _ = terminate.recv() => break Ok(()),
            _ = interrupt.recv() => break Ok(()),
            result = &mut scheduler => {
                scheduler_collected = true;
                break result.map_err(|_| Error("JOB_ERROR")).and_then(|r|r);
            },
            _ = watcher.tick() => {
                if let Ok(input) = crate::config::read_file(&service.config_path) {
                    let digest = crate::storage::hash(input.as_bytes());
                    if observed.as_ref() == Some(&digest) && service.last_config.lock().await.as_ref().map(|(hash,_)| hash) != Some(&digest) {
                        let _ = service.reload().await;
                    }
                    observed = Some(digest);
                }
            },
            Some(_) = tasks.join_next(), if !tasks.is_empty() => {},
            accepted = accept() => {
                let (mut stream, _) = match accepted {
                    Ok(connection) => connection,
                    Err(error) => break Err(error.into()),
                };
                if !stream.peer_cred().is_ok_and(|peer| peer.uid() == rustix::process::geteuid().as_raw()) { continue; }
                if let Ok(permit) = slots.clone().try_acquire_owned() {
                    let service = service.clone();
                    tasks.spawn(async move {
                        let _permit = permit;
                        let _ = tokio::time::timeout(ipc::TRANSPORT_TIMEOUT, service.connection(&mut stream)).await;
                    });
                } else {
                    let _ = tokio::time::timeout(Duration::from_millis(100), async {
                        let mut hello = ipc::hello();
                        hello["error"] = Error("RESOURCE_BUSY").value();
                        ipc::write(&mut stream, &hello).await
                    }).await;
                }
            }
        }
    };
    service
        .stopping
        .store(true, std::sync::atomic::Ordering::Release);
    service.wake_jobs();
    while tasks.join_next().await.is_some() {}
    if !scheduler_collected {
        let drained = scheduler
            .await
            .map_err(|_| Error("JOB_ERROR"))
            .and_then(|r| r);
        outcome = outcome.and(drained);
    }
    service.close(outcome).await
}
