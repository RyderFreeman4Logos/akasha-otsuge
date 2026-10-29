use crate::adapter::{
    native::{ProcessGroup, Reap, reap, spawn_error, wait_for_exit},
    *,
};
use futures::future::BoxFuture;
use rustix::process::Pid;
use serde_json::{Value, json};
use std::{
    os::{
        linux::net::SocketAddrExt,
        unix::{fs::DirBuilderExt, process::CommandExt},
    },
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::{UnixListener, UnixStream},
    process::{Child, ChildStderr, ChildStdin, ChildStdout},
};

const TOOLS: [&str; 9] = RESEARCH_TOOLS;

struct SetupCleanup(Option<PathBuf>);
impl Drop for SetupCleanup {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

/// A native one-shot process using the selected profile's installed plugin and authentication.
pub struct PiRunner {
    report: CapabilityReport,
    home: PathBuf,
    workspace: PathBuf,
    instructions: String,
}
impl PiRunner {
    pub async fn new(
        binary: &Path,
        native_home: &Path,
        workspace: &Path,
        instructions: &str,
    ) -> Result<Self> {
        if instructions.trim().is_empty() || instructions.len() > MAX_FRAME_BYTES {
            return Err(ErrorCode::InvalidArgument.into());
        }
        let mut report = super::probe(binary).await?;
        if report.version.as_deref() != Some(super::SUPPORTED_VERSION) {
            return Err(ErrorCode::AdapterUnsupported.into());
        }
        let home = native_home
            .canonicalize()
            .map_err(|_| ErrorCode::SourceUnavailable)?;
        let workspace = workspace
            .canonicalize()
            .map_err(|_| ErrorCode::SourceUnavailable)?;
        if !workspace.is_dir() {
            return Err(ErrorCode::SourceUnavailable.into());
        }
        super::installation::runner_extension(&home).map_err(|_| ErrorCode::AdapterUnsupported)?;
        report.machine_protocol = Some("pi-rpc/1.4.0".into());
        report.cancellation = true;
        report.outer_timeout = true;
        report.capabilities.extend(
            [
                "runner.events",
                "runner.cancel",
                "context.describe",
                "context.list",
                "context.read",
                "context.search",
                "memory.search",
                "memory.get",
                "code.search",
                "code.read",
                "delegate",
            ]
            .map(str::to_owned),
        );
        Ok(Self {
            report,
            home,
            workspace,
            instructions: instructions.into(),
        })
    }
}
impl AgentRunner for PiRunner {
    fn probe(&self) -> Result<CapabilityReport> {
        Ok(self.report.clone())
    }
    fn start<'a>(
        &'a mut self,
        spec: RunSpec,
        context: &'a mut dyn ContextSource,
    ) -> BoxFuture<'a, Result<Box<dyn RunHandle + 'a>>> {
        Box::pin(async move {
            spec.check()?;
            let extension = super::installation::runner_extension(&self.home)
                .map_err(|_| ErrorCode::AdapterUnsupported)?;
            let nonce = crate::storage::hash(spec.capability().transport_token().as_bytes());
            let directory = self.workspace.join(format!("akasha-run-{nonce}"));
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&directory)
                .map_err(|_| ErrorCode::SourceUnavailable)?;
            let mut setup = SetupCleanup(Some(directory.clone()));
            // Pi defaults both HTTP idle and provider requests to five minutes.
            let timeout_ms = u64::try_from(spec.deadline().remaining().as_millis())
                .map_err(|_| ErrorCode::ConfigInvalid)?
                .max(1);
            let settings = directory.join(".pi");
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&settings)
                .map_err(|_| ErrorCode::SourceUnavailable)?;
            std::fs::write(
                settings.join("settings.json"),
                json!({"httpIdleTimeoutMs": timeout_ms, "retry": {"provider": {"timeoutMs": timeout_ms}}})
                    .to_string(),
            )
            .map_err(|_| ErrorCode::SourceUnavailable)?;
            // Abstract UDS avoids filesystem path limits; peer UID and a per-run capability authenticate it.
            let address =
                std::os::unix::net::SocketAddr::from_abstract_name(format!("akasha-{nonce}"))
                    .map_err(|_| ErrorCode::SourceUnavailable)?;
            let listener = std::os::unix::net::UnixListener::bind_addr(&address)
                .map_err(|_| ErrorCode::SourceUnavailable)?;
            listener
                .set_nonblocking(true)
                .map_err(|_| ErrorCode::SourceUnavailable)?;
            let listener =
                UnixListener::from_std(listener).map_err(|_| ErrorCode::SourceUnavailable)?;
            let prompt = format!(
                "{}\n{}\nRequest: {}",
                self.instructions,
                spec.result_instructions(),
                serde_json::to_string(&spec.options().args)
                    .map_err(|_| ErrorCode::ProtocolInvalid)?
            );
            let prompt_id = format!("akasha-{nonce}");
            let mut prompt =
                serde_json::to_vec(&json!({"id":prompt_id,"type":"prompt","message":prompt}))
                    .map_err(|_| ErrorCode::ProtocolInvalid)?;
            if prompt.len() > MAX_FRAME_BYTES {
                return Err(ErrorCode::OutputLimit.into());
            }
            prompt.push(b'\n');
            let mut command = spec.command(
                self.report
                    .executable
                    .as_ref()
                    .ok_or(ErrorCode::ExecutorUnavailable)?,
                &directory,
            )?;
            command
                .args([
                    "--mode",
                    "rpc",
                    "--no-session",
                    "--no-builtin-tools",
                    "--no-extensions",
                    "--no-mcp",
                    "--no-skills",
                    "--no-prompt-templates",
                    "--no-themes",
                    "--no-context-files",
                    "--approve",
                    "-e",
                ])
                .arg(&extension)
                .args(["--tools", &TOOLS.join(",")])
                .env("PI_CODING_AGENT_DIR", &self.home)
                .env("AKASHA_PI_RUN_SOCKET", format!("akasha-{nonce}"))
                .env("AKASHA_PI_RUN_TOKEN", spec.capability().transport_token())
                .env(
                    "AKASHA_RUN_DEADLINE_MS",
                    spec.bridge_deadline_ms()?.to_string(),
                )
                .current_dir(&directory)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .process_group(0);
            spec.native_options
                .apply(crate::config::Host::Pi, &mut command)?;
            let mut child = tokio::process::Command::from(command)
                .kill_on_drop(!spec.guarded())
                .spawn()
                .map_err(spawn_error)?;
            let group = ProcessGroup(
                Pid::from_raw(child.id().ok_or(ErrorCode::RunnerFailed)? as i32)
                    .ok_or(ErrorCode::RunnerFailed)?,
                spec.guarded(),
            );
            let input = child.stdin.take().ok_or(ErrorCode::RunnerFailed)?;
            let stdout = child.stdout.take().ok_or(ErrorCode::RunnerFailed)?;
            let stderr = child.stderr.take().ok_or(ErrorCode::RunnerFailed)?;
            let handle = Handle {
                child: Some(child),
                group: Some(group),
                directory,
                listener,
                stdout,
                stderr,
                spec,
                context,
                home: self.home.clone(),
                input: Some(input),
                prompt: Some(prompt),
                prompt_id,
                prompt_sent: false,
                settled: false,
                last_assistant: None,
                initialized: false,
                ready: false,
                usage_seen: false,
                buffer: vec![],
                stderr_bytes: 0,
                stdout_closed: false,
                stderr_closed: false,
                result: None,
                status: None,
                finished: false,
                cleanup: None,
                error: None,
            };
            setup.0 = None;
            Ok(Box::new(handle) as Box<dyn RunHandle>)
        })
    }
}

