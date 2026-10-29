//! Native hook stdin and per-call private transport; no model-supplied identity.
use super::{CodexCall, NativeEvent};
use crate::{
    adapter::ConsultArgs,
    ipc::{self, Error, Result},
    storage::hash,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::{
    fs::{File, OpenOptions},
    io::{BufRead, BufReader, Read, Seek, SeekFrom, Write},
    os::unix::fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
};

#[derive(Deserialize)]
struct Hook {
    session_id: String,
    transcript_path: PathBuf,
    cwd: PathBuf,
    hook_event_name: String,
    #[serde(default)]
    turn_id: Option<String>,
    #[serde(default)]
    agent_id: Option<String>,
    #[serde(default)]
    tool_use_id: Option<String>,
    #[serde(default)]
    tool_name: Option<String>,
    #[serde(default)]
    tool_input: Value,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BoundCall {
    response: Value,
    call: CodexCall,
    args: ConsultArgs,
    transcript: PathBuf,
}

pub fn parse_hint(command: &str) -> Result<Option<ConsultArgs>> {
    let command = command.trim();
    if command == "akasha-otsuge consult" {
        return Ok(Some(ConsultArgs::new(None)?));
    }
    let Some(hint) = command.strip_prefix("akasha-otsuge consult ") else {
        return Ok(None);
    };
    let hint: String = serde_json::from_str(hint.trim()).map_err(|_| Error("INVALID_ARGUMENT"))?;
    Ok(Some(ConsultArgs::new(Some(hint))?))
}

pub fn quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn private_directory(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(Error("UNSAFE_PATH"));
    }
    match std::fs::DirBuilder::new().mode(0o700).create(path) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => return Err(e.into()),
    }
    let meta = std::fs::symlink_metadata(path)?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || meta.uid() != rustix::process::geteuid().as_raw()
        || meta.mode() & 0o777 != 0o700
    {
        return Err(Error("UNSAFE_PATH"));
    }
    Ok(())
}

fn private_file(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file()
        || meta.uid() != rustix::process::geteuid().as_raw()
        || meta.mode() & 0o777 != 0o600
        || meta.nlink() != 1
        || meta.len() > 16384
    {
        return Err(Error("UNSAFE_PATH"));
    }
    Ok(file)
}

fn read_call(path: &Path) -> Result<BoundCall> {
    let mut bytes = Vec::new();
    private_file(path)?.take(16385).read_to_end(&mut bytes)?;
    if bytes.len() > 16384 {
        return Err(Error("MESSAGE_TOO_LARGE"));
    }
    Ok(serde_json::from_slice(&bytes)?)
}

fn publish_call(path: &Path, bound: &BoundCall) -> Result<()> {
    let temporary = path.with_extension(format!(
        "{}-{}.pending",
        std::process::id(),
        hash(&serde_json::to_vec(bound)?)
    ));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)?;
    let result = (|| {
        file.write_all(&serde_json::to_vec(bound)?)?;
        file.sync_all()?;
        match rustix::fs::renameat_with(
            rustix::fs::CWD,
            &temporary,
            rustix::fs::CWD,
            path,
            rustix::fs::RenameFlags::NOREPLACE,
        ) {
            Ok(()) => Ok(()),
            Err(rustix::io::Errno::EXIST) => {
                let existing = read_call(path)?;
                if serde_json::to_value(&existing.call)? != serde_json::to_value(&bound.call)?
                    || existing.args != bound.args
                    || existing.transcript != bound.transcript
                    || existing.response != bound.response
                {
                    return Err(Error("BINDING_CONFLICT"));
                }
                Ok(())
            }
            Err(error) => Err(std::io::Error::from(error).into()),
        }
    })();
    drop(file);
    if temporary.try_exists()? {
        std::fs::remove_file(temporary)?;
    }
    result
}

fn call_path(state: &Path, hook: &Hook) -> Result<PathBuf> {
    let turn = hook
        .turn_id
        .as_deref()
        .filter(|s| ipc::identifier(s))
        .ok_or(Error("CALLER_AMBIGUOUS"))?;
    let call = hook
        .tool_use_id
        .as_deref()
        .filter(|s| ipc::identifier(s))
        .ok_or(Error("CALLER_AMBIGUOUS"))?;
    Ok(state.join(format!(
        "call-{}.json",
        hash(&serde_json::to_vec(&(&hook.session_id, turn, call))?)
    )))
}

async fn send(socket: &Path, event: NativeEvent) -> Result<Value> {
    let now = ipc::now_ms()?;
    let response = ipc::call(
        socket,
        &ipc::Request {
            protocol_version: ipc::VERSION,
            request_id: format!("codex-hook/{}/{now}", std::process::id()),
            deadline_ms: Some(now + ipc::TRANSPORT_TIMEOUT.as_millis() as u64),
            command: ipc::Command::Codex { event },
        },
    )
    .await?;
    if let Some(code) = response["error"]["code"].as_str() {
        return Err(Error(match code {
            "READ_DENIED" => "READ_DENIED",
            "PATH_DENIED" => "PATH_DENIED",
            "CALLER_AMBIGUOUS" => "CALLER_AMBIGUOUS",
            "ADAPTER_UNSUPPORTED" => "ADAPTER_UNSUPPORTED",
            "HISTORY_GAP" => "HISTORY_GAP",
            "BINDING_CONFLICT" => "BINDING_CONFLICT",
            _ => "BINDING_UNAVAILABLE",
        }));
    }
    response.get("data").cloned().ok_or(Error("PROTOCOL_ERROR"))
}

