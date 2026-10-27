//! Shared capture components. Real host parsers supply verified source identities.
mod file;
mod read;
mod schema;
pub use schema::*;

use crate::domain::{Ingress, Scope, SourceDraft, SourceSpan, source_change};
use crate::storage::{
    Change, CommitPlan, CommitReceipt, CommitRequest, RecordKey, RecordKind, Result, RevisionRef,
    Storage, StorageError, hash,
};
use serde::{Deserialize, Serialize};

const MAX_RAW: usize = 4 * 1024 * 1024;
const MAX_TEXT: usize = 1024 * 1024;
const RAW_CHUNK: usize = 32 * 1024;
const TEXT_CHUNK: usize = 16 * 1024;

fn error(code: &'static str) -> StorageError {
    StorageError::new(code)
}
fn id(s: &str) -> bool {
    !s.trim().is_empty() && s.len() <= 256
}
fn digest<T: Serialize>(v: &T) -> Result<String> {
    Ok(hash(&serde_json::to_vec(v)?))
}
fn key(kind: RecordKind, prefix: &str, suffix: &str) -> RecordKey {
    RecordKey {
        kind,
        id: format!("{prefix}/{suffix}"),
    }
}
fn event_key(r: &EventRef) -> Result<RecordKey> {
    Ok(key(RecordKind::Evidence, "capture-event", &digest(r)?))
}
fn valid_source(s: &SourceIdentity) -> bool {
    [&s.adapter, &s.profile, &s.session, &s.thread, &s.agent]
        .into_iter()
        .all(|v| id(v))
}
fn valid_ref(r: &EventRef) -> bool {
    valid_source(&r.source) && id(&r.event_id)
}

pub struct CaptureBinding {
    source: SourceIdentity,
    scope: Scope,
    stream: String,
    coverage: CoverageLevel,
}

impl CaptureBinding {
    /// A server assertion, not an authenticator; never infer these IDs from cwd.
    pub fn from_source(
        source: SourceIdentity,
        scope: Scope,
        stream: &str,
        coverage: CoverageLevel,
    ) -> Result<Self> {
        if !valid_source(&source) || !scope.contains(&scope) || !id(stream) {
            return Err(error("INVALID_CAPTURE_BINDING"));
        }
        Ok(Self {
            source,
            scope,
            stream: stream.into(),
            coverage,
        })
    }
    fn checkpoint_key(&self) -> Result<RecordKey> {
        Ok(key(
            RecordKind::State,
            "capture-checkpoint",
            &digest(&(&self.source, &self.scope, &self.stream))?,
        ))
    }
    fn message_key(&self, message: &str) -> Result<RecordKey> {
        if !id(message) {
            return Err(error("INVALID_CAPTURE_EVENT"));
        }
        Ok(key(
            RecordKind::State,
            "capture-message",
            &digest(&(&self.source, message))?,
        ))
    }
}

#[derive(Clone, Serialize, Deserialize)]
struct Intent {
    schema_version: u32,
    reference: EventRef,
    metadata: EventDraft,
    fingerprint: String,
    config_generation: u64,
}

#[derive(Serialize, Deserialize)]
struct MessageState {
    schema_version: u32,
    scope: Scope,
    branch: String,
    parent: Option<EventRef>,
    fork_cutoff: Option<EventRef>,
    visibility: Visibility,
    view: MessageView,
    final_sha256: Option<String>,
}

pub struct Capture<'a> {
    storage: &'a Storage,
}
impl<'a> Capture<'a> {
    pub fn new(storage: &'a Storage) -> Self {
        Self { storage }
    }

    fn receipt(&self, request: &str) -> Result<CommitReceipt> {
        self.storage
            .receipt(request)?
            .ok_or_else(|| error("CAPTURE_CORRUPT"))
    }

    async fn immutable(&self, change: Change, config: u64) -> Result<RevisionRef> {
        let receipt = self
            .storage
            .commit_or_reuse(|| {
                if let Some(old) = self.storage.current(&change.key)? {
                    if old.content != change.content {
                        return Err(error("CAPTURE_IDENTITY_CONFLICT"));
                    }
                    return Ok(CommitPlan::Reuse(self.receipt(&change.key.id)?));
                }
                Ok(CommitPlan::Write(CommitRequest {
                    request_id: change.key.id.clone(),
                    config_generation: config,
                    policy_revision: None,
                    changes: vec![change.clone()],
                }))
            })
            .await?;
        Ok(RevisionRef {
            key: change.key,
            commit_seq: receipt.commit_seq,
        })
    }