struct Handle<'a> {
    child: Option<Child>,
    group: Option<ProcessGroup>,
    directory: PathBuf,
    listener: UnixListener,
    stdout: ChildStdout,
    stderr: ChildStderr,
    spec: RunSpec,
    context: &'a mut dyn ContextSource,
    home: PathBuf,
    input: Option<ChildStdin>,
    prompt: Option<Vec<u8>>,
    prompt_id: String,
    prompt_sent: bool,
    settled: bool,
    last_assistant: Option<(String, String, ErrorCode)>,
    initialized: bool,
    ready: bool,
    usage_seen: bool,
    buffer: Vec<u8>,
    stderr_bytes: usize,
    stdout_closed: bool,
    stderr_closed: bool,
    result: Option<String>,
    status: Option<i32>,
    finished: bool,
    cleanup: Option<Reap>,
    error: Option<AdapterError>,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Request {
    protocol_version: u32,
    request_id: String,
    command: Command,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Command {
    op: String,
    token: String,
    event: Event,
}
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Event {
    Usage {
        input_tokens: u64,
        output_tokens: u64,
    },
    Ready {
        agent_dir: PathBuf,
        version: String,
        tools: Vec<String>,
        active_tools: Vec<String>,
    },
    Context {
        request: Value,
    },
}

impl Handle<'_> {
    async fn broker(&mut self, mut stream: UnixStream) -> Result<Option<RunnerEvent>> {
        if stream.peer_cred().map_err(|_| ErrorCode::ReadDenied)?.uid()
            != rustix::process::geteuid().as_raw()
        {
            return Err(ErrorCode::ReadDenied.into());
        }
        crate::ipc::write(&mut stream, &crate::ipc::hello())
            .await
            .map_err(|_| ErrorCode::ProtocolInvalid)?;
        let size = stream
            .read_u32()
            .await
            .map_err(|_| ErrorCode::ProtocolInvalid)? as usize;
        if size == 0 || size > 16384 {
            return Err(ErrorCode::OutputLimit.into());
        }
        self.spec.lease().charge_read(size as u64)?;
        let mut bytes = vec![0; size];
        stream
            .read_exact(&mut bytes)
            .await
            .map_err(|_| ErrorCode::ProtocolInvalid)?;
        let request: Request =
            serde_json::from_slice(&bytes).map_err(|_| ErrorCode::ProtocolInvalid)?;
        if request.protocol_version != 1
            || !super::super::valid_id(&request.request_id)
            || request.command.op != "pi_context"
            || self.settled
        {
            return Err(ErrorCode::ProtocolInvalid.into());
        }
        self.spec
            .capability()
            .authorize(&request.command.token, self.spec.lease())?;
        let (data, event) = match request.command.event {
            Event::Usage {
                input_tokens,
                output_tokens,
            } => {
                if !self.ready {
                    return Err(ErrorCode::AdapterUnsupported.into());
                }
                self.spec
                    .lease()
                    .report_tokens(input_tokens, output_tokens)?;
                self.usage_seen = true;
                (
                    json!({"accepted":true}),
                    Some(RunnerEvent::Usage {
                        input_tokens,
                        output_tokens,
                    }),
                )
            }
            Event::Ready {
                agent_dir,
                version,
                tools,
                active_tools,
            } => {
                if self.ready {
                    return Err(ErrorCode::ProtocolInvalid.into());
                }
                if agent_dir != self.home {
                    return Err(ErrorCode::ReadDenied.into());
                }
                if version != super::SUPPORTED_VERSION {
                    return Err(ErrorCode::AdapterUnsupported.into());
                }
                if tools != TOOLS || active_tools != TOOLS {
                    return Err(ErrorCode::ToolDenied.into());
                }
                self.ready = true;
                (json!({"ready":true}), Some(RunnerEvent::Started))
            }
            Event::Context { request } => {
                if !self.ready {
                    return Err(ErrorCode::AdapterUnsupported.into());
                }
                let request: ContextRequest =
                    serde_json::from_value(request).map_err(|_| ErrorCode::InvalidArgument)?;
                let name = request.tool();
                let response = self.context.execute(self.spec.lease(), request).await?;
                self.spec.check()?;
                (
                    serde_json::to_value(response).map_err(|_| ErrorCode::ProtocolInvalid)?,
                    Some(RunnerEvent::Tool { name: name.into() }),
                )
            }
        };
        let response = crate::ipc::response(Some(&request.request_id), Ok(data));
        let size = serde_json::to_vec(&response)
            .map_err(|_| ErrorCode::ProtocolInvalid)?
            .len();
        if size > MAX_FRAME_BYTES {
            return Err(ErrorCode::OutputLimit.into());
        }
        self.spec.lease().charge_output(size as u64)?;
        crate::ipc::write(&mut stream, &response)
            .await
            .map_err(|_| ErrorCode::ProtocolInvalid)?;
        Ok(event)
    }
    fn frame(&mut self, bytes: &[u8]) -> Result<()> {
        let value: Value = serde_json::from_slice(bytes).map_err(|_| ErrorCode::ProtocolInvalid)?;
        if self.settled {
            return Err(ErrorCode::ProtocolInvalid.into());
        }
        if !self.ready {
            return Err(ErrorCode::AdapterUnsupported.into());
        }
        if !self.prompt_sent {
            return Err(ErrorCode::ProtocolInvalid.into());
        }
        let kind = value["type"].as_str().ok_or(ErrorCode::ProtocolInvalid)?;
        if kind == "response" {
            if self.initialized || value["id"] != self.prompt_id || value["command"] != "prompt" {
                return Err(ErrorCode::ProtocolInvalid.into());
            }
            if value["success"] != true {
                return Err(native_failure(&value["error"]).into());
            }
            if value["data"]["disposition"] != "started" {
                return Err(ErrorCode::RunnerIncomplete.into());
            }
            self.initialized = true;
            return Ok(());
        }
        if !self.initialized {
            return Err(ErrorCode::ProtocolInvalid.into());
        }
        match kind {
            "agent_start" | "turn_start" | "auto_retry_start" => {
                self.last_assistant = None;
            }
            "message_end" => {
                let message = &value["message"];
                if message["role"] == "assistant" {
                    let blocks = message["content"]
                        .as_array()
                        .ok_or(ErrorCode::ProtocolInvalid)?;
                    let mut text = String::new();
                    for block in blocks {
                        if block["type"] == "toolCall" {
                            check_tool(block["name"].as_str())?;
                        }
                        if block["type"] == "text" {
                            text.push_str(
                                block["text"].as_str().ok_or(ErrorCode::ProtocolInvalid)?,
                            );
                        }
                    }
                    self.last_assistant = Some((
                        text,
                        message["stopReason"]
                            .as_str()
                            .ok_or(ErrorCode::ProtocolInvalid)?
                            .into(),
                        native_failure(&message["errorMessage"]),
                    ));
                }
            }
            "message_update" => {
                let delta = &value["assistantMessageEvent"];
                if delta["type"] == "toolcall_start" {
                    check_tool(delta["toolName"].as_str())?;
                }
            }
            "tool_execution_start" | "tool_execution_end" | "tool_execution_update" => {
                check_tool(value["toolName"].as_str())?;
            }
            "agent_settled" => {
                if value["aborted"] == true {
                    return Err(ErrorCode::Cancelled.into());
                }
                if value["aborted"] != false {
                    return Err(ErrorCode::ProtocolInvalid.into());
                }
                let (text, reason, failure) = self
                    .last_assistant
                    .take()
                    .ok_or(ErrorCode::RunnerIncomplete)?;
                if reason != "stop" {
                    return Err(failure.into());
                }
                if !self.usage_seen {
                    return Err(ErrorCode::RunnerIncomplete.into());
                }
                self.result = Some(self.spec.final_output(&text, false)?);
                self.settled = true;
            }
            "message_start"
            | "turn_end"
            | "agent_end"
            | "auto_retry_end"
            | "entry_appended"
            | "queue_update"
            | "session_info_changed"
            | "thinking_level_changed"
            | "compaction_start"
            | "compaction_end"
            | "summarization_retry_scheduled"
            | "summarization_retry_attempt_start"
            | "summarization_retry_finished" => {}
            "bash_execution_update" => return Err(ErrorCode::ToolDenied.into()),
            "extension_error" => return Err(ErrorCode::RunnerFailed.into()),
            _ => return Err(ErrorCode::ProtocolInvalid.into()),
        }
        Ok(())
    }
    async fn advance(&mut self) -> Result<Option<RunnerEvent>> {
        if let Some(error) = &self.error {
            return Err(error.clone());
        }
        if self.finished {
            return Ok(None);
        }
        loop {
            self.spec.check()?;
            if self.ready && !self.prompt_sent {
                let prompt = self.prompt.take().ok_or(ErrorCode::ProtocolInvalid)?;
                let input = self.input.as_mut().ok_or(ErrorCode::RunnerFailed)?;
                tokio::time::timeout(self.spec.deadline().remaining(), input.write_all(&prompt))
                    .await
                    .map_err(|_| ErrorCode::RunnerTimeout)?
                    .map_err(|_| ErrorCode::RunnerFailed)?;
                self.spec.check()?;
                self.prompt_sent = true;
            }
            if let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
                if end > MAX_FRAME_BYTES {
                    return Err(ErrorCode::OutputLimit.into());
                }
                let bytes: Vec<_> = self.buffer.drain(..=end).collect();
                self.frame(&bytes)?;
                if self.settled {
                    self.input.take();
                }
                continue;
            }
            if self.buffer.len() > MAX_FRAME_BYTES {
                return Err(ErrorCode::OutputLimit.into());
            }
            if self.stdout_closed && self.stderr_closed {
                if !self.buffer.is_empty() {
                    return Err(ErrorCode::ProtocolInvalid.into());
                }
                let child = self.child.as_ref().ok_or(ErrorCode::RunnerFailed)?;
                tokio::time::timeout(self.spec.deadline().remaining(), wait_for_exit(child))
                    .await
                    .map_err(|_| ErrorCode::RunnerTimeout)??;
                self.spec.check()?;
                self.begin_cleanup();
                let status = self
                    .cleanup
                    .as_mut()
                    .ok_or(ErrorCode::RunnerFailed)?
                    .wait()
                    .await
                    .ok_or(ErrorCode::RunnerTimeout)?;
                self.status = status.code();
                if !status.success() {
                    return Err(if !self.initialized && !self.ready {
                        ErrorCode::AdapterUnsupported
                    } else {
                        ErrorCode::RunnerFailed
                    }
                    .into());
                }
                if !self.settled {
                    return Err(ErrorCode::RunnerIncomplete.into());
                }
                return Ok(Some(RunnerEvent::Completed {
                    output: self.result.take().ok_or(ErrorCode::RunnerIncomplete)?,
                }));
            }
            let mut out = [0; 8192];
            let mut err = [0; 4096];
            tokio::select! {
                accepted=self.listener.accept()=>{
                    let (stream,_)=accepted.map_err(|_|ErrorCode::RunnerFailed)?;
                    let result=tokio::time::timeout(self.spec.deadline().remaining(),self.broker(stream)).await;
                    self.spec.check()?;
                    if let Some(event)=result.map_err(|_|ErrorCode::RunnerTimeout)??{return Ok(Some(event));}
                }
                read=self.stdout.read(&mut out),if !self.stdout_closed=>{
                    let count=read.map_err(|_|ErrorCode::RunnerFailed)?;self.stdout_closed=count==0;
                    self.spec.lease().charge_output(count as u64)?;self.buffer.extend_from_slice(&out[..count]);
                }
                read=self.stderr.read(&mut err),if !self.stderr_closed=>{
                    let count=read.map_err(|_|ErrorCode::RunnerFailed)?;self.stderr_closed=count==0;self.stderr_bytes+=count;
                    if self.stderr_bytes>8192{return Err(ErrorCode::OutputLimit.into());}
                }
                _=tokio::time::sleep(self.spec.deadline().remaining())=>{self.spec.check()?;return Err(ErrorCode::RunnerTimeout.into());}
            }
        }
    }
    fn begin_cleanup(&mut self) {
        self.input.take();
        drop(self.group.take());
        self.finished = true;
        if let Some(child) = self.child.take() {
            let directory = self.directory.clone();
            let lease = self.spec.lease_handle();
            self.cleanup = Some(reap(child, self.spec.guarded(), move || {
                let _lease = lease;
                let _ = std::fs::remove_dir_all(directory);
            }));
        }
    }
    async fn stop(&mut self) -> bool {
        self.begin_cleanup();
        if let Some(status) = match &mut self.cleanup {
            Some(c) => c.wait().await,
            None => None,
        } {
            self.status = status.code();
            true
        } else {
            false
        }
    }
}
impl RunHandle for Handle<'_> {
    fn next(&mut self) -> BoxFuture<'_, Result<Option<RunnerEvent>>> {
        Box::pin(async move {
            let stopped = self.spec.lease().interrupted();
            let result = tokio::select! {
                result = self.advance() => result,
                error = stopped => Err(error),
            };
            if let Err(error) = &result {
                self.error = Some(error.clone());
                self.stop().await;
            }
            result
        })
    }
    fn cancel(&mut self, reason: CancelReason) -> BoxFuture<'_, Result<CancelAck>> {
        Box::pin(async move {
            self.error.get_or_insert_with(|| match reason {
                CancelReason::PermissionRevoked => ErrorCode::ReadDenied.into(),
                CancelReason::Deadline => self
                    .spec
                    .check()
                    .err()
                    .unwrap_or(ErrorCode::RunnerTimeout.into()),
                _ => ErrorCode::Cancelled.into(),
            });
            let acknowledged = self.stop().await;
            Ok(CancelAck { acknowledged })
        })
    }
    fn exit_status(&mut self) -> BoxFuture<'_, Result<Option<i32>>> {
        Box::pin(async move {
            if let Some(status) = self.cleanup.as_mut().and_then(Reap::status) {
                self.status = status.code();
            }
            Ok(self.status)
        })
    }
}
impl Drop for Handle<'_> {
    fn drop(&mut self) {
        self.begin_cleanup();
    }
}

fn check_tool(name: Option<&str>) -> Result<()> {
    if name.is_some_and(|name| TOOLS.contains(&name)) {
        Ok(())
    } else {
        Err(ErrorCode::ToolDenied.into())
    }
}

fn native_failure(message: &Value) -> ErrorCode {
    let Some(message) = message.as_str() else {
        return ErrorCode::RunnerFailed;
    };
    // Classify native authentication errors without exposing credentials or provider text.
    if message.starts_with("No API key")
        || (message.starts_with("OAuth refresh failed for ")
            && message.contains("invalid_refresh_token"))
    {
        ErrorCode::AuthRequired
    } else {
        ErrorCode::RunnerFailed
    }
}
