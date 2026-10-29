use crate::{
    adapter::*,
    capture::*,
    config::Host,
    domain::{Domain, ReadContext, Scope},
    storage::{Storage, hash},
};
use std::{
    fs::OpenOptions,
    io::Read,
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HermesRegistration {
    pub capability: CapabilityReport,
    pub source: SourceIdentity,
    pub scope: Scope,
    pub principal: String,
    pub native_profile: String,
    pub profile_home: PathBuf,
    pub snapshot: PathBuf,
    pub workspace: PathBuf,
    pub task_id: String,
    pub parent_session: Option<String>,
}

/// Hook context transported privately; none of these fields are model tool arguments.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct HermesCall {
    pub session_id: String,
    pub task_id: String,
    pub turn_id: String,
    pub tool_call_id: String,
    pub profile_home: PathBuf,
    pub cwd: PathBuf,
}

pub struct HermesCaller {
    store: Arc<Storage>,
    registration: HermesRegistration,
    call: HermesCall,
    generation: u64,
}
impl HermesCaller {
    pub fn new(
        store: Arc<Storage>,
        registration: HermesRegistration,
        call: HermesCall,
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
impl CallerAdapter for HermesCaller {
    fn probe(&self) -> Result<CapabilityReport> {
        let mut report = self.registration.capability.clone();
        if report.host != Host::Hermes
            || report.version.as_deref().and_then(|v| v.split('+').next())
                != Some(super::SUPPORTED_VERSION)
        {
            return Err(ErrorCode::AdapterUnsupported.into());
        }
        report.binding = BindingLevel::ExactTurn;
        report.history = CoverageLevel::VisibleText;
        report.native_children = true;
        report.capabilities.extend(
            [
                "binding.session",
                "binding.turn",
                "binding.children",
                "history.raw",
            ]
            .map(str::to_owned),
        );
        Ok(report)
    }
    fn bind<'a>(
        &'a self,
        runtime_call: &'a str,
    ) -> futures::future::BoxFuture<'a, Result<Invocation>> {
        Box::pin(async move {
            let r = &self.registration;
            let c = &self.call;
            let capability = self.probe()?;
            if runtime_call != c.tool_call_id
                || c.session_id != r.source.session
                || c.task_id != r.task_id
                || c.profile_home != r.profile_home
                || r.source.adapter != "hermes"
                || r.source.thread != r.source.session
                || r.source.profile != profile_key(&r.profile_home)
                || ![&c.session_id, &c.task_id, &c.turn_id, &c.tool_call_id]
                    .into_iter()
                    .all(|s| super::super::valid_id(s))
            {
                return Err(ErrorCode::CallerAmbiguous.into());
            }
            if c.cwd != r.workspace
                || !r.workspace.is_absolute()
                || !r.profile_home.is_absolute()
                || r.profile_home.canonicalize().ok().as_ref() != Some(&r.profile_home)
            {
                return Err(ErrorCode::BindingUnavailable.into());
            }
            let proof = match &r.parent_session {
                Some(parent)
                    if super::super::valid_id(parent)
                        && parent != &r.source.session
                        && r.source.agent == r.source.session =>
                {
                    CallerProof::RegisteredChild {
                        thread: r.source.thread.clone(),
                        parent: parent.clone(),
                    }
                }
                None if r.source.agent == "main" => CallerProof::RegisteredRoot {
                    thread: r.source.thread.clone(),
                },
                _ => return Err(ErrorCode::CallerAmbiguous.into()),
            };
            let id = format!(
                "hermes/{}",
                hash(
                    &serde_json::to_vec(&(&r.source, &c.task_id, &c.turn_id, runtime_call))
                        .map_err(|_| ErrorCode::ProtocolInvalid)?
                )
            );
            let read = ReadContext::from_verified_binding(&r.principal, &id, "hermes")?;
            Domain::new(&self.store).permission(&read, Some(&r.scope))?;
            match Invocation::load(&self.store, &id, &r.principal).await {
                Ok(old) => {
                    let b = old.diagnostic();
                    if b.caller != r.source
                        || b.scope != r.scope
                        || b.workspace != r.workspace
                        || b.proof != proof
                        || b.turn_id.as_deref() != Some(&c.turn_id)
                        || b.tool_call_id.as_deref() != Some(runtime_call)
                    {
                        return Err(ErrorCode::BindingConflict.into());
                    }
                    return Ok(old);
                }
                Err(e) if e.code == ErrorCode::BindingUnavailable => {}
                Err(e) => return Err(e),
            }
            let store = self.store.clone();
            let registration = r.clone();
            let call = c.clone();
            let generation = self.generation;
            let runtime = tokio::runtime::Handle::current();
            let (cursor, tip, capture_seq) = tokio::task::spawn_blocking(move || {
                runtime.block_on(capture_fence(&store, &registration, &call, generation))
            })
            .await
            .map_err(|_| ErrorCode::SourceUnavailable)??;
            Invocation::bind(
                &self.store,
                RuntimeBinding {
                    invocation_id: id,
                    runtime_instance: r.source.session.clone(),
                    capability,
                    caller: r.source.clone(),
                    principal: r.principal.clone(),
                    scope: r.scope.clone(),
                    proof,
                    branch: r.source.session.clone(),
                    turn_id: Some(c.turn_id.clone()),
                    tool_call_id: Some(runtime_call.into()),
                    requested_cursor: cursor,
                    available_cursor: cursor,
                    tip: Some(tip),
                    capture_seq,
                    workspace: r.workspace.clone(),
                    config_generation: self.generation,
                },
            )
            .await
        })
    }
}

