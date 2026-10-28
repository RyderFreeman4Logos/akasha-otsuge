use crate::{
    capture::*,
    storage::{Result, Storage, StorageError},
};
use serde_json::{Value, json};

fn invalid() -> StorageError {
    StorageError::new("INVALID_CAPTURE_FRAME")
}
fn field<'a>(row: &'a Value, key: &str) -> Result<&'a str> {
    row[key]
        .as_str()
        .filter(|s| super::super::valid_id(s))
        .ok_or_else(invalid)
}

pub(crate) fn transcript_frame(
    store: &Storage,
    binding: &CaptureBinding,
    source: &SourceIdentity,
    line: &[u8],
) -> Result<Option<CaptureFrame>> {
    let row: Value = serde_json::from_slice(line).map_err(|_| invalid())?;
    if row
        .get("sessionId")
        .is_some_and(|id| id.as_str() != Some(&source.session))
    {
        return Err(invalid());
    }
    let kind = field(&row, "type")?;
    if matches!(
        kind,
        "queue-operation"
            | "last-prompt"
            | "cost-state"
            | "file-history-snapshot"
            | "progress"
            | "api-request"
            | "api-request-shape"
            | "api-request-blob"
            | "mode"
            | "atis-latch"
            | "tools-used"
    ) {
        return Ok(None);
    }
    if field(&row, "sessionId")? != source.session
        || row["isSidechain"].as_bool() == Some(true)
        || row.get("agentId").is_some_and(|v| !v.is_null())
    {
        return Err(invalid());
    }
    let event_id = field(&row, "uuid")?;
    let version = field(&row, "version")?;
    let compact = kind == "system" && row["subtype"] == "compact_boundary";
    let parent_key = if compact {
        "logicalParentUuid"
    } else {
        "parentUuid"
    };
    let parent = match row.get(parent_key) {
        None | Some(Value::Null) => None,
        Some(_) => Some(EventRef {
            source: source.clone(),
            event_id: field(&row, parent_key)?.into(),
        }),
    };
    let (content, text, visibility, event_kind) = match kind {
        "user" | "assistant" => {
            if row["message"]["role"].as_str() != Some(kind) {
                return Err(invalid());
            }
            let (content, text) = visible_content(&row["message"]["content"])?;
            let visibility = if row["isCompactSummary"] == true
                || row["isMeta"] == true
                || row["isVisibleInTranscriptOnly"] == true
            {
                Visibility::Session
            } else if kind == "assistant" {
                Visibility::Assistant
            } else if content
                .as_array()
                .is_some_and(|v| v.iter().any(|b| b["type"] == "tool_result"))
            {
                Visibility::Tool
            } else {
                Visibility::User
            };
            (
                json!({"role":kind,"content":content}),
                text,
                visibility,
                EventKind::Message {
                    message_id: event_id.into(),
                    phase: MessagePhase::Final,
                },
            )
        }
        "system" if row["subtype"] == "local_command" => {
            let text = row["content"].as_str().ok_or_else(invalid)?.to_owned();
            (json!(text), text, Visibility::Session, EventKind::Execution)
        }
        "attachment" => {
            let rendered = row["rendered"].as_array().ok_or_else(invalid)?;
            let mut content = Vec::new();
            let mut texts = Vec::new();
            for block in rendered {
                let (visible, text) = visible_content(&block["content"])?;
                content.push(visible);
                texts.push(text);
            }
            (
                json!({"attachment_type":field(&row["attachment"],"type")?,"rendered":content}),
                texts.join("\n"),
                Visibility::Session,
                EventKind::Execution,
            )
        }
        "system" if compact => {
            let last = parent.clone().ok_or_else(invalid)?;
            let first = first_ancestor(store, binding, &last)?;
            let originals = if first == last {
                vec![first.clone()]
            } else {
                vec![first.clone(), last.clone()]
            };
            let text = row["content"].as_str().ok_or_else(invalid)?.to_owned();
            (
                json!({"content":text,"compactMetadata":row["compactMetadata"],"logicalParentUuid":row["logicalParentUuid"]}),
                text,
                Visibility::Session,
                EventKind::Compaction {
                    first: Box::new(first),
                    last: Box::new(last),
                    originals,
                },
            )
        }
        _ => return Err(invalid()),
    };
    let mut raw = json!({"type":kind,"uuid":event_id,"parentUuid":row["parentUuid"],
        "sessionId":source.session,"version":version,"timestamp":row["timestamp"],
        "isMeta":row["isMeta"],"message":content});
    for key in ["isCompactSummary", "isVisibleInTranscriptOnly"] {
        if let Some(value) = row.get(key) {
            raw[key] = value.clone();
        }
    }
    let raw = serde_json::to_vec(&raw)?;
    Ok(Some(CaptureFrame {
        schema_version: 1,
        event: EventDraft {
            event_id: event_id.into(),
            source_version: version.into(),
            runtime_instance: source.session.clone(),
            branch: source.thread.clone(),
            parent,
            fork_cutoff: None,
            event_time_ms: None,
            observed_time_ms: crate::ipc::now_ms()
                .map_err(|_| invalid())?
                .try_into()
                .map_err(|_| invalid())?,
            visibility,
            kind: event_kind,
            text,
        },
        raw_hex: raw.iter().map(|b| format!("{b:02x}")).collect(),
    }))
}

fn first_ancestor(store: &Storage, binding: &CaptureBinding, last: &EventRef) -> Result<EventRef> {
    let capture = Capture::new(store);
    let at = store.sequence()?;
    let mut next = last.clone();
    let mut visited = std::collections::BTreeSet::new();
    // Bound source-frame work; cycles, missing links and overlarge ranges stay pending.
    for _ in 0..1024 {
        let page = capture.lineage(binding, &next, at, 64)?;
        if page.missing.is_some() || page.events.is_empty() {
            return Err(invalid());
        }
        for event in &page.events {
            if !visited.insert(serde_json::to_string(&event.event.reference)?) {
                return Err(invalid());
            }
        }
        match page.next {
            Some(reference) => next = reference,
            None => {
                return Ok(page
                    .events
                    .last()
                    .ok_or_else(invalid)?
                    .event
                    .reference
                    .clone());
            }
        }
    }
    Err(invalid())
}

fn visible_content(content: &Value) -> Result<(Value, String)> {
    if let Some(text) = content.as_str() {
        return Ok((content.clone(), text.into()));
    }
    let blocks = content.as_array().ok_or_else(invalid)?;
    let mut visible = Vec::new();
    let mut texts = Vec::new();
    for block in blocks {
        match field(block, "type")? {
            "thinking" | "redacted_thinking" => {}
            "text" => {
                let text = block["text"].as_str().ok_or_else(invalid)?;
                texts.push(text.to_owned());
                visible.push(json!({"type":"text","text":text}));
            }
            "tool_use" => {
                let id = field(block, "id")?;
                let name = field(block, "name")?;
                if !block["input"].is_object() {
                    return Err(invalid());
                }
                let value = json!({"type":"tool_use","id":id,"name":name,"input":block["input"]});
                texts.push(serde_json::to_string(&value)?);
                visible.push(value);
            }
            "tool_result" => {
                let id = field(block, "tool_use_id")?;
                let (content, text) = visible_content(&block["content"])?;
                texts.push(text);
                visible.push(json!({"type":"tool_result","tool_use_id":id,"content":content,"is_error":block["is_error"]}));
            }
            "image" | "document" => {
                visible.push(json!({"type":block["type"],"omitted":"non_text_payload"}));
            }
            _ => return Err(invalid()),
        }
    }
    Ok((Value::Array(visible), texts.join("\n")))
}
