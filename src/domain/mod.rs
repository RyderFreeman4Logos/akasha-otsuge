//! Typed provenance and adoption above the internal authority store.

mod history;
mod recall;
mod schema;

pub use history::*;
pub use recall::*;
pub use schema::*;

use serde::{Deserialize, Serialize};

use crate::config::ReviewMode;
use crate::storage::{
    Change, CommitReceipt, CommitRequest, RecordKey, RecordKind, Result, Revision, RevisionRef,
    Storage, StorageError, hash,
};

fn error(code: &'static str) -> StorageError {
    StorageError::new(code)
}
fn id(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256
}

impl Scope {
    fn validate(&self) -> Result<()> {
        if !id(&self.user)
            || [&self.project, &self.branch_or_version, &self.session]
                .iter()
                .any(|v| v.as_ref().is_some_and(|v| !id(v)))
            || (self.branch_or_version.is_some() && self.project.is_none())
        {
            return Err(error("INVALID_SCOPE"));
        }
        Ok(())
    }

    /// A ceiling permits only scopes retaining every one of its constraints.
    pub fn contains(&self, other: &Self) -> bool {
        self.validate().is_ok()
            && other.validate().is_ok()
            && self.user == other.user
            && [
                (&self.project, &other.project),
                (&self.branch_or_version, &other.branch_or_version),
                (&self.session, &other.session),
            ]
            .into_iter()
            .all(|(a, b)| a.is_none() || a == b)
    }

    pub fn more_specific_than(&self, other: &Self) -> bool {
        self != other && other.contains(self)
    }
}

impl MemoryRecord {
    pub fn is_rule(&self) -> bool {
        self.authority == Authority::HumanConfirmed
            && matches!(
                self.draft.kind,
                MemoryKind::Decision | MemoryKind::Preference | MemoryKind::Procedure
            )
    }
}

impl MemoryDraft {
    pub fn valid_at(&self, now_ms: i64) -> bool {
        self.valid_from_ms.is_none_or(|start| now_ms >= start)
            && self.valid_until_ms.is_none_or(|end| now_ms < end)
    }
}

/// Trusted server policy for this operation; never deserialize it from a worker request.
pub struct Policy {
    pub revision: String,
    pub config_generation: u64,
    pub mode: ReviewMode,
    pub scope: Scope,
    pub adopt_explicit_decisions: bool,
}

/// A server-established channel, deliberately absent from every input JSON schema.
#[derive(Serialize)]
pub struct Ingress {
    provenance: Provenance,
    scope: Scope,
    intent: StatementIntent,
    review: Option<ReviewTarget>,
}

impl Ingress {
    pub(crate) fn check_scope(&self, scope: &Scope) -> Result<()> {
        if self.scope.contains(scope) {
            Ok(())
        } else {
            Err(error("SCOPE_DENIED"))
        }
    }

    fn new(
        provenance: Provenance,
        scope: Scope,
        intent: StatementIntent,
        review: Option<ReviewTarget>,
    ) -> Result<Self> {
        scope.validate()?;
        provenance.validate()?;
        if review.as_ref().is_some_and(|target| {
            !id(&target.memory_id)
                || target.proposal_revision == 0
                || target.semantic_sha256.len() != 64
                || !target
                    .semantic_sha256
                    .bytes()
                    .all(|b| b.is_ascii_hexdigit())
        }) {
            return Err(error("INVALID_REVIEW"));
        }
        Ok(Self {
            provenance,
            scope,
            intent,
            review,
        })
    }

    /// The host/control adapter must already have verified a real human input and its intent.
    /// This constructor does not authenticate a host, role label, environment or same-UID process.
    pub fn from_verified_human_channel(
        actor: &str,
        binding: &str,
        scope: Scope,
        intent: StatementIntent,
    ) -> Result<Self> {
        Self::new(
            Provenance {
                actor: Actor::Human(actor.into()),
                origin: Origin::HumanStatement,
                binding: binding.into(),
                generator: None,
            },
            scope,
            intent,
            None,
        )
    }

    /// Only an already authenticated review operation may create this server context.
    pub fn from_verified_review_channel(
        actor: &str,
        binding: &str,
        scope: Scope,
        target: ReviewTarget,
    ) -> Result<Self> {
        let mut context =
            Self::from_verified_human_channel(actor, binding, scope, StatementIntent::Other)?;
        context.review = Some(target);
        Self::new(
            context.provenance,
            context.scope,
            context.intent,
            context.review,
        )
    }

