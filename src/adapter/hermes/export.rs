//! Canonical native JSONL export: one complete session per line.
use crate::{
    capture::*,
    storage::{Result, StorageError, hash},
};
use serde_json::{Value, json};
use std::collections::HashMap;

const MAX_EXPORT: usize = 8 * 1024 * 1024;
const MAX_MESSAGES: usize = 16_384;

fn invalid() -> StorageError {
    StorageError::new("INVALID_CAPTURE_FRAME")
}
fn unsupported() -> StorageError {
    StorageError::new("ADAPTER_UNSUPPORTED")
}
fn id<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .filter(|s| super::super::valid_id(s))
        .ok_or_else(unsupported)
}

struct Message {
    raw: Value,
    text: String,
    visibility: Visibility,
    time_ms: i64,
    summary: bool,
    visible: bool,
    first_row: u64,
    archived_row: Option<u64>,
}

/// The source is supplied by an authenticated caller, never discovered from this export.
/// Native profile labels alone are insufficient: the registered source also binds its home.
pub fn decode_export(
    source: &SourceIdentity,
    native_profile: &str,
    bytes: &[u8],
) -> Result<Vec<CaptureFrame>> {
    if bytes.len() > MAX_EXPORT {
        return Err(StorageError::new("CAPTURE_LIMIT"));
    }
    if source.adapter != "hermes"
        || source.session != source.thread
        || [&source.profile, &source.session, &source.agent]
            .iter()
            .any(|s| !super::super::valid_id(s))
    {
        return Err(StorageError::new("INVALID_CAPTURE_BINDING"));
    }
    // serde rejects trailing objects, so a multi-session export cannot silently select one.
    let native: Value = serde_json::from_slice(bytes).map_err(|_| invalid())?;
    if id(&native, "id")? != source.session || id(&native, "profile_name")? != native_profile {
        return Err(StorageError::new("BINDING_CONFLICT"));
    }
    let origin = id(&native, "source")?;
    let parent = match &native["parent_session_id"] {
        Value::Null => None,
        Value::String(s) if super::super::valid_id(s) && *s != source.session => Some(s),
        _ => return Err(invalid()),
    };
    let cwd = native["cwd"].as_str().ok_or_else(unsupported)?;
    if !std::path::Path::new(cwd).is_absolute() {
        return Err(invalid());
    }
    let raw = json!({
        "id":source.session, "source":origin, "profile_name":native_profile,
        "parent_session_id":parent, "cwd":cwd,
        "omitted":["native_instructions_and_configuration"]
    });
    let mut frames = vec![frame(
        source,
        None,
        &raw,
        raw.to_string(),
        Visibility::Session,
        None,
        None,
    )?];
    let rows = native["messages"].as_array().ok_or_else(unsupported)?;
    if rows.len() > MAX_MESSAGES {
        return Err(StorageError::new("CAPTURE_LIMIT"));
    }
    // Compaction clones rows with the same native UID. Keep the first position if any
    // identical copy is still active or archived; discarded undo rows do not reappear.
    let mut messages: Vec<Message> = Vec::new();
    let mut positions = HashMap::new();
    let mut previous_row = 0;
    for row in rows {
        let row_id = row["id"].as_u64().ok_or_else(invalid)?;
        if row_id <= previous_row {
            return Err(invalid());
        }
        previous_row = row_id;
        let message = message(row)?;
        let key = hash(&serde_json::to_vec(&message.raw)?);
        if let Some(&position) = positions.get(&key) {
            let existing: &mut Message = &mut messages[position];
            existing.visible |= message.visible;
            if existing.archived_row.is_none() {
                existing.archived_row = message.archived_row;
            }
        } else {
            positions.insert(key, messages.len());
            messages.push(message);
        }
    }
    let mut archived: Vec<(u64, EventRef)> = Vec::new();
    for message in messages.into_iter().filter(|m| m.visible) {
        let parent = frames.last().map(|f| reference(source, f));
        let kind = if message.summary {
            // A carried clone can be archived by a later compaction. Its physical
            // position prevents that later flag from changing an earlier summary.
            let originals: Vec<_> = archived
                .iter()
                .filter(|(row, _)| *row < message.first_row)
                .map(|(_, reference)| reference.clone())
                .collect();
            let first = originals
                .first()
                .cloned()
                .ok_or_else(|| StorageError::new("HISTORY_GAP"))?;
            let last = originals.last().cloned().ok_or_else(invalid)?;
            Some(EventKind::Compaction {
                first: Box::new(first),
                last: Box::new(last),
                originals: if originals.len() <= 64 {
                    originals.clone()
                } else {
                    vec![originals[0].clone(), originals[originals.len() - 1].clone()]
                },
            })
        } else {
            None
        };
        let next = frame(
            source,
            parent,
            &message.raw,
            message.text,
            message.visibility,
            Some(message.time_ms),
            kind,
        )?;
        if !message.summary
            && message.visibility != Visibility::Session
            && let Some(row) = message.archived_row
        {
            archived.push((row, reference(source, &next)));
        }
        frames.push(next);
    }
    Ok(frames)
}

