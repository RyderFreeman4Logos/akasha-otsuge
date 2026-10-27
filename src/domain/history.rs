use super::*;

/// Server-authenticated identity, never decoded from a worker payload.
#[derive(Clone, Debug, Serialize)]
pub struct ReadContext {
    pub(super) principal: String,
    pub(super) binding: String,
    pub(super) run_id: String,
}

impl ReadContext {
    pub fn from_verified_binding(principal: &str, binding: &str, run_id: &str) -> Result<Self> {
        if ![principal, binding, run_id].into_iter().all(id) {
            return Err(error("INVALID_READ_CONTEXT"));
        }
        Ok(Self {
            principal: principal.into(),
            binding: binding.into(),
            run_id: run_id.into(),
        })
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RetrievalMode {
    CurrentState,
    HistoricalState,
    RevisionHistory,
    ReceiptReplay,
    HistoricalSemantic,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct ReadResult<T> {
    pub data: T,
    pub read_seq: u64,
    pub validated_at_seq: u64,
    pub retrieval_mode: RetrievalMode,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct TimedMemory {
    pub memory: MemoryView,
    pub recorded_at_ms: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(tag = "state", content = "revision", rename_all = "snake_case")]
pub enum MemoryState {
    Absent,
    Active(TimedMemory),
    Retired(TimedMemory),
}

#[derive(Clone, Copy, Debug)]
pub struct PageBudget {
    pub limit: usize,
    pub max_bytes: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct StateCursor {
    query_sha256: String,
    after: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StatePage {
    pub items: Vec<TimedMemory>,
    pub next: Option<StateCursor>,
    pub examined: usize,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct HistoryCursor {
    query_sha256: String,
    after: u64,
}

#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum HistoricalStatus {
    Candidate,
    Active,
    Rejected,
    Retired,
    Superseded,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HistoryEntry {
    pub revision: TimedMemory,
    pub status: HistoricalStatus,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct HistoryPage {
    pub items: Vec<HistoryEntry>,
    pub next: Option<HistoryCursor>,
}

impl PageBudget {
    fn validate(self) -> Result<()> {
        if self.limit == 0 || self.limit > 64 || self.max_bytes == 0 || self.max_bytes > 1024 * 1024
        {
            return Err(error("INVALID_PAGE"));
        }
        Ok(())
    }
}

fn scope_prefix(scope: &Scope) -> Result<String> {
    Ok(format!("scope/{}/", hash(&serde_json::to_vec(scope)?)))
}

pub(super) fn scope_key(scope: &Scope, memory: &str) -> Result<RecordKey> {
    Ok(RecordKey {
        kind: RecordKind::State,
        id: format!("{}{}", scope_prefix(scope)?, hash(memory.as_bytes())),
    })
}

fn grant_key(principal: &str) -> RecordKey {
    RecordKey {
        kind: RecordKind::State,
        id: format!("read-grant/{}", hash(principal.as_bytes())),
    }
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReadGrant {
    schema_version: u32,
    principal: String,
    scope: Option<Scope>,
}

impl Domain<'_> {
    /// Administrative server operation; the caller must authenticate the grant change.
    pub async fn set_read_grant(
        &self,
        request: &str,
        principal: &str,
        scope: Option<&Scope>,
        expected: Option<u64>,
        config: u64,
    ) -> Result<CommitReceipt> {
        if !id(principal) {
            return Err(error("INVALID_READ_CONTEXT"));
        }
        if let Some(scope) = scope {
            scope.validate()?;
        }
        self.storage
            .commit(&CommitRequest {
                request_id: request.into(),
                config_generation: config,
                policy_revision: None,
                changes: vec![Change {
                    key: grant_key(principal),
                    expected_revision: expected,
                    content: serde_json::to_string(&ReadGrant {
                        schema_version: 1,
                        principal: principal.into(),
                        scope: scope.cloned(),
                    })?,
                }],
            })
            .await
    }

    pub(super) fn permission(&self, context: &ReadContext, scope: Option<&Scope>) -> Result<u64> {
        let revision = self
            .storage
            .current(&grant_key(&context.principal))?
            .ok_or_else(|| error("READ_DENIED"))?;
        let grant: ReadGrant = serde_json::from_str(&revision.content)?;
        if grant.schema_version != 1
            || grant.principal != context.principal
            || grant.scope.as_ref().is_some_and(|s| s.validate().is_err())
        {
            return Err(error("DOMAIN_CORRUPT"));
        }
        let allowed = grant.scope.ok_or_else(|| error("READ_DENIED"))?;
        if scope.is_some_and(|scope| !allowed.contains(scope)) {
            return Err(error("READ_DENIED"));
        }
        Ok(revision.commit_seq)
    }

    fn timed(&self, memory: MemoryView) -> Result<TimedMemory> {
        Ok(TimedMemory {
            recorded_at_ms: self.storage.recorded_at_ms(memory.revision.commit_seq)?,
            memory,
        })
    }

    pub(super) fn resolve_adoption(&self, state: Revision, memory_id: &str) -> Result<MemoryView> {
        let head: AdoptionHead = serde_json::from_str(&state.content)?;
        let memory = self
            .revision(memory_id, state.commit_seq)?
            .ok_or_else(|| error("DOMAIN_CORRUPT"))?;
        if head.schema_version != 1
            || head.memory_id != memory_id
            || head.scope != memory.record.draft.scope
            || head.active != (memory.record.lifecycle == Lifecycle::Active)
            || state.previous != memory.record.previous_adoption
            || matches!(
                memory.record.lifecycle,
                Lifecycle::Candidate | Lifecycle::Rejected
            )
        {
            return Err(error("DOMAIN_CORRUPT"));
        }
        Ok(memory)
    }

    pub(super) fn adopted_at(&self, memory: &str, at: u64) -> Result<Option<MemoryView>> {
        self.storage
            .at(&adoption_key(memory), at)?
            .map(|state| self.resolve_adoption(state, memory))
            .transpose()
    }

    fn authorize_memory(&self, context: &ReadContext, memory: &str) -> Result<()> {
        if !id(memory) {
            return Err(error("INVALID_MEMORY"));
        }
        self.permission(context, None)?;
        let current = self.memory(memory)?;
        self.permission(context, current.as_ref().map(|v| &v.record.draft.scope))?;
        Ok(())
    }

    pub async fn get(
        &self,
        context: &ReadContext,
        memory: &str,
        at: Option<u64>,
    ) -> Result<ReadResult<MemoryState>> {
        self.storage
            .serialized_read(|| {
                self.authorize_memory(context, memory)?;
                let validated = self.storage.sequence()?;
                let read_seq = at.unwrap_or(validated);
                let data = match self.adopted_at(memory, read_seq)? {
                    None => MemoryState::Absent,
                    Some(memory) if memory.record.lifecycle == Lifecycle::Active => {
                        MemoryState::Active(self.timed(memory)?)
                    }
                    Some(memory) => MemoryState::Retired(self.timed(memory)?),
                };
                Ok(ReadResult {
                    data,
                    read_seq,
                    validated_at_seq: validated,
                    retrieval_mode: if at.is_some() {
                        RetrievalMode::HistoricalState
                    } else {
                        RetrievalMode::CurrentState
                    },
                })
            })
            .await
    }

    /// Exact scope equality; process at most `limit` memberships plus one key of lookahead.
    pub async fn state(
        &self,
        context: &ReadContext,
        scope: &Scope,
        at: u64,
        cursor: Option<&StateCursor>,
        budget: PageBudget,
    ) -> Result<ReadResult<StatePage>> {
        budget.validate()?;
        scope.validate()?;
        let query = hash(&serde_json::to_vec(&(1, "state", scope, at))?);
        if cursor.is_some_and(|c| c.query_sha256 != query) {
            return Err(error("INVALID_CURSOR"));
        }
        self.storage
            .serialized_read(|| {
                self.permission(context, Some(scope))?;
                self.storage.check_point(at)?;
                let prefix = scope_prefix(scope)?;
                let start = RecordKey {
                    kind: RecordKind::State,
                    id: format!("{prefix}{}", "0".repeat(64)),
                };
                let end = RecordKey {
                    kind: RecordKind::State,
                    id: format!("{prefix}{}", "g".repeat(64)),
                };
                let keys = self.storage.head_keys(
                    &start,
                    &end,
                    cursor.map(|c| c.after.as_str()),
                    budget.limit,
                )?;
                let mut items = Vec::new();
                let mut examined = 0;
                let mut bytes = 0;
                for key in keys.iter().take(budget.limit) {
                    if let Some(state) = self.storage.at(key, at)? {
                        let head: AdoptionHead = serde_json::from_str(&state.content)?;
                        if head.scope != *scope || scope_key(&head.scope, &head.memory_id)? != *key
                        {
                            return Err(error("DOMAIN_CORRUPT"));
                        }
                        let seq = state.commit_seq;
                        let memory = self.resolve_adoption(state, &head.memory_id)?;
                        if self
                            .storage
                            .at(&adoption_key(&head.memory_id), at)?
                            .map(|v| v.commit_seq)
                            != Some(seq)
                        {
                            return Err(error("DOMAIN_CORRUPT"));
                        }
                        if head.active {
                            let timed = self.timed(memory)?;
                            let size = serde_json::to_vec(&timed)?.len();
                            if bytes + size > budget.max_bytes {
                                if examined == 0 {
                                    return Err(error("RESULT_TOO_LARGE"));
                                }
                                break;
                            }
                            bytes += size;
                            items.push(timed);
                        }
                    }
                    examined += 1;
                }
                let next = (examined < keys.len()).then(|| StateCursor {
                    query_sha256: query,
                    after: keys[examined - 1].id.clone(),
                });
                Ok(ReadResult {
                    data: StatePage {
                        items,
                        next,
                        examined,
                    },
                    read_seq: at,
                    validated_at_seq: self.storage.sequence()?,
                    retrieval_mode: RetrievalMode::HistoricalState,
                })
            })
            .await
    }

    pub async fn history(
        &self,
        context: &ReadContext,
        memory: &str,
        range: std::ops::RangeInclusive<u64>,
        cursor: Option<&HistoryCursor>,
        budget: PageBudget,
    ) -> Result<ReadResult<HistoryPage>> {
        budget.validate()?;
        let (from, to) = (*range.start(), *range.end());
        let query = hash(&serde_json::to_vec(&(1, "history", memory, from, to))?);
        if cursor.is_some_and(|c| c.query_sha256 != query) {
            return Err(error("INVALID_CURSOR"));
        }
        self.storage
            .serialized_read(|| {
                self.authorize_memory(context, memory)?;
                let seqs = self.storage.revision_seqs(
                    &memory_key(memory),
                    from,
                    to,
                    cursor.map(|c| c.after),
                    budget.limit,
                )?;
                let adopted = self.adopted_at(memory, to)?;
                let mut items = Vec::new();
                let mut bytes = 0;
                for seq in seqs.iter().take(budget.limit) {
                    let revision = self
                        .revision(memory, *seq)?
                        .ok_or_else(|| error("DOMAIN_CORRUPT"))?;
                    let status = match revision.record.lifecycle {
                        Lifecycle::Candidate => HistoricalStatus::Candidate,
                        Lifecycle::Rejected => HistoricalStatus::Rejected,
                        Lifecycle::Retired => HistoricalStatus::Retired,
                        Lifecycle::Active
                            if adopted
                                .as_ref()
                                .is_some_and(|v| v.revision.commit_seq == *seq) =>
                        {
                            HistoricalStatus::Active
                        }
                        Lifecycle::Active => HistoricalStatus::Superseded,
                    };
                    let entry = HistoryEntry {
                        revision: self.timed(revision)?,
                        status,
                    };
                    let size = serde_json::to_vec(&entry)?.len();
                    if bytes + size > budget.max_bytes {
                        if items.is_empty() {
                            return Err(error("RESULT_TOO_LARGE"));
                        }
                        break;
                    }
                    bytes += size;
                    items.push(entry);
                }
                let next = (items.len() < seqs.len()).then(|| HistoryCursor {
                    query_sha256: query,
                    after: seqs[items.len() - 1],
                });
                Ok(ReadResult {
                    data: HistoryPage { items, next },
                    read_seq: to,
                    validated_at_seq: self.storage.sequence()?,
                    retrieval_mode: RetrievalMode::RevisionHistory,
                })
            })
            .await
    }
}
