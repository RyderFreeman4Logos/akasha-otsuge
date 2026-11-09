//! Attributed execution observations; a receipt or an exit code is not causal proof.
use crate::capture::{EventRef, SourceIdentity};
use crate::domain::{DeliveryPhase, Scope, SourceSpan};
use crate::ipc::{Error, Result, identifier};
use crate::storage::{RecordKind, RevisionRef};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use std::path::{Component, Path, PathBuf};

pub const MAX_EVENT_BYTES: usize = 128 * 1024;
pub const PREVIEW_BYTES: usize = 4096;
pub const LOG_CHUNK_BYTES: usize = 512 * 1024;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RunPlan {
    pub event_id: String,
    pub invocation_id: String,
    #[serde(default)]
    pub artifacts: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct RunContext {
    pub run_id: String,
    pub workspace: PathBuf,
    pub data_dir: PathBuf,
    pub receipt_jobs: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogChunk {
    pub invocation_id: String,
    pub event_id: String,
    pub stream: Stream,
    pub part: u32,
    pub observed_at_ms: u64,
    pub raw_hex: String,
}

fn relative_file(path: &str) -> bool {
    !path.is_empty()
        && path.len() <= 4096
        && !path.contains('\0')
        && Path::new(path)
            .components()
            .all(|c| matches!(c, Component::Normal(_)))
}

pub fn validate_argv(argv: &[String]) -> Result<()> {
    if argv.is_empty()
        || argv.len() > 64
        || argv[0].is_empty()
        || argv.iter().any(|v| v.len() > 4096 || v.contains('\0'))
    {
        return Err(Error("INVALID_OUTCOME"));
    }
    Ok(())
}

impl RunPlan {
    pub fn validate(&self) -> Result<()> {
        let mut paths = BTreeSet::new();
        if !identifier(&self.event_id)
            || !identifier(&self.invocation_id)
            || self.artifacts.len() > 32
            || self
                .artifacts
                .iter()
                .any(|p| !relative_file(p) || !paths.insert(p))
        {
            return Err(Error("INVALID_OUTCOME"));
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Workspace {
    pub base_commit: Option<String>,
    pub index_sha256: Option<String>,
    pub diff_sha256: Option<String>,
    pub tree_sha256: Option<String>,
    pub gaps: Vec<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Execution {
    pub argv: Vec<String>,
    pub started_at_ms: u64,
    pub finished_at_ms: u64,
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spawn_error: Option<String>,
    pub workspace: PathBuf,
    pub before: Workspace,
    pub after: Workspace,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub artifact_gaps: Vec<ArtifactGap>,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ArtifactGapReason {
    Missing,
    Unavailable,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ArtifactGap {
    pub path: String,
    pub reason: ArtifactGapReason,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Artifact {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Severity {
    Info,
    Warning,
    Error,
    Critical,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Relation {
    Attached,
    Returned,
    CitedOrClaimedUsed,
    Suspected,
    Validated,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Stream {
    Stdout,
    Stderr,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct LogRef {
    pub stream: Stream,
    pub reference: EventRef,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Claim {
    pub job_id: String,
    pub memory: RevisionRef,
    pub relation: Relation,
    pub reason: String,
    pub evidence: Vec<SourceSpan>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Event {
    pub event_id: String,
    pub invocation_id: String,
    pub source: String,
    pub severity: Severity,
    pub occurred_at_ms: u64,
    pub execution: Option<Execution>,
    pub artifacts: Vec<Artifact>,
    pub logs: Vec<LogRef>,
    pub receipt_jobs: Vec<String>,
    pub claims: Vec<Claim>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Receipt {
    pub job_id: String,
    pub receipt_id: String,
    pub revision: u64,
    pub output_sha256: String,
    pub delivery: DeliveryPhase,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Link {
    pub job_id: String,
    pub receipt_id: String,
    pub memory: RevisionRef,
    pub relation: Relation,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Log {
    pub source: LogRef,
    pub revision: u64,
    pub bytes: usize,
    pub sha256: String,
    pub preview: String,
    pub preview_truncated: bool,
    pub preview_encoding: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
pub struct Record {
    pub schema_version: u32,
    pub run_id: String,
    pub caller: SourceIdentity,
    pub scope: Scope,
    pub invocation_revision: u64,
    pub config_generation: u64,
    pub event: Event,
    pub receipts: Vec<Receipt>,
    pub links: Vec<Link>,
    pub logs: Vec<Log>,
}

fn hex(value: &str, lengths: &[usize]) -> bool {
    lengths.contains(&value.len()) && value.bytes().all(|c| c.is_ascii_hexdigit())
}

impl Workspace {
    fn valid(&self) -> bool {
        let hashes = [&self.index_sha256, &self.diff_sha256, &self.tree_sha256];
        self.base_commit.as_ref().is_none_or(|v| hex(v, &[40, 64]))
            && hashes
                .iter()
                .all(|v| v.as_ref().is_none_or(|v| hex(v, &[64])))
            && self.gaps.len() <= 32
            && self.gaps.iter().all(|v| identifier(v))
            && (self.base_commit.is_some() && hashes.iter().all(|v| v.is_some())
                || !self.gaps.is_empty())
    }
}

impl Event {
    pub fn validate(&self) -> Result<()> {
        let mut jobs = BTreeSet::new();
        let mut artifacts = BTreeSet::new();
        if ![&self.event_id, &self.invocation_id, &self.source]
            .into_iter()
            .all(|v| identifier(v))
            || self.occurred_at_ms == 0
            || self.receipt_jobs.len() > 16
            || self
                .receipt_jobs
                .iter()
                .any(|v| !identifier(v) || !jobs.insert(v))
            || self.artifacts.len() > 32
            || self.artifacts.iter().any(|v| {
                !relative_file(&v.path) || !hex(&v.sha256, &[64]) || !artifacts.insert(&v.path)
            })
            || !self.artifacts.is_empty() && self.execution.is_none()
            || self.logs.len() > 64
            || self.claims.len() > 64
            || serde_json::to_vec(self)?.len() > MAX_EVENT_BYTES
        {
            return Err(Error("INVALID_OUTCOME"));
        }
        if let Some(execution) = &self.execution {
            let statuses = usize::from(execution.exit_code.is_some())
                + usize::from(execution.signal.is_some())
                + usize::from(execution.spawn_error.is_some());
            if validate_argv(&execution.argv).is_err()
                || !execution.workspace.is_absolute()
                || execution.workspace.as_os_str().len() > 4096
                || execution.started_at_ms > execution.finished_at_ms
                || statuses != 1
                || execution.exit_code.is_some_and(|v| !(0..=255).contains(&v))
                || execution.signal.is_some_and(|v| !(1..=64).contains(&v))
                || execution
                    .spawn_error
                    .as_ref()
                    .is_some_and(|v| v.is_empty() || v.len() > 4096)
                || !execution.before.valid()
                || !execution.after.valid()
                || execution.artifact_gaps.len() + self.artifacts.len() > 32
                || execution
                    .artifact_gaps
                    .iter()
                    .any(|gap| !relative_file(&gap.path) || !artifacts.insert(&gap.path))
            {
                return Err(Error("INVALID_OUTCOME"));
            }
        }
        for claim in &self.claims {
            if !identifier(&claim.job_id)
                || !self.receipt_jobs.contains(&claim.job_id)
                || claim.memory.key.kind != RecordKind::Memory
                || claim.memory.commit_seq == 0
                || !identifier(&claim.memory.key.id)
                || matches!(claim.relation, Relation::Attached | Relation::Returned)
                || claim.reason.trim().is_empty()
                || claim.reason.len() > 4096
                || claim.evidence.len() > 8
                || claim.relation == Relation::Validated && claim.evidence.is_empty()
            {
                return Err(Error("INVALID_OUTCOME_CLAIM"));
            }
        }
        Ok(())
    }
}
