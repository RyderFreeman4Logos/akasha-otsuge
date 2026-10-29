use super::*;
use crate::{
    adapter::{
        CallerAdapter,
        codex::{CodexCaller, CodexRegistration, NativeEvent},
    },
    capture::SourceIdentity,
    domain::Domain,
    storage::{Change, CommitRequest, RecordKey, RecordKind, hash},
};
use std::io::Read;

fn key(token: &str) -> RecordKey {
    RecordKey {
        kind: RecordKind::Evidence,
        id: format!("codex-registration/{}", hash(token.as_bytes())),
    }
}

impl Service {
    fn codex_registration(&self, token: &str) -> Result<CodexRegistration> {
        if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error("BINDING_UNAVAILABLE"));
        }
        let record = self
            .storage
            .current(&key(token))?
            .ok_or(Error("BINDING_UNAVAILABLE"))?;
        let reg: CodexRegistration = serde_json::from_str(&record.content)?;
        if reg.scope != jobs::owner_scope()
            || reg.principal != reg.scope.user
            || self.allowed_path(&reg.transcript)? != reg.transcript
            || self.allowed_path(&reg.workspace)? != reg.workspace
        {
            return Err(Error("READ_DENIED"));
        }
        if let Some(home) = &reg.native_home
            && self.allowed_path(home)? != *home
        {
            return Err(Error("READ_DENIED"));
        }
        Ok(reg)
    }

    pub(super) async fn codex(self: &Arc<Self>, event: NativeEvent) -> Result<serde_json::Value> {
        let service = self.clone();
        self.operation(async move {
            let guard = service.admission.lock().await;
            let scope = jobs::owner_scope();
            Domain::new(&service.storage).permission(&jobs::context()?, Some(&scope))?;
            let generation = service.config.pin().generation();
            match event {
                NativeEvent::Register {
                    binary,
                    session_id,
                    transcript_path,
                    cwd,
                    native_home,
                } => {
                    if !ipc::identifier(&session_id) {
                        return Err(Error("INVALID_REQUEST"));
                    }
                    let transcript = service.allowed_path(&transcript_path)?;
                    let workspace = service.allowed_path(&cwd)?;
                    if !workspace.is_dir() {
                        return Err(Error("PATH_DENIED"));
                    }
                    let capability = crate::adapter::codex::probe(&binary).await?;
                    if capability.version.as_deref() != Some("0.164.0") {
                        return Err(Error("ADAPTER_UNSUPPORTED"));
                    }
                    let reg = CodexRegistration {
                        capability,
                        source: SourceIdentity {
                            adapter: "codex".into(),
                            profile: service.profile.clone().unwrap_or_else(|| "default".into()),
                            session: session_id.clone(),
                            thread: session_id,
                            agent: "main".into(),
                        },
                        principal: scope.user.clone(),
                        scope,
                        transcript,
                        workspace,
                        native_home: native_home.map(|p| service.allowed_path(&p)).transpose()?,
                    };
                    let mut bytes = [0; 32];
                    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
                    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                    let key = key(&token);
                    service
                        .storage
                        .commit(&CommitRequest {
                            request_id: key.id.clone(),
                            config_generation: generation,
                            policy_revision: None,
                            changes: vec![Change {
                                key,
                                expected_revision: None,
                                content: serde_json::to_string(&reg)?,
                            }],
                        })
                        .await?;
                    Ok(json!({"registration":token}))
                }
                NativeEvent::Bind { registration, call } => {
                    let reg = service.codex_registration(&registration)?;
                    let id = call.tool_call_id.clone();
                    let invocation =
                        CodexCaller::new(service.storage.clone(), reg, call, generation)
                            .bind(&id)
                            .await?;
                    Ok(serde_json::to_value(invocation.diagnostic())?)
                }
                NativeEvent::Consult {
                    registration,
                    call,
                    args,
                } => {
                    let reg = service.codex_registration(&registration)?;
                    let native = jobs::consult::NativeExecutor {
                        host: reg.capability.host,
                        binary: reg
                            .capability
                            .executable
                            .clone()
                            .ok_or(Error("EXECUTOR_UNAVAILABLE"))?,
                        home: reg
                            .native_home
                            .clone()
                            .ok_or(Error("BINDING_UNAVAILABLE"))?,
                    };
                    let id = call.tool_call_id.clone();
                    let invocation =
                        CodexCaller::new(service.storage.clone(), reg, call, generation)
                            .bind(&id)
                            .await?;
                    drop(guard);
                    service.submit_consult(invocation, native, args).await
                }
                NativeEvent::Sync {
                    registration,
                    until_ordinal,
                } => {
                    let reg = service.codex_registration(&registration)?;
                    let id = format!(
                        "codex-capture/{}",
                        hash(&serde_json::to_vec(&(&reg.source, until_ordinal))?)
                    );
                    let input = ipc::CaptureInput {
                        source: reg.source,
                        scope: reg.scope,
                        stream: crate::adapter::codex::TRANSCRIPT_STREAM.into(),
                        coverage: crate::capture::CoverageLevel::VisibleText,
                        host: Some(crate::config::Host::Codex),
                        until_event: Some(format!("row:{until_ordinal}")),
                    };
                    drop(guard);
                    service.submit(id, input, reg.transcript).await
                }
            }
        })
        .await
    }
}
