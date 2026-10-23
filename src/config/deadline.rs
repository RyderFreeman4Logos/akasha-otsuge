use serde::Serialize;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::time::Instant;

use super::ConfigError;

pub fn duration(value: &str, field: &str) -> Result<Duration, ConfigError> {
    let (number, multiplier) = if let Some(n) = value.strip_suffix("ms") {
        (n, 1)
    } else if let Some(n) = value.strip_suffix('s') {
        (n, 1000)
    } else if let Some(n) = value.strip_suffix('m') {
        (n, 60_000)
    } else if let Some(n) = value.strip_suffix('h') {
        (n, 3_600_000)
    } else {
        return Err(ConfigError::invalid(
            field,
            "expected positive ms, s, m or h duration",
        ));
    };
    let millis = number
        .parse::<u64>()
        .ok()
        .filter(|_| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
        .and_then(|n| n.checked_mul(multiplier))
        .filter(|n| *n > 0)
        .ok_or_else(|| ConfigError::invalid(field, "duration is zero, malformed or too large"))?;
    let result = Duration::from_millis(millis);
    if Instant::now().checked_add(result).is_none() {
        return Err(ConfigError::invalid(
            field,
            "duration exceeds monotonic clock range",
        ));
    }
    Ok(result)
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Layer {
    Queue,
    Startup,
    Provider,
    Runner,
    Workflow,
    Idle,
}

#[derive(Clone, Debug)]
pub struct Deadline {
    layer: Layer,
    trigger: Layer,
    started: Instant,
    started_at_unix_ms: Option<u128>,
    end: Instant,
    configured: Duration,
}

#[derive(Debug, Serialize)]
pub struct DeadlineTrace {
    pub layer: Layer,
    pub started_at_unix_ms: Option<u128>,
    pub configured_ms: u128,
    pub effective_ms: u128,
    pub elapsed_ms: u128,
    pub remaining_ms: u128,
    pub limiting_layer: Layer,
    pub triggered_layer: Option<Layer>,
}

impl Deadline {
    /// Start execution after admission; a queue deadline is never its parent.
    pub fn start(
        layer: Layer,
        configured: Duration,
        outer: Option<&Self>,
    ) -> Result<Self, ConfigError> {
        if configured.is_zero() {
            return Err(ConfigError::invalid(
                "deadline",
                "zero is not an execution timeout",
            ));
        }
        if outer.is_some_and(|parent| parent.layer == Layer::Queue || layer == Layer::Queue) {
            return Err(ConfigError::invalid(
                "deadline",
                "queue and execution budgets must have separate clocks",
            ));
        }
        let started = Instant::now();
        let own_end = started
            .checked_add(configured)
            .ok_or_else(|| ConfigError::invalid("deadline", "deadline overflow"))?;
        let (end, trigger) = match outer {
            Some(parent) if parent.end <= own_end => (parent.end, parent.trigger),
            _ => (own_end, layer),
        };
        Ok(Self {
            layer,
            trigger,
            started,
            started_at_unix_ms: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .ok()
                .map(|d| d.as_millis()),
            end,
            configured,
        })
    }

    pub fn expired(&self) -> Option<Layer> {
        (Instant::now() >= self.end).then_some(self.trigger)
    }

    pub fn remaining(&self) -> Duration {
        self.end.saturating_duration_since(Instant::now())
    }

    pub fn trace(&self) -> DeadlineTrace {
        DeadlineTrace {
            layer: self.layer,
            started_at_unix_ms: self.started_at_unix_ms,
            configured_ms: self.configured.as_millis(),
            effective_ms: self.end.saturating_duration_since(self.started).as_millis(),
            elapsed_ms: Instant::now()
                .saturating_duration_since(self.started)
                .as_millis(),
            remaining_ms: self.remaining().as_millis(),
            limiting_layer: self.trigger,
            triggered_layer: self.expired(),
        }
    }
}
