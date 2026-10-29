//! Version-pinned native Pi integration.
use super::{BindingLevel, CapabilityReport, ErrorCode, Result, SelfTest};
mod transcript;
pub use transcript::reconcile_transcript;
mod caller;
pub mod context_bridge;
pub mod installation;
mod runner;
pub use caller::{PiCall, PiCaller, PiRegistration, profile_key};
pub use runner::PiRunner;
pub(crate) use transcript::header as session_header;

/// Same-user private transport. Registration authority is never a model tool parameter.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeEvent {
    Register {
        binary: std::path::PathBuf,
        agent_dir: std::path::PathBuf,
        session_id: String,
        transcript_path: std::path::PathBuf,
        cwd: std::path::PathBuf,
    },
    Bind {
        registration: String,
        call: PiCall,
    },
    Sync {
        registration: String,
        until_entry: String,
    },
}

pub const SUPPORTED_VERSION: &str = "1.4.0";
pub const TRANSCRIPT_STREAM: &str = "pi-native-tree-v1";

/// Observe only the native version; authentication and extension trust need separate checks.
pub async fn probe(binary: &std::path::Path) -> Result<CapabilityReport> {
    let (executable, text) =
        super::native::version_output(binary, std::time::Duration::from_secs(5)).await?;
    let version = text.trim();
    if version.split('.').count() != 3
        || !version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()))
    {
        return Err(ErrorCode::AdapterUnsupported.into());
    }
    Ok(CapabilityReport {
        host: crate::config::Host::Pi,
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