fn reference(source: &SourceIdentity, frame: &CaptureFrame) -> EventRef {
    EventRef {
        source: source.clone(),
        event_id: frame.event.event_id.clone(),
    }
}

fn message(row: &Value) -> Result<Message> {
    let uid = id(row, "message_uid")?;
    let role = id(row, "role")?;
    let visibility = match role {
        "user" => Visibility::User,
        "assistant" => Visibility::Assistant,
        "tool" => Visibility::Tool,
        "system" | "developer" => Visibility::Session,
        _ => return Err(unsupported()),
    };
    let flag = |key: &str| -> Result<bool> {
        match row[key].as_u64() {
            Some(0) => Ok(false),
            Some(1) => Ok(true),
            _ => Err(unsupported()),
        }
    };
    let archived = flag("compacted")?;
    let visible = flag("active")? | archived;
    let summary = match &row["_compressed_summary"] {
        Value::Null => false,
        Value::Bool(v) => *v,
        _ => return Err(unsupported()),
    };
    let time = row["timestamp"]
        .as_f64()
        .filter(|t| t.is_finite() && *t >= 0.0 && *t < (i64::MAX / 1000) as f64)
        .ok_or_else(unsupported)?;
    let mut raw = json!({"message_uid":uid,"role":role,"timestamp":row["timestamp"],
        "summary":summary});
    let mut text = String::new();
    if visibility == Visibility::Session {
        raw["omitted"] = json!(["native_harness_message"]);
    } else {
        text = visible_content(&row["content"])?;
        raw["content"] = json!(text);
        if let Some(calls) = row.get("tool_calls").filter(|v| !v.is_null()) {
            if role != "assistant" {
                return Err(unsupported());
            }
            let calls = calls.as_array().ok_or_else(unsupported)?;
            let mut safe = Vec::new();
            for call in calls {
                let call_id = id(call, "id")?;
                if call["type"] != "function" {
                    return Err(unsupported());
                }
                let name = id(&call["function"], "name")?;
                let arguments = call["function"]["arguments"]
                    .as_str()
                    .ok_or_else(unsupported)?;
                text.push_str(&format!("\n{name}\n{arguments}"));
                safe.push(json!({"id":call_id,"type":"function",
                    "function":{"name":name,"arguments":arguments}}));
            }
            raw["tool_calls"] = json!(safe);
        }
        if role == "tool" {
            raw["tool_call_id"] = json!(id(row, "tool_call_id")?);
            raw["tool_name"] = json!(id(row, "tool_name")?);
        }
    }
    Ok(Message {
        raw,
        text,
        visibility,
        time_ms: (time * 1000.0) as i64,
        summary,
        visible,
        first_row: row["id"].as_u64().ok_or_else(invalid)?,
        archived_row: archived.then(|| row["id"].as_u64()).flatten(),
    })
}

