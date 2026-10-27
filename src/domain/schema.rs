use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Scope {
    pub user: String,
    pub project: Option<String>,
    pub branch_or_version: Option<String>,
    pub session: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum MemoryKind {
    Decision,
    Preference,
    Fact,
    Procedure,
    Hypothesis,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceSpan {
    pub event_id: String,
    pub revision: u64,
    pub content_sha256: String,
    pub start: usize,
    pub end: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceDraft {
    pub event_id: String,
    pub scope: Scope,
    pub text: String,
    pub reported_role: Option<String>,
    pub event_time_ms: Option<i64>,
    pub observed_time_ms: i64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryDraft {
    pub kind: MemoryKind,
    pub scope: Scope,
    pub claim: String,
    pub rationale: String,
    pub conditions: Vec<String>,
    pub assumptions: Vec<String>,
    pub source_spans: Vec<SourceSpan>,
    pub valid_from_ms: Option<i64>,
    pub valid_until_ms: Option<i64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryWrite {
    pub request_id: String,
    pub memory_id: String,
    pub expected_revision: Option<u64>,
    pub draft: MemoryDraft,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(
    tag = "kind",
    content = "id",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum Actor {
    Human(String),
    Agent(String),
    Tool(String),
    Unverified(String),
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Origin {
    HumanStatement,
    Extraction,
    AutoGrill,
    TechnicalObservation,
    Unverified,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Generator {
    pub runner: String,
    pub model: String,
    pub prompt_version: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Provenance {
    pub actor: Actor,
    pub origin: Origin,
    pub binding: String,
    pub generator: Option<Generator>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StatementIntent {
    ExplicitDecision,
    Tentative,
    Other,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct SourceRecord {
    pub schema_version: u32,
    pub payload: SourceDraft,
    pub provenance: Provenance,
    pub statement_intent: StatementIntent,
    pub review_target: Option<ReviewTarget>,
    pub content_sha256: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Authority {
    HumanConfirmed,
    DelegatedApproved,
    Candidate,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Lifecycle {
    Candidate,
    Active,
    Rejected,
    Retired,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Approval {
    pub semantic_sha256: String,
    pub reviewer: Actor,
    pub source: SourceSpan,
    pub proposal_revision: Option<u64>,
    pub reason: String,
    pub action: ReviewAction,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryRecord {
    pub schema_version: u32,
    pub draft: MemoryDraft,
    pub provenance: Provenance,
    pub semantic_sha256: String,
    pub authority: Authority,
    pub lifecycle: Lifecycle,
    pub policy_revision: String,
    pub config_generation: u64,
    pub authorization: Authorization,
    pub approval: Option<Approval>,
    pub previous_adoption: Option<u64>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Authorization {
    pub mode: crate::config::ReviewMode,
    pub scope: Scope,
    pub adopt_explicit_decisions: bool,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MemoryView {
    pub revision: crate::storage::RevisionRef,
    pub previous: Option<u64>,
    pub record: MemoryRecord,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewAction {
    Approve,
    Reject,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewRequest {
    pub write: MemoryWrite,
    pub semantic_sha256: String,
    pub source: SourceSpan,
    pub reason: String,
    pub action: ReviewAction,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewTarget {
    pub memory_id: String,
    pub proposal_revision: u64,
    pub semantic_sha256: String,
    pub action: ReviewAction,
}
