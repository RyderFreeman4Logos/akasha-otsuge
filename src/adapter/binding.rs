use super::*;
use crate::{
    capture::{Capture, CaptureBinding, CoverageLevel, EventRef, SourceIdentity},
    domain::{Domain, ReadContext, Scope},
    storage::{
        Change, CommitPlan, CommitRequest, RecordKey, RecordKind, Storage, StorageError, hash,
    },
};
use std::path::PathBuf;

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum CallerProof {
    Unknown,
    RegisteredRoot { thread: String },
    RegisteredChild { thread: String, parent: String },
}

/// Server assertion from authenticated host transport, never a model tool schema.
#[derive(Clone, Debug, Serialize)]
pub struct RuntimeBinding {
    pub invocation_id: String,
    pub runtime_instance: String,
    pub capability: CapabilityReport,
    pub caller: SourceIdentity,
    pub principal: String,
    pub scope: Scope,
    pub proof: CallerProof,
    pub branch: String,
    pub turn_id: Option<String>,
    pub tool_call_id: Option<String>,
    pub requested_cursor: u64,
    pub available_cursor: u64,
    pub tip: Option<EventRef>,
    pub capture_seq: u64,
    pub workspace: PathBuf,
    pub config_generation: u64,
}

// Only stored evidence can deserialize runtime identity; tool input cannot.
#[derive(Deserialize)]
#[serde(remote = "RuntimeBinding", deny_unknown_fields)]
struct StoredBinding {
    invocation_id: String,
    runtime_instance: String,
    capability: CapabilityReport,
    caller: SourceIdentity,
    principal: String,
    scope: Scope,
    proof: CallerProof,
    branch: String,
    turn_id: Option<String>,
    tool_call_id: Option<String>,
    requested_cursor: u64,
    available_cursor: u64,
    tip: Option<EventRef>,
    capture_seq: u64,
    workspace: PathBuf,
    config_generation: u64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct StoredInvocation {
    schema_version: u32,
    #[serde(deserialize_with = "StoredBinding::deserialize")]
    binding: RuntimeBinding,
}
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum FenceCoverage {
    CompleteToFence,
    Lagging,
    Unavailable,
}
#[derive(Clone, Debug)]
pub struct Invocation {
    binding: RuntimeBinding,
    revision: u64,
}
impl Invocation {
    pub async fn bind(store: &Storage, binding: RuntimeBinding) -> Result<Self> {
        binding.validate()?;
        let record = StoredInvocation {
            schema_version: 1,
            binding: binding.clone(),
        };
        let content = serde_json::to_string(&record).map_err(|_| ErrorCode::ProtocolInvalid)?;
        if content.len() > 32 * 1024 {
            return Err(ErrorCode::OutputLimit.into());
        }
        let key = invocation_key(&binding.invocation_id);
        let receipt = store
            .commit_or_reuse(|| {
                binding.check_anchor(store)?;
                if let Some(old) = store.current(&key)? {
                    if old.content != content {
                        return Err(StorageError::new("BINDING_CONFLICT"));
                    }
                    return Ok(CommitPlan::Reuse(
                        store
                            .receipt(&key.id)?
                            .ok_or_else(|| StorageError::new("BINDING_UNAVAILABLE"))?,
                    ));
                }
                Ok(CommitPlan::Write(CommitRequest {
                    request_id: key.id.clone(),
                    config_generation: binding.config_generation,
                    policy_revision: None,
                    changes: vec![Change {
                        key: key.clone(),
                        expected_revision: None,
                        content: content.clone(),
                    }],
                }))
            })
            .await?;
        Ok(Self {
            binding,
            revision: receipt.commit_seq,
        })
    }
    pub async fn load(store: &Storage, id: &str, principal: &str) -> Result<Self> {
        if !valid_id(id) || !valid_id(principal) {
            return Err(ErrorCode::BindingUnavailable.into());
        }
        let invocation = store
            .serialized_read(|| {
                let row = store
                    .current(&invocation_key(id))?
                    .ok_or_else(|| StorageError::new("BINDING_UNAVAILABLE"))?;
                let record: StoredInvocation = serde_json::from_str(&row.content)?;
                if record.schema_version != 1 || record.binding.invocation_id != id {
                    return Err(StorageError::new("BINDING_UNAVAILABLE"));
                }
                if record.binding.principal != principal {
                    return Err(StorageError::new("READ_DENIED"));
                }
                record.binding.check_anchor(store)?;
                Ok(Self {
                    binding: record.binding,
                    revision: row.commit_seq,
                })
            })
            .await?;
        invocation.binding.validate()?;
        Ok(invocation)
    }
    pub fn id(&self) -> &str {
        &self.binding.invocation_id
    }
    pub fn diagnostic(&self) -> &RuntimeBinding {
        &self.binding
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn coverage(&self) -> FenceCoverage {
        if self.binding.tip.is_none()
            || matches!(
                self.binding.capability.history,
                CoverageLevel::Unavailable | CoverageLevel::MetadataOnly
            )
        {
            FenceCoverage::Unavailable
        } else if self.binding.available_cursor < self.binding.requested_cursor {
            FenceCoverage::Lagging
        } else {
            FenceCoverage::CompleteToFence
        }
    }
    pub fn fence_hash(&self) -> String {
        // This record is bounded and Serialize cannot fail for its field types.
        hash(
            &serde_json::to_vec(&StoredInvocation {
                schema_version: 1,
                binding: self.binding.clone(),
            })
            .expect("serializable invocation"),
        )
    }
    pub(crate) fn read_context(&self) -> crate::storage::Result<ReadContext> {
        self.binding.read_context()
    }
    pub(crate) fn capture_binding(&self) -> crate::storage::Result<CaptureBinding> {
        self.binding.capture_binding()
    }
}

fn invocation_key(id: &str) -> RecordKey {
    RecordKey {
        kind: RecordKind::Evidence,
        id: format!("invocation/{}", hash(id.as_bytes())),
    }
}

impl RuntimeBinding {
    pub fn binding_level(&self) -> BindingLevel {
        if self.turn_id.is_some() {
            BindingLevel::ExactTurn
        } else {
            BindingLevel::SessionOnly
        }
    }
    fn validate(&self) -> Result<()> {
        if ![
            &self.invocation_id,
            &self.runtime_instance,
            &self.principal,
            &self.branch,
            &self.caller.adapter,
            &self.caller.profile,
            &self.caller.session,
            &self.caller.thread,
            &self.caller.agent,
        ]
        .into_iter()
        .all(|s| valid_id(s))
        {
            return Err(ErrorCode::BindingUnavailable.into());
        }
        let native_child = match &self.proof {
            CallerProof::RegisteredRoot { thread } if thread == &self.caller.thread => false,
            CallerProof::RegisteredChild { thread, parent }
                if thread == &self.caller.thread && valid_id(parent) && parent != thread =>
            {
                true
            }
            _ => return Err(ErrorCode::CallerAmbiguous.into()),
        };
        self.capability
            .require_binding(self.binding_level(), native_child)?;
        if self.available_cursor > self.requested_cursor
            || self.available_cursor == 0 && self.tip.is_some()
            || self.tip.is_none() && self.available_cursor != 0
            || self.config_generation == 0
            || !self.workspace.is_absolute()
            || self.workspace.as_os_str().len() > 4096
            || !self.scope.contains(&self.scope)
            || self
                .turn_id
                .iter()
                .chain(&self.tool_call_id)
                .any(|s| !valid_id(s))
        {
            return Err(ErrorCode::ProtocolInvalid.into());
        }
        if self.tip.as_ref().is_some_and(|t| t.source != self.caller) {
            return Err(ErrorCode::CallerAmbiguous.into());
        }
        Ok(())
    }
    fn read_context(&self) -> crate::storage::Result<ReadContext> {
        ReadContext::from_verified_binding(&self.principal, &self.invocation_id, "adapter")
    }
    fn capture_binding(&self) -> crate::storage::Result<CaptureBinding> {
        CaptureBinding::from_source(
            self.caller.clone(),
            self.scope.clone(),
            "adapter-read",
            self.capability.history,
        )
    }
    fn check_anchor(&self, store: &Storage) -> crate::storage::Result<()> {
        store.check_point(self.capture_seq)?;
        Domain::new(store).permission(&self.read_context()?, Some(&self.scope))?;
        if let Some(tip) = &self.tip {
            let event = Capture::new(store)
                .event(&self.capture_binding()?, tip, self.capture_seq)?
                .ok_or_else(|| StorageError::new("HISTORY_GAP"))?;
            if event.event.reference.source != self.caller
                || event.event.metadata.branch != self.branch
                || event.event.metadata.runtime_instance != self.runtime_instance
            {
                return Err(StorageError::new("CALLER_AMBIGUOUS"));
            }
        }
        Ok(())
    }
}
