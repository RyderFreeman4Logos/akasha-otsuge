//! Native append-only session trees. The authenticated caller selects a leaf, never a file tail.
use crate::{
    capture::*,
    domain::Scope,
    storage::{Result, Storage, StorageError},
};
use serde_json::{Value, json};
use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader, Read},
    os::unix::fs::OpenOptionsExt,
    path::Path,
};

fn invalid() -> StorageError {
    StorageError::new("INVALID_CAPTURE_FRAME")
}
fn id<'a>(row: &'a Value, key: &str) -> Result<&'a str> {
    row[key]
        .as_str()
        .filter(|v| super::super::valid_id(v))
        .ok_or_else(invalid)
}
fn reference(source: &SourceIdentity, native: Option<&str>) -> EventRef {
    EventRef {
        source: source.clone(),
        event_id: native
            .map(|v| format!("entry:{v}"))
            .unwrap_or("session-header".into()),
    }
}

/// Identity and the path must come from a registered host; the file cannot authenticate a caller.
pub async fn reconcile_transcript(
    store: &Storage,
    source: &SourceIdentity,
    scope: Scope,
    path: &Path,
    limit: usize,
    config: u64,
) -> Result<ReconcileReport> {
    if source.adapter != "pi" || source.session != source.thread || source.agent != "main" {
        return Err(StorageError::new("INVALID_CAPTURE_BINDING"));
    }
    let binding = CaptureBinding::from_source(
        source.clone(),
        scope,
        super::TRANSCRIPT_STREAM,
        CoverageLevel::VisibleText,
    )?;
    header(path, source)?;
    Capture::new(store)
        .reconcile_decoded(
            &binding,
            path,
            limit,
            config,
            |line| decode(store, &binding, source, line).map(Some),
            |_| {},
        )
        .await
}

pub(super) fn header(path: &Path, source: &SourceIdentity) -> Result<Option<Value>> {
    let file = match OpenOptions::new()
        .read(true)
        .custom_flags(0o4000)
        .open(path)
    {
        Ok(file) => file,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e.into()),
    };
    if !file.metadata()?.is_file() {
        return Err(StorageError::new("INVALID_CAPTURE_FILE"));
    }
    let mut line = Vec::new();
    BufReader::new(file.take(65537)).read_until(b'\n', &mut line)?;
    if line.len() > 65536 {
        return Err(invalid());
    }
    if line.last() != Some(&b'\n') {
        return Ok(None);
    }
    let row: Value = serde_json::from_slice(&line).map_err(|_| invalid())?;
    validate_header(&row, source)?;
    Ok(Some(row))
}

fn validate_header(row: &Value, source: &SourceIdentity) -> Result<()> {
    if row["type"] != "session" {
        return Err(invalid());
    }
    if row["version"] != 3 {
        return Err(StorageError::new("ADAPTER_UNSUPPORTED"));
    }
    if id(row, "id")? != source.session {
        return Err(StorageError::new("BINDING_CONFLICT"));
    }
    let cwd = row["cwd"].as_str().ok_or_else(invalid)?;
    if cwd.len() > 4096 || !Path::new(cwd).is_absolute() {
        return Err(invalid());
    }
    if let Some(parent) = row.get("parentSession") {
        let path = parent.as_str().ok_or_else(invalid)?;
        if path.len() > 4096 || !Path::new(path).is_absolute() {
            return Err(invalid());
        }
    }
    Ok(())
}

