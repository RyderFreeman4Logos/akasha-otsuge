use super::*;

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryEdit {
    pub memory_id: String,
    pub expected_revision: u64,
    pub draft: MemoryDraft,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MemoryPatch {
    pub scope: Scope,
    pub changes: Vec<MemoryEdit>,
    pub reason: String,
    pub evidence: Vec<SourceSpan>,
    pub expires_at_ms: u64,
}

#[derive(Deserialize, Serialize)]
struct PatchAudit {
    schema_version: u32,
    principal: String,
    input_sha256: String,
    patch: MemoryPatch,
    provenance: Provenance,
    config_generation: u64,
    policy_revision: String,
}

fn key(request: &str) -> Result<RecordKey> {
    if !id(request) || request.chars().any(char::is_control) {
        return Err(error("INVALID_PATCH"));
    }
    Ok(RecordKey {
        kind: RecordKind::State,
        id: format!("memory-patch/{}", hash(request.as_bytes())),
    })
}

impl MemoryPatch {
    pub(crate) fn validate(&self) -> Result<()> {
        self.scope.validate()?;
        let mut targets = std::collections::BTreeSet::new();
        if self.reason.trim().is_empty()
            || self.reason.len() > 4096
            || self.changes.is_empty()
            || self.changes.len() > 8
            || self.expires_at_ms == 0
            || self.changes.iter().any(|change| {
                !id(&change.memory_id)
                    || change.expected_revision == 0
                    || !targets.insert(&change.memory_id)
                    || !self.scope.contains(&change.draft.scope)
            })
            || serde_json::to_vec(self)?.len() > 64 * 1024
        {
            return Err(error("INVALID_PATCH"));
        }
        if self.evidence.is_empty() || self.evidence.len() > 32 {
            return Err(error("INSUFFICIENT_EVIDENCE"));
        }
        for change in &self.changes {
            change.draft.validate()?;
            if !change
                .draft
                .source_spans
                .iter()
                .any(|span| self.evidence.contains(span))
            {
                return Err(error("INSUFFICIENT_EVIDENCE"));
            }
        }
        Ok(())
    }
}

impl Domain<'_> {
    pub(crate) async fn export_memory(
        &self,
        context: &ReadContext,
        memory: &str,
    ) -> Result<MemoryView> {
        self.storage
            .serialized_read(|| {
                self.permission(context, None)?;
                if !id(memory) {
                    return Err(error("INVALID_MEMORY"));
                }
                let memory = self
                    .memory(memory)?
                    .ok_or_else(|| error("MEMORY_NOT_FOUND"))?;
                self.permission(context, Some(&memory.record.draft.scope))?;
                Ok(memory)
            })
            .await
    }

    fn patch_audit(&self, context: &ReadContext, request: &str) -> Result<PatchAudit> {
        self.permission(context, None)?;
        let stored = self
            .storage
            .current(&key(request)?)?
            .ok_or_else(|| error("PATCH_NOT_FOUND"))?;
        let audit: PatchAudit = serde_json::from_str(&stored.content)?;
        if audit.schema_version != 1 || audit.principal != context.principal {
            return Err(error("READ_DENIED"));
        }
        self.permission(context, Some(&audit.patch.scope))?;
        Ok(audit)
    }

    pub(crate) async fn show_patch(
        &self,
        context: &ReadContext,
        request: &str,
    ) -> Result<serde_json::Value> {
        self.storage
            .serialized_read(|| Ok(serde_json::to_value(self.patch_audit(context, request)?)?))
            .await
    }

    pub(crate) async fn apply_patch(
        &self,
        context: &ReadContext,
        request: &str,
        patch: &MemoryPatch,
        ingress: &Ingress,
        policy: &Policy,
        now_ms: u64,
    ) -> Result<CommitReceipt> {
        self.storage
            .commit_or_reuse(|| {
                self.permission(context, Some(&patch.scope))?;
                patch.validate()?;
                let audit_key = key(request)?;
                let digest = hash(&serde_json::to_vec(&(
                    &context.principal,
                    patch,
                    &ingress.provenance,
                ))?);
                if let Some(receipt) = self.storage.receipt(request)? {
                    if !receipt.revisions.iter().any(|r| r.key == audit_key)
                        || self.patch_audit(context, request)?.input_sha256 != digest
                    {
                        return Err(error("IDEMPOTENCY_CONFLICT"));
                    }
                    return Ok(crate::storage::CommitPlan::Reuse(receipt));
                }
                if patch.expires_at_ms <= now_ms {
                    return Err(error("PATCH_EXPIRED"));
                }
                if !matches!(ingress.provenance.actor, Actor::Agent(_))
                    || ingress.provenance.origin != Origin::MemoryEdit
                {
                    return Err(error("INVALID_PROVENANCE"));
                }
                for span in &patch.evidence {
                    let source = self.span(span)?;
                    self.permission(context, Some(&source.payload.scope))?;
                }
                let mut changes = Vec::new();
                for edit in &patch.changes {
                    let old = self
                        .memory(&edit.memory_id)?
                        .ok_or_else(|| error("MEMORY_NOT_FOUND"))?;
                    self.permission(context, Some(&old.record.draft.scope))?;
                    if old.revision.commit_seq != edit.expected_revision {
                        return Err(error("REVISION_CONFLICT"));
                    }
                    if !patch.scope.contains(&old.record.draft.scope) {
                        return Err(error("SCOPE_DENIED"));
                    }
                    for span in &edit.draft.source_spans {
                        let source = self.span(span)?;
                        self.permission(context, Some(&source.payload.scope))?;
                    }
                    let write = MemoryWrite {
                        request_id: request.into(),
                        memory_id: edit.memory_id.clone(),
                        expected_revision: Some(edit.expected_revision),
                        draft: edit.draft.clone(),
                    };
                    let record = self.proposal_record(&write, ingress, policy)?;
                    changes.extend(self.memory_changes(&write, &record)?);
                }
                changes.push(Change {
                    key: audit_key,
                    expected_revision: None,
                    content: serde_json::to_string(&PatchAudit {
                        schema_version: 1,
                        principal: context.principal.clone(),
                        input_sha256: digest,
                        patch: patch.clone(),
                        provenance: ingress.provenance.clone(),
                        config_generation: policy.config_generation,
                        policy_revision: policy.revision.clone(),
                    })?,
                });
                Ok(crate::storage::CommitPlan::Write(CommitRequest {
                    request_id: request.into(),
                    config_generation: policy.config_generation,
                    policy_revision: Some(policy.revision.clone()),
                    changes,
                }))
            })
            .await
    }
}
