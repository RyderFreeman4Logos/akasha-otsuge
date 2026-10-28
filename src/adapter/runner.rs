use super::*;
use crate::config::deadline::{Deadline, Layer};
use crate::{capture::CoverageLevel, config::Host};
use futures::future::BoxFuture;
use std::time::Duration;
use std::{collections::BTreeSet, path::PathBuf};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum BindingLevel {
    Unavailable,
    SessionOnly,
    ExactTurn,
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum SelfTest {
    NotRun,
    Passed {
        at_unix_ms: u64,
    },
    Failed {
        at_unix_ms: u64,
        error: AdapterError,
    },
}
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CapabilityReport {
    pub host: Host,
    pub version: Option<String>,
    pub executable: Option<PathBuf>,
    pub adapter_version: String,
    pub binding: BindingLevel,
    pub native_children: bool,
    pub history: CoverageLevel,
    pub machine_protocol: Option<String>,
    pub cancellation: bool,
    pub outer_timeout: bool,
    pub capabilities: BTreeSet<String>,
    pub self_test: SelfTest,
}
#[derive(Debug, Serialize)]
pub struct Negotiated {
    pub unsupported_optional: Vec<String>,
}
impl CapabilityReport {
    pub fn negotiate(&self, required: &[&str], optional: &[&str]) -> Result<Negotiated> {
        if required.len() + optional.len() > 64
            || self.capabilities.len() > 64
            || !required.iter().chain(optional).all(|s| valid_id(s))
        {
            return Err(ErrorCode::ProtocolInvalid.into());
        }
        let supports = |s: &str| known_capability(s) && self.capabilities.contains(s);
        if !required.iter().all(|s| supports(s)) {
            return Err(ErrorCode::AdapterUnsupported.into());
        }
        Ok(Negotiated {
            unsupported_optional: optional
                .iter()
                .filter(|s| !supports(s))
                .map(|s| (*s).into())
                .collect(),
        })
    }
    pub fn require_binding(&self, level: BindingLevel, native_child: bool) -> Result<()> {
        if self.version.as_ref().is_none_or(|s| !valid_id(s))
            || self.executable.as_ref().is_none_or(|p| !p.is_absolute())
            || !valid_id(&self.adapter_version)
            || self.binding == BindingLevel::Unavailable
            || level == BindingLevel::Unavailable
            || level == BindingLevel::ExactTurn && self.binding != BindingLevel::ExactTurn
            || native_child && !self.native_children
        {
            return Err(ErrorCode::AdapterUnsupported.into());
        }
        Ok(())
    }
}

fn known_capability(s: &str) -> bool {
    ToolPolicy::authorize(s).is_ok()
        || matches!(
            s,
            "runner.events"
                | "runner.cancel"
                | "binding.session"
                | "binding.turn"
                | "binding.children"
                | "history.raw"
        )
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "event", rename_all = "snake_case", deny_unknown_fields)]
pub enum RunnerEvent {
    Accepted,
    Started,
    Progress {
        message: String,
    },
    Tool {
        name: String,
    },
    Usage {
        input_tokens: u64,
        output_tokens: u64,
    },
    Completed {
        output: String,
    },
    Failed {
        error: AdapterError,
    },
}

pub struct RunCollector<'a> {
    lease: &'a RunLease,
    max_chars: usize,
    output: Option<String>,
    error: Option<AdapterError>,
    started: bool,
    terminal: bool,
}
impl<'a> RunCollector<'a> {
    pub fn new(lease: &'a RunLease, max_chars: usize) -> Self {
        Self {
            lease,
            max_chars,
            output: None,
            error: None,
            started: false,
            terminal: false,
        }
    }
    pub fn push(&mut self, event: RunnerEvent) -> Result<()> {
        let result = self.accept(event);
        if let Err(error) = &result {
            self.error = Some(error.clone());
            self.output = None;
            self.terminal = true;
        }
        result
    }
    fn accept(&mut self, event: RunnerEvent) -> Result<()> {
        if self.terminal {
            return Err(ErrorCode::ProtocolInvalid.into());
        }
        let text = match &event {
            RunnerEvent::Progress { message } => message.as_str(),
            RunnerEvent::Tool { name } => name.as_str(),
            RunnerEvent::Completed { output } => output.as_str(),
            _ => "",
        };
        if text.len() > MAX_FRAME_BYTES {
            return Err(ErrorCode::OutputLimit.into());
        }
        let bytes = serde_json::to_vec(&event)
            .map_err(|_| ErrorCode::ProtocolInvalid)?
            .len();
        if bytes > MAX_FRAME_BYTES {
            return Err(ErrorCode::OutputLimit.into());
        }
        self.lease.charge_output(bytes as u64)?;
        match event {
            RunnerEvent::Started => {
                if self.started {
                    return Err(ErrorCode::ProtocolInvalid.into());
                }
                self.started = true;
            }
            RunnerEvent::Accepted if self.started => return Err(ErrorCode::ProtocolInvalid.into()),
            RunnerEvent::Tool { name } => ToolPolicy::authorize(&name)?,
            RunnerEvent::Completed { output } => {
                if !self.started || output.trim().is_empty() {
                    return Err(ErrorCode::RunnerIncomplete.into());
                }
                if output.chars().count() > self.max_chars {
                    return Err(ErrorCode::OutputLimit.into());
                }
                self.output = Some(output);
                self.terminal = true;
            }
            RunnerEvent::Failed { error } => {
                self.error = Some(error);
                self.terminal = true;
            }
            _ => {}
        }
        Ok(())
    }
    pub fn max_chars(&self) -> usize {
        self.max_chars
    }
    pub fn finish(self, exit: Option<i32>) -> Result<String> {
        self.lease.check()?;
        if let Some(error) = self.error {
            return Err(error);
        }
        if exit != Some(0) {
            return Err(ErrorCode::RunnerFailed.into());
        }
        self.output
            .ok_or_else(|| ErrorCode::RunnerIncomplete.into())
    }
}
pub struct ToolPolicy;
impl ToolPolicy {
    pub fn enforcement() -> &'static str {
        "tool_policy"
    }
    pub fn authorize(tool: &str) -> Result<()> {
        if matches!(
            tool,
            "context.describe"
                | "context.list"
                | "context.read"
                | "context.search"
                | "memory.search"
                | "memory.get"
                | "code.search"
                | "code.read"
                | "delegate"
        ) {
            Ok(())
        } else {
            Err(ErrorCode::ToolDenied.into())
        }
    }
}

