use serde::Serialize;

use super::reader::{Cell, Row};
use super::writer::Writer;
use crate::domain::{
    Actor, Ingress, MemoryDraft, MemoryKind, MemoryWrite, Origin, Provenance, Scope, SourceDraft,
    SourceSpan, migrated_candidate, source_change,
};
use crate::storage::{Result, StorageError, hash};

#[derive(Default, Serialize)]
pub(super) struct Summary {
    pub source_chunks: u64,
    pub candidate_memories: u64,
    pub retired_rows: u64,
    pub unknown_event_time_rows: u64,
    pub isolated_scope_rows: u64,
    pub unmapped_constraints_rows: u64,
}

fn text<'a>(row: &'a Row, columns: &[String], name: &str) -> Result<Option<&'a str>> {
    let index = columns
        .iter()
        .position(|column| column == name)
        .ok_or_else(|| StorageError::new("UNSUPPORTED_MEMPAL_SCHEMA"))?;
    match &row.values[index] {
        Cell::Null => Ok(None),
        Cell::Text(value) => Ok(Some(value)),
        _ => Err(StorageError::new("INVALID_MEMPAL_VALUE")),
    }
}

fn timestamp(value: Option<&str>) -> Option<i64> {
    let value = value?;
    chrono::DateTime::parse_from_rfc3339(value)
        .ok()
        .map(|time| time.timestamp_millis())
        .or_else(|| {
            chrono::NaiveDateTime::parse_from_str(value, "%Y-%m-%d %H:%M:%S%.f")
                .ok()
                .map(|time| time.and_utc().timestamp_millis())
        })
}

fn nonempty_id(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256
}

fn constrained(value: Option<&str>) -> bool {
    value.is_some_and(
        |value| match serde_json::from_str::<serde_json::Value>(value) {
            Ok(serde_json::Value::Null) => false,
            Ok(serde_json::Value::Object(value)) => !value.is_empty(),
            Ok(serde_json::Value::Array(value)) => !value.is_empty(),
            _ => true,
        },
    )
}

fn anchored_scope(
    project: Option<&str>,
    domain: Option<&str>,
    anchor_kind: Option<&str>,
    anchor: Option<&str>,
    root: Option<&str>,
    summary: &mut Summary,
) -> Result<Scope> {
    let project = match project {
        Some(value) if nonempty_id(value) => Some(value.to_owned()),
        None if domain == Some("global") && anchor_kind == Some("global") => None,
        _ => {
            summary.isolated_scope_rows += 1;
            Some(format!(
                "mempal-scope/{}",
                hash(&serde_json::to_vec(&(
                    project,
                    domain,
                    anchor_kind,
                    anchor,
                    root
                ))?)
            ))
        }
    };
    Ok(Scope {
        user: format!("uid:{}", rustix::process::geteuid().as_raw()),
        project,
        branch_or_version: (anchor_kind == Some("worktree"))
            .then(|| format!("mempal-worktree/{}", hash(anchor.unwrap_or("").as_bytes()))),
        session: None,
    })
}

struct Projection<'a> {
    prefix: String,
    original: &'a str,
    scope: Scope,
    role: Option<String>,
    event_time_ms: Option<i64>,
    candidate_kind: Option<MemoryKind>,
    valid_from_ms: Option<i64>,
    valid_until_ms: Option<i64>,
}

