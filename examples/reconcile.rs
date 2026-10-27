//! Offline component command for the explicit capture interchange format.
//! The installed product CLI connects to the daemon; this example owns an isolated store.
use akasha_otsuge::capture::{Capture, CaptureBinding, CoverageLevel, SourceIdentity};
use akasha_otsuge::config::{Paths, resolve};
use akasha_otsuge::domain::Scope;
use akasha_otsuge::storage::Storage;
use serde::Deserialize;
use std::io::{Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::PathBuf;
use std::process::ExitCode;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingFile {
    source: SourceIdentity,
    scope: Scope,
    stream: String,
    coverage: CoverageLevel,
}

fn run() -> Result<bool, &'static str> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 3 {
        return Err("INVALID_ARGUMENT");
    }
    let mut bytes = Vec::new();
    // Linux O_NONBLOCK also makes accidental FIFO binding paths fail promptly.
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(0o4000)
        .open(&args[1])
        .map_err(|_| "CAPTURE_BINDING_IO")?;
    if !file.metadata().map_err(|_| "CAPTURE_BINDING_IO")?.is_file() {
        return Err("INVALID_CAPTURE_BINDING");
    }
    file.take(16 * 1024 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| "CAPTURE_BINDING_IO")?;
    if bytes.len() > 16 * 1024 {
        return Err("INVALID_CAPTURE_BINDING");
    }
    let input: BindingFile =
        serde_json::from_slice(&bytes).map_err(|_| "INVALID_CAPTURE_BINDING")?;
    let binding =
        CaptureBinding::from_source(input.source, input.scope, &input.stream, input.coverage)
            .map_err(|e| e.code)?;
    let data_dir = PathBuf::from(&args[0]);
    let config = resolve(
        "",
        None,
        "",
        &Paths {
            socket_path: data_dir.join("unused-component.sock"),
            data_dir,
        },
    )
    .map_err(|e| e.code)?;
    let rt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "RUNTIME_ERROR")?;
    let report = rt.block_on(async {
        let store = Storage::open(&config).await.map_err(|e| e.code)?;
        let result = Capture::new(&store)
            .reconcile_jsonl(&binding, &PathBuf::from(&args[2]), 64, 1)
            .await;
        let close = store.close().await;
        let report = result.map_err(|e| e.code)?;
        close.map_err(|e| e.code)?;
        Ok::<_, &'static str>(report)
    })?;
    serde_json::to_writer(std::io::stdout().lock(), &report).map_err(|_| "IO_ERROR")?;
    std::io::stdout()
        .lock()
        .write_all(b"\n")
        .map_err(|_| "IO_ERROR")?;
    Ok(report.checkpoint.checkpoint.pending.is_none()
        && report.checkpoint.checkpoint.loss.is_empty())
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(3),
        Err(code) => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "{{\"schema_version\":1,\"error\":{{\"code\":\"{code}\"}}}}"
            );
            ExitCode::from(if code == "INVALID_ARGUMENT" { 2 } else { 1 })
        }
    }
}
