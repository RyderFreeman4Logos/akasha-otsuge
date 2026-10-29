use crate::{
    adapter::*,
    capture::*,
    config::Host,
    domain::{Domain, ReadContext, Scope},
    storage::{Storage, hash},
};
use serde_json::Value;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs::OpenOptions,
    io::{BufRead, BufReader, Read, Seek, SeekFrom},
    os::unix::fs::{MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    sync::Arc,
    time::{Duration, Instant},
};

#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PiRegistration {
    pub capability: CapabilityReport,
    pub source: SourceIdentity,
    pub scope: Scope,
    pub principal: String,
    pub agent_dir: PathBuf,
    pub transcript: PathBuf,
    pub workspace: PathBuf,
}

/// Private extension context, separate from the model's short-hint schema.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PiCall {
    pub session_id: String,
    pub leaf_id: String,
    pub turn_id: String,
    pub tool_call_id: String,
    pub agent_dir: PathBuf,
    pub transcript: PathBuf,
    pub cwd: PathBuf,
}

pub fn profile_key(home: &Path) -> String {
    format!("pi/{}", hash(home.as_os_str().as_encoded_bytes()))
}

pub struct PiCaller {
    store: Arc<Storage>,
    registration: PiRegistration,
    call: PiCall,
    generation: u64,
}
impl PiCaller {
    pub fn new(
        store: Arc<Storage>,
        registration: PiRegistration,
        call: PiCall,
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
impl CallerAdapter for PiCaller {
    fn probe(&self) -> Result<CapabilityReport> {
        let mut report = self.registration.capability.clone();
        if report.host != Host::Pi || report.version.as_deref() != Some(super::SUPPORTED_VERSION) {
            return Err(ErrorCode::AdapterUnsupported.into());
        }
        report.binding = BindingLevel::ExactTurn;
        report.history = CoverageLevel::VisibleText;
        report.native_children = false;
        report.capabilities.extend(
            [
                "binding.session",
                "binding.turn",
                "history.raw",
                "history.tree",
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
                || c.agent_dir != r.agent_dir
                || c.transcript != r.transcript
                || r.source.adapter != "pi"
                || r.source.thread != r.source.session
                || r.source.agent != "main"
                || r.source.profile != profile_key(&r.agent_dir)
                || ![&c.session_id, &c.leaf_id, &c.turn_id, &c.tool_call_id]
                    .into_iter()
                    .all(|v| super::super::valid_id(v))
            {
                return Err(ErrorCode::CallerAmbiguous.into());
            }
            if c.cwd != r.workspace
                || !r.workspace.is_absolute()
                || !r.transcript.is_absolute()
                || r.agent_dir.canonicalize().ok().as_ref() != Some(&r.agent_dir)
            {
                return Err(ErrorCode::BindingUnavailable.into());
            }
            let id = format!(
                "pi/{}",
                hash(
                    &serde_json::to_vec(&(&r.source, &c.turn_id, runtime_call))
                        .map_err(|_| ErrorCode::ProtocolInvalid)?
                )
            );
            let read = ReadContext::from_verified_binding(&r.principal, &id, "pi")?;
            Domain::new(&self.store).permission(&read, Some(&r.scope))?;
            let proof = CallerProof::RegisteredRoot {
                thread: r.source.thread.clone(),
            };
            let tip = EventRef {
                source: r.source.clone(),
                event_id: format!("entry:{}", c.turn_id),
            };
            match Invocation::load(&self.store, &id, &r.principal).await {
                Ok(old) => {
                    let b = old.diagnostic();
                    if b.caller != r.source
                        || b.scope != r.scope
                        || b.workspace != r.workspace
                        || b.proof != proof
                        || b.turn_id.as_deref() != Some(&c.turn_id)
                        || b.tool_call_id.as_deref() != Some(runtime_call)
                        || b.tip.as_ref() != Some(&tip)
                    {
                        return Err(ErrorCode::BindingConflict.into());
                    }
                    return Ok(old);
                }
                Err(e) if e.code == ErrorCode::BindingUnavailable => {}
                Err(e) => return Err(e),
            }
            let store = self.store.clone();
            let r_copy = r.clone();
            let c_copy = c.clone();
            let generation = self.generation;
            let runtime = tokio::runtime::Handle::current();
            let (count, at) = tokio::task::spawn_blocking(move || {
                runtime.block_on(capture_fence(&store, &r_copy, &c_copy, generation))
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
                    branch: r.source.thread.clone(),
                    turn_id: Some(c.turn_id.clone()),
                    tool_call_id: Some(runtime_call.into()),
                    requested_cursor: count,
                    available_cursor: count,
                    tip: Some(tip),
                    capture_seq: at,
                    workspace: r.workspace.clone(),
                    config_generation: self.generation,
                },
            )
            .await
        })
    }
}

/// The actual native branch must contain the persisted top-level model call. Nested execution
/// IDs are not transcript entries and cannot borrow their parent's invocation.
fn native_fence(r: &PiRegistration, c: &PiCall) -> Result<Value> {
    let header =
        super::transcript::header(&r.transcript, &r.source)?.ok_or(ErrorCode::HistoryGap)?;
    if header["cwd"].as_str() != r.workspace.to_str() {
        return Err(ErrorCode::BindingUnavailable.into());
    }
    let mut file = OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(&r.transcript)
        .map_err(|_| ErrorCode::SourceUnavailable)?;
    let meta = file.metadata().map_err(|_| ErrorCode::SourceUnavailable)?;
    if !meta.is_file() || meta.uid() != rustix::process::geteuid().as_raw() {
        return Err(ErrorCode::ReadDenied.into());
    }
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
    let mut entries = BTreeMap::new();
    loop {
        line.clear();
        reader
            .read_until(b'\n', &mut line)
            .map_err(|_| ErrorCode::SourceUnavailable)?;
        if line.last() != Some(&b'\n') {
            break;
        }
        let value: Value = serde_json::from_slice(&line).map_err(|_| ErrorCode::HistoryGap)?;
        if value["type"] == "session" {
            continue;
        }
        let id = value["id"]
            .as_str()
            .filter(|id| super::super::valid_id(id))
            .ok_or(ErrorCode::HistoryGap)?
            .to_owned();
        if entries.len() >= 16_384 || entries.insert(id, value).is_some() {
            return Err(ErrorCode::CallerAmbiguous.into());
        }
    }
    let mut next = Some(c.leaf_id.as_str());
    let mut seen = BTreeSet::new();
    let mut ancestor = false;
    while let Some(id) = next {
        if !seen.insert(id) {
            return Err(ErrorCode::CallerAmbiguous.into());
        }
        let entry = entries.get(id).ok_or(ErrorCode::HistoryGap)?;
        if id == c.turn_id {
            ancestor = true;
            break;
        }
        next = entry["parentId"].as_str();
    }
    if !ancestor {
        return Err(ErrorCode::CallerAmbiguous.into());
    }
    let row = entries.remove(&c.turn_id).ok_or(ErrorCode::HistoryGap)?;
    if row["type"] != "message" || row["message"]["role"] != "assistant" {
        return Err(ErrorCode::CallerAmbiguous.into());
    }
    let calls: Vec<_> = row["message"]["content"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|v| v["type"] == "toolCall" && v["id"] == c.tool_call_id)
        .collect();
    if calls.len() != 1 || calls[0]["name"] != "akasha_consult" {
        return Err(ErrorCode::CallerAmbiguous.into());
    }
    serde_json::from_value::<ConsultArgs>(calls[0]["arguments"].clone())
        .map_err(|_| ErrorCode::InvalidArgument)?;
    Ok(row)
}

async fn capture_fence(
    store: &Storage,
    r: &PiRegistration,
    c: &PiCall,
    generation: u64,
) -> Result<(u64, u64)> {
    let end = Instant::now() + Duration::from_secs(5);
    let row = native_fence(r, c)?;
    let binding = CaptureBinding::from_source(
        r.source.clone(),
        r.scope.clone(),
        super::TRANSCRIPT_STREAM,
        CoverageLevel::VisibleText,
    )?;
    let expected = super::transcript::decode(
        store,
        &binding,
        &r.source,
        &serde_json::to_vec(&row).map_err(|_| ErrorCode::ProtocolInvalid)?,
    )?;
    let expected = crate::capture::decode_hex(&expected.raw_hex, 4 * 1024 * 1024)?;
    let capture = Capture::new(store);
    let tip = EventRef {
        source: r.source.clone(),
        event_id: format!("entry:{}", c.turn_id),
    };
    for _ in 0..=256 {
        if Instant::now() >= end {
            break;
        }
        let at = store.sequence()?;
        if let Some(event) = capture.event(&binding, &tip, at)? {
            if capture.raw(&event)? != expected {
                return Err(ErrorCode::BindingConflict.into());
            }
            let mut next = Some(tip.clone());
            let mut seen = BTreeSet::new();
            let mut count = 0;
            while let Some(reference) = next {
                if seen.len() >= 16_384 || Instant::now() >= end {
                    return Err(ErrorCode::HistoryGap.into());
                }
                let page = capture.lineage(&binding, &reference, at, 64)?;
                if page.missing.is_some() {
                    return Err(ErrorCode::HistoryGap.into());
                }
                for event in page.events {
                    if !seen.insert(event.event.reference.event_id) {
                        return Err(ErrorCode::HistoryGap.into());
                    }
                    count += 1;
                }
                next = page.next;
            }
            return Ok((count, at));
        }
        let report = super::reconcile_transcript(
            store,
            &r.source,
            r.scope.clone(),
            &r.transcript,
            64,
            generation,
        )
        .await?;
        if report.processed == 0 {
            break;
        }
        // A later partial or bad frame must not erase a complete earlier invocation boundary.
    }
    Err(ErrorCode::HistoryGap.into())
}
