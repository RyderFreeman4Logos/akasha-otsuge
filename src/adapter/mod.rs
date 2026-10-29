//! Shared host contracts; host integrations authenticate runtime identity.
mod budget;
pub mod claude;
pub use budget::*;
mod runner;
pub use runner::*;
mod binding;
pub use binding::*;
mod context;
pub use context::*;
mod code;
pub use code::*;
pub mod codex;
pub mod hermes;
mod native;
pub mod pi;
use serde::{Deserialize, Serialize, de::DeserializeOwned};
use std::io::{BufRead, Read};

pub const MAX_FRAME_BYTES: usize = 128 * 1024;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "SCREAMING_SNAKE_CASE")]
pub enum ErrorCode {
    InvalidArgument,
    ProtocolInvalid,
    OutputLimit,
    BudgetExhausted,
    DelegationUnavailable,
    Cancelled,
    WorkflowTimeout,
    QueueTimeout,
    ProviderTimeout,
    RunnerTimeout,
    ConfigInvalid,
    BindingUnavailable,
    CallerAmbiguous,
    AdapterUnsupported,
    HistoryGap,
    ExecutorUnavailable,
    AuthRequired,
    RunnerIncomplete,
    RunnerFailed,
    ToolDenied,
    ReadDenied,
    SourceUnavailable,
    BindingConflict,
    SourceChanged,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct AdapterError {
    pub code: ErrorCode,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub source_code: Option<String>,
}

pub type Result<T> = std::result::Result<T, AdapterError>;

impl From<ErrorCode> for AdapterError {
    fn from(code: ErrorCode) -> Self {
        Self {
            code,
            source_code: None,
        }
    }
}

impl From<crate::storage::StorageError> for AdapterError {
    fn from(error: crate::storage::StorageError) -> Self {
        let code = match error.code {
            "READ_DENIED" | "SCOPE_DENIED" => ErrorCode::ReadDenied,
            "BINDING_CONFLICT" => ErrorCode::BindingConflict,
            "BINDING_UNAVAILABLE" => ErrorCode::BindingUnavailable,
            "HISTORY_GAP" => ErrorCode::HistoryGap,
            "CALLER_AMBIGUOUS" => ErrorCode::CallerAmbiguous,
            _ => ErrorCode::SourceUnavailable,
        };
        Self {
            code,
            source_code: Some(error.code.into()),
        }
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(try_from = "RawArgs")]
pub struct ConsultArgs {
    pub hint: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RawArgs {
    hint: Option<String>,
}

impl TryFrom<RawArgs> for ConsultArgs {
    type Error = &'static str;
    fn try_from(raw: RawArgs) -> std::result::Result<Self, Self::Error> {
        if raw.hint.as_ref().is_some_and(|s| s.chars().count() > 512) {
            return Err("hint exceeds 512 Unicode characters");
        }
        Ok(Self {
            hint: raw
                .hint
                .map(|s| s.trim().to_owned())
                .filter(|s| !s.is_empty()),
        })
    }
}

impl ConsultArgs {
    pub fn new(hint: Option<String>) -> Result<Self> {
        Self::try_from(RawArgs { hint }).map_err(|_| ErrorCode::InvalidArgument.into())
    }
    pub fn parse(json: &str) -> Result<Self> {
        if json.len() > 8 * 1024 {
            return Err(ErrorCode::InvalidArgument.into());
        }
        serde_json::from_str(json).map_err(|_| ErrorCode::InvalidArgument.into())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Envelope<T> {
    pub schema_version: u32,
    pub request_id: String,
    pub sequence: u64,
    pub payload: T,
}

pub struct FrameStream {
    request: String,
    next: u64,
    failed: bool,
}
impl FrameStream {
    pub fn handshake(
        &mut self,
        input: &mut impl BufRead,
        required: &[&str],
        optional: &[&str],
    ) -> Result<Negotiated> {
        if self.next != 0 {
            self.failed = true;
            return Err(ErrorCode::ProtocolInvalid.into());
        }
        let report = self.read::<CapabilityReport>(input)?;
        self.failed = true;
        let negotiated = report
            .ok_or(ErrorCode::ProtocolInvalid)?
            .payload
            .negotiate(required, optional)?;
        self.failed = false;
        Ok(negotiated)
    }
    pub fn new(request: &str) -> Result<Self> {
        if !valid_id(request) {
            return Err(ErrorCode::InvalidArgument.into());
        }
        Ok(Self {
            request: request.into(),
            next: 0,
            failed: false,
        })
    }
    pub fn read<T: DeserializeOwned>(
        &mut self,
        input: &mut impl BufRead,
    ) -> Result<Option<Envelope<T>>> {
        if self.failed {
            return Err(ErrorCode::ProtocolInvalid.into());
        }
        self.failed = true;
        let mut line = Vec::new();
        input
            .take((MAX_FRAME_BYTES + 1) as u64)
            .read_until(b'\n', &mut line)
            .map_err(|_| AdapterError::from(ErrorCode::ProtocolInvalid))?;
        if line.len() > MAX_FRAME_BYTES {
            return Err(ErrorCode::OutputLimit.into());
        }
        if line.is_empty() {
            self.failed = false;
            return Ok(None);
        }
        if line.last() != Some(&b'\n') {
            return Err(ErrorCode::ProtocolInvalid.into());
        }
        let frame: Envelope<T> = serde_json::from_slice(&line)
            .map_err(|_| AdapterError::from(ErrorCode::ProtocolInvalid))?;
        if frame.schema_version != 1
            || frame.request_id != self.request
            || frame.sequence != self.next
        {
            return Err(ErrorCode::ProtocolInvalid.into());
        }
        self.next = self.next.checked_add(1).ok_or(ErrorCode::ProtocolInvalid)?;
        self.failed = false;
        Ok(Some(frame))
    }
}

fn valid_id(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 256 && !s.chars().any(char::is_control)
}
