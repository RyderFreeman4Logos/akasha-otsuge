use super::*;

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ReviewState {
    Open,
    Investigating,
    Resolved,
    Dismissed,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct ReviewRecord {
    schema_version: u32,
    memory_id: String,
    proposal_request: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    proposal_revision: Option<u64>,
    semantic_sha256: String,
    scope: Scope,
    state: ReviewState,
    reason: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    resolution: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewItem {
    pub id: String,
    pub revision: u64,
    pub memory_id: String,
    pub proposal_revision: u64,
    pub semantic_sha256: String,
    pub scope: Scope,
    pub state: ReviewState,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewDetail {
    pub item: ReviewItem,
    pub proposal: MemoryView,
    pub result: Option<MemoryView>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewPage {
    pub items: Vec<ReviewItem>,
    pub next: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewUpdate {
    pub id: String,
    pub expected_revision: u64,
    pub state: ReviewState,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewOpen {
    pub memory_id: String,
    pub expected_revision: u64,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewDecision {
    pub id: String,
    pub expected_revision: u64,
    pub action: ReviewAction,
    pub draft: Option<MemoryDraft>,
    pub reason: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ReviewPreview {
    pub before: MemoryView,
    pub after: MemoryDraft,
    pub action: ReviewAction,
    pub reason: String,
    pub semantic_sha256: String,
    pub config_generation: u64,
    pub preview_sha256: String,
}

#[derive(Deserialize, Serialize)]
struct DecisionAudit {
    input_sha256: String,
    preview: ReviewPreview,
}

fn decision_key(request: &str) -> Result<RecordKey> {
    if !id(request) || request.chars().any(char::is_control) {
        return Err(error("INVALID_REVIEW"));
    }
    Ok(RecordKey {
        kind: RecordKind::State,
        id: format!("review-decision/{}", hash(request.as_bytes())),
    })
}

fn decision_digest(context: &ReadContext, decision: &ReviewDecision) -> Result<String> {
    Ok(hash(&serde_json::to_vec(&(&context.principal, decision))?))
}

fn key(id: &str) -> Result<RecordKey> {
    if id.len() != 64 || !id.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(error("INVALID_REVIEW"));
    }
    Ok(RecordKey {
        kind: RecordKind::State,
        id: format!("review/{id}"),
    })
}

fn reason(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 4096 {
        return Err(error("INVALID_REVIEW"));
    }
    Ok(())
}

pub(super) fn candidate_change(
    write: &MemoryWrite,
    record: &MemoryRecord,
) -> Result<Option<Change>> {
    let needs_review = record.lifecycle == Lifecycle::Candidate
        || (record.authority == Authority::DelegatedApproved
            && (record.draft.kind != MemoryKind::Fact || !record.draft.assumptions.is_empty()));
    if !needs_review {
        return Ok(None);
    }
    let id = hash(&serde_json::to_vec(&(&write.memory_id, &write.request_id))?);
    Ok(Some(Change {
        key: key(&id)?,
        expected_revision: None,
        content: serde_json::to_string(&ReviewRecord {
            schema_version: 1,
            memory_id: write.memory_id.clone(),
            proposal_request: write.request_id.clone(),
            proposal_revision: None,
            semantic_sha256: record.semantic_sha256.clone(),
            scope: record.draft.scope.clone(),
            state: ReviewState::Open,
            resolution: None,
            reason: if record.lifecycle == Lifecycle::Candidate {
                "candidate_requires_review"
            } else {
                "active_automated_conclusion"
            }
            .into(),
        })?,
    }))
}

impl Domain<'_> {
    fn review_detail(&self, id: &str, revision: Revision) -> Result<ReviewDetail> {
        let record: ReviewRecord = serde_json::from_str(&revision.content)?;
        if record.schema_version != 1
            || record.scope.validate().is_err()
            || reason(&record.reason).is_err()
        {
            return Err(error("DOMAIN_CORRUPT"));
        }
        let target_revision = match record.proposal_revision {
            Some(seq) if seq > 0 && record.proposal_request.is_empty() => seq,
            None => {
                let receipt = self
                    .storage
                    .receipt(&record.proposal_request)?
                    .ok_or_else(|| error("DOMAIN_CORRUPT"))?;
                if !receipt
                    .revisions
                    .iter()
                    .any(|r| r.key == memory_key(&record.memory_id))
                {
                    return Err(error("DOMAIN_CORRUPT"));
                }
                receipt.commit_seq
            }
            _ => return Err(error("DOMAIN_CORRUPT")),
        };
        let proposal = self
            .revision(&record.memory_id, target_revision)?
            .ok_or_else(|| error("DOMAIN_CORRUPT"))?;
        if proposal.record.semantic_sha256 != record.semantic_sha256
            || proposal.record.draft.scope != record.scope
        {
            return Err(error("DOMAIN_CORRUPT"));
        }
        let result = record
            .resolution
            .as_ref()
            .map(|id| {
                let receipt = self
                    .storage
                    .receipt(id)?
                    .ok_or_else(|| error("DOMAIN_CORRUPT"))?;
                self.revision(&record.memory_id, receipt.commit_seq)?
                    .ok_or_else(|| error("DOMAIN_CORRUPT"))
            })
            .transpose()?;
        Ok(ReviewDetail {
            item: ReviewItem {
                id: id.into(),
                revision: revision.commit_seq,
                memory_id: record.memory_id,
                proposal_revision: proposal.revision.commit_seq,
                semantic_sha256: record.semantic_sha256,
                scope: record.scope,
                state: record.state,
                reason: record.reason,
            },
            proposal,
            result,
        })
    }

    pub async fn review_open(
        &self,
        context: &ReadContext,
        request: &str,
        open: &ReviewOpen,
        config_generation: u64,
    ) -> Result<CommitReceipt> {
        self.storage
            .commit_checked(|| {
                reason(&open.reason)?;
                self.permission(context, None)?;
                let target = self
                    .revision(&open.memory_id, open.expected_revision)?
                    .ok_or_else(|| error("REVISION_NOT_FOUND"))?;
                self.permission(context, Some(&target.record.draft.scope))?;
                let previous = self.storage.receipt(request)?;
                if previous.is_none()
                    && self
                        .memory(&open.memory_id)?
                        .is_none_or(|m| m.revision != target.revision)
                {
                    return Err(error("REVISION_CONFLICT"));
                }
                let id = hash(&serde_json::to_vec(&(&open.memory_id, request))?);
                let record = ReviewRecord {
                    schema_version: 1,
                    memory_id: open.memory_id.clone(),
                    proposal_request: String::new(),
                    proposal_revision: Some(open.expected_revision),
                    semantic_sha256: target.record.semantic_sha256,
                    scope: target.record.draft.scope,
                    state: ReviewState::Open,
                    reason: open.reason.clone(),
                    resolution: None,
                };
                Ok(CommitRequest {
                    request_id: request.into(),
                    config_generation: previous.map_or(config_generation, |r| r.config_generation),
                    policy_revision: None,
                    changes: vec![Change {
                        key: key(&id)?,
                        expected_revision: None,
                        content: serde_json::to_string(&record)?,
                    }],
                })
            })
            .await
    }

    fn decision_preview(
        &self,
        context: &ReadContext,
        request: &str,
        decision: &ReviewDecision,
        policy: &Policy,
    ) -> Result<ReviewPreview> {
        reason(&decision.reason)?;
        self.permission(context, None)?;
        let audit_key = decision_key(request)?;
        let input_sha256 = decision_digest(context, decision)?;
        if let Some(receipt) = self.storage.receipt(request)? {
            let audit: DecisionAudit = serde_json::from_str(
                &self
                    .storage
                    .revision(&audit_key, receipt.commit_seq)?
                    .ok_or_else(|| error("IDEMPOTENCY_CONFLICT"))?
                    .content,
            )?;
            if audit.input_sha256 != input_sha256 {
                return Err(error("IDEMPOTENCY_CONFLICT"));
            }
            self.permission(context, Some(&audit.preview.before.record.draft.scope))?;
            return Ok(audit.preview);
        }
        let revision = self
            .storage
            .current(&key(&decision.id)?)?
            .ok_or_else(|| error("REVIEW_NOT_FOUND"))?;
        let detail = self.review_detail(&decision.id, revision)?;
        self.permission(context, Some(&detail.item.scope))?;
        if detail.item.revision != decision.expected_revision
            || !matches!(
                detail.item.state,
                ReviewState::Open | ReviewState::Investigating
            )
            || self
                .memory(&detail.item.memory_id)?
                .is_none_or(|m| m.revision != detail.proposal.revision)
        {
            return Err(error("REVISION_CONFLICT"));
        }
        let draft = decision
            .draft
            .clone()
            .unwrap_or_else(|| detail.proposal.record.draft.clone());
        let write = MemoryWrite {
            request_id: request.into(),
            memory_id: detail.item.memory_id.clone(),
            expected_revision: Some(detail.item.proposal_revision),
            draft: draft.clone(),
        };
        let ingress = Ingress::unverified(&context.principal, "review-preview", detail.item.scope)?;
        self.prepare(&write, &ingress, policy)?;
        let semantic_sha256 =
            semantic_hash(&write.memory_id, &draft, &detail.proposal.record.provenance)?;
        if decision.action == ReviewAction::Reject
            && semantic_sha256 != detail.proposal.record.semantic_sha256
        {
            return Err(error("REVIEW_HASH_MISMATCH"));
        }
        let mut preview = ReviewPreview {
            before: detail.proposal,
            after: draft,
            action: decision.action,
            reason: decision.reason.clone(),
            semantic_sha256,
            config_generation: policy.config_generation,
            preview_sha256: String::new(),
        };
        preview.preview_sha256 = hash(&serde_json::to_vec(&(
            request,
            &input_sha256,
            &policy.revision,
            &preview,
        ))?);
        Ok(preview)
    }

    pub(crate) async fn preview_decision(
        &self,
        context: &ReadContext,
        request: &str,
        decision: &ReviewDecision,
        policy: &Policy,
    ) -> Result<ReviewPreview> {
        self.storage
            .serialized_read(|| self.decision_preview(context, request, decision, policy))
            .await
    }

    /// The daemon authenticates its interactive operator executable before this boundary.
    pub(crate) async fn apply_decision(
        &self,
        context: &ReadContext,
        request: &str,
        decision: &ReviewDecision,
        digest: &str,
        binding: &str,
        policy: &Policy,
    ) -> Result<CommitReceipt> {
        self.storage
            .commit_or_reuse(|| {
                let preview = self.decision_preview(context, request, decision, policy)?;
                if preview.preview_sha256 != digest {
                    return Err(error("REVIEW_PREVIEW_CHANGED"));
                }
                if let Some(receipt) = self.storage.receipt(request)? {
                    return Ok(crate::storage::CommitPlan::Reuse(receipt));
                }
                let target = ReviewTarget {
                    memory_id: preview.before.revision.key.id.clone(),
                    proposal_revision: preview.before.revision.commit_seq,
                    semantic_sha256: preview.semantic_sha256.clone(),
                    action: decision.action,
                };
                let ingress = Ingress::from_verified_review_channel(
                    &context.principal,
                    binding,
                    preview.after.scope.clone(),
                    target,
                )?;
                // The storage writer lease fixes this sequence for source, memory, queue and audit together.
                let seq = self
                    .storage
                    .sequence()?
                    .checked_add(1)
                    .ok_or_else(|| error("SEQUENCE_EXHAUSTED"))?;
                let source = SourceDraft {
                    event_id: format!("review-operation/{}", hash(request.as_bytes())),
                    scope: preview.after.scope.clone(),
                    text: serde_json::to_string(
                        &serde_json::json!({"decision":decision,"preview_sha256":digest}),
                    )?,
                    reported_role: None,
                    event_time_ms: None,
                    observed_time_ms: i64::try_from(
                        std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .map_err(|_| error("INVALID_SYSTEM_CLOCK"))?
                            .as_millis(),
                    )
                    .map_err(|_| error("INVALID_SYSTEM_CLOCK"))?,
                };
                let source_change = source_change(&source, &ingress)?;
                let source_record: SourceRecord = serde_json::from_str(&source_change.content)?;
                let review = ReviewRequest {
                    write: MemoryWrite {
                        request_id: request.into(),
                        memory_id: preview.before.revision.key.id.clone(),
                        expected_revision: Some(preview.before.revision.commit_seq),
                        draft: preview.after.clone(),
                    },
                    semantic_sha256: preview.semantic_sha256.clone(),
                    source: SourceSpan {
                        event_id: source.event_id,
                        revision: seq,
                        content_sha256: source_record.content_sha256.clone(),
                        start: 0,
                        end: source.text.len(),
                    },
                    reason: decision.reason.clone(),
                    action: decision.action,
                };
                let memory = self.review_record(&review, &ingress, policy, Some(source_record))?;
                let mut changes = self.memory_changes(&review.write, &memory)?;
                changes.push(source_change);
                let k = key(&decision.id)?;
                let mut record: ReviewRecord = serde_json::from_str(
                    &self
                        .storage
                        .revision(&k, decision.expected_revision)?
                        .ok_or_else(|| error("REVIEW_NOT_FOUND"))?
                        .content,
                )?;
                record.state = ReviewState::Resolved;
                record.reason = decision.reason.clone();
                record.resolution = Some(request.into());
                changes.push(Change {
                    key: k,
                    expected_revision: Some(decision.expected_revision),
                    content: serde_json::to_string(&record)?,
                });
                changes.push(Change {
                    key: decision_key(request)?,
                    expected_revision: None,
                    content: serde_json::to_string(&DecisionAudit {
                        input_sha256: decision_digest(context, decision)?,
                        preview,
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

    pub async fn review_show(
        &self,
        context: &ReadContext,
        id: &str,
        at: Option<u64>,
    ) -> Result<ReviewDetail> {
        self.storage
            .serialized_read(|| {
                self.permission(context, None)?;
                let revision = self
                    .storage
                    .at(&key(id)?, at.unwrap_or(self.storage.sequence()?))?
                    .ok_or_else(|| error("REVIEW_NOT_FOUND"))?;
                let detail = self.review_detail(id, revision)?;
                self.permission(context, Some(&detail.item.scope))?;
                Ok(detail)
            })
            .await
    }

    pub async fn review_list(
        &self,
        context: &ReadContext,
        scope: &Scope,
        after: Option<&str>,
    ) -> Result<ReviewPage> {
        self.storage
            .serialized_read(|| {
                self.permission(context, Some(scope))?;
                let after = after.map(key).transpose()?;
                let keys = self.storage.head_keys(
                    &key(&"0".repeat(64))?,
                    &RecordKey {
                        kind: RecordKind::State,
                        id: format!("review/{}", "g".repeat(64)),
                    },
                    after.as_ref().map(|k| k.id.as_str()),
                    32,
                )?;
                let more = keys.len() > 32;
                let mut items = Vec::new();
                let mut next = None;
                for k in keys.into_iter().take(32) {
                    let id = &k.id[7..];
                    let revision = self
                        .storage
                        .current(&k)?
                        .ok_or_else(|| error("DOMAIN_CORRUPT"))?;
                    let detail = self.review_detail(id, revision)?;
                    if scope.contains(&detail.item.scope) {
                        items.push(detail.item);
                    }
                    next = Some(id.to_owned());
                }
                Ok(ReviewPage {
                    items,
                    next: more.then_some(next).flatten(),
                })
            })
            .await
    }

    pub async fn review_update(
        &self,
        context: &ReadContext,
        request_id: &str,
        update: &ReviewUpdate,
        config_generation: u64,
    ) -> Result<CommitReceipt> {
        self.storage
            .commit_checked(|| {
                reason(&update.reason)?;
                self.permission(context, None)?;
                let k = key(&update.id)?;
                let revision = self
                    .storage
                    .revision(&k, update.expected_revision)?
                    .ok_or_else(|| error("REVIEW_NOT_FOUND"))?;
                let detail = self.review_detail(&update.id, revision.clone())?;
                self.permission(context, Some(&detail.item.scope))?;
                if detail.item.state == update.state
                    || (matches!(
                        detail.item.state,
                        ReviewState::Resolved | ReviewState::Dismissed
                    ) && update.state != ReviewState::Open)
                {
                    return Err(error("INVALID_REVIEW_TRANSITION"));
                }
                let mut record: ReviewRecord = serde_json::from_str(&revision.content)?;
                record.state = update.state;
                record.reason = update.reason.clone();
                Ok(CommitRequest {
                    request_id: request_id.into(),
                    config_generation: self
                        .storage
                        .receipt(request_id)?
                        .map_or(config_generation, |r| r.config_generation),
                    policy_revision: None,
                    changes: vec![Change {
                        key: k,
                        expected_revision: Some(update.expected_revision),
                        content: serde_json::to_string(&record)?,
                    }],
                })
            })
            .await
    }
}
