use super::*;
use crate::{
    adapter::{
        CallerAdapter,
        claude::{ClaudeCaller, NativeEvent, caller::Registration},
    },
    capture::SourceIdentity,
    domain::Domain,
    storage::{Change, CommitRequest, RecordKey, RecordKind, hash},
};
use std::io::Read;

fn key(token: &str) -> RecordKey {
    RecordKey {
        kind: RecordKind::Evidence,
        id: format!("claude-registration/{}", hash(token.as_bytes())),
    }
}

impl Service {
    pub(super) async fn claude(self: &Arc<Self>, event: NativeEvent) -> Result<serde_json::Value> {
        let service = self.clone();
        self.operation(async move {
            let _guard = service.admission.lock().await;
            let scope = jobs::owner_scope();
            Domain::new(&service.storage).permission(&jobs::context()?, Some(&scope))?;
            let generation = service.config.pin().generation();
            match event {
                NativeEvent::Register {
                    binary,
                    session_id,
                    transcript_path,
                    cwd,
                } => {
                    if !ipc::identifier(&session_id) {
                        return Err(Error("INVALID_REQUEST"));
                    }
                    let transcript = service.allowed_path(&transcript_path)?;
                    let workspace = service.allowed_path(&cwd)?;
                    if !workspace.is_dir() {
                        return Err(Error("PATH_DENIED"));
                    }
                    let capability = crate::adapter::claude::probe(&binary).await?;
                    if capability.version.as_deref() != Some("2.1.311") {
                        return Err(Error("ADAPTER_UNSUPPORTED"));
                    }
                    let mut bytes = [0u8; 32];
                    File::open("/dev/urandom")?.read_exact(&mut bytes)?;
                    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
                    let registration = Registration {
                        capability,
                        source: SourceIdentity {
                            adapter: "claude_code".into(),
                            profile: service.profile.clone().unwrap_or_else(|| "default".into()),
                            session: session_id.clone(),
                            thread: session_id,
                            agent: "main".into(),
                        },
                        scope,
                        transcript,
                        workspace,
                    };
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
                                content: serde_json::to_string(&registration)?,
                            }],
                        })
                        .await?;
                    Ok(json!({"registration":token}))
                }
                NativeEvent::Bind(call) => {
                    if call.registration.len() != 64
                        || !call.registration.bytes().all(|b| b.is_ascii_hexdigit())
                    {
                        return Err(Error("BINDING_UNAVAILABLE"));
                    }
                    let record = service
                        .storage
                        .current(&key(&call.registration))?
                        .ok_or(Error("BINDING_UNAVAILABLE"))?;
                    let registration: Registration = serde_json::from_str(&record.content)?;
                    if registration.scope != scope
                        || service.allowed_path(&registration.transcript)?
                            != registration.transcript
                        || service.allowed_path(&registration.workspace)? != registration.workspace
                    {
                        return Err(Error("READ_DENIED"));
                    }
                    let caller = ClaudeCaller {
                        store: service.storage.clone(),
                        registration,
                        call,
                        generation,
                    };
                    let invocation = caller.bind(&caller.call.tool_call_id).await?;
                    Ok(serde_json::to_value(invocation.diagnostic())?)
                }
            }
        })
        .await
    }
}
