use std::io::{self, Write};
use std::path::PathBuf;
use std::process::ExitCode;

use akasha_otsuge::config::{self, Paths};
use akasha_otsuge::ipc::{self, Command, Error, Request, Result};

const HELP: &str = "akasha-otsuge — local, traceable cross-agent memory

Usage: akasha-otsuge [--config FILE] [--profile NAME] [--socket PATH] COMMAND

Commands:
  daemon run                 Run the local daemon in the foreground
  daemon start               Start a detached local daemon
  daemon status              Show daemon and storage status
  daemon stop                Stop intake and drain the daemon
  doctor --json              Inspect the local daemon
  doctor --host codex|hermes|pi [--binary PATH]
                             Inspect the installed native CLI
  adapter install codex --binary PATH --launcher PATH --state-dir PATH
  adapter uninstall codex --launcher PATH
  adapter install hermes --binary PATH --home PATH
  adapter uninstall hermes --home PATH
  adapter install pi --binary PATH --agent-dir PATH
  adapter uninstall pi --agent-dir PATH
  config validate|reload|status
  config show --effective
  capture reconcile|codex BINDING_JSON INPUT_JSONL --idempotency-key ID
  model run PLAN_JSON --idempotency-key ID
  index sync --idempotency-key ID
  index status
  search QUERY --scope JSON_FILE --idempotency-key ID
  show SOURCE_ID --revision COMMIT --range START_BYTE END_BYTE
  job show|cancel ID
  job list [--after KEY]
  memory show ID [--at COMMIT]
  memory history ID [--from COMMIT] [--to COMMIT] [--cursor JSON_FILE]
  state show --scope JSON_FILE --at COMMIT [--cursor JSON_FILE]
  state diff FROM TO --scope JSON_FILE [--cursor JSON_FILE]
  receipt show|replay ID
  access grant SCOPE_JSON
  access revoke

Options:
  -h, --help                 Print help
  -V, --version              Print build version
";
const VERSION: &str = concat!("akasha-otsuge ", env!("CARGO_PKG_VERSION"), "\n");

