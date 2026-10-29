use crate::adapter::*;
use serde_json::{Value, json};
use std::{fs::OpenOptions, io::Read, os::unix::fs::OpenOptionsExt, path::Path};

pub(super) const DISABLED: &[&str] = &[
    "remote_models",
    "shell_tool",
    "multi_agent",
    "multi_agent_v2",
    "plugins",
    "hooks",
    "apps",
    "code_mode",
    "code_mode_only",
    "goals",
    "view_image",
    "image_generation",
    "sleep_tool",
    "js_repl",
    "request_permissions",
    "request_permissions_tool",
    "default_mode_request_user_input",
    "send_message_to_user_async",
    "token_budget",
    "current_time_reminder",
    "tool_suggest",
    "browser_use",
    "computer_use",
    "memory_tool",
    "memories",
    "skill_search",
    "external_agent_memory_import",
];

pub(super) fn catalog(home: &Path) -> Result<Value> {
    // shortcut: native0.164.0 model cache is required; add native catalog discovery for a newer host.
    let file = OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NONBLOCK | rustix::fs::OFlags::NOFOLLOW).bits() as i32)
        .open(home.join("models_cache.json"))
        .map_err(|_| ErrorCode::AdapterUnsupported)?;
    let meta = file.metadata().map_err(|_| ErrorCode::SourceUnavailable)?;
    if !meta.is_file() || meta.len() > 8 * 1024 * 1024 {
        return Err(ErrorCode::OutputLimit.into());
    }
    let mut bytes = Vec::new();
    file.take(8 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ErrorCode::SourceUnavailable)?;
    if bytes.len() > 8 * 1024 * 1024 {
        return Err(ErrorCode::OutputLimit.into());
    }
    let mut value: Value =
        serde_json::from_slice(&bytes).map_err(|_| ErrorCode::ProtocolInvalid)?;
    let models = value["models"]
        .as_array_mut()
        .filter(|v| !v.is_empty() && v.len() <= 256)
        .ok_or(ErrorCode::AdapterUnsupported)?;
    let mut names = std::collections::BTreeSet::new();
    for model in models.iter_mut() {
        let name = model["slug"]
            .as_str()
            .filter(|s| super::super::valid_id(s))
            .ok_or(ErrorCode::AdapterUnsupported)?
            .to_owned();
        if !names.insert(name) {
            return Err(ErrorCode::AdapterUnsupported.into());
        }
        for (key, value) in [
            ("shell_type", json!("disabled")),
            ("apply_patch_tool_type", Value::Null),
            ("experimental_supported_tools", json!([])),
            ("supports_search_tool", json!(false)),
            ("node_repl_disabled", json!(true)),
            ("tool_mode", json!("direct")),
        ] {
            model[key] = value;
        }
    }
    Ok(json!({"models":models}))
}

pub(super) fn tools() -> Value {
    let schemas = [
        ("describe", json!({}), vec![]),
        (
            "list",
            json!({"cursor":{"type":["string","null"]},"limit":{"type":"integer","minimum":1,"maximum":16}}),
            vec!["limit"],
        ),
        (
            "read",
            json!({"handle":{"type":"string"},"start":{"type":"integer","minimum":0},"max_bytes":{"type":"integer","minimum":1,"maximum":16384}}),
            vec!["handle", "start", "max_bytes"],
        ),
        (
            "search",
            json!({"cursor":{"type":["string","null"]},"query":{"type":"string"},"limit":{"type":"integer","minimum":1,"maximum":16}}),
            vec!["query", "limit"],
        ),
    ];
    json!(schemas.into_iter().map(|(name, properties, required)| json!({
        "type":"function","name":format!("context_{name}"),"description":format!("{name} the authorized caller context"),
        "inputSchema":{"type":"object","properties":properties,"required":required,"additionalProperties":false}
    })).collect::<Vec<_>>())
}