fn visible_content(content: &Value) -> Result<String> {
    match content {
        Value::Null => Ok(String::new()),
        Value::String(s) => Ok(s.clone()),
        Value::Array(blocks) => {
            let mut text = Vec::new();
            for block in blocks {
                match block["type"].as_str() {
                    Some("text" | "input_text" | "output_text") => {
                        text.push(block["text"].as_str().ok_or_else(unsupported)?.to_owned());
                    }
                    Some(
                        "image_url" | "image" | "input_audio" | "audio" | "video_url" | "file",
                    ) => {
                        text.push("[attachment payload unavailable]".into());
                    }
                    _ => return Err(unsupported()),
                }
            }
            Ok(text.join("\n"))
        }
        _ => Err(unsupported()),
    }
}

fn frame(
    source: &SourceIdentity,
    parent: Option<EventRef>,
    raw: &Value,
    text: String,
    visibility: Visibility,
    time: Option<i64>,
    kind: Option<EventKind>,
) -> Result<CaptureFrame> {
    let raw_bytes = serde_json::to_vec(raw)?;
    if raw_bytes.len() > 4 * 1024 * 1024 || text.len() > 1024 * 1024 {
        return Err(StorageError::new("CAPTURE_LIMIT"));
    }
    // An undo or edited carrier gets a distinct ancestry; old fences stay immutable.
    let event_id = format!(
        "export:{}",
        hash(&serde_json::to_vec(&(source, &parent, raw))?)
    );
    let kind = kind.unwrap_or_else(|| {
        if visibility == Visibility::Session {
            EventKind::Execution
        } else {
            EventKind::Message {
                message_id: event_id.clone(),
                phase: MessagePhase::Final,
            }
        }
    });
    Ok(CaptureFrame {
        schema_version: 1,
        event: EventDraft {
            event_id,
            source_version: "hermes-export-0.22.5".into(),
            runtime_instance: source.session.clone(),
            branch: source.session.clone(),
            parent,
            fork_cutoff: None,
            event_time_ms: time,
            observed_time_ms: 0,
            visibility,
            kind,
            text,
        },
        raw_hex: crate::capture::encode_hex(&raw_bytes),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    const BEFORE: &[u8] = include_bytes!("../../../tests/fixtures/hermes/0.22.5/root-before.jsonl");
    const AFTER: &[u8] =
        include_bytes!("../../../tests/fixtures/hermes/0.22.5/root-compacted.jsonl");
    fn source() -> SourceIdentity {
        SourceIdentity {
            adapter: "hermes".into(),
            profile: "registered-profile-home-key".into(),
            session: "20261029_021600_a00001".into(),
            thread: "20261029_021600_a00001".into(),
            agent: "main".into(),
        }
    }
    #[test]
    fn native_export_preserves_compacted_originals_and_deduplicates_carried_uids() {
        let before = decode_export(&source(), "custom", BEFORE).unwrap();
        let after = decode_export(&source(), "custom", AFTER).unwrap();
        assert_eq!(before.len(), 6);
        assert_eq!(after.len(), 8);
        assert_eq!(&after[..6], before.as_slice());
        assert!(matches!(after[6].event.kind, EventKind::Compaction { .. }));
        if let EventKind::Compaction {
            first,
            last,
            originals,
        } = &after[6].event.kind
        {
            assert_eq!(**first, reference(&source(), &before[1]));
            // The carried tail stays visible, but it was not one of the archived originals.
            assert_eq!(**last, reference(&source(), &before[4]));
            assert!(!originals.contains(&reference(&source(), &before[5])));
        }
        let text = after
            .iter()
            .map(|f| f.event.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        assert!(text.contains("Original public preference: use small files."));
        assert!(text.contains("Public working explanation."));
        assert!(text.contains("read_file"));
        assert!(text.contains("Synthetic file contents."));
        assert!(text.contains("New question after compaction."));
        assert_eq!(text.matches("Answer before interruption.").count(), 1);
        let raw = after
            .iter()
            .flat_map(|f| {
                f.raw_hex
                    .as_bytes()
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|pair| u8::from_str_radix(std::str::from_utf8(pair).unwrap(), 16).unwrap())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        let raw = String::from_utf8(raw).unwrap();
        assert!(!raw.contains("PRIVATE_REASONING_SENTINEL"));
        assert!(!raw.contains("PRIVATE_CONFIG_SENTINEL"));
        assert!(raw.contains("m_user"));
        assert!(raw.contains("m_tail"));
    }

    #[test]
    fn export_identity_and_size_failures_do_not_become_empty_history() {
        assert_eq!(
            decode_export(&source(), "other", BEFORE).unwrap_err().code,
            "BINDING_CONFLICT"
        );
        let mut wrong = source();
        wrong.session = "other-session".into();
        wrong.thread = wrong.session.clone();
        assert_eq!(
            decode_export(&wrong, "custom", BEFORE).unwrap_err().code,
            "BINDING_CONFLICT"
        );
        let multi = [BEFORE, BEFORE].concat();
        assert_eq!(
            decode_export(&source(), "custom", &multi).unwrap_err().code,
            "INVALID_CAPTURE_FRAME"
        );
        assert_eq!(
            decode_export(&source(), "custom", &vec![b' '; 8 * 1024 * 1024 + 1])
                .unwrap_err()
                .code,
            "CAPTURE_LIMIT"
        );
        let mut native: serde_json::Value = serde_json::from_slice(BEFORE).unwrap();
        native["messages"][0]["message_uid"] = serde_json::Value::Null;
        assert_eq!(
            decode_export(&source(), "custom", &serde_json::to_vec(&native).unwrap())
                .unwrap_err()
                .code,
            "ADAPTER_UNSUPPORTED"
        );
        native["messages"][0]["message_uid"] = "m_user".into();
        native["messages"][0]["role"] = "unknown-native-role".into();
        assert_eq!(
            decode_export(&source(), "custom", &serde_json::to_vec(&native).unwrap())
                .unwrap_err()
                .code,
            "ADAPTER_UNSUPPORTED"
        );
    }

    #[test]
    fn later_compaction_does_not_rewrite_an_earlier_summary_or_carried_tail() {
        let before = decode_export(&source(), "custom", AFTER).unwrap();
        let mut native: Value = serde_json::from_slice(AFTER).unwrap();
        let mut tail = native["messages"][7].clone();
        tail["id"] = 10.into();
        for index in [5, 6] {
            native["messages"][index]["active"] = 0.into();
            native["messages"][index]["compacted"] = 1.into();
        }
        native["messages"][7]["active"] = 0.into();
        let rows = native["messages"].as_array_mut().unwrap();
        rows.push(json!({"id":9,"message_uid":"m_summary2","role":"user",
            "content":"Second summary.","timestamp":1793240168.0,
            "active":1,"compacted":0,"_compressed_summary":true}));
        rows.push(tail);
        let after =
            decode_export(&source(), "custom", &serde_json::to_vec(&native).unwrap()).unwrap();
        assert_eq!(&after[..8], before.as_slice());
        assert_eq!(after.len(), 9);
        if let EventKind::Compaction { last, .. } = &after[8].event.kind {
            assert_eq!(**last, reference(&source(), &before[5]));
        } else {
            panic!("expected the second compaction");
        }
    }

    #[test]
    fn removed_rows_stay_out_of_current_lineage_and_child_provenance_is_explicit() {
        let mut native: serde_json::Value = serde_json::from_slice(AFTER).unwrap();
        native["messages"][0]["compacted"] = 0.into();
        let frames =
            decode_export(&source(), "custom", &serde_json::to_vec(&native).unwrap()).unwrap();
        assert!(
            !frames
                .iter()
                .any(|f| f.event.text.contains("Original public preference"))
        );
        assert!(
            frames
                .iter()
                .any(|f| f.event.text.contains("Public working explanation"))
        );
        let mut child = source();
        child.session = "20261029_021601_a00002".into();
        child.thread = child.session.clone();
        child.agent = "native-child".into();
        let frames = decode_export(
            &child,
            "custom",
            include_bytes!("../../../tests/fixtures/hermes/0.22.5/child.jsonl"),
        )
        .unwrap();
        assert_eq!(frames.len(), 2);
        assert!(frames[0].event.text.contains("20261029_021600_a00001"));
        assert!(frames[0].event.text.contains("delegate_task"));
        assert!(
            frames[1]
                .event
                .text
                .contains("Child task with its own provenance.")
        );
    }
}