fn output(bytes: &[u8], stderr: bool) -> Result<()> {
    if stderr {
        io::stderr().lock().write_all(bytes)?;
    } else {
        io::stdout().lock().write_all(bytes)?;
    }
    Ok(())
}
fn paths() -> Result<(PathBuf, Paths)> {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .filter(|p| p.is_absolute())
        .ok_or(Error("INVALID_ARGUMENT"))?;
    let from_env = |key: &str, fallback: PathBuf| {
        std::env::var_os(key)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .unwrap_or(fallback)
    };
    let config =
        from_env("XDG_CONFIG_HOME", home.join(".config")).join("akasha-otsuge/config.toml");
    let data_dir = from_env("XDG_DATA_HOME", home.join(".local/share")).join("akasha-otsuge");
    let socket_path =
        from_env("XDG_RUNTIME_DIR", home.join(".local/run")).join("akasha-otsuge/service.sock");
    Ok((
        config,
        Paths {
            data_dir,
            socket_path,
        },
    ))
}
async fn execute(args: Vec<std::ffi::OsString>) -> Result<u8> {
    let (mut config_path, defaults) = paths()?;
    let mut explicit_config = false;
    let mut socket = None;
    let mut profile = None;
    let mut index = 0;
    while index < args.len() {
        match args[index].to_str() {
            Some("--config") => {
                config_path = args.get(index + 1).ok_or(Error("INVALID_ARGUMENT"))?.into();
                explicit_config = true;
            }
            Some("--socket") => {
                socket = Some(PathBuf::from(
                    args.get(index + 1).ok_or(Error("INVALID_ARGUMENT"))?,
                ));
            }
            Some("--profile") => {
                profile = Some(
                    args.get(index + 1)
                        .and_then(|s| s.to_str())
                        .ok_or(Error("INVALID_ARGUMENT"))?
                        .to_owned(),
                );
            }
            _ => break,
        }
        index += 2;
    }
    let command: Vec<&str> = args[index..]
        .iter()
        .map(|s| s.to_str().ok_or(Error("INVALID_ARGUMENT")))
        .collect::<Result<_>>()?;
    if let ["doctor", "--host", "codex", flags @ ..] = command.as_slice() {
        let flags = options(flags, &["--binary"])?;
        let binary = std::path::Path::new(flags.get("--binary").copied().unwrap_or("codex"));
        let report = akasha_otsuge::adapter::codex::probe(binary).await?;
        let mut bytes = serde_json::to_vec(&serde_json::json!({"schema_version":1,"data":report,
            "coverage":{"version_supported":report.version.as_deref()==Some("0.164.0"),"supported_versions":["0.164.0"],"native_children":false,"model_auth":"not_checked","hook_trust":"not_checked"}}))?;
        bytes.push(b'\n');
        output(&bytes, false)?;
        return Ok(0);
    }
    if let ["doctor", "--host", "hermes", flags @ ..] = command.as_slice() {
        let flags = options(flags, &["--binary"])?;
        let binary = std::path::Path::new(flags.get("--binary").copied().unwrap_or("hermes"));
        let report = akasha_otsuge::adapter::hermes::probe(binary).await?;
        let supported = akasha_otsuge::adapter::hermes::SUPPORTED_VERSION;
        let mut bytes = serde_json::to_vec(&serde_json::json!({"schema_version":1,"data":report,
            "coverage":{"version_supported":akasha_otsuge::adapter::hermes::supported_version(report.version.as_deref()),"supported_versions":[supported],"qualified_builds":[akasha_otsuge::adapter::hermes::QUALIFIED_BUILD],"native_children":false,"model_auth":"not_checked","hook_trust":"not_checked"}}))?;
        bytes.push(b'\n');
        output(&bytes, false)?;
        return Ok(0);
    }
    if let ["doctor", "--host", "pi", flags @ ..] = command.as_slice() {
        let flags = options(flags, &["--binary"])?;
        let binary = std::path::Path::new(flags.get("--binary").copied().unwrap_or("pi"));
        let report = akasha_otsuge::adapter::pi::probe(binary).await?;
        let supported = akasha_otsuge::adapter::pi::SUPPORTED_VERSION;
        let mut bytes = serde_json::to_vec(&serde_json::json!({"schema_version":1,"data":report,
            "coverage":{"version_supported":report.version.as_deref()==Some(supported),"supported_versions":[supported],"native_children":false,"model_auth":"not_checked","hook_trust":"not_checked"}}))?;
        bytes.push(b'\n');
        output(&bytes, false)?;
        return Ok(0);
    }
    if let ["adapter", "codex-bound", path] = command.as_slice() {
        let response =
            akasha_otsuge::adapter::codex::hooks::bound_response(std::path::Path::new(path))?;
        let mut bytes = serde_json::to_vec(&response)?;
        bytes.push(b'\n');
        output(&bytes, false)?;
        return Ok(0);
    }
    if let ["adapter", "pi-context"] = command.as_slice() {
        use std::io::Read;
        let mut bytes = Vec::new();
        io::stdin().take(16385).read_to_end(&mut bytes)?;
        if bytes.len() > 16384 {
            return Err(Error("MESSAGE_TOO_LARGE"));
        }
        let reply =
            akasha_otsuge::adapter::pi::context_bridge::call(serde_json::from_slice(&bytes)?)
                .await?;
        let mut bytes = serde_json::to_vec(&reply)?;
        bytes.push(b'\n');
        output(&bytes, false)?;
        return Ok(if reply.get("error").is_some() { 1 } else { 0 });
    }
    if let [
        "adapter",
        "uninstall",
        host @ ("codex" | "hermes" | "pi"),
        flags @ ..,
    ] = command.as_slice()
    {
        let key = if *host == "pi" {
            "--agent-dir"
        } else if *host == "hermes" {
            "--home"
        } else {
            "--launcher"
        };
        let flags = options(flags, &[key])?;
        let path = std::path::Path::new(flags.get(key).ok_or(Error("INVALID_ARGUMENT"))?);
        let response = if *host == "pi" {
            akasha_otsuge::adapter::pi::installation::uninstall(path)?
        } else if *host == "hermes" {
            akasha_otsuge::adapter::hermes::installation::uninstall(path)?
        } else {
            akasha_otsuge::adapter::codex::installation::uninstall(path)?
        };
        let mut bytes =
            serde_json::to_vec(&serde_json::json!({"schema_version":1,"data":response}))?;
        bytes.push(b'\n');
        output(&bytes, false)?;
        return Ok(0);
    }
    let install_flags = if let [
        "adapter",
        "install",
        host @ ("codex" | "hermes" | "pi"),
        flags @ ..,
    ] = command.as_slice()
    {
        let keys: &[&str] = if *host == "pi" {
            &["--binary", "--agent-dir"]
        } else if *host == "hermes" {
            &["--binary", "--home"]
        } else {
            &["--binary", "--launcher", "--state-dir"]
        };
        Some((*host, options(flags, keys)?))
    } else {
        None
    };
    let hook_flags = if let ["adapter", "codex-hook", flags @ ..] = command.as_slice() {
        Some(options(flags, &["--binary", "--state-dir"])?)
    } else {
        None
    };
    let operation = match command.as_slice() {
        ["adapter", "codex-hook", ..] => None,
        ["adapter", "install", "codex" | "hermes" | "pi", ..] => None,
        ["adapter", host @ ("codex-call" | "hermes-call" | "pi-call")] => {
            use std::io::Read;
            let mut bytes = Vec::new();
            io::stdin().take(16 * 1024 + 1).read_to_end(&mut bytes)?;
            if bytes.len() > 16 * 1024 {
                return Err(Error("MESSAGE_TOO_LARGE"));
            }
            Some(if *host == "pi-call" {
                Command::Pi {
                    event: serde_json::from_slice(&bytes)?,
                }
            } else if *host == "hermes-call" {
                Command::Hermes {
                    event: serde_json::from_slice(&bytes)?,
                }
            } else {
                Command::Codex {
                    event: serde_json::from_slice(&bytes)?,
                }
            })
        }
        ["daemon", "run" | "start"] => None,
        ["daemon", "status"] => Some(Command::Status),
        ["daemon", "stop"] => Some(Command::Stop),
        ["doctor"] | ["doctor", "--json"] => Some(Command::Doctor),
        ["config", "validate"] => Some(Command::ConfigValidate),
        ["config", "reload"] => Some(Command::ConfigReload),
        ["config", "status"] | ["config", "status", "--json"] => Some(Command::ConfigStatus),
        ["config", "show", "--effective"] => Some(Command::ConfigShow),
        [
            "capture",
            format @ ("reconcile" | "codex"),
            binding,
            path,
            "--idempotency-key",
            id,
        ] => {
            let input = config::read_file(std::path::Path::new(binding))?;
            if input.len() > 16 * 1024 {
                return Err(Error("MESSAGE_TOO_LARGE"));
            }
            let mut binding: ipc::CaptureInput = serde_json::from_str(&input)?;
            if *format == "codex" {
                binding.host = Some(config::Host::Codex);
            }
            Some(Command::Reconcile {
                binding,
                path: PathBuf::from(path),
                idempotency_key: (*id).into(),
            })
        }
        ["model", "run", path, "--idempotency-key", id] => {
            let input = config::read_file(std::path::Path::new(path))?;
            if input.len() > akasha_otsuge::provider::MAX_PLAN_BYTES {
                return Err(Error("MESSAGE_TOO_LARGE"));
            }
            Some(Command::ModelRun {
                plan: serde_json::from_str(&input)?,
                idempotency_key: (*id).into(),
            })
        }
        ["show", id, "--revision", revision, "--range", start, end] => {
            Some(Command::OriginalShow {
                range: akasha_otsuge::domain::OriginalRange {
                    event_id: (*id).into(),
                    revision: number(revision)?,
                    start: usize::try_from(number(start)?)
                        .map_err(|_| Error("INVALID_ARGUMENT"))?,
                    end: usize::try_from(number(end)?).map_err(|_| Error("INVALID_ARGUMENT"))?,
                },
            })
        }
        ["index", "status"] => Some(Command::IndexStatus),
        ["index", "sync", "--idempotency-key", id] => Some(Command::RetrievalRun {
            plan: akasha_otsuge::retrieval::Request::Sync,
            idempotency_key: (*id).into(),
        }),
        ["search", query, "--scope", scope, "--idempotency-key", id] => {
            let input = config::read_file(std::path::Path::new(scope))?;
            if input.len() > 4096 {
                return Err(Error("MESSAGE_TOO_LARGE"));
            }
            Some(Command::RetrievalRun {
                plan: akasha_otsuge::retrieval::Request::Search {
                    query: (*query).into(),
                    scope: serde_json::from_str(&input)?,
                },
                idempotency_key: (*id).into(),
            })
        }
        ["job", "show", id] => Some(Command::JobShow { id: (*id).into() }),
        ["job", "cancel", id] => Some(Command::JobCancel { id: (*id).into() }),
        ["job", "list"] => Some(Command::JobList { after: None }),
        ["job", "list", "--after", after] => Some(Command::JobList {
            after: Some((*after).into()),
        }),
        ["memory", "show", id] => Some(Command::MemoryShow {
            id: (*id).into(),
            at: None,
        }),
        ["memory", "show", id, "--at", at] => Some(Command::MemoryShow {
            id: (*id).into(),
            at: Some(number(at)?),
        }),
        ["memory", "history", id, flags @ ..] => {
            let flags = options(flags, &["--from", "--to", "--cursor"])?;
            Some(Command::MemoryHistory {
                id: (*id).into(),
                from: flags
                    .get("--from")
                    .map(|v| number(v))
                    .transpose()?
                    .unwrap_or(1),
                to: flags.get("--to").map(|v| number(v)).transpose()?,
                cursor: flags.get("--cursor").map(|v| json_file(v)).transpose()?,
            })
        }
        ["state", "show", flags @ ..] => {
            let flags = options(flags, &["--scope", "--at", "--cursor"])?;
            Some(Command::StateShow {
                scope: json_file(flags.get("--scope").ok_or(Error("INVALID_ARGUMENT"))?)?,
                at: number(flags.get("--at").ok_or(Error("INVALID_ARGUMENT"))?)?,
                cursor: flags.get("--cursor").map(|v| json_file(v)).transpose()?,
            })
        }
        ["state", "diff", from, to, flags @ ..] => {
            let flags = options(flags, &["--scope", "--cursor"])?;
            Some(Command::StateDiff {
                scope: json_file(flags.get("--scope").ok_or(Error("INVALID_ARGUMENT"))?)?,
                from: number(from)?,
                to: number(to)?,
                cursor: flags.get("--cursor").map(|v| json_file(v)).transpose()?,
            })
        }
        ["receipt", "show", id] => Some(Command::ReceiptShow { id: (*id).into() }),
        ["receipt", "replay", id] => Some(Command::ReceiptReplay { id: (*id).into() }),
        ["access", "revoke"] => Some(Command::AccessSet { scope: None }),
        ["access", "grant", scope] => Some(Command::AccessSet {
            scope: Some(json_file(scope)?),
        }),
        _ => return Err(Error("INVALID_ARGUMENT")),
    };
    let input = if operation.is_some() && socket.is_some() {
        String::new()
    } else if explicit_config || config_path.exists() {
        config::read_file(&config_path)?
    } else {
        String::new()
    };
    let resolved = config::resolve(
        &input,
        if operation.is_some() && socket.is_some() {
            None
        } else {
            profile.as_deref()
        },
        "",
        &defaults,
    )?;
    if let Some((host, flags)) = install_flags {
        let path = |key| {
            flags
                .get(key)
                .map(std::path::Path::new)
                .ok_or(Error("INVALID_ARGUMENT"))
        };
        let socket = socket
            .as_deref()
            .unwrap_or(&resolved.config().paths.socket_path);
        let response = if host == "pi" {
            akasha_otsuge::adapter::pi::installation::install(
                path("--binary")?,
                path("--agent-dir")?,
                socket,
            )
            .await?
        } else if host == "hermes" {
            akasha_otsuge::adapter::hermes::installation::install(
                path("--binary")?,
                path("--home")?,
                socket,
            )
            .await?
        } else {
            akasha_otsuge::adapter::codex::installation::install(
                path("--binary")?,
                path("--launcher")?,
                path("--state-dir")?,
                socket,
            )
            .await?
        };
        let mut bytes =
            serde_json::to_vec(&serde_json::json!({"schema_version":1,"data":response}))?;
        bytes.push(b'\n');
        output(&bytes, false)?;
        return Ok(0);
    }
    if let Some(flags) = hook_flags {
        use std::io::Read;
        let binary = std::path::Path::new(flags.get("--binary").ok_or(Error("INVALID_ARGUMENT"))?);
        let state =
            std::path::Path::new(flags.get("--state-dir").ok_or(Error("INVALID_ARGUMENT"))?);
        let socket = socket.unwrap_or_else(|| resolved.config().paths.socket_path.clone());
        let mut bytes = Vec::new();
        io::stdin().take(65537).read_to_end(&mut bytes)?;
        let response = match akasha_otsuge::adapter::codex::hooks::handle(
            &socket, binary, state, &bytes,
        )
        .await
        {
            Ok(value) => value,
            Err(error)
                if serde_json::from_slice::<serde_json::Value>(&bytes)
                    .is_ok_and(|v| v["hook_event_name"] == "PreToolUse") =>
            {
                serde_json::json!({"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"deny","permissionDecisionReason":error.0}})
            }
            Err(error) => return Err(error),
        };
        let mut output_bytes = serde_json::to_vec(&response)?;
        output_bytes.push(b'\n');
        output(&output_bytes, false)?;
        return Ok(0);
    }
    if let Some(operation) = operation {
        let socket = socket.unwrap_or_else(|| resolved.config().paths.socket_path.clone());
        let now = ipc::now_ms()?;
        let request = Request {
            protocol_version: ipc::VERSION,
            request_id: format!("cli/{}/{now}", std::process::id()),
            deadline_ms: Some(now + ipc::TRANSPORT_TIMEOUT.as_millis() as u64),
            command: operation,
        };
        let replay = matches!(&request.command, Command::ReceiptReplay { .. });
        let response = if replay {
            ipc::replay(&socket, &request, |bytes| {
                output(bytes, false)?;
                io::stdout().lock().flush()?;
                Ok(())
            })
            .await?
        } else {
            ipc::call(&socket, &request).await?
        };
        let code = response.get("error").and_then(|e| e["code"].as_str());
        if replay && code.is_none() {
            return Ok(0);
        }
        let mut bytes = serde_json::to_vec(&response)?;
        bytes.push(b'\n');
        output(&bytes, code.is_some())?;
        Ok(match code {
            None => 0,
            Some("RESOURCE_BUSY") => 4,
            Some(_) => 1,
        })
    } else {
        if socket
            .as_ref()
            .is_some_and(|s| *s != resolved.config().paths.socket_path)
        {
            return Err(Error("CONFIG_INVALID"));
        }
        if command == ["daemon", "start"] {
            let response = akasha_otsuge::daemon::start(&resolved, &args).await?;
            let mut bytes = serde_json::to_vec(&response)?;
            bytes.push(b'\n');
            output(&bytes, false)?;
            return Ok(0);
        }
        akasha_otsuge::daemon::run(resolved, config_path, profile).await?;
        Ok(0)
    }
}
fn options<'a>(
    args: &'a [&str],
    names: &[&str],
) -> Result<std::collections::BTreeMap<&'a str, &'a str>> {
    let mut options = std::collections::BTreeMap::new();
    for pair in args.chunks(2) {
        if pair.len() != 2
            || !names.contains(&pair[0])
            || options.insert(pair[0], pair[1]).is_some()
        {
            return Err(Error("INVALID_ARGUMENT"));
        }
    }
    Ok(options)
}
fn number(value: &str) -> Result<u64> {
    value.parse().map_err(|_| Error("INVALID_ARGUMENT"))
}
fn json_file<T: serde::de::DeserializeOwned>(path: &str) -> Result<T> {
    let input = config::read_file(std::path::Path::new(path))?;
    if input.len() > 16384 {
        return Err(Error("MESSAGE_TOO_LARGE"));
    }
    Ok(serde_json::from_str(&input)?)
}
fn run() -> Result<u8> {
    let args: Vec<_> = std::env::args_os().skip(1).collect();
    if args.is_empty()
        || args.len() == 1 && matches!(args[0].to_str(), Some("-h" | "--help" | "help"))
    {
        output(HELP.as_bytes(), false)?;
        return Ok(0);
    }
    if args.len() == 1 && matches!(args[0].to_str(), Some("-V" | "--version" | "version")) {
        output(VERSION.as_bytes(), false)?;
        return Ok(0);
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?
        .block_on(execute(args))
}
fn main() -> ExitCode {
    match run() {
        Ok(code) => ExitCode::from(code),
        Err(error) => {
            let value = error.value();
            let _ = writeln!(
                io::stderr().lock(),
                "{{\"schema_version\":1,\"error\":{value}}}"
            );
            ExitCode::from(error.exit_code())
        }
    }
}