impl Projection<'_> {
    fn write(
        &self,
        source: &str,
        observed_at_ms: u64,
        writer: &mut Writer,
        summary: &mut Summary,
    ) -> Result<()> {
        let provenance = Provenance {
            actor: Actor::Unverified("mempal".into()),
            origin: Origin::Unverified,
            binding: format!("snapshot:{source}"),
            generator: None,
        };
        let ingress = Ingress::unverified("mempal", &provenance.binding, self.scope.clone())?;
        let mut remaining = self.original;
        let mut part = 0_u64;
        while !remaining.is_empty() {
            let mut end = remaining.len().min(16 * 1024);
            while !remaining.is_char_boundary(end) {
                end -= 1;
            }
            let content = &remaining[..end];
            let memory_id = format!("{}/{part}", self.prefix);
            let event_id = format!("{memory_id}/source");
            let source_draft = SourceDraft {
                event_id: event_id.clone(),
                scope: self.scope.clone(),
                text: content.into(),
                reported_role: self.role.clone().filter(|value| nonempty_id(value)),
                event_time_ms: self.event_time_ms,
                observed_time_ms: observed_at_ms
                    .try_into()
                    .map_err(|_| StorageError::new("INVALID_CLOCK"))?,
            };
            let candidate = self
                .candidate_kind
                .as_ref()
                .filter(|_| !content.trim().is_empty());
            writer.group(|revision| {
                let mut changes = vec![source_change(&source_draft, &ingress)?];
                if let Some(kind) = candidate {
                    let write = MemoryWrite {
                        request_id: memory_id.clone(),
                        memory_id: memory_id.clone(),
                        expected_revision: None,
                        draft: MemoryDraft {
                            kind: kind.clone(),
                            scope: self.scope.clone(),
                            claim: content.into(),
                            rationale: String::new(),
                            conditions: Vec::new(),
                            assumptions: Vec::new(),
                            valid_from_ms: self.valid_from_ms,
                            valid_until_ms: self.valid_until_ms,
                            source_spans: vec![SourceSpan {
                                event_id: event_id.clone(),
                                revision,
                                content_sha256: hash(content.as_bytes()),
                                start: 0,
                                end: content.len(),
                            }],
                        },
                    };
                    changes.extend(migrated_candidate(&write, provenance.clone(), revision)?);
                }
                Ok(changes)
            })?;
            summary.source_chunks += 1;
            summary.candidate_memories += u64::from(candidate.is_some());
            remaining = &remaining[end..];
            part += 1;
        }
        Ok(())
    }
}

fn drawer(
    row: &Row,
    columns: &[String],
    source: &str,
    observed_at_ms: u64,
    writer: &mut Writer,
    summary: &mut Summary,
) -> Result<()> {
    let get = |name| text(row, columns, name);
    let original = get("content")?.ok_or_else(|| StorageError::new("INVALID_MEMPAL_VALUE"))?;
    let scope = anchored_scope(
        get("project_id")?,
        get("domain")?,
        get("anchor_kind")?,
        get("anchor_id")?,
        get("source_root")?,
        summary,
    )?;
    let retired = get("deleted_at")?.is_some()
        || get("compacted_into")?.is_some()
        || matches!(get("status")?, Some("retired" | "superseded" | "demoted"));
    if retired {
        summary.retired_rows += 1;
    }
    let event_time_ms = timestamp(get("added_at")?);
    if event_time_ms.is_none() {
        summary.unknown_event_time_rows += 1;
    }
    let from = get("valid_from")?;
    let until = get("valid_until")?;
    let valid_from_ms = timestamp(from);
    let valid_until_ms = timestamp(until);
    let constrained = constrained(get("scope_constraints")?)
        || from.is_some() && valid_from_ms.is_none()
        || until.is_some() && valid_until_ms.is_none()
        || valid_from_ms
            .zip(valid_until_ms)
            .is_some_and(|(a, b)| a >= b);
    if constrained {
        summary.unmapped_constraints_rows += 1;
    }
    let kind = match get("memory_kind")? {
        Some("decision") => MemoryKind::Decision,
        Some("profile_trait") => MemoryKind::Preference,
        Some("skill") => MemoryKind::Procedure,
        Some("foresight") => MemoryKind::Hypothesis,
        Some("evidence" | "knowledge" | "atomic_fact" | "case" | "profile_fact") => {
            MemoryKind::Fact
        }
        _ => return Err(StorageError::new("INVALID_MEMPAL_VALUE")),
    };
    Projection {
        prefix: format!("mempal/{source}/drawers/{}", row.rowid),
        original,
        scope,
        role: get("source_type")?.map(|value| format!("mempal:{value}")),
        event_time_ms,
        candidate_kind: (!retired && !constrained).then_some(kind),
        valid_from_ms,
        valid_until_ms,
    }
    .write(source, observed_at_ms, writer, summary)
}

