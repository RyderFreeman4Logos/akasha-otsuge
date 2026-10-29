//! Bounded, one-request-per-connection local management protocol.
use std::path::Path;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use serde_json::{Value, json};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::UnixStream;

pub const VERSION: u32 = 1;
pub const MAX_MESSAGE: usize = 2 * 1024 * 1024;
pub const TRANSPORT_TIMEOUT: Duration = Duration::from_secs(10);

#[derive(Clone, Copy, Debug)]
pub struct Error(pub &'static str);
pub type Result<T> = std::result::Result<T, Error>;
impl From<std::io::Error> for Error {
    fn from(_: std::io::Error) -> Self {
        Self("IO_ERROR")
    }
}
impl From<serde_json::Error> for Error {
    fn from(_: serde_json::Error) -> Self {
        Self("INVALID_REQUEST")
    }
}
impl From<crate::storage::StorageError> for Error {
    fn from(error: crate::storage::StorageError) -> Self {
        Self(error.code)
    }
}
impl From<crate::config::ConfigError> for Error {
    fn from(error: crate::config::ConfigError) -> Self {
        Self(error.code)
    }
}
impl From<crate::adapter::AdapterError> for Error {
    fn from(error: crate::adapter::AdapterError) -> Self {
        use crate::adapter::ErrorCode::*;
        Self(match error.code {
            InvalidArgument => "INVALID_ARGUMENT",
            ProtocolInvalid => "PROTOCOL_INVALID",
            OutputLimit => "OUTPUT_LIMIT",
            BudgetExhausted => "BUDGET_EXHAUSTED",
            DelegationUnavailable => "DELEGATION_UNAVAILABLE",
            Cancelled => "CANCELLED",
            WorkflowTimeout => "WORKFLOW_TIMEOUT",
            QueueTimeout => "QUEUE_TIMEOUT",
            ProviderTimeout => "PROVIDER_TIMEOUT",
            RunnerTimeout => "RUNNER_TIMEOUT",
            ConfigInvalid => "CONFIG_INVALID",
            BindingUnavailable => "BINDING_UNAVAILABLE",
            CallerAmbiguous => "CALLER_AMBIGUOUS",
            AdapterUnsupported => "ADAPTER_UNSUPPORTED",
            HistoryGap => "HISTORY_GAP",
            ExecutorUnavailable => "EXECUTOR_UNAVAILABLE",
            AuthRequired => "AUTH_REQUIRED",
            RunnerIncomplete => "RUNNER_INCOMPLETE",
            RunnerFailed => "RUNNER_FAILED",
            ToolDenied => "TOOL_DENIED",
            ReadDenied => "READ_DENIED",
            SourceUnavailable => "SOURCE_UNAVAILABLE",
            BindingConflict => "BINDING_CONFLICT",
            SourceChanged => "SOURCE_CHANGED",
        })
    }
}
impl Error {
    pub fn exit_code(self) -> u8 {
        match self.0 {
            "INVALID_ARGUMENT" | "INVALID_REQUEST" | "CONFIG_INVALID" => 2,
            "DAEMON_UNAVAILABLE" => 3,
            "RESOURCE_BUSY" => 4,
            _ => 1,
        }
    }
    pub fn value(self) -> Value {
        let message = match self.0 {
            "INVALID_ARGUMENT" => "Use --help for supported arguments",
            "IO_ERROR" => "Could not complete input or output",
            "DAEMON_UNAVAILABLE" => "The local daemon is unavailable",
            _ => "The operation could not be completed",
        };
        json!({"code":self.0,"message":message})
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Request {
    pub protocol_version: u32,
    pub request_id: String,
    #[serde(default)]
    pub deadline_ms: Option<u64>,
    pub command: Command,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum Command {
    Hermes {
        event: crate::adapter::hermes::NativeEvent,
    },
    Codex {
        event: crate::adapter::codex::NativeEvent,
    },
    Status,
    Doctor,
    Stop,
    ConfigValidate,
    ConfigReload,
    ConfigStatus,
    ConfigShow,
    Reconcile {
        binding: CaptureInput,
        path: std::path::PathBuf,
        idempotency_key: String,
    },
    ModelRun {
        plan: crate::provider::Plan,
        idempotency_key: String,
    },
    RetrievalRun {
        plan: crate::retrieval::Request,
        idempotency_key: String,
    },
    IndexStatus,
    OriginalShow {
        range: crate::domain::OriginalRange,
    },
    JobShow {
        id: String,
    },
    JobCancel {
        id: String,
    },
    JobList {
        after: Option<String>,
    },
    MemoryShow {
        id: String,
        at: Option<u64>,
    },
    MemoryHistory {
        id: String,
        from: u64,
        to: Option<u64>,
        cursor: Option<crate::domain::HistoryCursor>,
    },
    StateShow {
        scope: crate::domain::Scope,
        at: u64,
        cursor: Option<crate::domain::StateCursor>,
    },
    StateDiff {
        scope: crate::domain::Scope,
        from: u64,
        to: u64,
        cursor: Option<crate::domain::StateCursor>,
    },
    ReceiptShow {
        id: String,
    },
    ReceiptReplay {
        id: String,
    },
    AccessSet {
        scope: Option<crate::domain::Scope>,
    },
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureInput {
    pub source: crate::capture::SourceIdentity,
    pub scope: crate::domain::Scope,
    pub stream: String,
    pub coverage: crate::capture::CoverageLevel,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub host: Option<crate::config::Host>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub until_event: Option<String>,
}
impl CaptureInput {
    pub(crate) fn binding(&self) -> Result<crate::capture::CaptureBinding> {
        if self
            .until_event
            .as_ref()
            .is_some_and(|id| !identifier(id) || self.host.is_none())
        {
            return Err(Error("INVALID_CAPTURE_BINDING"));
        }
        if let Some(host) = self.host {
            let host_valid = match host {
                crate::config::Host::Codex => {
                    self.source.adapter == "codex"
                        && self.source.agent == "main"
                        && self.stream == crate::adapter::codex::TRANSCRIPT_STREAM
                }
                crate::config::Host::Hermes => {
                    self.source.adapter == "hermes"
                        && (self.source.agent == "main" || self.source.agent == self.source.session)
                        && self.stream == crate::adapter::hermes::BACKGROUND_STREAM
                }
                _ => return Err(Error("ADAPTER_UNSUPPORTED")),
            };
            if !host_valid
                || self.source.session != self.source.thread
                || self.coverage != crate::capture::CoverageLevel::VisibleText
            {
                return Err(Error("INVALID_CAPTURE_BINDING"));
            }
        }
        Ok(crate::capture::CaptureBinding::from_source(
            self.source.clone(),
            self.scope.clone(),
            &self.stream,
            self.coverage,
        )?)
    }
}

pub fn now_ms() -> Result<u64> {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .ok()
        .and_then(|d| u64::try_from(d.as_millis()).ok())
        .ok_or(Error("CLOCK_ERROR"))
}

pub(crate) fn identifier(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

pub(crate) fn hello() -> Value {
    json!({"protocol_version":VERSION,"capabilities":["daemon", "config", "capture_jobs", "model_jobs", "retrieval_jobs", "index_status", "original_range"],"max_message_bytes":MAX_MESSAGE})
}

pub(crate) fn response(request: Option<&str>, result: Result<Value>) -> Value {
    match result {
        Ok(data) => json!({"schema_version":VERSION,"request_id":request,"sequence":1,"data":data}),
        Err(error) => {
            json!({"schema_version":VERSION,"request_id":request,"sequence":1,"error":error.value()})
        }
    }
}

pub(crate) async fn read<T: DeserializeOwned>(stream: &mut UnixStream) -> Result<T> {
    let size = stream.read_u32().await? as usize;
    if size == 0 || size > MAX_MESSAGE {
        return Err(Error("MESSAGE_TOO_LARGE"));
    }
    let mut bytes = vec![0; size];
    stream.read_exact(&mut bytes).await?;
    Ok(serde_json::from_slice(&bytes)?)
}
pub(crate) async fn write(stream: &mut UnixStream, value: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(value)?;
    if bytes.len() > MAX_MESSAGE {
        return Err(Error("RESULT_TOO_LARGE"));
    }
    stream.write_u32(bytes.len() as u32).await?;
    stream.write_all(&bytes).await?;
    Ok(())
}

async fn exchange(path: &Path, request: &Request) -> Result<(UnixStream, Value)> {
    tokio::time::timeout(TRANSPORT_TIMEOUT, async {
        let mut stream = UnixStream::connect(path)
            .await
            .map_err(|_| Error("DAEMON_UNAVAILABLE"))?;
        if stream.peer_cred()?.uid() != rustix::process::geteuid().as_raw() {
            return Err(Error("PEER_DENIED"));
        }
        let greeting: Value = read(&mut stream).await?;
        if greeting["protocol_version"] != VERSION || greeting["max_message_bytes"] != MAX_MESSAGE {
            return Err(Error("PROTOCOL_VERSION"));
        }
        if greeting["error"]["code"] == "RESOURCE_BUSY" {
            return Ok((
                stream,
                response(Some(&request.request_id), Err(Error("RESOURCE_BUSY"))),
            ));
        }
        write(&mut stream, request).await?;
        let result: Value = read(&mut stream).await?;
        if result["schema_version"] != VERSION
            || result["request_id"].as_str() != Some(&request.request_id)
            || result["sequence"] != 1
        {
            return Err(Error("PROTOCOL_ERROR"));
        }
        Ok((stream, result))
    })
    .await
    .map_err(|_| Error("IPC_TIMEOUT"))?
}

pub async fn call(path: &Path, request: &Request) -> Result<Value> {
    Ok(exchange(path, request).await?.1)
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct DeliveryAck {
    pub protocol_version: u32,
    pub receipt_id: String,
    pub output_sha256: String,
}

pub async fn replay(
    path: &Path,
    request: &Request,
    emit: impl FnOnce(&[u8]) -> Result<()>,
) -> Result<Value> {
    let (mut stream, response) = exchange(path, request).await?;
    if response.get("error").is_some() {
        return Ok(response);
    }
    let bytes = response["data"]["output"]
        .as_str()
        .ok_or(Error("PROTOCOL_ERROR"))?
        .as_bytes();
    let receipt_id = response["data"]["receipt_id"]
        .as_str()
        .ok_or(Error("PROTOCOL_ERROR"))?;
    let digest = crate::storage::hash(bytes);
    if response["data"]["output_sha256"] != digest {
        return Err(Error("PROTOCOL_ERROR"));
    }
    emit(bytes)?;
    tokio::time::timeout(TRANSPORT_TIMEOUT, async {
        write(
            &mut stream,
            &DeliveryAck {
                protocol_version: VERSION,
                receipt_id: receipt_id.into(),
                output_sha256: digest,
            },
        )
        .await?;
        let acknowledgement: Value = read(&mut stream).await?;
        if acknowledgement["schema_version"] != VERSION
            || acknowledgement["request_id"] != request.request_id
            || acknowledgement["sequence"] != 2
        {
            return Err(Error("PROTOCOL_ERROR"));
        }
        Ok(acknowledgement)
    })
    .await
    .map_err(|_| Error("IPC_TIMEOUT"))?
}
