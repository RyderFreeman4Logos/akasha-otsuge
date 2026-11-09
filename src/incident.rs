//! Reported investigation signals; grouping and priority do not establish causality.
use crate::config::IncidentTrigger;
use crate::domain::{ReviewState, Scope};
use crate::editor::Target;
use crate::ipc::{Error, Result, identifier};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Signal {
    pub kind: IncidentTrigger,
    pub key: String,
    pub reason: String,
    #[serde(default)]
    pub targets: Vec<Target>,
}

impl Signal {
    pub(crate) fn validate(&self) -> Result<()> {
        let mut targets = std::collections::BTreeSet::new();
        if !identifier(&self.key)
            || self.reason.trim().is_empty()
            || self.reason.len() > 4096
            || self.targets.len() > 8
            || self.targets.iter().any(|target| {
                !identifier(&target.memory_id)
                    || target.expected_revision == 0
                    || !targets.insert(&target.memory_id)
            })
        {
            return Err(Error("INVALID_INCIDENT"));
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Priority {
    Normal,
    High,
    Urgent,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Investigation {
    Pending,
    Running,
    Finished,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Record {
    pub schema_version: u32,
    pub id: String,
    pub principal: String,
    pub scope: Scope,
    pub invocation_id: String,
    pub signal: Signal,
    pub first_outcome: String,
    pub last_outcome: String,
    pub occurrences: u64,
    pub first_report_at_ms: u64,
    pub last_report_at_ms: u64,
    pub expires_at_ms: u64,
    pub priority: Priority,
    pub priority_reasons: Vec<String>,
    pub state: ReviewState,
    pub config_generation: u64,
    pub investigation: Investigation,
    pub job_id: Option<String>,
    pub error: Option<String>,
    pub operator_reason: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub(crate) struct Event {
    pub event_id: String,
    pub invocation_id: String,
}
