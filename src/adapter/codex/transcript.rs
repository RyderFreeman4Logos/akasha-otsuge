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
fn field<'a>(value: &'a Value, key: &str) -> Result<&'a str> {
    value[key]
        .as_str()
        .filter(|s| super::super::valid_id(s))
        .ok_or_else(invalid)
}
fn reference(source: &SourceIdentity, ordinal: u64) -> EventRef {
    EventRef {
        source: source.clone(),
        event_id: format!("row:{ordinal}"),
    }
}

/// The caller supplies authenticated identity; a transcript path is not proof of a caller.
pub async fn reconcile_transcript(
    store: &Storage,
    source: &SourceIdentity,
    scope: Scope,
    path: &Path,
    limit: usize,
    config: u64,
) -> Result<ReconcileReport> {
    if source.adapter != "codex" || source.session != source.thread || source.agent != "main" {
        return Err(StorageError::new("INVALID_CAPTURE_BINDING"));
    }
    let binding = CaptureBinding::from_source(
        source.clone(),
        scope,
        super::TRANSCRIPT_STREAM,
        CoverageLevel::VisibleText,
    )?;
    // Verify the pinned native header even when resuming beyond it.
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
    match OpenOptions::new()
        .read(true)
        .custom_flags(0o4000)
        .open(path)
    {
        Ok(file) => {
            if !file.metadata()?.is_file() {
                return Err(StorageError::new("INVALID_CAPTURE_FILE"));
            }
            let mut bytes = Vec::new();
            BufReader::new(file.take(4 * 1024 * 1024 + 1)).read_until(b'\n', &mut bytes)?;
            if bytes.len() > 4 * 1024 * 1024 {
                return Err(StorageError::new("INVALID_CAPTURE_FRAME"));
            }
            if bytes.last() == Some(&b'\n') {
                let header: Value = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
                if header["type"] != "session_meta" {
                    return Err(invalid());
                }
                validate_meta(&header, source)?;
                return Ok(Some(header));
            }
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
        Err(e) => return Err(e.into()),
    }
    Ok(None)
}

fn validate_meta(row: &Value, source: &SourceIdentity) -> Result<()> {
    let p = &row["payload"];
    if p["cli_version"] != "0.164.0" {
        return Err(StorageError::new("ADAPTER_UNSUPPORTED"));
    }
    if field(p, "id")? != source.session || field(p, "session_id")? != source.session {
        return Err(StorageError::new("BINDING_CONFLICT"));
    }
    let ordinal = row["ordinal"].as_u64().ok_or_else(invalid)?;
    if let Some(parent) = p.get("forked_from_id") {
        let parent = parent
            .as_str()
            .filter(|s| super::super::valid_id(s))
            .ok_or_else(invalid)?;
        if parent == source.session
            || ordinal == 0
            || p["forked_from_ordinal_exclusive"].as_u64() != Some(ordinal)
            || p["history_base"]["thread_id"].as_str() != Some(parent)
            || p["history_base"]["end_ordinal_exclusive"].as_u64() != Some(ordinal)
        {
            return Err(invalid());
        }
    } else if ordinal != 0 {
        return Err(invalid());
    }
    Ok(())
}

fn decode(
    store: &Storage,
    binding: &CaptureBinding,
    source: &SourceIdentity,
    line: &[u8],
) -> Result<CaptureFrame> {
    let row: Value = serde_json::from_slice(line).map_err(|_| invalid())?;
    let ordinal = row["ordinal"].as_u64().ok_or_else(invalid)?;
    let kind = field(&row, "type")?;
    let payload = &row["payload"];
    let event_id = format!("row:{ordinal}");
    let mut parent = ordinal.checked_sub(1).map(|i| reference(source, i));
    let mut fork_cutoff = None;
    let mut event_kind = EventKind::Execution;
    let mut visibility = Visibility::Session;
    let mut text = String::new();
    let mut raw = row.clone();
    if kind != "session_meta" {
        let previous = parent.as_ref().ok_or_else(invalid)?;
        if Capture::new(store)
            .event(binding, previous, store.sequence()?)?
            .is_none()
        {
            return Err(invalid());
        }
    }
    match kind {
        "session_meta" => {
            validate_meta(&row, source)?;
            if let Some(id) = payload["forked_from_id"].as_str() {
                let mut ancestor = source.clone();
                ancestor.session = id.into();
                ancestor.thread = id.into();
                parent = Some(reference(&ancestor, ordinal - 1));
                fork_cutoff = parent.clone();
                event_kind = EventKind::Fork;
            }
            raw["payload"] = select(
                payload,
                &[
                    "id",
                    "session_id",
                    "cli_version",
                    "cwd",
                    "runtime_workspace_roots",
                    "source",
                    "originator",
                    "forked_from_id",
                    "forked_from_ordinal_exclusive",
                    "history_base",
                    "context_window",
                ],
            );
            raw["omitted"] = json!(["native_instructions_and_configuration"]);
        }
        "response_item" => {
            let item_type = field(payload, "type")?;
            match item_type {
                "message" => {
                    let role = field(payload, "role")?;
                    let harness =
                        payload["internal_chat_message_metadata_passthrough"]["content_item_kinds"]
                            .as_array()
                            .is_some_and(|kinds| {
                                !kinds.is_empty()
                                    && kinds.iter().all(|k| {
                                        k.as_str().is_some_and(|k| !k.starts_with("user."))
                                    })
                            });
                    match role {
                        "user" if !harness => visibility = Visibility::User,
                        "assistant" => visibility = Visibility::Assistant,
                        "user" | "developer" | "system" => {}
                        _ => return Err(invalid()),
                    }
                    if visibility != Visibility::Session {
                        text = visible_blocks(&payload["content"])?;
                        raw["payload"]["content"] = safe_blocks(&payload["content"])?;
                        event_kind = EventKind::Message {
                            message_id: event_id.clone(),
                            phase: MessagePhase::Final,
                        };
                    } else {
                        raw["payload"] = select(payload, &["type", "id", "role"]);
                        raw["omitted"] = json!(["native_harness_message"]);
                    }
                }
                "function_call" | "custom_tool_call" => {
                    visibility = Visibility::Tool;
                    field(payload, "call_id")?;
                    let name = field(payload, "name")?;
                    let input = if item_type == "function_call" {
                        "arguments"
                    } else {
                        "input"
                    };
                    let args = payload[input].as_str().ok_or_else(invalid)?;
                    text = format!("{name}\n{args}");
                }
                "function_call_output" | "custom_tool_call_output" => {
                    visibility = Visibility::Tool;
                    field(payload, "call_id")?;
                    if let Some(output) = payload["output"].as_str() {
                        text = output.into();
                    } else {
                        text = visible_blocks(&payload["output"])?;
                        raw["payload"]["output"] = safe_blocks(&payload["output"])?;
                    }
                }
                "reasoning" => {
                    visibility = Visibility::Assistant;
                    // Only the native public summary is visible; encrypted/private reasoning is omitted.
                    text = visible_blocks(&payload["summary"])?;
                    raw["payload"] = select(payload, &["type", "id", "summary"]);
                    raw["omitted"] = json!(["private_reasoning"]);
                }
                _ => return Err(invalid()),
            }
        }
        "compacted" => {
            text = payload["message"].as_str().ok_or_else(invalid)?.into();
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
            raw["payload"] = select(
                payload,
                &[
                    "message",
                    "window_number",
                    "first_window_id",
                    "previous_window_id",
                    "window_id",
                    "compaction_response_id",
                ],
            );
            raw["omitted"] = json!(["replacement_history_duplicates_and_private_state"]);
        }
        "event_msg" => {
            let event = field(payload, "type")?;
            match event {
                "task_started" | "task_complete" | "thread_settings_applied" | "token_count" => {
                    raw["payload"] = select(payload, &["type", "turn_id", "root_turn_id"]);
                }
                "item_completed" => {
                    raw["payload"] = json!({"type":event, "turn_id":payload["turn_id"], "item":select(&payload["item"], &["id", "type"])});
                    raw["omitted"] = json!(["duplicate_item_content"]);
                }
                "task_aborted" | "turn_aborted" => {
                    event_kind = EventKind::Abandoned;
                }
                "error" | "warning" => {
                    text = payload["message"].as_str().ok_or_else(invalid)?.into();
                }
                _ => return Err(invalid()),
            }
        }
        "world_state" => {
            raw["payload"] = json!({});
            raw["omitted"] = json!(["native_world_state"]);
        }
        "turn_context" => {
            raw["payload"] = select(
                payload,
                &[
                    "turn_id",
                    "root_turn_id",
                    "cwd",
                    "approval_policy",
                    "sandbox_policy",
                    "model",
                ],
            );
            raw["omitted"] = json!(["native_turn_configuration"]);
        }
        "token_usage_record" => {
            if payload["session_id"] != source.session {
                return Err(invalid());
            }
        }
        _ => return Err(invalid()),
    }
    let raw = serde_json::to_vec(&raw)?;
    Ok(CaptureFrame {
        schema_version: 1,
        event: EventDraft {
            event_id,
            source_version: "0.164.0".into(),
            runtime_instance: source.session.clone(),
            branch: source.thread.clone(),
            parent,
            fork_cutoff,
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
    })
}

fn select(value: &Value, keys: &[&str]) -> Value {
    let mut result = serde_json::Map::new();
    for key in keys {
        if let Some(v) = value.get(key) {
            result.insert((*key).into(), v.clone());
        }
    }
    Value::Object(result)
}
fn safe_blocks(value: &Value) -> Result<Value> {
    let blocks = value.as_array().ok_or_else(invalid)?;
    blocks.iter().map(|block| {
        Ok(match field(block, "type")? {
            "input_text" | "output_text" | "summary_text" => {
                json!({"type":block["type"], "text":block["text"].as_str().ok_or_else(invalid)?})
            }
            "input_image" | "image_url" => json!({"type":block["type"], "omitted":"binary_attachment"}),
            _ => return Err(invalid()),
        })
    }).collect::<Result<Vec<_>>>().map(Value::Array)
}
fn visible_blocks(value: &Value) -> Result<String> {
    Ok(safe_blocks(value)?
        .as_array()
        .ok_or_else(invalid)?
        .iter()
        .map(|block| {
            block["text"]
                .as_str()
                .unwrap_or("[binary attachment unavailable]")
        })
        .collect::<Vec<_>>()
        .join("\n"))
}
fn first_ancestor(store: &Storage, binding: &CaptureBinding, last: &EventRef) -> Result<EventRef> {
    let capture = Capture::new(store);
    let mut next = last.clone();
    let mut seen = std::collections::BTreeSet::new();
    for _ in 0..1024 {
        let page = capture.lineage(binding, &next, store.sequence()?, 64)?;
        if page.missing.is_some() || page.events.is_empty() {
            return Err(invalid());
        }
        for event in &page.events {
            if !seen.insert(serde_json::to_string(&event.event.reference)?) {
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
