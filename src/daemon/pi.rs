use super::*;
use crate::{
    adapter::{
        CallerAdapter,
        pi::{self, NativeEvent, PiCaller, PiRegistration},
    },
    capture::SourceIdentity,
    domain::Domain,
    storage::{Change, CommitRequest, RecordKey, RecordKind, hash},
};
use std::io::Read;

fn token_key(token: &str) -> RecordKey {
    RecordKey {
        kind: RecordKind::Evidence,
        id: format!("pi-registration/{}", hash(token.as_bytes())),
    }
}

impl Service {
    fn pi_registration(&self, token: &str) -> Result<PiRegistration> {
        if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error("BINDING_UNAVAILABLE"));
        }
        let record = self
            .storage
            .current(&token_key(token))?
            .ok_or(Error("BINDING_UNAVAILABLE"))?;
        let reg: PiRegistration = serde_json::from_str(&record.content)?;
        if reg.scope != jobs::owner_scope()
            || reg.principal != reg.scope.user
            || self.allowed_path(&reg.agent_dir)? != reg.agent_dir
            || self.allowed_path(&reg.transcript)? != reg.transcript
            || self.allowed_path(&reg.workspace)? != reg.workspace
        {
            return Err(Error("READ_DENIED"));
        }
        Ok(reg)
    }

    pub(super) async fn pi(self: &Arc<Self>, event: NativeEvent) -> Result<serde_json::Value> {
        let service = self.clone();
        self.operation(async move {
            let guard = service.admission.lock().await;
            let scope = jobs::owner_scope();
            Domain::new(&service.storage).permission(&jobs::context()?, Some(&scope))?;
            let generation = service.config.pin().generation();
            match event {
                NativeEvent::Register {
                    binary,
                    agent_dir,
                    session_id,
                    transcript_path,
                    cwd,
                } => {
                    if !ipc::identifier(&session_id) {
                        return Err(Error("INVALID_REQUEST"));
                    }
                    let agent_dir = service.allowed_path(&agent_dir)?;
                    let transcript = service.allowed_path(&transcript_path)?;
                    let workspace = service.allowed_path(&cwd)?;
                    if !agent_dir.is_dir() || !workspace.is_dir() {
                        return Err(Error("PATH_DENIED"));
                    }
                    let source = SourceIdentity {
                        adapter: "pi".into(),
                        profile: pi::profile_key(&agent_dir),
                        session: session_id.clone(),
                        thread: session_id,
                        agent: "main".into(),
                    };
                    let header =
                        pi::session_header(&transcript, &source)?.ok_or(Error("HISTORY_GAP"))?;
                    if header["cwd"].as_str() != workspace.to_str() {
                        return Err(Error("BINDING_CONFLICT"));
                    }
                    let capability = pi::probe(&binary).await?;
                    if capability.version.as_deref() != Some(pi::SUPPORTED_VERSION) {
                        return Err(Error("ADAPTER_UNSUPPORTED"));
                    }
                    let identity = RecordKey {
                        kind: RecordKind::Evidence,
                        id: format!(
                            "pi-session-registration/{}",
                            hash(&serde_json::to_vec(&source)?)
                        ),
                    };
                    if let Some(prior) = service.storage.current(&identity)? {
                        let token: String = serde_json::from_str(&prior.content)?;
                        let old = service.pi_registration(&token)?;
                        if old.source != source
                            || old.transcript != transcript
                            || old.workspace != workspace
                            || old.agent_dir != agent_dir
                        {
                            return Err(Error("BINDING_CONFLICT"));
                        }
                        return Ok(json!({"registration":token}));
                    }
                    let reg = PiRegistration {
                        capability,
                        source,
                        scope: scope.clone(),
                        principal: scope.user,
                        agent_dir,
                        transcript,
                        workspace,
                    };
                    let mut bytes = [0; 32];
                    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
                    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                    let key = token_key(&token);
                    service
                        .storage
                        .commit(&CommitRequest {
                            request_id: key.id.clone(),
                            config_generation: generation,
                            policy_revision: None,
                            changes: vec![
                                Change {
                                    key,
                                    expected_revision: None,
                                    content: serde_json::to_string(&reg)?,
                                },
                                Change {
                                    key: identity,
                                    expected_revision: None,
                                    content: serde_json::to_string(&token)?,
                                },
                            ],
                        })
                        .await?;
                    Ok(json!({"registration":token}))
                }
                NativeEvent::Bind { registration, call } => {
                    let reg = service.pi_registration(&registration)?;
                    let id = call.tool_call_id.clone();
                    let invocation = PiCaller::new(service.storage.clone(), reg, call, generation)
                        .bind(&id)
                        .await?;
                    Ok(serde_json::to_value(invocation.diagnostic())?)
                }
                NativeEvent::Sync {
                    registration,
                    until_entry,
                } => {
                    if !ipc::identifier(&until_entry) {
                        return Err(Error("INVALID_REQUEST"));
                    }
                    let reg = service.pi_registration(&registration)?;
                    let id = format!(
                        "pi-capture/{}",
                        hash(&serde_json::to_vec(&(&reg.source, &until_entry))?)
                    );
                    let input = ipc::CaptureInput {
                        source: reg.source,
                        scope: reg.scope,
                        stream: pi::TRANSCRIPT_STREAM.into(),
                        coverage: crate::capture::CoverageLevel::VisibleText,
                        host: Some(crate::config::Host::Pi),
                        until_event: Some(format!("entry:{until_entry}")),
                    };
                    drop(guard);
                    service.submit(id, input, reg.transcript).await
                }
            }
        })
        .await
    }
}