    pub async fn ingest(
        &self,
        binding: &CaptureBinding,
        draft: &EventDraft,
        raw: &[u8],
        ingress: &Ingress,
        advance: Option<&Advance>,
        config: u64,
    ) -> Result<EventView> {
        validate(draft, raw, config)?;
        ingress.check_scope(&binding.scope)?;
        if let Some(a) = advance {
            validate_advance(binding, a)?;
        }
        let reference = EventRef {
            source: binding.source.clone(),
            event_id: draft.event_id.clone(),
        };
        if draft.parent.as_ref() == Some(&reference) {
            return Err(error("INVALID_CAPTURE_EVENT"));
        }
        let token = digest(&reference)?;
        let mut stable = draft.clone();
        stable.text.clear();
        stable.observed_time_ms = 0;
        stable.source_version.clear();
        let fingerprint = digest(&(
            &reference,
            &binding.scope,
            binding.coverage,
            &stable,
            hash(raw),
            hash(draft.text.as_bytes()),
            ingress,
        ))?;
        let intent_key = key(RecordKind::Evidence, "capture-intent", &token);
        self.storage
            .commit_or_reuse(|| {
                if let Some(old) = self.storage.current(&intent_key)? {
                    let old: Intent = serde_json::from_str(&old.content)?;
                    if old.schema_version != 1
                        || old.reference != reference
                        || old.fingerprint != fingerprint
                    {
                        return Err(error("CAPTURE_IDENTITY_CONFLICT"));
                    }
                    return Ok(CommitPlan::Reuse(self.receipt(&intent_key.id)?));
                }
                let mut metadata = draft.clone();
                metadata.text.clear();
                let intent = Intent {
                    schema_version: 1,
                    reference: reference.clone(),
                    metadata,
                    fingerprint: fingerprint.clone(),
                    config_generation: config,
                };
                Ok(CommitPlan::Write(CommitRequest {
                    request_id: intent_key.id.clone(),
                    config_generation: config,
                    policy_revision: None,
                    changes: vec![Change {
                        key: intent_key.clone(),
                        expected_revision: None,
                        content: serde_json::to_string(&intent)?,
                    }],
                }))
            })
            .await?;
        let intent: Intent = serde_json::from_str(
            &self
                .storage
                .current(&intent_key)?
                .ok_or_else(|| error("CAPTURE_CORRUPT"))?
                .content,
        )?;
        #[cfg(test)]
        crate::storage::crash_tests::checkpoint("capture-intent");
        let mut record = EventRecord {
            schema_version: 1,
            reference: reference.clone(),
            scope: binding.scope.clone(),
            metadata: intent.metadata,
            fingerprint,
            raw_bytes: raw.len(),
            raw_sha256: hash(raw),
            text_bytes: draft.text.len(),
            text_sha256: hash(draft.text.as_bytes()),
            raw: Vec::new(),
            normalized: Vec::new(),
            coverage: binding.coverage,
            publication_request_id: String::new(),
        };
        for part in raw.chunks(RAW_CHUNK) {
            let sha256 = hash(part);
            let revision = self
                .immutable(
                    Change {
                        key: key(RecordKind::Evidence, "capture-blob", &sha256),
                        expected_revision: None,
                        content: encode_hex(part),
                    },
                    intent.config_generation,
                )
                .await?;
            record.raw.push(BlobPart {
                revision,
                bytes: part.len(),
                sha256,
            });
            #[cfg(test)]
            crate::storage::crash_tests::checkpoint("capture-raw");
        }
        let mut start = 0;
        while start < draft.text.len() {
            let mut end = (start + TEXT_CHUNK).min(draft.text.len());
            while !draft.text.is_char_boundary(end) {
                end -= 1;
            }
            let text = &draft.text[start..end];
            let event_id = format!("capture/{token}/{:04x}", record.normalized.len());
            let change = source_change(
                &SourceDraft {
                    event_id: event_id.clone(),
                    scope: binding.scope.clone(),
                    text: text.into(),
                    reported_role: Some(
                        match draft.visibility {
                            Visibility::User => "user",
                            Visibility::Assistant => "assistant",
                            Visibility::Tool => "tool",
                            Visibility::Session => "session",
                        }
                        .into(),
                    ),
                    event_time_ms: draft.event_time_ms,
                    observed_time_ms: record.metadata.observed_time_ms,
                },
                ingress,
            )?;
            let revision = self.immutable(change, intent.config_generation).await?;
            record.normalized.push(SourceSpan {
                event_id,
                revision: revision.commit_seq,
                content_sha256: hash(text.as_bytes()),
                start: 0,
                end: text.len(),
            });
            start = end;
            #[cfg(test)]
            crate::storage::crash_tests::checkpoint("capture-source");
        }
        let event_key = event_key(&reference)?;
        let checkpoint_key = binding.checkpoint_key()?;
        let request = format!(
            "capture-publish/{}",
            digest(&(&record.fingerprint, advance.map(|a| (&checkpoint_key, a))))?
        );
        record.publication_request_id = request.clone();
        self.storage
            .commit_or_reuse(|| {
                if let Some(receipt) = self.storage.receipt(&request)? {
                    return Ok(CommitPlan::Reuse(receipt));
                }
                let existing = self.event(binding, &reference, self.storage.sequence()?)?;
                let mut changes = Vec::new();
                if let Some(old) = existing {
                    if old.event.fingerprint != record.fingerprint {
                        return Err(error("CAPTURE_IDENTITY_CONFLICT"));
                    }
                    if advance.is_none() {
                        return Ok(CommitPlan::Reuse(
                            self.receipt(&old.event.publication_request_id)?,
                        ));
                    }
                } else {
                    self.message_changes(binding, &record, &mut changes)?;
                    changes.push(Change {
                        key: event_key.clone(),
                        expected_revision: None,
                        content: serde_json::to_string(&record)?,
                    });
                }
                if let Some(a) = advance {
                    changes.push(Change {
                        key: binding.checkpoint_key()?,
                        expected_revision: a.expected_revision,
                        content: serde_json::to_string(&a.checkpoint)?,
                    });
                }
                Ok(CommitPlan::Write(CommitRequest {
                    request_id: request.clone(),
                    config_generation: intent.config_generation,
                    policy_revision: None,
                    changes,
                }))
            })
            .await?;
        #[cfg(test)]
        crate::storage::crash_tests::checkpoint("capture-published");
        self.event(binding, &reference, self.storage.sequence()?)?
            .ok_or_else(|| error("CAPTURE_CORRUPT"))
    }

