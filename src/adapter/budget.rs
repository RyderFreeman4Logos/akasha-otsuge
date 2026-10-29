use super::*;
use crate::config::deadline::{Deadline, Layer};
use std::sync::{Arc, Mutex};
use tokio::sync::{OwnedSemaphorePermit, Semaphore};

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BudgetLimits {
    pub starts: u32,
    pub depth: u32,
    pub reads: u64,
    pub read_bytes: u64,
    pub output_bytes: u64,
    pub tokens: u64,
}
impl Default for BudgetLimits {
    fn default() -> Self {
        Self {
            starts: 3,
            depth: 1,
            reads: 128,
            read_bytes: 4 * 1024 * 1024,
            output_bytes: 1024 * 1024,
            tokens: 128_000,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, PartialEq, Serialize)]
pub struct BudgetUsage {
    pub starts: u32,
    pub reads: u64,
    pub read_bytes: u64,
    pub output_bytes: u64,
    pub tokens: u64,
}
#[derive(Clone, Debug)]
pub struct RunBudget(Arc<BudgetState>);
#[derive(Debug)]
struct BudgetState {
    origin: String,
    fence: String,
    slots: Arc<Semaphore>,
    limits: BudgetLimits,
    deadline: Deadline,
    state: Mutex<Counters>,
    stopped: tokio::sync::watch::Sender<()>,
}
#[derive(Debug, Default)]
struct Counters {
    usage: BudgetUsage,
    cancelled: bool,
    exhausted: bool,
}
#[derive(Debug)]
pub struct RunLease {
    budget: RunBudget,
    depth: u32,
    reported_tokens: Mutex<(u64, u64)>,
    _permit: OwnedSemaphorePermit,
}
impl RunBudget {
    pub fn new(
        origin: &str,
        fence: &str,
        slots: Arc<Semaphore>,
        limits: BudgetLimits,
        deadline: Deadline,
    ) -> Result<Self> {
        if !valid_id(origin)
            || !valid_id(fence)
            || limits.starts == 0
            || limits.depth >= limits.starts
            || limits.reads == 0
            || limits.read_bytes == 0
            || limits.output_bytes == 0
            || limits.tokens == 0
        {
            return Err(ErrorCode::ConfigInvalid.into());
        }
        Ok(Self(Arc::new(BudgetState {
            origin: origin.into(),
            fence: fence.into(),
            slots,
            limits,
            deadline,
            state: Mutex::new(Counters::default()),
            stopped: tokio::sync::watch::channel(()).0,
        })))
    }
    pub fn start(&self) -> Result<RunLease> {
        self.reserve(0)
    }
    pub(crate) fn reserve(&self, depth: u32) -> Result<RunLease> {
        let mut state = self
            .0
            .state
            .lock()
            .map_err(|_| ErrorCode::BudgetExhausted)?;
        self.check(&state)?;
        if depth > self.0.limits.depth || state.usage.starts >= self.0.limits.starts {
            return Err(ErrorCode::BudgetExhausted.into());
        }
        let permit = self
            .0
            .slots
            .clone()
            .try_acquire_owned()
            .map_err(|_| AdapterError::from(ErrorCode::DelegationUnavailable))?;
        state.usage.starts += 1;
        Ok(RunLease {
            budget: self.clone(),
            depth,
            reported_tokens: Mutex::new((0, 0)),
            _permit: permit,
        })
    }
    pub fn usage(&self) -> Result<BudgetUsage> {
        Ok(self
            .0
            .state
            .lock()
            .map_err(|_| ErrorCode::BudgetExhausted)?
            .usage)
    }
    pub fn limits(&self) -> BudgetLimits {
        self.0.limits
    }
    pub fn deadline(&self) -> &Deadline {
        &self.0.deadline
    }
    pub fn cancel(&self) {
        // Poisoning already denies every subsequent operation.
        if let Ok(mut state) = self.0.state.lock() {
            state.cancelled = true;
            self.0.stopped.send_replace(());
        }
    }
    fn check(&self, state: &Counters) -> Result<()> {
        Self::check_deadline(state, &self.0.deadline)
    }
    fn check_deadline(state: &Counters, deadline: &Deadline) -> Result<()> {
        if state.cancelled {
            return Err(ErrorCode::Cancelled.into());
        }
        if state.exhausted {
            return Err(ErrorCode::BudgetExhausted.into());
        }
        if let Some(layer) = deadline.expired() {
            return Err(match layer {
                Layer::Workflow => ErrorCode::WorkflowTimeout,
                Layer::Provider => ErrorCode::ProviderTimeout,
                Layer::Queue => ErrorCode::QueueTimeout,
                _ => ErrorCode::RunnerTimeout,
            }
            .into());
        }
        Ok(())
    }
}
impl RunLease {
    pub fn delegate(&self) -> Result<Self> {
        self.budget.reserve(
            self.depth
                .checked_add(1)
                .ok_or(ErrorCode::BudgetExhausted)?,
        )
    }
    pub fn origin(&self) -> &str {
        &self.budget.0.origin
    }
    pub fn fence(&self) -> &str {
        &self.budget.0.fence
    }
    pub fn depth(&self) -> u32 {
        self.depth
    }
    pub fn charge_read(&self, bytes: u64) -> Result<()> {
        let mut state = self
            .budget
            .0
            .state
            .lock()
            .map_err(|_| ErrorCode::BudgetExhausted)?;
        self.budget.check(&state)?;
        let reads = state
            .usage
            .reads
            .checked_add(1)
            .ok_or(ErrorCode::BudgetExhausted)?;
        let total = state
            .usage
            .read_bytes
            .checked_add(bytes)
            .ok_or(ErrorCode::BudgetExhausted)?;
        if reads > self.budget.0.limits.reads || total > self.budget.0.limits.read_bytes {
            return Err(ErrorCode::BudgetExhausted.into());
        }
        state.usage.reads = reads;
        state.usage.read_bytes = total;
        Ok(())
    }
    pub fn charge_output(&self, bytes: u64) -> Result<()> {
        let mut state = self
            .budget
            .0
            .state
            .lock()
            .map_err(|_| ErrorCode::BudgetExhausted)?;
        self.budget.check(&state)?;
        let total = state
            .usage
            .output_bytes
            .checked_add(bytes)
            .ok_or(ErrorCode::BudgetExhausted)?;
        if total > self.budget.0.limits.output_bytes {
            return Err(ErrorCode::BudgetExhausted.into());
        }
        state.usage.output_bytes = total;
        Ok(())
    }
    pub fn check(&self) -> Result<()> {
        self.check_deadline(self.deadline())
    }
    pub fn charge_tokens(&self, input: u64, output: u64) -> Result<()> {
        let mut state = self
            .budget
            .0
            .state
            .lock()
            .map_err(|_| ErrorCode::BudgetExhausted)?;
        let total = state
            .usage
            .tokens
            .checked_add(input)
            .and_then(|n| n.checked_add(output));
        // Native usage has already occurred; retain overshoot and stop every branch.
        state.usage.tokens = total.unwrap_or(u64::MAX);
        if total.is_none() || state.usage.tokens > self.budget.0.limits.tokens {
            state.exhausted = true;
            self.budget.0.stopped.send_replace(());
        }
        self.budget.check(&state)
    }
    pub(super) fn interrupted(&self) -> futures::future::BoxFuture<'static, AdapterError> {
        let budget = self.budget.clone();
        Box::pin(async move {
            let mut stopped = budget.0.stopped.subscribe();
            loop {
                let result = budget
                    .0
                    .state
                    .lock()
                    .map_err(|_| AdapterError::from(ErrorCode::BudgetExhausted))
                    .and_then(|state| budget.check(&state));
                if let Err(error) = result {
                    return error;
                }
                if stopped.changed().await.is_err() {
                    return ErrorCode::Cancelled.into();
                }
            }
        })
    }
    pub fn report_tokens(&self, input: u64, output: u64) -> Result<()> {
        let mut previous = self
            .reported_tokens
            .lock()
            .map_err(|_| ErrorCode::BudgetExhausted)?;
        let incoming = input
            .checked_sub(previous.0)
            .ok_or(ErrorCode::ProtocolInvalid)?;
        let outgoing = output
            .checked_sub(previous.1)
            .ok_or(ErrorCode::ProtocolInvalid)?;
        let result = self.charge_tokens(incoming, outgoing);
        *previous = (input, output);
        result
    }
    pub(super) fn check_deadline(&self, deadline: &Deadline) -> Result<()> {
        let state = self
            .budget
            .0
            .state
            .lock()
            .map_err(|_| ErrorCode::BudgetExhausted)?;
        RunBudget::check_deadline(&state, deadline)
    }
    pub fn deadline(&self) -> &Deadline {
        self.budget.deadline()
    }
}
