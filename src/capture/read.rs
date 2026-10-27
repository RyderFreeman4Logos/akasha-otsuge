use super::*;
use crate::domain::Domain;
use std::collections::BTreeSet;

impl Capture<'_> {
    pub fn event(
        &self,
        binding: &CaptureBinding,
        reference: &EventRef,
        at: u64,
    ) -> Result<Option<EventView>> {
        if !valid_ref(reference) {
            return Err(error("INVALID_CAPTURE_EVENT"));
        }
        let Some(revision) = self.storage.at(&event_key(reference)?, at)? else {
            return Ok(None);
        };
        let event: EventRecord = serde_json::from_str(&revision.content)?;
        if event.schema_version != 1
            || event.reference != *reference
            || event.metadata.event_id != reference.event_id
            || !event.metadata.text.is_empty()
            || event.raw_bytes > MAX_RAW
            || event.text_bytes > MAX_TEXT
            || event.raw.len() > MAX_RAW / RAW_CHUNK
            || event.normalized.len() > MAX_TEXT / (TEXT_CHUNK - 4) + 1
            || event
                .raw
                .iter()
                .any(|v| v.revision.commit_seq >= revision.commit_seq)
            || event
                .normalized
                .iter()
                .any(|v| v.revision >= revision.commit_seq)
        {
            return Err(error("CAPTURE_CORRUPT"));
        }
        if !binding.scope.contains(&event.scope) {
            return Err(error("SCOPE_DENIED"));
        }
        Ok(Some(EventView {
            revision: revision.commit_seq,
            event,
        }))
    }

    pub fn raw(&self, event: &EventView) -> Result<Vec<u8>> {
        if event.event.raw_bytes > MAX_RAW || event.event.raw.len() > MAX_RAW / RAW_CHUNK {
            return Err(error("CAPTURE_CORRUPT"));
        }
        let mut bytes = Vec::with_capacity(event.event.raw_bytes);
        for part in &event.event.raw {
            if part.bytes > RAW_CHUNK
                || part.revision.key != key(RecordKind::Evidence, "capture-blob", &part.sha256)
                || part.revision.commit_seq >= event.revision
            {
                return Err(error("CAPTURE_CORRUPT"));
            }
            let content = self
                .storage
                .revision(&part.revision.key, part.revision.commit_seq)?
                .ok_or_else(|| error("CAPTURE_CORRUPT"))?
                .content;
            let chunk = decode_hex(&content, RAW_CHUNK)?;
            if chunk.len() != part.bytes
                || hash(&chunk) != part.sha256
                || bytes.len() + chunk.len() > event.event.raw_bytes
            {
                return Err(error("CAPTURE_CORRUPT"));
            }
            bytes.extend(chunk);
        }
        if bytes.len() != event.event.raw_bytes || hash(&bytes) != event.event.raw_sha256 {
            return Err(error("CAPTURE_CORRUPT"));
        }
        Ok(bytes)
    }

    pub fn text(&self, event: &EventView) -> Result<String> {
        if event.event.text_bytes > MAX_TEXT
            || event.event.normalized.len() > MAX_TEXT / (TEXT_CHUNK - 4) + 1
        {
            return Err(error("CAPTURE_CORRUPT"));
        }
        let domain = Domain::new(self.storage);
        let token = digest(&event.event.reference)?;
        let mut text = String::with_capacity(event.event.text_bytes);
        for (i, span) in event.event.normalized.iter().enumerate() {
            if span.event_id != format!("capture/{token}/{i:04x}")
                || span.start != 0
                || span.revision >= event.revision
            {
                return Err(error("CAPTURE_CORRUPT"));
            }
            let source = domain.source(&span.event_id, span.revision)?;
            if source.content_sha256 != span.content_sha256
                || source.payload.scope != event.event.scope
                || span.end != source.payload.text.len()
                || text.len() + span.end > event.event.text_bytes
            {
                return Err(error("CAPTURE_CORRUPT"));
            }
            text.push_str(&source.payload.text);
        }
        if text.len() != event.event.text_bytes || hash(text.as_bytes()) != event.event.text_sha256
        {
            return Err(error("CAPTURE_CORRUPT"));
        }
        Ok(text)
    }

    pub fn checkpoint(&self, binding: &CaptureBinding) -> Result<Option<CheckpointView>> {
        self.storage
            .current(&binding.checkpoint_key()?)?
            .map(|v| {
                let checkpoint: Checkpoint = serde_json::from_str(&v.content)?;
                validate_advance(
                    binding,
                    &Advance {
                        expected_revision: Some(v.commit_seq),
                        checkpoint: checkpoint.clone(),
                    },
                )?;
                Ok(CheckpointView {
                    revision: v.commit_seq,
                    checkpoint,
                })
            })
            .transpose()
    }

    pub fn message(
        &self,
        binding: &CaptureBinding,
        message_id: &str,
    ) -> Result<Option<MessageView>> {
        self.storage
            .current(&binding.message_key(message_id)?)?
            .map(|v| {
                let state: MessageState = serde_json::from_str(&v.content)?;
                if state.schema_version != 1
                    || state.view.final_event.is_some() != state.final_sha256.is_some()
                {
                    return Err(error("CAPTURE_CORRUPT"));
                }
                if !binding.scope.contains(&state.scope) {
                    return Err(error("SCOPE_DENIED"));
                }
                Ok(state.view)
            })
            .transpose()
    }

    /// Walk explicit parent links at one fixed point. Never concatenate branches by time.
    pub fn lineage(
        &self,
        binding: &CaptureBinding,
        tip: &EventRef,
        at: u64,
        limit: usize,
    ) -> Result<LineagePage> {
        self.storage.check_point(at)?;
        if limit == 0 || limit > 64 || !valid_ref(tip) {
            return Err(error("INVALID_CAPTURE_PAGE"));
        }
        let mut page = LineagePage {
            events: Vec::new(),
            next: Some(tip.clone()),
            missing: None,
            read_seq: at,
        };
        let mut seen = BTreeSet::new();
        while let Some(reference) = page.next.take() {
            if !seen.insert(digest(&reference)?) {
                return Err(error("CAPTURE_PARENT_CYCLE"));
            }
            let Some(event) = self.event(binding, &reference, at)? else {
                page.missing = Some(reference);
                break;
            };
            if let Some(parent) = &event.event.metadata.parent {
                if let Some(ancestor) = self.event(binding, parent, at)? {
                    if (ancestor.event.metadata.branch != event.event.metadata.branch
                        || ancestor.event.reference.source != event.event.reference.source)
                        && event.event.metadata.fork_cutoff.as_ref() != Some(parent)
                    {
                        return Err(error("CAPTURE_BRANCH_CONFLICT"));
                    }
                } else {
                    page.missing = Some(parent.clone());
                }
            }
            page.next = event.event.metadata.parent.clone();
            page.events.push(event);
            if page.missing.is_some() {
                page.next = None;
                break;
            }
            if page.events.len() == limit {
                break;
            }
        }
        Ok(page)
    }

    pub fn missing_references(
        &self,
        binding: &CaptureBinding,
        event: &EventView,
        at: u64,
    ) -> Result<Vec<EventRef>> {
        self.storage.check_point(at)?;
        let mut refs = event.event.metadata.parent.iter().collect::<Vec<_>>();
        if let EventKind::Compaction {
            first,
            last,
            originals,
        } = &event.event.metadata.kind
        {
            if originals.len() > 64 {
                return Err(error("CAPTURE_CORRUPT"));
            }
            refs.extend([first.as_ref(), last.as_ref()]);
            refs.extend(originals);
        }
        let mut missing = Vec::new();
        for reference in refs {
            if self.event(binding, reference, at)?.is_none() && !missing.contains(reference) {
                missing.push(reference.clone());
            }
        }
        Ok(missing)
    }
}
