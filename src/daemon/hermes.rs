use super::*;
use crate::{
    adapter::{
        CallerAdapter,
        hermes::{self, HermesCaller, HermesRegistration, NativeEvent},
    },
    capture::{CoverageLevel, SourceIdentity},
    domain::Domain,
    storage::hash,
};
use std::io::Write;

fn field<'a>(v: &'a serde_json::Value, key: &str) -> Result<&'a str> {
    v[key]
        .as_str()
        .filter(|s| ipc::identifier(s))
        .ok_or(Error("ADAPTER_UNSUPPORTED"))
}

fn spool(
    home: &Path,
    source: &SourceIdentity,
    frames: &[crate::capture::CaptureFrame],
) -> Result<PathBuf> {
    let mut root = home.to_path_buf();
    for component in ["plugin-data", "akasha-otsuge", "capture"] {
        root.push(component);
        match fs::DirBuilder::new().mode(0o700).create(&root) {
            Ok(()) => {}
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
            Err(e) => return Err(e.into()),
        }
        let meta = fs::symlink_metadata(&root)?;
        if !meta.is_dir()
            || meta.file_type().is_symlink()
            || meta.uid() != rustix::process::geteuid().as_raw()
            || meta.mode() & 0o022 != 0
        {
            return Err(Error("UNSAFE_PATH"));
        }
    }
    private_dir(&root)?;
    let key = hash(&serde_json::to_vec(source)?);
    let path = root.join(format!("{key}.jsonl"));
    let temporary = root.join(format!("{key}-{}.pending", std::process::id()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .custom_flags(rustix::fs::OFlags::NOFOLLOW.bits() as i32)
        .open(&temporary)?;
    let result = (|| {
        for frame in frames {
            serde_json::to_writer(&mut file, frame)?;
            file.write_all(b"\n")?;
        }
        file.sync_all()?;
        fs::rename(&temporary, &path)?;
        File::open(&root)?.sync_all()?;
        Ok::<_, Error>(path)
    })();
    drop(file);
    if temporary.try_exists()? {
        fs::remove_file(temporary)?;
    }
    result
}

impl Service {
    pub(super) async fn hermes(self: &Arc<Self>, event: NativeEvent) -> Result<serde_json::Value> {
        let service = self.clone();
        self.operation(async move {
            let guard = service.admission.lock().await;
            let scope = jobs::owner_scope();
            Domain::new(&service.storage).permission(&jobs::context()?, Some(&scope))?;
            let generation = service.config.pin().generation();
            let (binary, home, session, call, args) = match event {
                NativeEvent::Bind { binary, home, call } => {
                    (binary, home, call.session_id.clone(), Some(call), None)
                }
                NativeEvent::Consult {
                    binary,
                    home,
                    call,
                    args,
                } => (
                    binary,
                    home,
                    call.session_id.clone(),
                    Some(call),
                    Some(args),
                ),
                NativeEvent::Sync {
                    binary,
                    home,
                    session_id,
                } => (binary, home, session_id, None, None),
            };
            if !ipc::identifier(&session) {
                return Err(Error("INVALID_REQUEST"));
            }
            let canonical_home = service.allowed_path(&home)?;
            if !canonical_home.is_dir() {
                return Err(Error("PATH_DENIED"));
            }
            if call
                .as_ref()
                .is_some_and(|c| c.profile_home != canonical_home)
            {
                return Err(Error("CALLER_AMBIGUOUS"));
            }
            let capability = hermes::probe(&binary).await?;
            if !hermes::supported_version(capability.version.as_deref()) {
                return Err(Error("ADAPTER_UNSUPPORTED"));
            }
            let bytes = hermes::export_session(
                capability
                    .executable
                    .as_deref()
                    .ok_or(Error("EXECUTOR_UNAVAILABLE"))?,
                &home,
                &session,
            )
            .await?;
            let native: serde_json::Value = serde_json::from_slice(&bytes)?;
            if field(&native, "id")? != session {
                return Err(Error("BINDING_CONFLICT"));
            }
            let profile = field(&native, "profile_name")?.to_owned();
            let workspace = service.allowed_path(Path::new(
                native["cwd"].as_str().ok_or(Error("ADAPTER_UNSUPPORTED"))?,
            ))?;
            let parent = match &native["parent_session_id"] {
                serde_json::Value::Null => None,
                serde_json::Value::String(s) if ipc::identifier(s) => Some(s.clone()),
                _ => return Err(Error("ADAPTER_UNSUPPORTED")),
            };
            let source = SourceIdentity {
                adapter: "hermes".into(),
                profile: hermes::profile_key(&canonical_home),
                session: session.clone(),
                thread: session.clone(),
                agent: if parent.is_some() {
                    session.clone()
                } else {
                    "main".into()
                },
            };
            let frames = hermes::export::decode_export(&source, &profile, &bytes)?;
            if let Some(call) = call {
                let id = call.tool_call_id.clone();
                let native = jobs::consult::NativeExecutor {
                    host: capability.host,
                    binary: capability
                        .executable
                        .clone()
                        .ok_or(Error("EXECUTOR_UNAVAILABLE"))?,
                    home: canonical_home.clone(),
                };
                let registration = HermesRegistration {
                    capability,
                    source,
                    scope: scope.clone(),
                    principal: scope.user,
                    native_profile: profile,
                    profile_home: canonical_home.clone(),
                    snapshot: canonical_home.join("state.db"),
                    workspace,
                    task_id: call.task_id.clone(),
                    parent_session: parent,
                };
                let bound =
                    HermesCaller::new(service.storage.clone(), registration, call, generation)
                        .with_export(bytes)
                        .bind(&id)
                        .await?;
                if let Some(args) = args {
                    drop(guard);
                    let job = service.submit_consult(bound, native, args).await?;
                    return service
                        .show_consult(job["id"].as_str().ok_or(Error("JOB_CORRUPT"))?)
                        .await;
                }
                Ok(serde_json::to_value(bound.diagnostic())?)
            } else {
                let path = spool(&canonical_home, &source, &frames)?;
                let id = format!(
                    "hermes-capture/{}",
                    hash(&serde_json::to_vec(&(&source, &frames))?)
                );
                let input = ipc::CaptureInput {
                    source,
                    scope,
                    stream: hermes::BACKGROUND_STREAM.into(),
                    coverage: CoverageLevel::VisibleText,
                    host: Some(crate::config::Host::Hermes),
                    until_event: frames.last().map(|f| f.event.event_id.clone()),
                };
                drop(guard);
                service.submit(id, input, path).await
            }
        })
        .await
    }
}
