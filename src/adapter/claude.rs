//! Claude Code's native process and event boundary.
mod bridge;
pub mod installation;
pub use bridge::read_bridge;
pub(crate) mod caller;
pub use caller::{ClaudeCaller, NativeCall, NativeEvent};
mod runner;
mod transcript;
use super::native::{ProcessGroup, Reap, reap, resolve, spawn_error, wait_for_exit};
use super::{BindingLevel, CapabilityReport, ErrorCode, Result, SelfTest};
use crate::{capture::CoverageLevel, config::Host};
pub use runner::ClaudeRunner;
use std::{path::Path, time::Duration};
pub(crate) use transcript::transcript_frame;

/// Runs only the CLI version query; it does not certify a model or caller binding.
pub async fn probe(binary: &Path) -> Result<CapabilityReport> {
    let (executable, text) = super::native::version_output(binary, Duration::from_secs(5)).await?;
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
