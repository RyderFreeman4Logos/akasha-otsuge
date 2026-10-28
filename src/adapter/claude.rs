//! Claude Code's native process and event boundary.
mod bridge;
pub mod installation;
pub use bridge::read_bridge;
pub(crate) mod caller;
pub use caller::{ClaudeCaller, NativeCall, NativeEvent};
mod runner;
mod transcript;
use super::{BindingLevel, CapabilityReport, ErrorCode, Result, SelfTest};
use crate::{capture::CoverageLevel, config::Host};
pub use runner::ClaudeRunner;
use rustix::process::{Pid, Signal, kill_process_group};
use std::{
    os::unix::{fs::PermissionsExt, process::CommandExt},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::io::{AsyncRead, AsyncReadExt};
pub(crate) use transcript::transcript_frame;

/// Runs only the CLI version query; it does not certify a model or caller binding.
pub async fn probe(binary: &Path) -> Result<CapabilityReport> {
    let executable = resolve(binary)?;
    let mut command = std::process::Command::new(&executable);
    command
        .arg("--version")
        .stdin(Stdio::null())
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
    let stdout = child.stdout.take().ok_or(ErrorCode::RunnerFailed)?;
    let stderr = child.stderr.take().ok_or(ErrorCode::RunnerFailed)?;
    let result = tokio::time::timeout(Duration::from_secs(5), async {
        tokio::try_join!(bounded(stdout), bounded(stderr), async {
            child
                .wait()
                .await
                .map_err(|_| ErrorCode::RunnerFailed.into())
        })
    })
    .await
    .map_err(|_| ErrorCode::RunnerTimeout);
    drop(group);
    let _ = child.start_kill();
    let _ = child.wait().await;
    let (stdout, _, status) = result??;
    if !status.success() {
        return Err(ErrorCode::RunnerFailed.into());
    }
    let text = std::str::from_utf8(&stdout).map_err(|_| ErrorCode::AdapterUnsupported)?;
    let version = text
        .trim()
        .strip_suffix(" (Claude Code)")
        .ok_or(ErrorCode::AdapterUnsupported)?;
    if version.split('.').count() != 3
        || !version
            .split('.')
            .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(ErrorCode::AdapterUnsupported.into());
    }
    Ok(CapabilityReport {
        host: Host::ClaudeCode,
        version: Some(version.into()),
        executable: Some(executable),
        adapter_version: env!("CARGO_PKG_VERSION").into(),
        binding: BindingLevel::Unavailable,
        native_children: false,
        history: CoverageLevel::Unavailable,
        machine_protocol: None,
        cancellation: false,
        outer_timeout: false,
        capabilities: Default::default(),
        self_test: SelfTest::NotRun,
    })
}

fn resolve(binary: &Path) -> Result<PathBuf> {
    let candidates = if binary.components().count() > 1 || binary.is_absolute() {
        vec![binary.to_owned()]
    } else {
        std::env::split_paths(&std::env::var_os("PATH").unwrap_or_default())
            .map(|dir| dir.join(binary))
            .collect()
    };
    candidates
        .into_iter()
        .find(|p| {
            p.metadata()
                .is_ok_and(|m| m.is_file() && m.permissions().mode() & 0o111 != 0)
        })
        .and_then(|p| p.canonicalize().ok())
        .ok_or_else(|| ErrorCode::ExecutorUnavailable.into())
}

struct ProcessGroup(Pid);
impl Drop for ProcessGroup {
    fn drop(&mut self) {
        let _ = kill_process_group(self.0, Signal::KILL);
    }
}

async fn bounded(input: impl AsyncRead + Unpin) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    input
        .take(4097)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| ErrorCode::RunnerFailed)?;
    if bytes.len() > 4096 {
        return Err(ErrorCode::OutputLimit.into());
    }
    Ok(bytes)
}
