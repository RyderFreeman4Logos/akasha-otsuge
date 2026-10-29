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
    async fn save_claude_registration(
        &self,
        registration: Registration,
        generation: u64,
    ) -> Result<serde_json::Value> {
        let content = serde_json::to_string(&registration)?;
        let mut changes = Vec::new();
        if registration.parent.is_some() {
            let identity = RecordKey {
                kind: RecordKind::Evidence,
                id: format!(
                    "claude-child/{}",
                    hash(&serde_json::to_vec(&registration.source)?)
                ),
            };
            if let Some(old) = self.storage.current(&identity)? {
                if old.content != content {
                    return Err(Error("BINDING_CONFLICT"));
                }
            } else {
                changes.push(Change {
                    key: identity,
                    expected_revision: None,
                    content: content.clone(),
                });
            }
        }
        let mut bytes = [0u8; 32];
        File::open("/dev/urandom")?.read_exact(&mut bytes)?;
        let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
        let key = key(&token);
        let request_id = key.id.clone();
        changes.push(Change {
            key,
            expected_revision: None,
            content,
        });
        self.storage
            .commit(&CommitRequest {
                request_id,
                config_generation: generation,
                policy_revision: None,
                changes,
            })
            .await?;
        Ok(json!({"registration":token}))
    }
    fn claude_registration(&self, token: &str) -> Result<Registration> {
        if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Error("BINDING_UNAVAILABLE"));
        }
        let record = self
            .storage
            .current(&key(token))?
            .ok_or(Error("BINDING_UNAVAILABLE"))?;
        let registration: Registration = serde_json::from_str(&record.content)?;
        if registration.scope != jobs::owner_scope()
            || self.allowed_path(&registration.transcript)? != registration.transcript
            || self.allowed_path(&registration.workspace)? != registration.workspace
        {
            return Err(Error("READ_DENIED"));
        }
        if let Some(home) = &registration.native_home
            && self.allowed_path(home)? != *home
        {
            return Err(Error("READ_DENIED"));
        }
        Ok(registration)
    }
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
                    start_kind,
                    native_home,
                } => {
                    if !ipc::identifier(&session_id)
                        || start_kind.as_deref().is_some_and(|kind| {
                            !matches!(kind, "startup" | "resume" | "clear" | "compact" | "fork")
                        })
                    {
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
                        parent: None,
                        start_kind,
                        native_home: native_home
                            .map(|path| service.allowed_path(&path))
                            .transpose()?,
                    };
                    service
                        .save_claude_registration(registration, generation)
                        .await
                }
                NativeEvent::RegisterChild {
                    parent_registration,
                    agent_id,
                } => {
                    if agent_id.is_empty()
                        || agent_id.len() > 128
                        || agent_id == "main"
                        || !agent_id
                            .bytes()
                            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
                    {
                        return Err(Error("INVALID_REQUEST"));
                    }
                    let parent = service.claude_registration(&parent_registration)?;
                    if parent.source.agent == agent_id {
                        return Err(Error("CALLER_AMBIGUOUS"));
                    }
                    // Pinned 2.1.311 native layout, verified by SubagentStop's actual path.
                    let directory = parent.transcript.parent().ok_or(Error("PATH_DENIED"))?;
                    let directory = if parent.parent.is_none() {
                        directory.join(&parent.source.session).join("subagents")
                    } else {
                        directory.to_path_buf()
                    };
                    let transcript =
                        service.allowed_path(&directory.join(format!("agent-{agent_id}.jsonl")))?;
                    let source = SourceIdentity {
                        thread: format!("{}/{}", parent.source.session, agent_id),
                        agent: agent_id,
                        ..parent.source.clone()
                    };
                    let registration = Registration {
                        source,
                        transcript,
                        parent: Some(parent.source.thread),
                        capability: parent.capability,
                        scope: parent.scope,
                        workspace: parent.workspace,
                        start_kind: None,
                        native_home: parent.native_home,
                    };
                    service
                        .save_claude_registration(registration, generation)
                        .await
                }
                NativeEvent::Bind(call) => {
                    let registration = service.claude_registration(&call.registration)?;
                    let caller = ClaudeCaller {
                        store: service.storage.clone(),
                        registration,
                        call,
                        generation,
                    };
                    let invocation = caller.bind(&caller.call.tool_call_id).await?;
                    Ok(serde_json::to_value(invocation.diagnostic())?)
                }
                NativeEvent::Consult { call, args } => {
                    let registration = service.claude_registration(&call.registration)?;
                    let native = jobs::consult::NativeExecutor {
                        host: registration.capability.host,
                        binary: registration
                            .capability
                            .executable
                            .clone()
                            .ok_or(Error("EXECUTOR_UNAVAILABLE"))?,
                        home: registration
                            .native_home
                            .clone()
                            .ok_or(Error("BINDING_UNAVAILABLE"))?,
                    };
                    let caller = ClaudeCaller {
                        store: service.storage.clone(),
                        registration,
                        call,
                        generation,
                    };
                    let invocation = caller.bind(&caller.call.tool_call_id).await?;
                    drop(_guard);
                    let job = service.submit_consult(invocation, native, args).await?;
                    service
                        .show_consult(job["id"].as_str().ok_or(Error("JOB_CORRUPT"))?)
                        .await
                }
                NativeEvent::Sync {
                    registration: token,
                    tip,
                } => {
                    if !ipc::identifier(&tip) {
                        return Err(Error("INVALID_REQUEST"));
                    }
                    let registration = service.claude_registration(&token)?;
                    let id = format!(
                        "claude-capture/{}",
                        hash(&serde_json::to_vec(&(&registration.source, &tip))?)
                    );
                    let binding = ipc::CaptureInput {
                        source: registration.source,
                        scope: registration.scope,
                        stream: "native-file".into(),
                        coverage: crate::capture::CoverageLevel::VisibleText,
                        host: Some(crate::config::Host::ClaudeCode),
                        until_event: Some(tip),
                    };
                    drop(_guard);
                    service.submit(id, binding, registration.transcript).await
                }
            }
        })
        .await
    }
}