    fn message_changes(
        &self,
        binding: &CaptureBinding,
        event: &EventRecord,
        changes: &mut Vec<Change>,
    ) -> Result<()> {
        let EventKind::Message { message_id, phase } = &event.metadata.kind else {
            return Ok(());
        };
        let message_key = binding.message_key(message_id)?;
        let previous = self.storage.current(&message_key)?;
        let mut state = match &previous {
            Some(v) => serde_json::from_str::<MessageState>(&v.content)?,
            None => MessageState {
                schema_version: 1,
                scope: binding.scope.clone(),
                branch: event.metadata.branch.clone(),
                parent: event.metadata.parent.clone(),
                fork_cutoff: event.metadata.fork_cutoff.clone(),
                visibility: event.metadata.visibility,
                view: MessageView {
                    final_event: None,
                    delta_count: 0,
                },
                final_sha256: None,
            },
        };
        if state.schema_version != 1
            || state.scope != event.scope
            || state.branch != event.metadata.branch
            || state.parent != event.metadata.parent
            || state.fork_cutoff != event.metadata.fork_cutoff
            || state.visibility != event.metadata.visibility
        {
            return Err(error("CAPTURE_MESSAGE_CONFLICT"));
        }
        match phase {
            MessagePhase::Final => {
                if state
                    .final_sha256
                    .as_ref()
                    .is_some_and(|v| v != &event.text_sha256)
                {
                    return Err(error("CAPTURE_MESSAGE_CONFLICT"));
                }
                if state.view.final_event.is_some() {
                    return Ok(());
                }
                state.view.final_event = Some(event.reference.clone());
                state.final_sha256 = Some(event.text_sha256.clone());
            }
            MessagePhase::Delta { index } => {
                let slot = key(
                    RecordKind::Evidence,
                    "capture-delta",
                    &digest(&(&binding.source, message_id, index))?,
                );
                if let Some(old) = self.storage.current(&slot)? {
                    if old.content != event.text_sha256 {
                        return Err(error("CAPTURE_MESSAGE_CONFLICT"));
                    }
                    return Ok(());
                }
                changes.push(Change {
                    key: slot,
                    expected_revision: None,
                    content: event.text_sha256.clone(),
                });
                state.view.delta_count = state
                    .view
                    .delta_count
                    .checked_add(1)
                    .ok_or_else(|| error("CAPTURE_LIMIT"))?;
            }
        }
        changes.push(Change {
            key: message_key,
            expected_revision: previous.map(|v| v.commit_seq),
            content: serde_json::to_string(&state)?,
        });
        Ok(())
    }

