//! Native Hermes protocol and identity bridge.
use super::{BindingLevel, CapabilityReport, ErrorCode, Result, SelfTest};
mod caller;
pub mod capture;
pub mod export;
mod native;
pub use caller::{HermesCall, HermesCaller, HermesRegistration, profile_key};
pub use native::NativeEvent;
pub(crate) use native::export_session;

pub const SUPPORTED_VERSION: &str = "0.22.5";
pub const BACKGROUND_STREAM: &str = "hermes-native-background-v1";

/// Only observes the native version. Authentication and interoperability need separate checks.
pub async fn probe(binary: &std::path::Path) -> Result<CapabilityReport> {
    let (executable, text) = super::native::version_output(binary).await?;
    let version = text
        .strip_prefix("Hermes Agent v")
        .and_then(|s| s.split_whitespace().next())
        .filter(|s| {
            s.len() <= 128
                && s.bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b".+-".contains(&b))
                && s.split('+').next().is_some_and(|base| {
                    base.split('.').count() == 3
                        && base.split('.').all(|part| {
                            !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit())
                        })
                })
        })
        .ok_or(ErrorCode::AdapterUnsupported)?;
    Ok(CapabilityReport {
        host: crate::config::Host::Hermes,
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