    pub fn agent(
        actor: &str,
        binding: &str,
        scope: Scope,
        origin: Origin,
        generator: Generator,
    ) -> Result<Self> {
        Self::new(
            Provenance {
                actor: Actor::Agent(actor.into()),
                origin,
                binding: binding.into(),
                generator: Some(generator),
            },
            scope,
            StatementIntent::Other,
            None,
        )
    }

    pub fn tool(actor: &str, binding: &str, scope: Scope) -> Result<Self> {
        Self::new(
            Provenance {
                actor: Actor::Tool(actor.into()),
                origin: Origin::TechnicalObservation,
                binding: binding.into(),
                generator: None,
            },
            scope,
            StatementIntent::Other,
            None,
        )
    }

    pub fn unverified(actor: &str, binding: &str, scope: Scope) -> Result<Self> {
        Self::new(
            Provenance {
                actor: Actor::Unverified(actor.into()),
                origin: Origin::Unverified,
                binding: binding.into(),
                generator: None,
            },
            scope,
            StatementIntent::Other,
            None,
        )
    }
}

impl Provenance {
    fn validate(&self) -> Result<()> {
        let (actor, compatible) = match &self.actor {
            Actor::Human(v) => (
                v,
                self.origin == Origin::HumanStatement && self.generator.is_none(),
            ),
            Actor::Agent(v) => (
                v,
                matches!(self.origin, Origin::Extraction | Origin::AutoGrill)
                    && self.generator.is_some(),
            ),
            Actor::Tool(v) => (
                v,
                self.origin == Origin::TechnicalObservation && self.generator.is_none(),
            ),
            Actor::Unverified(v) => (
                v,
                self.origin == Origin::Unverified && self.generator.is_none(),
            ),
        };
        if !id(actor)
            || !id(&self.binding)
            || !compatible
            || self
                .generator
                .as_ref()
                .is_some_and(|g| !id(&g.runner) || !id(&g.model) || !id(&g.prompt_version))
        {
            return Err(error("INVALID_PROVENANCE"));
        }
        Ok(())
    }
}

impl MemoryDraft {
    pub(crate) fn validate(&self) -> Result<()> {
        self.scope.validate()?;
        if self.claim.trim().is_empty()
            || self.claim.len() > 32 * 1024
            || self.rationale.len() > 16 * 1024
            || self.conditions.len() > 32
            || self.assumptions.len() > 32
            || self
                .conditions
                .iter()
                .chain(&self.assumptions)
                .any(|v| v.trim().is_empty() || v.len() > 4096)
            || self.source_spans.is_empty()
            || self.source_spans.len() > 32
            || self
                .valid_from_ms
                .zip(self.valid_until_ms)
                .is_some_and(|(from, until)| from >= until)
        {
            return Err(error("INVALID_MEMORY"));
        }
        Ok(())
    }
}

/// Covers the logical identity, scope, conditions, all evidence and original provenance.
pub fn semantic_hash(
    memory_id: &str,
    draft: &MemoryDraft,
    provenance: &Provenance,
) -> Result<String> {
    Ok(hash(&serde_json::to_vec(&(
        1, memory_id, draft, provenance,
    ))?))
}