fn card(
    row: &Row,
    columns: &[String],
    source: &str,
    observed_at_ms: u64,
    writer: &mut Writer,
    summary: &mut Summary,
) -> Result<()> {
    let get = |name| text(row, columns, name);
    let scope = anchored_scope(
        None,
        get("domain")?,
        get("anchor_kind")?,
        get("anchor_id")?,
        None,
        summary,
    )?;
    let retired = matches!(get("status")?, Some("demoted" | "retired"));
    let constrained = constrained(get("scope_constraints")?);
    summary.retired_rows += u64::from(retired);
    summary.unmapped_constraints_rows += u64::from(constrained);
    let event_time_ms = timestamp(get("created_at")?);
    summary.unknown_event_time_rows += u64::from(event_time_ms.is_none());
    let mut projection = Projection {
        prefix: format!("mempal/{source}/knowledge_cards/{}", row.rowid),
        original: get("statement")?.ok_or_else(|| StorageError::new("INVALID_MEMPAL_VALUE"))?,
        scope,
        role: Some("mempal:knowledge_card".into()),
        event_time_ms,
        candidate_kind: (!retired && !constrained).then_some(MemoryKind::Fact),
        valid_from_ms: None,
        valid_until_ms: None,
    };
    projection.write(source, observed_at_ms, writer, summary)?;
    projection.prefix.push_str("/content");
    projection.original =
        get("content")?.ok_or_else(|| StorageError::new("INVALID_MEMPAL_VALUE"))?;
    projection.candidate_kind = None;
    projection.write(source, observed_at_ms, writer, summary)
}

fn epoch_millis(row: &Row, columns: &[String]) -> Result<Option<i64>> {
    let index = columns
        .iter()
        .position(|column| column == "timestamp_epoch")
        .ok_or_else(|| StorageError::new("UNSUPPORTED_MEMPAL_SCHEMA"))?;
    match &row.values[index] {
        Cell::Integer(seconds) => Ok(seconds.checked_mul(1000)),
        Cell::Real(bits) => {
            let seconds = f64::from_bits(
                u64::from_str_radix(bits, 16)
                    .map_err(|_| StorageError::new("INVALID_MEMPAL_VALUE"))?,
            );
            let millis = (seconds * 1000.0).floor();
            Ok(
                (millis.is_finite() && millis >= i64::MIN as f64 && millis < -(i64::MIN as f64))
                    .then_some(millis as i64),
            )
        }
        Cell::Null => Ok(None),
        _ => Err(StorageError::new("INVALID_MEMPAL_VALUE")),
    }
}

fn conversation(
    row: &Row,
    columns: &[String],
    source: &str,
    observed_at_ms: u64,
    writer: &mut Writer,
    summary: &mut Summary,
) -> Result<()> {
    let get = |name| text(row, columns, name);
    let project = get("project_path")?;
    let branch = get("git_branch")?;
    let project = match project {
        Some(value) if nonempty_id(value) => Some(value.to_owned()),
        None if branch.is_none() => None,
        _ => {
            summary.isolated_scope_rows += 1;
            Some(format!(
                "mempal-scope/{}",
                hash(&serde_json::to_vec(&(project, branch))?)
            ))
        }
    };
    let scope = Scope {
        user: format!("uid:{}", rustix::process::geteuid().as_raw()),
        project,
        branch_or_version: branch.map(|value| {
            if nonempty_id(value) {
                value.to_owned()
            } else {
                format!("mempal-branch/{}", hash(value.as_bytes()))
            }
        }),
        session: Some(format!(
            "mempal-session/{}",
            hash(&serde_json::to_vec(&(
                get("tool")?,
                get("hermes_profile")?,
                get("session_id")?
            ))?)
        )),
    };
    let event_time_ms = epoch_millis(row, columns)?;
    summary.unknown_event_time_rows += u64::from(event_time_ms.is_none());
    Projection {
        prefix: format!("mempal/{source}/conversation_turns/{}", row.rowid),
        original: get("content")?.ok_or_else(|| StorageError::new("INVALID_MEMPAL_VALUE"))?,
        scope,
        role: get("role")?.map(str::to_owned),
        event_time_ms,
        candidate_kind: None,
        valid_from_ms: None,
        valid_until_ms: None,
    }
    .write(source, observed_at_ms, writer, summary)
}

pub(super) fn row(
    table: &str,
    row: &Row,
    columns: &[String],
    source: &str,
    observed_at_ms: u64,
    writer: &mut Writer,
    summary: &mut Summary,
) -> Result<()> {
    match table {
        "drawers" => drawer(row, columns, source, observed_at_ms, writer, summary),
        "knowledge_cards" => card(row, columns, source, observed_at_ms, writer, summary),
        "conversation_turns" => conversation(row, columns, source, observed_at_ms, writer, summary),
        _ => Ok(()),
    }
}
