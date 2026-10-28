use crate::{
    adapter::*,
    capture::*,
    config::Host,
    domain::{Domain, ReadContext, Scope},
    storage::{Storage, hash},
};
use serde_json::Value;
use std::{
    fs::OpenOptions,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    os::unix::fs::OpenOptionsExt,
    path::PathBuf,
    sync::Arc,
    time::{Duration, Instant},
};

/// Server-owned registration, not a model tool argument.
#[derive(Clone)]
pub struct CodexRegistration {
    pub capability: CapabilityReport,
    pub source: SourceIdentity,
    pub scope: Scope,
    pub principal: String,
    pub transcript: PathBuf,
    pub workspace: PathBuf,
}

/// Actual native hook fields transported outside the model's short hint.
#[derive(Clone)]
pub struct CodexCall {
    pub session_id: String,
    pub agent_id: Option<String>,
    pub turn_id: String,
    pub tool_call_id: String,
    pub cwd: PathBuf,
}

pub struct CodexCaller {
    store: Arc<Storage>,
    registration: CodexRegistration,
    call: CodexCall,
    generation: u64,
}
impl CodexCaller {
    pub fn new(
        store: Arc<Storage>,
        registration: CodexRegistration,
        call: CodexCall,
        generation: u64,
    ) -> Self {
        Self {
            store,
            registration,
            call,
            generation,
        }
    }
}
impl CallerAdapter for CodexCaller {
    fn probe(&self) -> Result<CapabilityReport> {
        let mut report = self.registration.capability.clone();
        if report.host != Host::Codex || report.version.as_deref() != Some("0.164.0") {
            return Err(ErrorCode::AdapterUnsupported.into());
        }
        report.binding = BindingLevel::ExactTurn;
        report.history = CoverageLevel::VisibleText;
        report.native_children = false;
        report
            .capabilities
            .extend(["binding.session", "binding.turn", "history.raw"].map(str::to_owned));
        Ok(report)
    }
    fn bind<'a>(
        &'a self,
        runtime_call: &'a str,
    ) -> futures::future::BoxFuture<'a, Result<Invocation>> {
        Box::pin(async move {
            let reg = &self.registration;
            let call = &self.call;
            let capability = self.probe()?;
            if call.agent_id.is_some() {
                return Err(ErrorCode::AdapterUnsupported.into());
            }
            if runtime_call != call.tool_call_id
                || call.session_id != reg.source.session
                || reg.source.adapter != "codex"
                || reg.source.thread != reg.source.session
                || reg.source.agent != "main"
                || ![&call.session_id, &call.turn_id, &call.tool_call_id]
                    .into_iter()
                    .all(|s| super::super::valid_id(s))
            {
                return Err(ErrorCode::CallerAmbiguous.into());
            }
            if call.cwd != reg.workspace || !reg.workspace.is_absolute() {
                return Err(ErrorCode::BindingUnavailable.into());
            }
            let id = format!(
                "codex/{}",
                hash(
                    &serde_json::to_vec(&(&reg.source, &call.turn_id, runtime_call))
                        .map_err(|_| ErrorCode::ProtocolInvalid)?
                )
            );
            let read = ReadContext::from_verified_binding(&reg.principal, &id, "codex")?;
            Domain::new(&self.store).permission(&read, Some(&reg.scope))?;
            let proof = CallerProof::RegisteredRoot {
                thread: reg.source.thread.clone(),
            };
            match Invocation::load(&self.store, &id, &reg.principal).await {
                Ok(old) => {
                    let bound = old.diagnostic();
                    if bound.caller != reg.source
                        || bound.scope != reg.scope
                        || bound.workspace != reg.workspace
                        || bound.proof != proof
                        || bound.turn_id.as_deref() != Some(&call.turn_id)
                        || bound.tool_call_id.as_deref() != Some(runtime_call)
                    {
                        return Err(ErrorCode::BindingConflict.into());
                    }
                    return Ok(old);
                }
                Err(error) if error.code == ErrorCode::BindingUnavailable => {}
                Err(error) => return Err(error),
            }
            let store = self.store.clone();
            let registration = reg.clone();
            let native_call = call.clone();
            let generation = self.generation;
            let runtime = tokio::runtime::Handle::current();
            let (ordinal, capture_seq) = tokio::task::spawn_blocking(move || {
                runtime.block_on(capture_fence(
                    &store,
                    &registration,
                    &native_call,
                    generation,
                ))
            })
            .await
            .map_err(|_| ErrorCode::SourceUnavailable)??;
            let cursor = ordinal.checked_add(1).ok_or(ErrorCode::ProtocolInvalid)?;
            Invocation::bind(
                &self.store,
                RuntimeBinding {
                    invocation_id: id,
                    runtime_instance: reg.source.session.clone(),
                    capability,
                    caller: reg.source.clone(),
                    principal: reg.principal.clone(),
                    scope: reg.scope.clone(),
                    proof,
                    branch: reg.source.thread.clone(),
                    turn_id: Some(call.turn_id.clone()),
                    tool_call_id: Some(runtime_call.into()),
                    requested_cursor: cursor,
                    available_cursor: cursor,
                    tip: Some(EventRef {
                        source: reg.source.clone(),
                        event_id: format!("row:{ordinal}"),
                    }),
                    capture_seq,
                    workspace: reg.workspace.clone(),
                    config_generation: self.generation,
                },
            )
            .await
        })
    }
}