pub(super) fn decode(
    store: &Storage,
    binding: &CaptureBinding,
    source: &SourceIdentity,
    line: &[u8],
) -> Result<CaptureFrame> {
    let row: Value = serde_json::from_slice(line).map_err(|_| invalid())?;
    let kind = id(&row, "type")?;
    let is_header = kind == "session";
    let native_id = if is_header {
        None
    } else {
        Some(id(&row, "id")?)
    };
    let parent = if is_header {
        None
    } else {
        let native_parent = match row.get("parentId") {
            Some(Value::Null) => None,
            Some(Value::String(s))
                if super::super::valid_id(s) && Some(s.as_str()) != native_id =>
            {
                Some(s.as_str())
            }
            _ => return Err(invalid()),
        };
        Some(reference(source, native_parent))
    };
    let mut event_kind = EventKind::Execution;
    let mut visibility = Visibility::Session;
    let mut text = String::new();
    let mut raw = select(&row, &["type", "id", "parentId", "timestamp"]);
    match kind {
        "session" => {
            validate_header(&row, source)?;
            raw = select(
                &row,
                &["type", "version", "id", "timestamp", "cwd", "parentSession"],
            );
            // Native fork files contain their copied path. Preserve that exact local tree;
            // parentSession is provenance, not permission to read a guessed parent session.
            if row.get("parentSession").is_some() {
                event_kind = EventKind::Fork;
            }
        }
        "message" => {
            let message = &row["message"];
            let role = id(message, "role")?;
            raw["message"] = select(
                message,
                &[
                    "role",
                    "timestamp",
                    "api",
                    "provider",
                    "model",
                    "stopReason",
                    "toolCallId",
                    "toolName",
                    "isError",
                    "customType",
                    "display",
                ],
            );
            match role {
                "system" => {
                    raw["omitted"] = json!(["native_system_prompt_and_tools"]);
                }
                "user" | "assistant" | "toolResult" | "custom" => {
                    visibility = match role {
                        "user" => Visibility::User,
                        "assistant" => Visibility::Assistant,
                        "toolResult" => Visibility::Tool,
                        _ => Visibility::Session,
                    };
                    let (content, visible) = content(&message["content"])?;
                    text = visible;
                    raw["message"]["content"] = content;
                    raw["omitted"] = json!(["private_thinking_signatures_and_details"]);
                    event_kind = EventKind::Message {
                        message_id: reference(source, native_id).event_id,
                        phase: MessagePhase::Final,
                    };
                }
                "bashExecution" => {
                    visibility = Visibility::Tool;
                    let command = message["command"].as_str().ok_or_else(invalid)?;
                    let output = message["output"].as_str().ok_or_else(invalid)?;
                    text = format!("{command}\n{output}");
                    raw["message"] = select(
                        message,
                        &[
                            "role",
                            "command",
                            "output",
                            "exitCode",
                            "cancelled",
                            "truncated",
                            "excludeFromContext",
                            "timestamp",
                        ],
                    );
                    raw["omitted"] = json!(["external_full_output"]);
                }
                _ => return Err(invalid()),
            }
        }
        "compaction" => {
            text = row["summary"].as_str().ok_or_else(invalid)?.into();
            let kept = id(&row, "firstKeptEntryId")?;
            let last = parent.clone().ok_or_else(invalid)?;
            let first = first_ancestor(store, binding, &last)?;
            event_kind = EventKind::Compaction {
                first: Box::new(first.clone()),
                last: Box::new(last.clone()),
                originals: if first == last {
                    vec![first]
                } else {
                    vec![first, last]
                },
            };
            raw["summary"] = json!(text);
            raw["firstKeptEntryId"] = json!(kept);
            raw["tokensBefore"] = row["tokensBefore"].clone();
            raw["omitted"] = json!(["native_system_checkpoint_and_details"]);
        }
        "context_edit" => {
            raw["targetId"] = json!(id(&row, "targetId")?);
            raw["replacement"] = match row.get("replacement") {
                Some(Value::Null) => Value::Null,
                Some(replacement) => {
                    let (safe, _) = content(&replacement["content"])?;
                    json!({"content":safe})
                }
                None => return Err(invalid()),
            };
            // This edits future model context only. Do not replace original captured messages.
            text = format!(
                "Context edit for {}: {}",
                raw["targetId"], raw["replacement"]
            );
        }
        "branch_summary" => {
            text = row["summary"].as_str().ok_or_else(invalid)?.into();
            raw["summary"] = json!(text);
            raw["fromId"] = row["fromId"].clone();
            raw["omitted"] = json!(["extension_details"]);
        }
        "custom_message" => {
            let (safe, visible) = content(&row["content"])?;
            text = visible;
            raw["content"] = safe;
            raw["customType"] = row["customType"].clone();
            raw["display"] = row["display"].clone();
            raw["omitted"] = json!(["extension_details"]);
        }
        "custom" => {
            raw["customType"] = row["customType"].clone();
            raw["omitted"] = json!(["extension_private_state"]);
        }
        "model_change" | "thinking_level_change" | "usage" | "label" | "session_info" => {
            for key in [
                "provider",
                "modelId",
                "thinkingLevel",
                "kind",
                "targetId",
                "label",
                "name",
            ] {
                if let Some(value) = row.get(key) {
                    raw[key] = value.clone();
                }
            }
        }
        _ => return Err(invalid()),
    }
    let bytes = serde_json::to_vec(&raw)?;
    Ok(CaptureFrame {
        schema_version: 1,
        event: EventDraft {
            event_id: reference(source, native_id).event_id,
            source_version: "pi-session-v3".into(),
            runtime_instance: source.session.clone(),
            branch: source.thread.clone(),
            parent,
            fork_cutoff: None,
            event_time_ms: row["message"]["timestamp"].as_i64(),
            observed_time_ms: crate::ipc::now_ms()
                .map_err(|_| invalid())?
                .try_into()
                .map_err(|_| invalid())?,
            visibility,
            kind: event_kind,
            text,
        },
        raw_hex: bytes.iter().map(|b| format!("{b:02x}")).collect(),
    })
}

fn select(value: &Value, fields: &[&str]) -> Value {
    let mut result = serde_json::Map::new();
    for key in fields {
        if let Some(value) = value.get(key) {
            result.insert((*key).into(), value.clone());
        }
    }
    Value::Object(result)
}

fn content(value: &Value) -> Result<(Value, String)> {
    if let Some(text) = value.as_str() {
        return Ok((json!(text), text.into()));
    }
    let blocks = value.as_array().ok_or_else(invalid)?;
    let mut safe = Vec::new();
    let mut texts = Vec::new();
    for block in blocks {
        match id(block, "type")? {
            "text" => {
                let text = block["text"].as_str().ok_or_else(invalid)?;
                safe.push(json!({"type":"text", "text":text}));
                texts.push(text.into());
            }
            "thinking" => {
                safe.push(json!({"type":"thinking", "omitted":"private_thinking"}));
            }
            "image" => {
                safe.push(json!({"type":"image", "omitted":"binary_attachment"}));
                texts.push("[binary attachment unavailable]".into());
            }
            "toolCall" => {
                let name = id(block, "name")?;
                let call = id(block, "id")?;
                if !block["arguments"].is_object() {
                    return Err(invalid());
                }
                safe.push(json!({"type":"toolCall", "id":call, "name":name, "arguments":block["arguments"]}));
                texts.push(format!("{name}\n{}", block["arguments"]));
            }
            _ => return Err(invalid()),
        }
    }
    Ok((Value::Array(safe), texts.join("\n")))
}

fn first_ancestor(store: &Storage, binding: &CaptureBinding, last: &EventRef) -> Result<EventRef> {
    let capture = Capture::new(store);
    let mut next = last.clone();
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..256 {
        let page = capture.lineage(binding, &next, store.sequence()?, 64)?;
        if page.missing.is_some() || page.events.is_empty() {
            return Err(invalid());
        }
        for event in &page.events {
            if !seen.insert(event.event.reference.event_id.clone()) {
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
    Err(StorageError::new("CAPTURE_LIMIT"))
}
