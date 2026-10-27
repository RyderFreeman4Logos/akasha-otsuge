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
pub struct MemoryDifference {
    pub before: Option<TimedMemory>,
    pub after: Option<TimedMemory>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct StateDifference {
    pub from: u64,
    pub to: u64,
    pub items: Vec<MemoryDifference>,
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

    pub(crate) fn permission(&self, context: &ReadContext, scope: Option<&Scope>) -> Result<u64> {
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

    pub(crate) async fn ensure_local_reader(
        &self,
        principal: &str,
        scope: &Scope,
        config: u64,
    ) -> Result<()> {
        if self.storage.current(&grant_key(principal))?.is_none() {
            self.set_read_grant(
                &format!("local-reader/{}", hash(principal.as_bytes())),
                principal,
                Some(scope),
                None,
                config,
            )
            .await?;
        }
        Ok(())
    }

    pub(crate) async fn change_local_reader(
        &self,
        request: &str,
        principal: &str,
        scope: Option<&Scope>,
        config: u64,
    ) -> Result<CommitReceipt> {
        let previous = self
            .storage
            .current(&grant_key(principal))?
            .map(|r| r.commit_seq);
        self.set_read_grant(request, principal, scope, previous, config)
            .await
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

    fn scoped_memory_at(
        &self,
        key: &RecordKey,
        scope: &Scope,
        at: u64,
    ) -> Result<Option<TimedMemory>> {
        let Some(state) = self.storage.at(key, at)? else {
            return Ok(None);
        };
        let head: AdoptionHead = serde_json::from_str(&state.content)?;
        if head.scope != *scope || scope_key(&head.scope, &head.memory_id)? != *key {
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
        Ok(Some(self.timed(memory)?))
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
                    if let Some(timed) = self.scoped_memory_at(key, scope, at)?
                        && timed.memory.record.lifecycle == Lifecycle::Active
                    {
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

    /// Read applicable confirmed rules independently of semantic or keyword scores.
    pub async fn applicable_rules(
        &self,
        context: &ReadContext,
        scope: &Scope,
        at: u64,
        now_ms: i64,
        limit: usize,
    ) -> Result<(Vec<TimedMemory>, bool)> {
        self.permission(context, Some(scope))?;
        let mut scopes = Vec::new();
        for mask in 0..8 {
            let ancestor = Scope {
                user: scope.user.clone(),
                project: if mask & 1 == 0 {
                    scope.project.clone()
                } else {
                    None
                },
                branch_or_version: if mask & 2 == 0 {
                    scope.branch_or_version.clone()
                } else {
                    None
                },
                session: if mask & 4 == 0 {
                    scope.session.clone()
                } else {
                    None
                },
            };
            if ancestor.contains(scope) && !scopes.contains(&ancestor) {
                match self.permission(context, Some(&ancestor)) {
                    Ok(_) => scopes.push(ancestor),
                    Err(e) if e.code == "READ_DENIED" => (),
                    Err(e) => return Err(e),
                }
            }
        }
        let mut items = Vec::new();
        let mut examined = 0;
        let mut complete = true;
        for scope in scopes {
            if examined == limit {
                complete = false;
                break;
            }
            let page = self
                .state(
                    context,
                    &scope,
                    at,
                    None,
                    PageBudget {
                        limit: (limit - examined).min(64),
                        // A legal 128 KiB record also needs its bounded revision envelope.
                        max_bytes: 256 * 1024,
                    },
                )
                .await?
                .data;
            examined += page.examined;
            complete &= page.next.is_none();
            items.extend(page.items.into_iter().filter(|item| {
                item.memory.record.is_rule() && item.memory.record.draft.valid_at(now_ms)
            }));
        }
        Ok((items, complete))
    }

    pub async fn diff(
        &self,
        context: &ReadContext,
        scope: &Scope,
        from: u64,
        to: u64,
        cursor: Option<&StateCursor>,
        budget: PageBudget,
    ) -> Result<ReadResult<StateDifference>> {
        budget.validate()?;
        scope.validate()?;
        let query = hash(&serde_json::to_vec(&(1, "diff", scope, from, to))?);
        if cursor.is_some_and(|c| c.query_sha256 != query) {
            return Err(error("INVALID_CURSOR"));
        }
        self.storage
            .serialized_read(|| {
                self.permission(context, Some(scope))?;
                self.storage.check_point(from)?;
                self.storage.check_point(to)?;
                let prefix = scope_prefix(scope)?;
                let keys = self.storage.head_keys(
                    &RecordKey {
                        kind: RecordKind::State,
                        id: format!("{prefix}{}", "0".repeat(64)),
                    },
                    &RecordKey {
                        kind: RecordKind::State,
                        id: format!("{prefix}{}", "g".repeat(64)),
                    },
                    cursor.map(|c| c.after.as_str()),
                    budget.limit,
                )?;
                let mut items = Vec::new();
                let mut examined = 0;
                let mut bytes = 0;
                for key in keys.iter().take(budget.limit) {
                    let before = self.scoped_memory_at(key, scope, from)?;
                    let after = self.scoped_memory_at(key, scope, to)?;
                    if before != after {
                        let item = MemoryDifference { before, after };
                        let size = serde_json::to_vec(&item)?.len();
                        if bytes + size > budget.max_bytes {
                            if examined == 0 {
                                return Err(error("RESULT_TOO_LARGE"));
                            }
                            break;
                        }
                        bytes += size;
                        items.push(item);
                    }
                    examined += 1;
                }
                let next = (examined < keys.len()).then(|| StateCursor {
                    query_sha256: query,
                    after: keys[examined - 1].id.clone(),
                });
                Ok(ReadResult {
                    data: StateDifference {
                        from,
                        to,
                        items,
                        next,
                        examined,
                    },
                    read_seq: to,
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