fn matches_call(row: &Value, call: &CodexCall) -> bool {
    row["type"] == "response_item"
        && row["payload"]["type"] == "function_call"
        && matches!(
            row["payload"]["name"].as_str(),
            Some("exec_command" | "shell_command")
        )
        && row["payload"]["call_id"].as_str() == Some(&call.tool_call_id)
        && row["payload"]["internal_chat_message_metadata_passthrough"]["turn_id"].as_str()
            == Some(&call.turn_id)
}

fn native_fence(reg: &CodexRegistration, call: &CodexCall) -> Result<u64> {
    let header = super::transcript::header(&reg.transcript, &reg.source)
        .map_err(|error| {
            if error.code == "ADAPTER_UNSUPPORTED" {
                AdapterError::from(ErrorCode::AdapterUnsupported)
            } else {
                error.into()
            }
        })?
        .ok_or(ErrorCode::HistoryGap)?;
    // Missing agent_id alone never proves a root: require the registered native source.
    if !matches!(header["payload"]["source"].as_str(), Some("cli" | "exec"))
        || header["payload"]["thread_source"] != "user"
    {
        return Err(ErrorCode::AdapterUnsupported.into());
    }
    let cwd = header["payload"]["cwd"]
        .as_str()
        .ok_or(ErrorCode::BindingUnavailable)?;
    if PathBuf::from(cwd)
        .canonicalize()
        .map_err(|_| ErrorCode::BindingUnavailable)?
        != reg.workspace
    {
        return Err(ErrorCode::BindingUnavailable.into());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags(0o4000)
        .open(&reg.transcript)
        .map_err(|_| ErrorCode::SourceUnavailable)?;
    let meta = file.metadata().map_err(|_| ErrorCode::SourceUnavailable)?;
    if !meta.is_file() {
        return Err(ErrorCode::SourceUnavailable.into());
    }
    // Only the recent bounded tail can attest a new call; older retries use their stored fence.
    let start = meta.len().saturating_sub(8 * 1024 * 1024);
    file.seek(SeekFrom::Start(start))
        .map_err(|_| ErrorCode::SourceUnavailable)?;
    let mut reader = BufReader::new(file.take(meta.len() - start));
    let mut line = Vec::new();
    if start != 0 {
        reader
            .read_until(b'\n', &mut line)
            .map_err(|_| ErrorCode::SourceUnavailable)?;
    }
    let mut found = None;
    loop {
        line.clear();
        reader
            .read_until(b'\n', &mut line)
            .map_err(|_| ErrorCode::SourceUnavailable)?;
        if line.last() != Some(&b'\n') {
            break;
        }
        let row: Value = serde_json::from_slice(&line).map_err(|_| ErrorCode::HistoryGap)?;
        if row["type"] == "response_item"
            && row["payload"]["call_id"].as_str() == Some(&call.tool_call_id)
        {
            // A tool result repeats call_id, but it does not establish an invocation boundary.
            if row["payload"]["type"] == "function_call_output" {
                continue;
            }
            if !matches_call(&row, call) || found.is_some() {
                return Err(ErrorCode::CallerAmbiguous.into());
            }
            found = Some(row["ordinal"].as_u64().ok_or(ErrorCode::HistoryGap)?);
        }
    }
    found.ok_or_else(|| ErrorCode::CallerAmbiguous.into())
}

async fn capture_fence(
    store: &Storage,
    reg: &CodexRegistration,
    call: &CodexCall,
    generation: u64,
) -> Result<(u64, u64)> {
    let end = Instant::now() + Duration::from_secs(2);
    let ordinal = native_fence(reg, call)?;
    let binding = CaptureBinding::from_source(
        reg.source.clone(),
        reg.scope.clone(),
        super::TRANSCRIPT_STREAM,
        CoverageLevel::VisibleText,
    )?;
    let tip = EventRef {
        source: reg.source.clone(),
        event_id: format!("row:{ordinal}"),
    };
    let capture = Capture::new(store);
    for processed in 0..=256 {
        let at = store.sequence()?;
        if let Some(event) = capture.event(&binding, &tip, at)? {
            let row: Value =
                serde_json::from_slice(&capture.raw(&event)?).map_err(|_| ErrorCode::HistoryGap)?;
            if !matches_call(&row, call) || row["ordinal"].as_u64() != Some(ordinal) {
                return Err(ErrorCode::BindingConflict.into());
            }
            return Ok((ordinal, at));
        }
        if processed == 256 || Instant::now() >= end {
            break;
        }
        let report = super::reconcile_transcript(
            store,
            &reg.source,
            reg.scope.clone(),
            &reg.transcript,
            1,
            generation,
        )
        .await?;
        if report.processed == 0 && report.deduplicated == 0 {
            break;
        }
        if report
            .checkpoint
            .checkpoint
            .pending
            .as_ref()
            .is_some_and(|p| *p != PendingReason::Backlog)
        {
            break;
        }
    }
    Err(ErrorCode::HistoryGap.into())
}