pub trait CallerAdapter: Send + Sync {
    fn probe(&self) -> Result<CapabilityReport>;
    fn bind<'a>(&'a self, runtime_call: &'a str) -> BoxFuture<'a, Result<Invocation>>;
}
pub trait AgentRunner: Send {
    fn probe(&self) -> Result<CapabilityReport>;
    fn start<'a>(
        &'a mut self,
        spec: RunSpec,
        context: &'a mut dyn ContextSource,
    ) -> BoxFuture<'a, Result<Box<dyn RunHandle + 'a>>>;
}
pub trait RunHandle: Send {
    fn next(&mut self) -> BoxFuture<'_, Result<Option<RunnerEvent>>>;
    fn cancel(&mut self, reason: CancelReason) -> BoxFuture<'_, Result<CancelAck>>;
    fn exit_status(&mut self) -> BoxFuture<'_, Result<Option<i32>>>;
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CancelReason {
    User,
    Deadline,
    PermissionRevoked,
    Shutdown,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CancelAck {
    pub acknowledged: bool,
}

pub struct ReadCapability {
    token: String,
    origin: String,
    fence: String,
}
impl std::fmt::Debug for ReadCapability {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("ReadCapability([redacted])")
    }
}
impl ReadCapability {
    pub fn issue(invocation: &Invocation) -> Result<Self> {
        let mut bytes = [0u8; 32];
        std::fs::File::open("/dev/urandom")
            .and_then(|mut f| f.read_exact(&mut bytes))
            .map_err(|_| ErrorCode::SourceUnavailable)?;
        Ok(Self {
            token: bytes.iter().map(|v| format!("{v:02x}")).collect(),
            origin: invocation.id().into(),
            fence: invocation.fence_hash(),
        })
    }
    pub fn transport_token(&self) -> &str {
        &self.token
    }
    pub fn origin(&self) -> &str {
        &self.origin
    }
    pub fn fence(&self) -> &str {
        &self.fence
    }
    pub fn authorize(&self, token: &str, lease: &RunLease) -> Result<()> {
        lease.check()?;
        if token.len() != self.token.len()
            || token
                .bytes()
                .zip(self.token.bytes())
                .fold(0, |difference, (a, b)| difference | (a ^ b))
                != 0
            || lease.origin() != self.origin
            || lease.fence() != self.fence
        {
            return Err(ErrorCode::ReadDenied.into());
        }
        Ok(())
    }
}
#[derive(Debug)]
pub struct RunOptions {
    pub job_id: String,
    pub executor_profile: String,
    pub instructions_version: String,
    pub runner_timeout: Duration,
    pub args: ConsultArgs,
}
#[derive(Debug)]
pub struct RunSpec {
    options: RunOptions,
    origin: String,
    config_generation: u64,
    capability: ReadCapability,
    lease: std::sync::Arc<RunLease>,
    deadline: Deadline,
}
impl RunSpec {
    pub fn new(
        invocation: &Invocation,
        mut options: RunOptions,
        capability: ReadCapability,
        lease: RunLease,
    ) -> Result<Self> {
        if ![
            &options.job_id,
            &options.executor_profile,
            &options.instructions_version,
        ]
        .into_iter()
        .all(|s| valid_id(s))
        {
            return Err(ErrorCode::InvalidArgument.into());
        }
        options.args = ConsultArgs::new(options.args.hint)?;
        capability.authorize(capability.transport_token(), &lease)?;
        if lease.origin() != invocation.id() || lease.fence() != invocation.fence_hash() {
            return Err(ErrorCode::ReadDenied.into());
        }
        let deadline = Deadline::start(
            Layer::Runner,
            options.runner_timeout,
            Some(lease.deadline()),
        )
        .map_err(|_| ErrorCode::ConfigInvalid)?;
        Ok(Self {
            options,
            origin: invocation.id().into(),
            config_generation: invocation.diagnostic().config_generation,
            capability,
            lease: std::sync::Arc::new(lease),
            deadline,
        })
    }
    pub fn options(&self) -> &RunOptions {
        &self.options
    }
    pub fn lease(&self) -> &RunLease {
        &self.lease
    }
    pub fn lease_handle(&self) -> std::sync::Arc<RunLease> {
        std::sync::Arc::clone(&self.lease)
    }
    pub fn capability(&self) -> &ReadCapability {
        &self.capability
    }
    pub fn deadline(&self) -> &Deadline {
        &self.deadline
    }
    pub fn origin_invocation_id(&self) -> &str {
        &self.origin
    }
    pub fn config_generation(&self) -> u64 {
        self.config_generation
    }
    pub fn check(&self) -> Result<()> {
        self.lease.check()?;
        if let Some(layer) = self.deadline.expired() {
            return Err(if layer == Layer::Workflow {
                ErrorCode::WorkflowTimeout
            } else {
                ErrorCode::RunnerTimeout
            }
            .into());
        }
        Ok(())
    }
}
