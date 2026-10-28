use super::super::*;
use crate::{
    capture::*,
    config::Host,
    domain::Scope,
    storage::{Storage, hash},
};
use std::{path::PathBuf, sync::Arc};

/// Private host transport only. These fields are never part of a model tool schema.
#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeEvent {
    Register {
        binary: PathBuf,
        session_id: String,
        transcript_path: PathBuf,
        cwd: PathBuf,
    },
    Bind(NativeCall),
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeCall {
    pub registration: String,
    pub session_id: String,
    pub agent_id: Option<String>,
    pub turn_id: String,
    pub tool_call_id: String,
    pub cursor: u64,
    pub tip: String,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Registration {
    pub capability: CapabilityReport,
    pub source: SourceIdentity,
    pub scope: Scope,
    pub transcript: PathBuf,
    pub workspace: PathBuf,
}

pub struct ClaudeCaller {
    pub(crate) store: Arc<Storage>,
    pub(crate) registration: Registration,
    pub(crate) call: NativeCall,
    pub(crate) generation: u64,
}

impl CallerAdapter for ClaudeCaller {
    fn probe(&self) -> Result<CapabilityReport> {
        let mut report = self.registration.capability.clone();
        if report.host != Host::ClaudeCode || report.version.as_deref() != Some("2.1.311") {
            return Err(ErrorCode::AdapterUnsupported.into());
        }
        report.binding = BindingLevel::ExactTurn;
        report.history = CoverageLevel::VisibleText;
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
            let call = &self.call;
            let registered = &self.registration;
            if runtime_call != call.tool_call_id
                || call.session_id != registered.source.session
                || call.agent_id.is_some()
                || registered.source.agent != "main"
                || ![&call.turn_id, &call.tool_call_id, &call.tip]
                    .into_iter()
                    .all(|s| valid_id(s))
                || call.cursor == 0
            {
                return Err(ErrorCode::CallerAmbiguous.into());
            }
            let id = format!(
                "claude/{}",
                hash(
                    &serde_json::to_vec(&(&call.registration, runtime_call))
                        .map_err(|_| ErrorCode::ProtocolInvalid)?
                )
            );
            let tip = EventRef {
                source: registered.source.clone(),
                event_id: call.tip.clone(),
            };
            match Invocation::load(&self.store, &id, &registered.scope.user).await {
                Ok(invocation) => {
                    let old = invocation.diagnostic();
                    if old.caller != registered.source
                        || old.turn_id.as_deref() != Some(&call.turn_id)
                        || old.tool_call_id.as_deref() != Some(runtime_call)
                        || old.requested_cursor != call.cursor
                        || old.tip.as_ref() != Some(&tip)
                    {
                        return Err(ErrorCode::BindingConflict.into());
                    }
                    return Ok(invocation);
                }
                Err(error) if error.code == ErrorCode::BindingUnavailable => (),
                Err(error) => return Err(error),
            }
            let store = self.store.clone();
            let registration = registered.clone();
            let capture_tip = tip.clone();
            let generation = self.generation;
            let runtime = tokio::runtime::Handle::current();
            let capture_seq = tokio::task::spawn_blocking(move || {
                runtime.block_on(async {
                    let binding = CaptureBinding::from_source(
                        registration.source.clone(),
                        registration.scope.clone(),
                        "native-file",
                        CoverageLevel::VisibleText,
                    )?;
                    let capture = Capture::new(&store);
                    let end = std::time::Instant::now() + std::time::Duration::from_secs(2);
                    for _ in 0..256 {
                        let at = store.sequence()?;
                        if capture.event(&binding, &capture_tip, at)?.is_some() {
                            return Ok(at);
                        }
                        if std::time::Instant::now() >= end {
                            break;
                        }
                        let report = capture
                            .reconcile_decoded(
                                &binding,
                                &registration.transcript,
                                1,
                                generation,
                                |line| {
                                    super::transcript_frame(
                                        &store,
                                        &binding,
                                        &registration.source,
                                        line,
                                    )
                                },
                                |_| {},
                            )
                            .await?;
                        if report.checkpoint.checkpoint.pending != Some(PendingReason::Backlog) {
                            let at = store.sequence()?;
                            if capture.event(&binding, &capture_tip, at)?.is_some() {
                                return Ok(at);
                            }
                            break;
                        }
                    }
                    Err(crate::storage::StorageError::new("HISTORY_GAP"))
                })
            })
            .await
            .map_err(|_| ErrorCode::SourceUnavailable)??;
            Invocation::bind(
                &self.store,
                RuntimeBinding {
                    invocation_id: id,
                    runtime_instance: registered.source.session.clone(),
                    capability: self.probe()?,
                    caller: registered.source.clone(),
                    principal: registered.scope.user.clone(),
                    scope: registered.scope.clone(),
                    proof: CallerProof::RegisteredRoot {
                        thread: registered.source.thread.clone(),
                    },
                    branch: registered.source.thread.clone(),
                    turn_id: Some(call.turn_id.clone()),
                    tool_call_id: Some(call.tool_call_id.clone()),
                    requested_cursor: call.cursor,
                    available_cursor: call.cursor,
                    tip: Some(tip),
                    capture_seq,
                    workspace: registered.workspace.clone(),
                    config_generation: self.generation,
                },
            )
            .await
        })
    }
}
