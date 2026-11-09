mod workspace;

use akasha_otsuge::{
    capture::EventRef,
    ipc::{self, Command, Error, Request},
    outcome::{
        self, Artifact, ArtifactGap, ArtifactGapReason, Event, Execution, LogChunk, LogRef,
        RunContext, RunPlan, Severity, Stream,
    },
};
use serde_json::{Value, json};
use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Write},
    os::unix::{
        fs::{DirBuilderExt, MetadataExt, OpenOptionsExt},
        process::ExitStatusExt,
    },
    path::Path,
    process::{Command as Process, Stdio},
};

pub(super) enum Failure {
    Local(Error),
    Remote(Value),
}
impl From<Error> for Failure {
    fn from(value: Error) -> Self {
        Self::Local(value)
    }
}
impl From<std::io::Error> for Failure {
    fn from(value: std::io::Error) -> Self {
        Self::Local(value.into())
    }
}
impl From<serde_json::Error> for Failure {
    fn from(value: serde_json::Error) -> Self {
        Self::Local(value.into())
    }
}
type Result<T> = std::result::Result<T, Failure>;

async fn call(socket: &Path, command: Command) -> Result<Value> {
    let now = ipc::now_ms()?;
    let result = ipc::call(
        socket,
        &Request {
            protocol_version: ipc::VERSION,
            request_id: format!("outcome/{}/{now}", std::process::id()),
            deadline_ms: Some(now + ipc::TRANSPORT_TIMEOUT.as_millis() as u64),
            command,
        },
    )
    .await?;
    if result.get("error").is_some() {
        return Err(Failure::Remote(result));
    }
    Ok(result["data"].clone())
}

fn private_root(path: &Path) -> Result<()> {
    fs::DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(path)?;
    let meta = fs::symlink_metadata(path)?;
    if !meta.is_dir()
        || meta.file_type().is_symlink()
        || meta.uid() != rustix::process::geteuid().as_raw()
        || meta.mode() & 0o777 != 0o700
    {
        return Err(Error("UNSAFE_PATH").into());
    }
    Ok(())
}

fn new_file(path: &Path) -> std::io::Result<File> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
}

fn save(path: &Path, value: &impl serde::Serialize) -> Result<()> {
    let mut file = new_file(path)?;
    serde_json::to_writer(&mut file, value)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}

