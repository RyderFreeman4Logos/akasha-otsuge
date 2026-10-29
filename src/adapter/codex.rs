//! Version-pinned native Codex integration.
mod transcript;
pub use transcript::reconcile_transcript;
mod caller;
pub use caller::{CodexCall, CodexCaller, CodexRegistration, NativeEvent};
pub mod hooks;
pub mod installation;
mod runner;
mod runner_policy;
pub use runner::CodexRunner;

pub const TRANSCRIPT_STREAM: &str = "codex-native-v1";

/// Reports an observed CLI version; authentication and model capabilities are separate.
pub async fn probe(binary: &std::path::Path) -> super::Result<super::CapabilityReport> {
    use super::{BindingLevel, CapabilityReport, ErrorCode, SelfTest};
    let (executable, text) = super::native::version_output(binary).await?;
    let version = text
        .trim()
        .strip_prefix("codex-cli ")
        .ok_or(ErrorCode::AdapterUnsupported)?;
    if version.split('.').count() != 3
        || !version
            .split('.')
            .all(|s| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(ErrorCode::AdapterUnsupported.into());
    }
    Ok(CapabilityReport {
        host: crate::config::Host::Codex,
        version: Some(version.into()),
        executable: Some(executable),
        adapter_version: env!("CARGO_PKG_VERSION").into(),
        binding: BindingLevel::Unavailable,
        native_children: false,
        history: crate::capture::CoverageLevel::Unavailable,
        machine_protocol: None,
        cancellation: false,
        outer_timeout: false,
        capabilities: Default::default(),
        self_test: SelfTest::NotRun,
    })
}
