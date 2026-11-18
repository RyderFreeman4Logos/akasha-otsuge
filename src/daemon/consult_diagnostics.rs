use super::{AdapterError, Error, ErrorCode};
use crate::adapter::ReadRangeRejection;
use serde::Serialize;

#[derive(Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum Stage {
    ContextDispatch,
    ContextBoundary,
    ContextPage,
    RecallCoverage,
    Conclusion,
    ConclusionValidation,
    Runner,
}

#[derive(Serialize)]
struct Entry {
    stage: Stage,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool: Option<&'static str>,
    code: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    reason: Option<ReadRangeRejection>,
    count: u16,
}

#[derive(Default, Serialize)]
pub(super) struct Diagnostics {
    entries: Vec<Entry>,
    omitted: u16,
}

impl Diagnostics {
    pub(super) fn record(&mut self, stage: Stage, tool: Option<&'static str>, value: &str) {
        self.record_reason(stage, tool, value, None);
    }

    pub(super) fn record_reason(
        &mut self,
        stage: Stage,
        tool: Option<&'static str>,
        value: &str,
        reason: Option<ReadRangeRejection>,
    ) {
        // A conclusion or a stored coverage gap can contain arbitrary user text.
        // Keep only canonical adapter codes and the native index readiness code.
        if value.len() > 32 {
            return;
        }
        let code = match value {
            "INDEX_NOT_READY" => "INDEX_NOT_READY",
            _ => {
                let Ok(code) = serde_json::from_value::<ErrorCode>(serde_json::json!(value)) else {
                    return;
                };
                Error::from(AdapterError::from(code)).0
            }
        };
        if let Some(entry) = self.entries.iter_mut().find(|entry| {
            entry.stage == stage
                && entry.tool == tool
                && entry.code == code
                && entry.reason == reason
        }) {
            entry.count = entry.count.saturating_add(1);
        } else if self.entries.len() < 32 {
            self.entries.push(Entry {
                stage,
                tool,
                code,
                reason,
                count: 1,
            });
        } else {
            self.omitted = self.omitted.saturating_add(1);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn diagnostic_counts_omit_private_labels_and_stay_bounded() {
        let mut diagnostics = Diagnostics::default();
        diagnostics.record(Stage::Conclusion, None, "private path or model text");
        assert!(diagnostics.entries.is_empty());

        for code in [
            "INVALID_ARGUMENT",
            "READ_DENIED",
            "HISTORY_GAP",
            "INDEX_NOT_READY",
            "SOURCE_UNAVAILABLE",
        ] {
            for stage in [
                Stage::ContextDispatch,
                Stage::ContextBoundary,
                Stage::ContextPage,
                Stage::RecallCoverage,
                Stage::Conclusion,
                Stage::ConclusionValidation,
                Stage::Runner,
            ] {
                diagnostics.record(stage, None, code);
            }
        }
        assert_eq!(diagnostics.entries.len(), 32);
        assert_eq!(diagnostics.omitted, 3);
        diagnostics.entries[0].count = u16::MAX;
        diagnostics.record(Stage::ContextDispatch, None, "INVALID_ARGUMENT");
        assert_eq!(diagnostics.entries[0].count, u16::MAX);
        diagnostics.omitted = u16::MAX;
        diagnostics.record(Stage::Runner, None, "SOURCE_UNAVAILABLE");
        assert_eq!(diagnostics.omitted, u16::MAX);
    }
}