fn source_key(event: &str) -> RecordKey {
    RecordKey {
        kind: RecordKind::Evidence,
        id: format!("source/{}", hash(event.as_bytes())),
    }
}
fn memory_key(memory: &str) -> RecordKey {
    RecordKey {
        kind: RecordKind::Memory,
        id: memory.into(),
    }
}
fn adoption_key(memory: &str) -> RecordKey {
    RecordKey {
        kind: RecordKind::State,
        id: format!("adoption/{}", hash(memory.as_bytes())),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct AdoptionHead {
    schema_version: u32,
    memory_id: String,
    active: bool,
    scope: Scope,
}

pub struct Domain<'a> {
    storage: &'a Storage,
}

pub(crate) fn source_change(source: &SourceDraft, ingress: &Ingress) -> Result<Change> {
    if !id(&source.event_id)
        || source.text.is_empty()
        || source.text.len() > 64 * 1024
        || source.reported_role.as_ref().is_some_and(|v| !id(v))
    {
        return Err(error("INVALID_SOURCE"));
    }
    ingress.check_scope(&source.scope)?;
    let record = SourceRecord {
        schema_version: 1,
        payload: source.clone(),
        provenance: ingress.provenance.clone(),
        statement_intent: ingress.intent,
        review_target: ingress.review.clone(),
        content_sha256: hash(source.text.as_bytes()),
    };
    Ok(Change {
        key: source_key(&source.event_id),
        expected_revision: None,
        content: serde_json::to_string(&record)?,
    })
}

impl<'a> Domain<'a> {
    pub fn new(storage: &'a Storage) -> Self {
        Self { storage }
    }

    pub async fn record_source(
        &self,
        request_id: &str,
        source: &SourceDraft,
        ingress: &Ingress,
        config_generation: u64,
    ) -> Result<CommitReceipt> {
        let change = source_change(source, ingress)?;
        self.storage
            .commit(&CommitRequest {
                request_id: request_id.into(),
                config_generation,
                policy_revision: None,
                changes: vec![change],
            })
            .await
    }

    pub fn source(&self, event_id: &str, revision: u64) -> Result<SourceRecord> {
        if !id(event_id) || revision == 0 {
            return Err(error("INVALID_EVIDENCE"));
        }
        let stored = self
            .storage
            .revision(&source_key(event_id), revision)?
            .ok_or_else(|| error("INVALID_EVIDENCE"))?;
        let source: SourceRecord = serde_json::from_str(&stored.content)?;
        if source.schema_version != 1
            || source.payload.event_id != event_id
            || source.content_sha256 != hash(source.payload.text.as_bytes())
            || source.provenance.validate().is_err()
            || source.payload.scope.validate().is_err()
        {
            return Err(error("DOMAIN_CORRUPT"));
        }
        Ok(source)
    }

    fn span(&self, span: &SourceSpan) -> Result<SourceRecord> {
        let source = self.source(&span.event_id, span.revision)?;
        if span.content_sha256 != source.content_sha256
            || span.start >= span.end
            || source.payload.text.get(span.start..span.end).is_none()
        {
            return Err(error("INVALID_EVIDENCE"));
        }
        Ok(source)
    }

    fn read_memory(&self, revision: Revision) -> Result<MemoryView> {
        let record: MemoryRecord = serde_json::from_str(&revision.content)?;
        let approval_valid = match record.authority {
            Authority::HumanConfirmed => record.approval.as_ref().is_some_and(|approval| {
                approval.semantic_sha256 == record.semantic_sha256
                    && matches!(approval.reviewer, Actor::Human(_))
                    && matches!(
                        (approval.action, record.lifecycle),
                        (ReviewAction::Approve, Lifecycle::Active)
                            | (
                                ReviewAction::Reject,
                                Lifecycle::Retired | Lifecycle::Rejected
                            )
                    )
            }),
            Authority::DelegatedApproved => {
                record.approval.is_none()
                    && record.lifecycle == Lifecycle::Active
                    && matches!(record.provenance.actor, Actor::Agent(_) | Actor::Tool(_))
            }
            Authority::Candidate => {
                record.approval.is_none() && record.lifecycle == Lifecycle::Candidate
            }
        };
        if record.schema_version != 1
            || record.draft.validate().is_err()
            || record.provenance.validate().is_err()
            || record.config_generation == 0
            || !id(&record.policy_revision)
            || !record.authorization.scope.contains(&record.draft.scope)
            || !approval_valid
            || record.semantic_sha256
                != semantic_hash(&revision.key.id, &record.draft, &record.provenance)?
            || record
                .previous_adoption
                .is_some_and(|seq| seq == 0 || seq >= revision.commit_seq)
        {
            return Err(error("DOMAIN_CORRUPT"));
        }
        Ok(MemoryView {
            revision: RevisionRef {
                key: revision.key,
                commit_seq: revision.commit_seq,
            },
            previous: revision.previous,
            record,
        })
    }

    pub fn memory(&self, memory_id: &str) -> Result<Option<MemoryView>> {
        self.storage
            .current(&memory_key(memory_id))?
            .map(|r| self.read_memory(r))
            .transpose()
    }

    pub fn revision(&self, memory_id: &str, seq: u64) -> Result<Option<MemoryView>> {
        self.storage
            .revision(&memory_key(memory_id), seq)?
            .map(|r| self.read_memory(r))
            .transpose()
    }

    /// The adopted version is independent of later proposals awaiting a decision.
    pub fn active(&self, memory_id: &str) -> Result<Option<MemoryView>> {
        if !id(memory_id) {
            return Err(error("INVALID_MEMORY"));
        }
        let Some(state) = self.storage.current(&adoption_key(memory_id))? else {
            return Ok(None);
        };
        let memory = self.resolve_adoption(state, memory_id)?;
        Ok((memory.record.lifecycle == Lifecycle::Active).then_some(memory))
    }

    fn prepare(
        &self,
        write: &MemoryWrite,
        ingress: &Ingress,
        policy: &Policy,
    ) -> Result<(Option<MemoryView>, Vec<SourceRecord>)> {
        write.draft.validate()?;
        if !id(&write.request_id)
            || !id(&write.memory_id)
            || !id(&policy.revision)
            || policy.config_generation == 0
        {
            return Err(error("INVALID_MEMORY"));
        }
        if !ingress.scope.contains(&write.draft.scope) || !policy.scope.contains(&write.draft.scope)
        {
            return Err(error("SCOPE_DENIED"));
        }
        let mut sources = Vec::with_capacity(write.draft.source_spans.len());
        for span in &write.draft.source_spans {
            let source = self.span(span)?;
            if !source.payload.scope.contains(&write.draft.scope) {
                return Err(error("SCOPE_DENIED"));
            }
            sources.push(source);
        }
        let previous = write
            .expected_revision
            .map(|seq| {
                self.revision(&write.memory_id, seq)?
                    .ok_or_else(|| error("REVISION_NOT_FOUND"))
            })
            .transpose()?;
        if let Some(previous) = &previous {
            if previous.record.draft.scope != write.draft.scope {
                return Err(error("SCOPE_CHANGE_REQUIRES_NEW_MEMORY"));
            }
            if previous.record.draft.kind != write.draft.kind {
                return Err(error("MEMORY_KIND_CONFLICT"));
            }
        }
        Ok((previous, sources))
    }

    fn record(
        &self,
        write: &MemoryWrite,
        provenance: Provenance,
        policy: &Policy,
        previous: Option<&MemoryView>,
    ) -> Result<MemoryRecord> {
        let previous_adoption = previous.and_then(|p| {
            if matches!(
                p.record.lifecycle,
                Lifecycle::Candidate | Lifecycle::Rejected
            ) {
                p.record.previous_adoption
            } else {
                Some(p.revision.commit_seq)
            }
        });
        Ok(MemoryRecord {
            schema_version: 1,
            semantic_sha256: semantic_hash(&write.memory_id, &write.draft, &provenance)?,
            draft: write.draft.clone(),
            provenance,
            authority: Authority::Candidate,
            lifecycle: Lifecycle::Candidate,
            policy_revision: policy.revision.clone(),
            config_generation: policy.config_generation,
            authorization: Authorization {
                mode: policy.mode,
                scope: policy.scope.clone(),
                adopt_explicit_decisions: policy.adopt_explicit_decisions,
            },
            approval: None,
            previous_adoption,
        })
    }

    async fn persist(&self, write: &MemoryWrite, record: MemoryRecord) -> Result<CommitReceipt> {
        let mut changes = vec![Change {
            key: memory_key(&write.memory_id),
            expected_revision: write.expected_revision,
            content: serde_json::to_string(&record)?,
        }];
        if matches!(record.lifecycle, Lifecycle::Active | Lifecycle::Retired) {
            let content = serde_json::to_string(&AdoptionHead {
                schema_version: 1,
                memory_id: write.memory_id.clone(),
                active: record.lifecycle == Lifecycle::Active,
                scope: record.draft.scope.clone(),
            })?;
            for key in [
                adoption_key(&write.memory_id),
                history::scope_key(&record.draft.scope, &write.memory_id)?,
            ] {
                changes.push(Change {
                    key,
                    expected_revision: record.previous_adoption,
                    content: content.clone(),
                });
            }
        }
        self.storage
            .commit(&CommitRequest {
                request_id: write.request_id.clone(),
                config_generation: record.config_generation,
                policy_revision: Some(record.policy_revision),
                changes,
            })
            .await
    }

    pub async fn propose(
        &self,
        write: &MemoryWrite,
        ingress: &Ingress,
        policy: &Policy,
    ) -> Result<CommitReceipt> {
        let (previous, sources) = self.prepare(write, ingress, policy)?;
        let mut record =
            self.record(write, ingress.provenance.clone(), policy, previous.as_ref())?;
        let delegated = matches!(ingress.provenance.actor, Actor::Agent(_) | Actor::Tool(_))
            && match policy.mode {
                ReviewMode::Manual => false,
                ReviewMode::Automatic => true,
                ReviewMode::Policy => {
                    write.draft.kind == MemoryKind::Fact
                        && sources
                            .iter()
                            .all(|s| matches!(s.provenance.actor, Actor::Tool(_)))
                }
            };
        let tentative_taste = matches!(
            write.draft.kind,
            MemoryKind::Preference | MemoryKind::Decision
        ) && sources
            .iter()
            .any(|s| s.statement_intent == StatementIntent::Tentative);
        let adopted = record
            .previous_adoption
            .map(|seq| {
                self.revision(&write.memory_id, seq)?
                    .ok_or_else(|| error("DOMAIN_CORRUPT"))
            })
            .transpose()?;
        let human_norm = previous.iter().chain(adopted.iter()).any(|p| {
            p.record.authority == Authority::HumanConfirmed
                && matches!(
                    p.record.draft.kind,
                    MemoryKind::Preference | MemoryKind::Decision
                )
        });
        if delegated
            && !tentative_taste
            && !human_norm
            && write.draft.kind != MemoryKind::Hypothesis
        {
            record.authority = Authority::DelegatedApproved;
            record.lifecycle = Lifecycle::Active;
        }
        self.persist(write, record).await
    }

    pub async fn adopt_statement(
        &self,
        write: &MemoryWrite,
        ingress: &Ingress,
        policy: &Policy,
        source: &SourceSpan,
    ) -> Result<CommitReceipt> {
        if !matches!(ingress.provenance.actor, Actor::Human(_))
            || ingress.review.is_some()
            || ingress.intent != StatementIntent::ExplicitDecision
            || !policy.adopt_explicit_decisions
        {
            return Err(error("AUTHORITY_DENIED"));
        }
        let (previous, _) = self.prepare(write, ingress, policy)?;
        let original = self.span(source)?;
        if original.provenance != ingress.provenance
            || original.statement_intent != StatementIntent::ExplicitDecision
            || original.review_target.is_some()
            || !write.draft.source_spans.contains(source)
            || original.payload.text[source.start..source.end].trim() != write.draft.claim
            || source.start != 0
            || source.end != original.payload.text.len()
            || !write.draft.rationale.is_empty()
            || !write.draft.conditions.is_empty()
            || !write.draft.assumptions.is_empty()
            || write.draft.valid_from_ms.is_some()
            || write.draft.valid_until_ms.is_some()
            || write.draft.kind == MemoryKind::Hypothesis
        {
            return Err(error("AUTHORITY_DENIED"));
        }
        let mut record =
            self.record(write, ingress.provenance.clone(), policy, previous.as_ref())?;
        record.authority = Authority::HumanConfirmed;
        record.lifecycle = Lifecycle::Active;
        record.approval = Some(Approval {
            semantic_sha256: record.semantic_sha256.clone(),
            reviewer: ingress.provenance.actor.clone(),
            source: source.clone(),
            proposal_revision: write.expected_revision,
            reason: "explicit decision adopted under recorded policy".into(),
            action: ReviewAction::Approve,
        });
        self.persist(write, record).await
    }

    pub async fn review(
        &self,
        request: &ReviewRequest,
        ingress: &Ingress,
        policy: &Policy,
    ) -> Result<CommitReceipt> {
        let target = ReviewTarget {
            memory_id: request.write.memory_id.clone(),
            proposal_revision: request.write.expected_revision.unwrap_or(0),
            semantic_sha256: request.semantic_sha256.clone(),
            action: request.action,
        };
        if !matches!(ingress.provenance.actor, Actor::Human(_))
            || ingress.review.as_ref() != Some(&target)
        {
            return Err(error("AUTHORITY_DENIED"));
        }
        if request.reason.trim().is_empty() || request.reason.len() > 4096 {
            return Err(error("INVALID_REVIEW"));
        }
        let (previous, _) = self.prepare(&request.write, ingress, policy)?;
        let previous = previous.ok_or_else(|| error("REVISION_NOT_FOUND"))?;
        let source = self.span(&request.source)?;
        if source.provenance != ingress.provenance
            || source.review_target != ingress.review
            || !source.payload.scope.contains(&request.write.draft.scope)
        {
            return Err(error("AUTHORITY_DENIED"));
        }
        let mut record = self.record(
            &request.write,
            previous.record.provenance.clone(),
            policy,
            Some(&previous),
        )?;
        if request.semantic_sha256 != record.semantic_sha256 {
            return Err(error("REVIEW_HASH_MISMATCH"));
        }
        if request.action == ReviewAction::Reject
            && record.semantic_sha256 != previous.record.semantic_sha256
        {
            return Err(error("REVIEW_HASH_MISMATCH"));
        }
        record.authority = Authority::HumanConfirmed;
        record.lifecycle = match request.action {
            ReviewAction::Approve => Lifecycle::Active,
            ReviewAction::Reject if previous.record.lifecycle == Lifecycle::Active => {
                Lifecycle::Retired
            }
            ReviewAction::Reject => Lifecycle::Rejected,
        };
        record.approval = Some(Approval {
            semantic_sha256: record.semantic_sha256.clone(),
            reviewer: ingress.provenance.actor.clone(),
            source: request.source.clone(),
            proposal_revision: request.write.expected_revision,
            reason: request.reason.clone(),
            action: request.action,
        });
        self.persist(&request.write, record).await
    }
}