async fn register(socket: &Path, binary: &Path, hook: &Hook) -> Result<String> {
    let response = send(
        socket,
        NativeEvent::Register {
            binary: binary.into(),
            session_id: hook.session_id.clone(),
            transcript_path: hook.transcript_path.clone(),
            cwd: hook.cwd.clone(),
        },
    )
    .await?;
    response["registration"]
        .as_str()
        .filter(|s| s.len() == 64 && s.bytes().all(|b| b.is_ascii_hexdigit()))
        .map(str::to_owned)
        .ok_or(Error("PROTOCOL_ERROR"))
}

fn latest_ordinal(path: &Path) -> Result<u64> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(rustix::fs::OFlags::NONBLOCK.bits() as i32)
        .open(path)?;
    let meta = file.metadata()?;
    if !meta.is_file() {
        return Err(Error("INVALID_CAPTURE_FILE"));
    }
    let start = meta.len().saturating_sub(8 * 1024 * 1024);
    file.seek(SeekFrom::Start(start))?;
    let mut reader = BufReader::new(file.take(meta.len() - start));
    let mut line = Vec::new();
    let mut latest = None;
    if start != 0 {
        reader.read_until(b'\n', &mut line)?;
    }
    loop {
        line.clear();
        reader.read_until(b'\n', &mut line)?;
        if line.last() != Some(&b'\n') {
            break;
        }
        let value: Value = serde_json::from_slice(&line)?;
        latest = Some(value["ordinal"].as_u64().ok_or(Error("HISTORY_GAP"))?);
    }
    latest.ok_or(Error("HISTORY_GAP"))
}

pub async fn handle(socket: &Path, binary: &Path, state: &Path, payload: &[u8]) -> Result<Value> {
    if payload.len() > 65536 {
        return Err(Error("MESSAGE_TOO_LARGE"));
    }
    let hook: Hook = serde_json::from_slice(payload)?;
    if !ipc::identifier(&hook.session_id)
        || !hook.cwd.is_absolute()
        || !hook.transcript_path.is_absolute()
    {
        return Err(Error("CALLER_AMBIGUOUS"));
    }
    let selected =
        hook.hook_event_name == "PreToolUse" && hook.tool_name.as_deref() == Some("Bash");
    let args = if selected {
        parse_hint(
            hook.tool_input["command"]
                .as_str()
                .ok_or(Error("INVALID_ARGUMENT"))?,
        )?
    } else {
        None
    };
    if !selected
        && !matches!(
            hook.hook_event_name.as_str(),
            "SessionStart" | "Stop" | "PostToolUse" | "PostToolUseFailure"
        )
    {
        return Ok(json!({}));
    }
    if selected && args.is_none() {
        return Ok(json!({}));
    }
    if hook.agent_id.is_some() {
        return Err(Error("ADAPTER_UNSUPPORTED"));
    }
    private_directory(state)?;
    if matches!(
        hook.hook_event_name.as_str(),
        "PostToolUse" | "PostToolUseFailure"
    ) {
        let path = call_path(state, &hook)?;
        if path.try_exists()? {
            let bound = read_call(&path)?;
            if bound.call.session_id != hook.session_id
                || Some(bound.call.turn_id) != hook.turn_id
                || Some(bound.call.tool_call_id) != hook.tool_use_id
            {
                return Err(Error("BINDING_CONFLICT"));
            }
            std::fs::remove_file(path)?;
        }
        return Ok(json!({}));
    }
    let Some(args) = args else {
        let until_ordinal = latest_ordinal(&hook.transcript_path)?;
        let registration = register(socket, binary, &hook).await?;
        send(
            socket,
            NativeEvent::Sync {
                registration,
                until_ordinal,
            },
        )
        .await?;
        return Ok(json!({}));
    };
    let path = call_path(state, &hook)?;
    let call = CodexCall {
        session_id: hook.session_id.clone(),
        agent_id: hook.agent_id.clone(),
        turn_id: hook.turn_id.clone().ok_or(Error("CALLER_AMBIGUOUS"))?,
        tool_call_id: hook.tool_use_id.clone().ok_or(Error("CALLER_AMBIGUOUS"))?,
        cwd: hook.cwd.canonicalize()?,
    };
    if path.try_exists()? {
        let bound = read_call(&path)?;
        if serde_json::to_value(&bound.call)? != serde_json::to_value(&call)?
            || bound.args != args
            || bound.transcript != hook.transcript_path
        {
            return Err(Error("BINDING_CONFLICT"));
        }
    } else if std::fs::read_dir(state)?.take(258).count() >= 258 {
        return Err(Error("RESOURCE_BUSY"));
    }
    let registration = register(socket, binary, &hook).await?;
    let response = send(
        socket,
        NativeEvent::Bind {
            registration,
            call: call.clone(),
        },
    )
    .await?;
    publish_call(
        &path,
        &BoundCall {
            response,
            call,
            args,
            transcript: hook.transcript_path,
        },
    )?;
    let bridge = std::env::current_exe()?;
    let command = format!(
        "{} adapter codex-bound {}",
        quote(bridge.to_str().ok_or(Error("UNSAFE_PATH"))?),
        quote(path.to_str().ok_or(Error("UNSAFE_PATH"))?)
    );
    Ok(
        json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow","updatedInput":{"command":command}}}),
    )
}

pub fn bound_response(path: &Path) -> Result<Value> {
    // Native sandbox commands cannot connect to the daemon; the trusted hook already did.
    Ok(json!({"schema_version":1,"data":read_call(path)?.response}))
}
