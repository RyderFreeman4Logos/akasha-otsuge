//! Version-pinned native Pi integration.
use super::{BindingLevel, CapabilityReport, ErrorCode, Result, SelfTest};

pub const SUPPORTED_VERSION: &str = "1.4.0";

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