/// Canonical home is part of the source identity even when native labels both say `custom`.
pub fn profile_key(home: &Path) -> String {
    format!("hermes/{}", hash(home.as_os_str().as_encoded_bytes()))
}

pub(super) fn read_snapshot(path: &Path) -> Result<Vec<u8>> {
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(path)
        .map_err(|_| ErrorCode::SourceUnavailable)?;
    let before = file.metadata().map_err(|_| ErrorCode::SourceUnavailable)?;
    if !before.is_file()
        || before.uid() != rustix::process::geteuid().as_raw()
        || before.mode() & 0o777 != 0o600
        || before.nlink() != 1
    {
        return Err(ErrorCode::ReadDenied.into());
    }
    const LIMIT: u64 = 8 * 1024 * 1024;
    if before.len() > LIMIT {
        return Err(ErrorCode::OutputLimit.into());
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(LIMIT + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| ErrorCode::SourceUnavailable)?;
    if bytes.len() as u64 > LIMIT {
        return Err(ErrorCode::OutputLimit.into());
    }
    let after = file.metadata().map_err(|_| ErrorCode::SourceUnavailable)?;
    if before.len() != bytes.len() as u64
        || before.len() != after.len()
        || before.mtime() != after.mtime()
        || before.mtime_nsec() != after.mtime_nsec()
        || before.ctime() != after.ctime()
        || before.ctime_nsec() != after.ctime_nsec()
    {
        return Err(ErrorCode::SourceChanged.into());
    }
    Ok(bytes)
}

async fn capture_fence(
    store: &Storage,
    r: &HermesRegistration,
    c: &HermesCall,
    generation: u64,
) -> Result<(u64, EventRef, u64)> {
    let started = Instant::now();
    let bytes = read_snapshot(&r.snapshot)?;
    let frames = super::export::decode_export(&r.source, &r.native_profile, &bytes)?;
    let native: serde_json::Value =
        serde_json::from_slice(&bytes).map_err(|_| ErrorCode::ProtocolInvalid)?;
    if native["cwd"].as_str() != r.workspace.to_str()
        || native["parent_session_id"].as_str() != r.parent_session.as_deref()
        || (r.parent_session.is_some() && native["source"] != "delegate_task")
    {
        return Err(ErrorCode::CallerAmbiguous.into());
    }
    let mut position = None;
    for (i, frame) in frames.iter().enumerate() {
        let raw = crate::capture::decode_hex(&frame.raw_hex, 4 * 1024 * 1024)?;
        let value: serde_json::Value =
            serde_json::from_slice(&raw).map_err(|_| ErrorCode::ProtocolInvalid)?;
        for call in value["tool_calls"].as_array().into_iter().flatten() {
            if call["id"] == c.tool_call_id {
                if position.is_some() || call["function"]["name"] != "akasha_consult" {
                    return Err(ErrorCode::CallerAmbiguous.into());
                }
                serde_json::from_str::<ConsultArgs>(
                    call["function"]["arguments"]
                        .as_str()
                        .ok_or(ErrorCode::ProtocolInvalid)?,
                )
                .map_err(|_| ErrorCode::InvalidArgument)?;
                position = Some(i);
            }
        }
    }
    let count = position.ok_or(ErrorCode::CallerAmbiguous)? + 1;
    loop {
        if started.elapsed() >= Duration::from_secs(5) {
            return Err(ErrorCode::HistoryGap.into());
        }
        let report = super::capture::reconcile_frames(
            store,
            &r.source,
            r.scope.clone(),
            &frames[..count],
            64,
            generation,
        )
        .await?;
        if report.capture.checkpoint.checkpoint.pending.is_none() {
            return Ok((
                count as u64,
                report.tip.ok_or(ErrorCode::HistoryGap)?,
                store.sequence()?,
            ));
        }
    }
}