    pub async fn advance_checkpoint(
        &self,
        binding: &CaptureBinding,
        advance: &Advance,
        config: u64,
    ) -> Result<CheckpointView> {
        validate_advance(binding, advance)?;
        if config == 0 {
            return Err(error("INVALID_CAPTURE_EVENT"));
        }
        let key = binding.checkpoint_key()?;
        let request = format!("capture-advance/{}", digest(&(&key, advance))?);
        if let Some(old) = self.checkpoint(binding)?
            && Some(old.revision) == advance.expected_revision
            && old.checkpoint == advance.checkpoint
        {
            return Ok(old);
        }
        let receipt = self
            .storage
            .commit_or_reuse(|| {
                if let Some(receipt) = self.storage.receipt(&request)? {
                    return Ok(CommitPlan::Reuse(receipt));
                }
                Ok(CommitPlan::Write(CommitRequest {
                    request_id: request,
                    config_generation: config,
                    policy_revision: None,
                    changes: vec![Change {
                        key,
                        expected_revision: advance.expected_revision,
                        content: serde_json::to_string(&advance.checkpoint)?,
                    }],
                }))
            })
            .await?;
        Ok(CheckpointView {
            revision: receipt.commit_seq,
            checkpoint: advance.checkpoint.clone(),
        })
    }
}

fn validate(d: &EventDraft, raw: &[u8], config: u64) -> Result<()> {
    let reference_ok = d.parent.iter().chain(&d.fork_cutoff).all(valid_ref);
    if config == 0
        || ![
            &d.event_id,
            &d.source_version,
            &d.runtime_instance,
            &d.branch,
        ]
        .into_iter()
        .all(|v| id(v))
        || raw.len() > MAX_RAW
        || d.text.len() > MAX_TEXT
        || !reference_ok
        || d.fork_cutoff.is_some() && d.fork_cutoff != d.parent
    {
        return Err(error("INVALID_CAPTURE_EVENT"));
    }
    match &d.kind {
        EventKind::Message { message_id, .. } if !id(message_id) => {
            return Err(error("INVALID_CAPTURE_EVENT"));
        }
        EventKind::Compaction {
            first,
            last,
            originals,
        } if !valid_ref(first)
            || !valid_ref(last)
            || originals.is_empty()
            || originals.len() > 64
            || !originals.iter().all(valid_ref) =>
        {
            return Err(error("INVALID_CAPTURE_EVENT"));
        }
        _ => {}
    }
    let mut metadata = d.clone();
    metadata.text.clear();
    if serde_json::to_vec(&metadata)?.len() > 32 * 1024 {
        return Err(error("INVALID_CAPTURE_EVENT"));
    }
    Ok(())
}
fn validate_advance(binding: &CaptureBinding, a: &Advance) -> Result<()> {
    if a.expected_revision == Some(0)
        || a.checkpoint.cursor.len() > 4096
        || a.checkpoint.coverage != binding.coverage
        || a.checkpoint.loss.len() > 2
    {
        return Err(error("INVALID_CAPTURE_CHECKPOINT"));
    }
    Ok(())
}
fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        out.push(HEX[(b >> 4) as usize] as char);
        out.push(HEX[(b & 15) as usize] as char);
    }
    out
}
fn decode_hex(text: &str, max: usize) -> Result<Vec<u8>> {
    if text.len() > max * 2 || !text.len().is_multiple_of(2) {
        return Err(error("INVALID_CAPTURE_FRAME"));
    }
    text.as_bytes()
        .as_chunks::<2>()
        .0
        .iter()
        .map(|p| {
            let a = (p[0] as char)
                .to_digit(16)
                .ok_or_else(|| error("INVALID_CAPTURE_FRAME"))?;
            let b = (p[1] as char)
                .to_digit(16)
                .ok_or_else(|| error("INVALID_CAPTURE_FRAME"))?;
            Ok(((a << 4) | b) as u8)
        })
        .collect()
}