pub(super) async fn run(socket: &Path, plan: RunPlan, argv: Vec<String>) -> Result<(u8, Value)> {
    plan.validate()?;
    outcome::validate_argv(&argv)?;
    let context: RunContext = serde_json::from_value(
        call(
            socket,
            Command::OutcomeContext {
                invocation_id: plan.invocation_id.clone(),
            },
        )
        .await?,
    )?;
    match call(
        socket,
        Command::OutcomeShow {
            id: plan.event_id.clone(),
        },
    )
    .await
    {
        Ok(_) => return Err(Error("OUTCOME_ALREADY_STARTED").into()),
        Err(Failure::Remote(v)) if v["error"]["code"] == "OUTCOME_NOT_FOUND" => {}
        Err(error) => return Err(error),
    }
    let parent = context.data_dir.join("outcome-runs");
    private_root(&parent)?;
    let stage = parent.join(workspace::hash(plan.event_id.as_bytes()));
    fs::DirBuilder::new()
        .mode(0o700)
        .create(&stage)
        .map_err(|e| {
            if e.kind() == std::io::ErrorKind::AlreadyExists {
                Error("OUTCOME_ALREADY_STARTED")
            } else {
                e.into()
            }
        })?;
    save(
        &stage.join("admission.json"),
        &json!({"plan":plan,"argv":argv,"context":context}),
    )?;
    File::open(&stage)?.sync_all()?;
    File::open(&parent)?.sync_all()?;
    let root = File::from(
        rustix::fs::open(
            &context.workspace,
            rustix::fs::OFlags::RDONLY
                | rustix::fs::OFlags::DIRECTORY
                | rustix::fs::OFlags::NOFOLLOW
                | rustix::fs::OFlags::CLOEXEC,
            rustix::fs::Mode::empty(),
        )
        .map_err(std::io::Error::from)?,
    );
    let before = workspace::observe(&context.workspace, &root);
    let stdout = new_file(&stage.join("stdout"))?;
    let stderr = new_file(&stage.join("stderr"))?;
    let started_at_ms = ipc::now_ms()?;
    // Only this explicit invocation executes the child; an existing marker always refuses replay.
    let status = Process::new(&argv[0])
        .args(&argv[1..])
        .current_dir(&context.workspace)
        .stdin(Stdio::inherit())
        .stdout(stdout.try_clone()?)
        .stderr(stderr.try_clone()?)
        .status();
    let finished_at_ms = ipc::now_ms()?;
    stdout.sync_all()?;
    stderr.sync_all()?;
    let (exit_code, signal, spawn_error, code) = match status {
        Ok(status) => (
            status.code(),
            status.signal(),
            None,
            status
                .code()
                .unwrap_or_else(|| 128 + status.signal().unwrap_or(0)) as u8,
        ),
        Err(error) => (None, None, Some(error.to_string()), 127),
    };
    let mut execution = Execution {
        argv,
        started_at_ms,
        finished_at_ms,
        exit_code,
        signal,
        spawn_error,
        workspace: context.workspace.clone(),
        before,
        after: workspace::observe(&context.workspace, &root),
        artifact_gaps: Vec::new(),
    };
    let mut artifacts = Vec::new();
    for path in &plan.artifacts {
        match workspace::file(&root, path.as_ref()) {
            Ok((bytes, sha256, _)) => artifacts.push(Artifact {
                path: path.clone(),
                bytes,
                sha256,
            }),
            Err(error) => execution.artifact_gaps.push(ArtifactGap {
                path: path.clone(),
                reason: if error.kind() == std::io::ErrorKind::NotFound {
                    ArtifactGapReason::Missing
                } else {
                    ArtifactGapReason::Unavailable
                },
            }),
        }
    }
    save(&stage.join("execution.json"), &execution)?;
    let mut event = Event {
        event_id: plan.event_id.clone(),
        invocation_id: plan.invocation_id.clone(),
        source: "command-wrapper".into(),
        severity: Severity::Info,
        incident: plan.incident,
        occurred_at_ms: finished_at_ms,
        execution: Some(execution),
        artifacts,
        logs: Vec::new(),
        receipt_jobs: context.receipt_jobs,
        claims: Vec::new(),
    };
    for (name, stream) in [("stdout", Stream::Stdout), ("stderr", Stream::Stderr)] {
        let mut input = File::open(stage.join(name))?;
        // Bound to the bytes observed when the direct child exited, not later descendants.
        let observed = input.metadata()?.len();
        let mut remaining = observed;
        let mut part = 0;
        loop {
            if event.logs.len() == 64 {
                return Err(Error("OUTCOME_LOG_LIMIT").into());
            }
            let mut bytes = vec![0; remaining.min(outcome::LOG_CHUNK_BYTES as u64) as usize];
            input.read_exact(&mut bytes)?;
            let reference: EventRef = serde_json::from_value(
                call(
                    socket,
                    Command::OutcomeLogWrite {
                        chunk: LogChunk {
                            invocation_id: plan.invocation_id.clone(),
                            event_id: plan.event_id.clone(),
                            stream,
                            part,
                            observed_at_ms: finished_at_ms,
                            raw_hex: workspace::hex(&bytes),
                        },
                    },
                )
                .await?,
            )?;
            event.logs.push(LogRef {
                stream,
                reference,
                completeness: outcome::LogCompleteness::Complete,
            });
            remaining -= bytes.len() as u64;
            part += 1;
            if remaining == 0 {
                break;
            }
        }
        if input.metadata()?.len() != observed {
            return Err(Error("OUTCOME_LOG_CHANGED").into());
        }
    }
    event.validate()?;
    save(&stage.join("event.json"), &event)?;
    let receipt = call(socket, Command::OutcomeSubmit { event }).await?;
    save(&stage.join("receipt.json"), &receipt)?;
    File::open(&stage)?.sync_all()?;
    let record = call(socket, Command::OutcomeShow { id: plan.event_id }).await?;
    for name in [
        "stdout",
        "stderr",
        "admission.json",
        "execution.json",
        "event.json",
    ] {
        fs::remove_file(stage.join(name))?;
    }
    File::open(&stage)?.sync_all()?;
    Ok((code, record))
}
