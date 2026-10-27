use serde::{Deserialize, Serialize};

use crate::domain::{Scope, SourceSpan};
use crate::storage::RevisionRef;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceIdentity {
    pub adapter: String,
    pub profile: String,
    pub session: String,
    pub thread: String,
    pub agent: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventRef {
    pub source: SourceIdentity,
    pub event_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "phase", rename_all = "snake_case", deny_unknown_fields)]
pub enum MessagePhase {
    Delta { index: u64 },
    Final,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum EventKind {
    Message {
        message_id: String,
        phase: MessagePhase,
    },
    Compaction {
        first: Box<EventRef>,
        last: Box<EventRef>,
        originals: Vec<EventRef>,
    },
    Resume,
    Fork,
    Abandoned,
    Execution,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Visibility {
    User,
    Assistant,
    Tool,
    Session,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct EventDraft {
    pub event_id: String,
    pub source_version: String,
    pub runtime_instance: String,
    pub branch: String,
    pub parent: Option<EventRef>,
    pub fork_cutoff: Option<EventRef>,
    pub event_time_ms: Option<i64>,
    pub observed_time_ms: i64,
    pub visibility: Visibility,
    pub kind: EventKind,
    pub text: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CoverageLevel {
    AvailablePayloads,
    VisibleText,
    MetadataOnly,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum LossReason {
    Rotation,
    Truncation,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum PendingReason {
    Backlog,
    PartialTail,
    InvalidFrame,
    OversizedFrame,
    SourceMissing,
    Conflict,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Checkpoint {
    pub cursor: String,
    pub coverage: CoverageLevel,
    pub loss: Vec<LossReason>,
    pub pending: Option<PendingReason>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Advance {
    pub expected_revision: Option<u64>,
    pub checkpoint: Checkpoint,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CheckpointView {
    pub revision: u64,
    pub checkpoint: Checkpoint,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct BlobPart {
    pub revision: RevisionRef,
    pub bytes: usize,
    pub sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EventRecord {
    pub schema_version: u32,
    pub reference: EventRef,
    pub scope: Scope,
    /// Text is stored in immutable normalized source spans, never duplicated here.
    pub metadata: EventDraft,
    pub fingerprint: String,
    pub raw_bytes: usize,
    pub raw_sha256: String,
    pub text_bytes: usize,
    pub text_sha256: String,
    pub raw: Vec<BlobPart>,
    pub normalized: Vec<SourceSpan>,
    pub coverage: CoverageLevel,
    pub publication_request_id: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct EventView {
    pub revision: u64,
    pub event: EventRecord,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct MessageView {
    pub final_event: Option<EventRef>,
    pub delta_count: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct LineagePage {
    pub events: Vec<EventView>,
    pub next: Option<EventRef>,
    pub missing: Option<EventRef>,
    pub read_seq: u64,
}

/// Explicit interchange format, not a parser for any of the four host formats.
#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CaptureFrame {
    pub schema_version: u32,
    pub event: EventDraft,
    pub raw_hex: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReconcileReport {
    pub processed: usize,
    pub deduplicated: usize,
    pub checkpoint: CheckpointView,
}
