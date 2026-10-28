use super::{ProcessGroup, Reap, probe, reap, wait_for_exit};
use crate::adapter::*;
use futures::future::BoxFuture;
use rustix::process::Pid;
use serde_json::Value;
use std::{
    os::fd::AsRawFd,
    os::unix::{fs::DirBuilderExt, process::CommandExt},
    path::{Path, PathBuf},
    process::Stdio,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{UnixListener, UnixStream};
use tokio::process::{Child, ChildStderr, ChildStdout};

const TOOLS: [&str; 4] = [
    "mcp__akasha-runner__context_describe",
    "mcp__akasha-runner__context_list",
    "mcp__akasha-runner__context_read",
    "mcp__akasha-runner__context_search",
];

#[derive(serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadRequest {
    token: String,
    request: ContextRequest,
}

struct SetupCleanup(Option<PathBuf>);
impl Drop for SetupCleanup {
    fn drop(&mut self) {
        if let Some(path) = &self.0 {
            let _ = std::fs::remove_dir_all(path);
        }
    }
}

pub struct ClaudeRunner {
    report: CapabilityReport,
    workspace: PathBuf,
    bridge: PathBuf,
    instructions: String,
}
impl ClaudeRunner {
    pub async fn new(
        binary: &Path,
        bridge: &Path,
        workspace: &Path,
        instructions: &str,
    ) -> Result<Self> {
        if instructions.trim().is_empty() || instructions.len() > MAX_FRAME_BYTES {
            return Err(ErrorCode::InvalidArgument.into());
        }
        let mut report = probe(binary).await?;
        if report.version.as_deref() != Some("2.1.311") {
            return Err(ErrorCode::AdapterUnsupported.into());
        }
        let workspace = workspace
            .canonicalize()
            .map_err(|_| ErrorCode::SourceUnavailable)?;
        if !workspace.is_dir() {
            return Err(ErrorCode::SourceUnavailable.into());
        }
        report.machine_protocol = Some("claude-stream-json/2.1.311".into());
        report.cancellation = true;
        report.outer_timeout = true;
        report
            .capabilities
            .extend(["runner.events".into(), "runner.cancel".into()]);
        let bridge = super::resolve(bridge)?;
        Ok(Self {
            report,
            workspace,
            bridge,
            instructions: instructions.into(),
        })
    }
}
impl AgentRunner for ClaudeRunner {
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
            let binary = self
                .report
                .executable
                .as_ref()
                .ok_or(ErrorCode::ExecutorUnavailable)?;
            let directory = self.workspace.join(format!(
                "akasha-run-{}",
                crate::storage::hash(spec.capability().transport_token().as_bytes())
            ));
            std::fs::DirBuilder::new()
                .mode(0o700)
                .create(&directory)
                .map_err(|_| ErrorCode::SourceUnavailable)?;
            let mut cleanup = SetupCleanup(Some(directory.clone()));
            // A descriptor path keeps Unix sockets usable under long SSD paths.
            let directory_fd =
                std::fs::File::open(&directory).map_err(|_| ErrorCode::SourceUnavailable)?;
            let socket = format!(
                "/proc/{}/fd/{}/read.sock",
                std::process::id(),
                directory_fd.as_raw_fd()
            );
            let listener = UnixListener::bind(&socket).map_err(|_| ErrorCode::SourceUnavailable)?;
            let plugin = directory.join("plugin");
            std::fs::create_dir_all(plugin.join(".claude-plugin")).and_then(|_| std::fs::create_dir(plugin.join("hooks")))
                .and_then(|_| std::fs::write(plugin.join(".claude-plugin/plugin.json"), r#"{"name":"akasha-runner","version":"0.1.0","description":"Bound read-only context"}"#))
                .and_then(|_| std::fs::write(plugin.join("hooks/hooks.json"), r#"{"modules":["./register.js"]}"#))
                .and_then(|_| std::fs::write(plugin.join("hooks/register.js"), include_str!("runner.js")))
                .map_err(|_| ErrorCode::SourceUnavailable)?;
            let mut command = std::process::Command::new(binary);
            // Native2.1.311 requires a builtin to register plugin tools; the plugin denies ToolSearch calls.
            command
                .args([
                    "-p",
                    "--output-format",
                    "stream-json",
                    "--verbose",
                    "--tools",
                    "ToolSearch",
                    "--strict-mcp-config",
                    "--mcp-config",
                    "{\"mcpServers\":{}}",
                    "--setting-sources",
                    "",
                    "--system-prompt",
                    &self.instructions,
                ])
                .env("CLAUDE_CODE_DISABLE_CLAUDE_MDS", "1")
                .env("DISABLE_AUTOUPDATER", "1")
                .env("AKASHA_RUN_SOCKET", &socket)
                .env("AKASHA_RUN_TOKEN", spec.capability().transport_token())
                .env("AKASHA_RUN_BRIDGE", &self.bridge)
                .arg("--plugin-dir")
                .arg(&plugin)
                .current_dir(&directory)
                .stdin(Stdio::piped())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .process_group(0);
            let mut child = tokio::process::Command::from(command)
                .kill_on_drop(true)
                .spawn()
                .map_err(|_| ErrorCode::ExecutorUnavailable)?;
            let group = ProcessGroup(
                Pid::from_raw(child.id().ok_or(ErrorCode::RunnerFailed)? as i32)
                    .ok_or(ErrorCode::RunnerFailed)?,
            );
            let mut stdin = child.stdin.take().ok_or(ErrorCode::RunnerFailed)?;
            let stdout = child.stdout.take().ok_or(ErrorCode::RunnerFailed)?;
            let stderr = child.stderr.take().ok_or(ErrorCode::RunnerFailed)?;
            let mut handle = Handle {
                child: Some(child),
                group: Some(group),
                directory,
                _directory_fd: directory_fd,
                listener,
                stdout,
                stderr,
                spec,
                context,
                buffer: Vec::new(),
                stderr_bytes: 0,
                stdout_closed: false,
                stderr_closed: false,
                session: None,
                tools: Default::default(),
                result: None,
                status: None,
                finished: false,
                cleanup: None,
                error: None,
            };
            cleanup.0 = None;
            // JSON keeps a slash-leading hint from invoking native commands.
            let input = serde_json::json!({"hint":handle.spec.options().args.hint}).to_string();
            let written = tokio::time::timeout(
                handle.spec.deadline().remaining(),
                stdin.write_all(input.as_bytes()),
            )
            .await;
            drop(stdin);
            if !matches!(written, Ok(Ok(()))) {
                handle.stop().await;
                handle.spec.check()?;
                return Err(ErrorCode::RunnerFailed.into());
            }
            Ok(Box::new(handle) as Box<dyn RunHandle>)
        })
    }
}

struct Handle<'a> {
    child: Option<Child>,
    group: Option<ProcessGroup>,
    directory: PathBuf,
    _directory_fd: std::fs::File,
    listener: UnixListener,
    stdout: ChildStdout,
    stderr: ChildStderr,
    spec: RunSpec,
    context: &'a mut dyn ContextSource,
    buffer: Vec<u8>,
    stderr_bytes: usize,
    stdout_closed: bool,
    stderr_closed: bool,
    session: Option<String>,
    tools: std::collections::BTreeMap<String, bool>,
    result: Option<String>,
    status: Option<i32>,
    finished: bool,
    cleanup: Option<Reap>,
    error: Option<AdapterError>,
}
impl Handle<'_> {
    fn frame(&mut self, bytes: &[u8]) -> Result<Option<RunnerEvent>> {
        let value: Value = serde_json::from_slice(bytes).map_err(|_| ErrorCode::ProtocolInvalid)?;
        let session = value["session_id"]
            .as_str()
            .filter(|s| !s.is_empty() && s.len() <= 256)
            .ok_or(ErrorCode::ProtocolInvalid)?;
        if self.result.is_some() {
            return Err(ErrorCode::ProtocolInvalid.into());
        }
        if value["type"] == "system" && value["subtype"] == "init" {
            if self.session.is_some() {
                return Err(ErrorCode::ProtocolInvalid.into());
            }
            let tools = value["tools"]
                .as_array()
                .ok_or(ErrorCode::ProtocolInvalid)?;
            let expected: std::collections::BTreeSet<_> =
                TOOLS.into_iter().chain(["ToolSearch"]).collect();
            let actual: std::collections::BTreeSet<_> =
                tools.iter().filter_map(Value::as_str).collect();
            if value["claude_code_version"] != "2.1.311"
                || tools.len() != expected.len()
                || actual != expected
                || value["mcp_servers"]
                    != serde_json::json!([{"name":"akasha-runner","status":"connected","source":"plugin"}])
            {
                return Err(ErrorCode::AdapterUnsupported.into());
            }
            self.session = Some(session.into());
            return Ok(Some(RunnerEvent::Started));
        }
        if self.session.as_deref() != Some(session) {
            return Err(ErrorCode::ProtocolInvalid.into());
        }
        if value
            .get("parent_tool_use_id")
            .is_some_and(|id| !id.is_null())
        {
            return Err(ErrorCode::ProtocolInvalid.into());
        }
        match value["type"].as_str() {
            Some("system") if value["subtype"] == "api_retry" => {
                let attempt = value["attempt"]
                    .as_u64()
                    .ok_or(ErrorCode::ProtocolInvalid)?;
                let maximum = value["max_retries"]
                    .as_u64()
                    .ok_or(ErrorCode::ProtocolInvalid)?;
                if attempt == 0 || attempt > maximum || value["retry_delay_ms"].as_u64().is_none() {
                    return Err(ErrorCode::ProtocolInvalid.into());
                }
                Ok(Some(RunnerEvent::Progress {
                    message: "Provider retry".into(),
                }))
            }
            Some("assistant") => {
                if let Some(error) = value["error"].as_str() {
                    return Err(if error == "authentication_failed" {
                        ErrorCode::AuthRequired
                    } else {
                        ErrorCode::RunnerFailed
                    }
                    .into());
                }
                let blocks = value["message"]["content"]
                    .as_array()
                    .ok_or(ErrorCode::ProtocolInvalid)?;
                for block in blocks.iter().filter(|b| b["type"] == "tool_use") {
                    if !block["name"]
                        .as_str()
                        .is_some_and(|name| TOOLS.contains(&name))
                    {
                        return Err(ErrorCode::ToolDenied.into());
                    }
                    let id = block["id"]
                        .as_str()
                        .filter(|id| super::super::valid_id(id))
                        .ok_or(ErrorCode::ProtocolInvalid)?;
                    if self.tools.insert(id.into(), true).is_some() {
                        return Err(ErrorCode::ProtocolInvalid.into());
                    }
                }
                Ok(None)
            }
            Some("user") => {
                let blocks = value["message"]["content"]
                    .as_array()
                    .filter(|blocks| !blocks.is_empty())
                    .ok_or(ErrorCode::ProtocolInvalid)?;
                for block in blocks {
                    if block["type"] != "tool_result" {
                        return Err(ErrorCode::ProtocolInvalid.into());
                    }
                    let id = block["tool_use_id"]
                        .as_str()
                        .ok_or(ErrorCode::ProtocolInvalid)?;
                    let pending = self
                        .tools
                        .get_mut(id)
                        .filter(|pending| **pending)
                        .ok_or(ErrorCode::ProtocolInvalid)?;
                    *pending = false;
                }
                Ok(None)
            }
            Some("result") => {
                if value["subtype"] != "success" || value["is_error"] != false {
                    return Err(ErrorCode::RunnerFailed.into());
                }
                if self.tools.values().any(|pending| *pending) {
                    return Err(ErrorCode::RunnerIncomplete.into());
                }
                self.result = Some(
                    value["result"]
                        .as_str()
                        .filter(|s| !s.trim().is_empty())
                        .ok_or(ErrorCode::RunnerIncomplete)?
                        .into(),
                );
                Ok(None)
            }
            _ => Err(ErrorCode::ProtocolInvalid.into()),
        }
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
            if let Some(end) = self.buffer.iter().position(|b| *b == b'\n') {
                if end > MAX_FRAME_BYTES {
                    return Err(ErrorCode::OutputLimit.into());
                }
                let bytes: Vec<_> = self.buffer.drain(..=end).collect();
                if let Some(event) = self.frame(&bytes)? {
                    return Ok(Some(event));
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
                let child = self.child.as_mut().ok_or(ErrorCode::RunnerFailed)?;
                match tokio::time::timeout(self.spec.deadline().remaining(), wait_for_exit(child))
                    .await
                {
                    Ok(result) => result.map_err(|_| ErrorCode::RunnerFailed)?,
                    Err(_) => {
                        self.spec.check()?;
                        return Err(ErrorCode::RunnerTimeout.into());
                    }
                };
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
                    return Err(ErrorCode::RunnerFailed.into());
                }
                return Ok(Some(RunnerEvent::Completed {
                    output: self.result.take().ok_or(ErrorCode::RunnerIncomplete)?,
                }));
            }
            let mut out = [0; 8192];
            let mut err = [0; 4096];
            tokio::select! {
                accepted = self.listener.accept() => {
                    let (mut stream, _) = accepted.map_err(|_| ErrorCode::SourceUnavailable)?;
                    let limit = self.spec.deadline().remaining().min(std::time::Duration::from_secs(10));
                    let _ = tokio::time::timeout(limit, self.serve(&mut stream)).await;
                }
                result = self.stdout.read(&mut out), if !self.stdout_closed => {
                    let count = result.map_err(|_| ErrorCode::RunnerFailed)?;
                    self.stdout_closed = count == 0;
                    self.spec.lease().charge_output(count as u64)?;
                    self.buffer.extend_from_slice(&out[..count]);
                }
                result = self.stderr.read(&mut err), if !self.stderr_closed => {
                    let count = result.map_err(|_| ErrorCode::RunnerFailed)?;
                    self.stderr_closed = count == 0;
                    self.stderr_bytes += count;
                    if self.stderr_bytes > 8192 { return Err(ErrorCode::OutputLimit.into()); }
                }
                _ = tokio::time::sleep(self.spec.deadline().remaining()) => {
                    self.spec.check()?;
                    return Err(ErrorCode::RunnerTimeout.into());
                }
            }
        }
    }
    async fn serve(&mut self, stream: &mut UnixStream) -> Result<()> {
        if !stream
            .peer_cred()
            .is_ok_and(|peer| peer.uid() == rustix::process::geteuid().as_raw())
        {
            return Err(ErrorCode::ReadDenied.into());
        }
        let result = async {
            let request: ReadRequest = crate::ipc::read_bounded(stream, MAX_FRAME_BYTES)
                .await
                .map_err(|_| ErrorCode::ProtocolInvalid)?;
            self.spec.check()?;
            self.spec
                .capability()
                .authorize(&request.token, self.spec.lease())?;
            self.context
                .execute(self.spec.lease(), request.request)
                .await
        }
        .await;
        let response = match result {
            Ok(data) => serde_json::json!({"data":data}),
            Err(error) => serde_json::json!({"error":error}),
        };
        let bytes = serde_json::to_vec(&response).map_err(|_| ErrorCode::ProtocolInvalid)?;
        if bytes.len() > MAX_FRAME_BYTES {
            return Err(ErrorCode::OutputLimit.into());
        }
        self.spec.lease().charge_output(bytes.len() as u64)?;
        crate::ipc::write(stream, &response)
            .await
            .map_err(|_| ErrorCode::SourceUnavailable.into())
    }
    fn begin_cleanup(&mut self) {
        drop(self.group.take());
        self.finished = true;
        if let Some(child) = self.child.take() {
            let directory = self.directory.clone();
            let lease = self.spec.lease_handle();
            self.cleanup = Some(reap(child, move || {
                let _lease = lease;
                let _ = std::fs::remove_dir_all(directory);
            }));
        }
    }
    async fn stop(&mut self) -> bool {
        self.begin_cleanup();
        if let Some(status) = match &mut self.cleanup {
            Some(cleanup) => cleanup.wait().await,
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
            let result = self.advance().await;
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
