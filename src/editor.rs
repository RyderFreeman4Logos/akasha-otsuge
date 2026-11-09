//! Bounded feedback for an external investigator; identity comes from a stored binding.
use crate::domain::MemoryPatch;
use crate::ipc::{Error, Result};
use serde::{Deserialize, Serialize};

pub(crate) const PROMPT_VERSION: &str = "memory-editor-v1";

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Target {
    pub memory_id: String,
    pub expected_revision: u64,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct Feedback {
    pub invocation_id: String,
    pub executor: String,
    pub targets: Vec<Target>,
    pub feedback: String,
    pub expires_at_ms: u64,
}

impl Feedback {
    pub(crate) fn validate(&self) -> Result<()> {
        let mut ids = std::collections::BTreeSet::new();
        if !crate::ipc::identifier(&self.invocation_id)
            || !crate::ipc::identifier(&self.executor)
            || self.executor == "caller"
            || self.targets.is_empty()
            || self.targets.len() > 8
            || self.feedback.trim().is_empty()
            || self.feedback.len() > 4096
            || self.expires_at_ms == 0
            || self.targets.iter().any(|t| {
                !crate::ipc::identifier(&t.memory_id)
                    || t.expected_revision == 0
                    || !ids.insert(&t.memory_id)
            })
        {
            return Err(Error("INVALID_FEEDBACK"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Classification {
    MemoryError,
    StaleConditions,
    RetrievalGap,
    ExecutionError,
    Environment,
    RequirementsChanged,
    InsufficientEvidence,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Report {
    pub classification: Classification,
    pub reason: String,
    pub patch: Option<MemoryPatch>,
}

impl Report {
    pub(crate) fn parse(text: &str) -> Result<Self> {
        let report: Self = serde_json::from_str(text).map_err(|_| Error("INVALID_PATCH"))?;
        if report.reason.trim().is_empty()
            || report.reason.len() > 4096
            || report.patch.is_some()
                && !matches!(
                    report.classification,
                    Classification::MemoryError | Classification::StaleConditions
                )
        {
            return Err(Error("INVALID_PATCH"));
        }
        Ok(report)
    }
}
