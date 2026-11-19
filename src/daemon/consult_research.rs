use super::*;
use crate::adapter::{
    AdapterError, BoundWorkspace, CodeRead, ContextRequest, ContextResponse, ContextSource,
    ErrorCode, MAX_FRAME_BYTES, ReadBounds, RunLease,
};
use crate::domain::{
    CandidateEvidence, Ingress, MemoryState, OriginalRange, ReviewState, SourceDraft, SourceSpan,
};
use crate::storage::RevisionRef;
use futures::future::BoxFuture;
use std::collections::BTreeMap;
#[path = "consult_diagnostics.rs"]
mod diagnostics;
use diagnostics::{Diagnostics, Stage};
#[path = "incident_context.rs"]
mod incidents;
#[path = "consult_run.rs"]
mod runs;

struct CapturedRead {
    truncated: bool,
    total_bytes: usize,
    ranges: Vec<(usize, usize)>,
}

fn covers_text(total_bytes: usize, mut ranges: Vec<(usize, usize)>) -> bool {
    if ranges.is_empty() {
        return false;
    }
    ranges.sort_unstable();
    let mut end = 0;
    for (start, next) in ranges {
        if start > end {
            return false;
        }
        end = end.max(next);
    }
    end == total_bytes
}

enum ResearchRead {
    ProposalPage {
        query: String,
        after: Option<String>,
        found: BTreeMap<(String, u64), CandidateEvidence>,
    },
    Memory {
        key: (String, u64),
        detail: Value,
        sources: Vec<SourceSpan>,
        read_seq: u64,
        truncated: bool,
    },
}

#[derive(Default)]
pub(super) struct ResearchSelection {
    pub adopted: Vec<RevisionRef>,
    pub proposals: Vec<CandidateEvidence>,
    pub details: Vec<Value>,
    pub sources: Vec<SourceSpan>,
}

pub(super) struct Research<'a> {
    service: &'a Service,
    id: &'a str,
    invocation: Invocation,
    config: Arc<crate::config::Snapshot<()>>,
    workflow: &'a Deadline,
    captured: CapturedContext<'a>,
    captured_reads: BTreeMap<String, CapturedRead>,
    workspace: Option<BoundWorkspace<'a, ()>>,
    handles: BTreeMap<String, u64>,
    proposals: BTreeMap<(String, u64), CandidateEvidence>,
    proposal_cursors: BTreeMap<String, String>,
    expanded: BTreeMap<(String, u64), Value>,
    searched: bool,
    operations: u64,
    native: NativeExecutor,
    executors: BTreeMap<String, NativeExecutor>,
    editor: Option<crate::editor::Feedback>,
    incident: Option<String>,
    incident_expected: BTreeSet<String>,
    incident_reads: BTreeMap<String, incidents::ReadEvidence>,
    budget: RunBudget,
    runs: Vec<Value>,
    diagnostics: Diagnostics,
    current_run: Option<usize>,
    pub selected: Vec<RevisionRef>,
    pub sources: Vec<SourceSpan>,
    pub code: Vec<Value>,
    pub gaps: Vec<String>,
    pub read_seq: u64,
    pub index_generation: Option<String>,
}

impl<'a> Research<'a> {
    pub fn new(
        service: &'a Service,
        id: &'a str,
        invocation: Invocation,
        config: Arc<crate::config::Snapshot<()>>,
        workflow: &'a Deadline,
        plan: Plan,
        budget: RunBudget,
    ) -> Result<Self> {
        Ok(Self {
            captured: CapturedContext::new(&service.storage, invocation.clone())?,
            captured_reads: BTreeMap::new(),
            read_seq: invocation.diagnostic().capture_seq,
            service,
            id,
            invocation,
            config,
            workflow,
            workspace: None,
            handles: plan
                .editor
                .as_ref()
                .map(|e| {
                    e.targets
                        .iter()
                        .map(|t| (t.memory_id.clone(), t.expected_revision))
                        .collect()
                })
                .unwrap_or_default(),
            proposals: BTreeMap::new(),
            proposal_cursors: BTreeMap::new(),
            expanded: BTreeMap::new(),
            searched: false,
            operations: 0,
            native: plan.native,
            executors: plan.executors,
            editor: plan.editor,
            incident: plan.incident.clone(),
            incident_expected: plan
                .incident
                .as_ref()
                .map(|_| BTreeSet::from(["incident:outcome".into()]))
                .unwrap_or_default(),
            incident_reads: BTreeMap::new(),
            budget,
            runs: vec![],
            diagnostics: Diagnostics::default(),
            current_run: None,
            selected: vec![],
            sources: vec![],
            code: vec![],
            gaps: vec![],
            index_generation: None,
        })
    }

    async fn authorize(&self, lease: &RunLease) -> Result<ReadContext> {
        if lease.origin() != self.invocation.id() || lease.fence() != self.invocation.fence_hash() {
            return Err(Error("READ_DENIED"));
        }
        lease.check()?;
        let _guard = self.service.admission.lock().await;
        let (_, job) = self.service.job(self.id)?;
        self.service.authorize_job(&job)?;
        if let Some(id) = &self.incident {
            self.service.authorize_incident_execution(id)?;
        }
        if job.cancel_requested {
            return Err(Error("CANCELLED"));
        }
        self.service.job_read_context(&job)
    }

    fn workspace(&mut self) -> Result<&mut BoundWorkspace<'a, ()>> {
        if self.workspace.is_none() {
            self.workspace = Some(BoundWorkspace::new(
                &self.service.storage,
                self.invocation.clone(),
                &self.service.config,
            )?);
        }
        Ok(self
            .workspace
            .as_mut()
            .expect("initialized bound workspace"))
    }

    fn gap(&mut self, gap: &str) {
        if !self.gaps.iter().any(|g| g == gap) && self.gaps.len() < 32 {
            self.gaps.push(gap.into());
        }
    }

    pub fn coverage_gaps(
        &self,
        answer: &crate::adapter::ResearchConclusion,
    ) -> Result<Vec<String>> {
        let incomplete = self
            .captured_reads
            .values()
            .any(|read| read.truncated && !covers_text(read.total_bytes, read.ranges.clone()));
        self.conclusion_gaps(answer, incomplete.then_some("context_read_budget"))
    }

    fn conclusion_gaps(
        &self,
        answer: &crate::adapter::ResearchConclusion,
        read_gap: Option<&str>,
    ) -> Result<Vec<String>> {
        let mut gaps = self.gaps.clone();
        if self.incident.is_some() {
            for handle in &self.incident_expected {
                if !self
                    .incident_reads
                    .get(handle)
                    .is_some_and(incidents::ReadEvidence::complete)
                {
                    let gap = if handle == "incident:outcome" {
                        "outcome_not_read"
                    } else {
                        "outcome_log_not_read"
                    };
                    if !gaps.iter().any(|g| g == gap) {
                        if gaps.len() == 32 {
                            return Err(Error("BUDGET_EXHAUSTED"));
                        }
                        gaps.push(gap.into());
                    }
                }
            }
        }
        for gap in answer
            .gaps
            .iter()
            .map(String::as_str)
            .chain(
                (self.invocation.coverage() != crate::adapter::FenceCoverage::CompleteToFence)
                    .then_some("history_gap"),
            )
            .chain((!answer.conflicts.is_empty()).then_some("unresolved_conflict"))
            .chain((!self.proposal_cursors.is_empty()).then_some("proposal_page_remaining"))
            .chain(read_gap)
        {
            if !gaps.iter().any(|g| g == gap) {
                if gaps.len() == 32 {
                    return Err(Error("BUDGET_EXHAUSTED"));
                }
                gaps.push(gap.into());
            }
        }
        Ok(gaps)
    }

    async fn memory_search(&mut self, lease: &RunLease, query: String) -> Result<Value> {
        let plan = crate::retrieval::Request::Search {
            query,
            scope: self.invocation.diagnostic().scope.clone(),
        };
        plan.validate()?;
        // Reserve the bounded returned record envelope before accessing the index.
        lease.charge_read(MAX_FRAME_BYTES as u64)?;
        let index = self.service.retrieval_index(self.id, self.workflow).await?;
        let vector = self.service.query_vector_snapshot(&self.config)?;
        let result = self
            .service
            .search(
                self.id,
                self.workflow,
                index,
                self.config.clone(),
                Some(vector.generation()),
                plan,
            )
            .await?;
        let context = self.authorize(lease).await?;
        let archived = Domain::new(&self.service.storage)
            .recall_receipt(
                &context,
                result["receipt_id"].as_str().ok_or(Error("JOB_CORRUPT"))?,
            )
            .await?
            .data;
        for reference in &archived.record.selected {
            if !self.selected.contains(reference) {
                if self.selected.len() == 32 {
                    return Err(Error("BUDGET_EXHAUSTED"));
                }
                self.selected.push(reference.clone());
            }
            self.handles
                .insert(reference.key.id.clone(), reference.commit_seq);
        }
        for gap in &archived.record.coverage.gaps {
            self.diagnostics
                .record(Stage::RecallCoverage, Some("memory.search"), gap);
            self.gap(gap);
        }
        self.read_seq = self.read_seq.max(archived.record.read_seq);
        self.index_generation = archived.record.index_generation;
        let mut output: Value = serde_json::from_str(&archived.output)?;
        output
            .as_object_mut()
            .ok_or(Error("JOB_CORRUPT"))?
            .remove("model_calls");
        Ok(output)
    }

    async fn proposal_search(
        &self,
        lease: &RunLease,
        query: &str,
        cursor: Option<&str>,
        read: &mut Option<ResearchRead>,
    ) -> Result<Value> {
        // The editor publishes through its patch checkpoint, which validates adopted targets.
        if self.editor.is_some() {
            return Err(Error("TOOL_DENIED"));
        }
        if query.trim().is_empty() || query.chars().count() > 512 {
            return Err(Error("INVALID_ARGUMENT"));
        }
        let needle = query.trim().to_lowercase();
        // A runner may continue only a page issued for this query and invocation.
        let cursor_for =
            |after: &str| hash(format!("{}:{needle}:{after}", self.invocation.id()).as_bytes());
        let after = match cursor {
            None => None,
            Some(token) => Some(
                self.proposal_cursors
                    .get(&needle)
                    .filter(|after| cursor_for(after) == token)
                    .ok_or(Error("INVALID_ARGUMENT"))?
                    .as_str(),
            ),
        };
        lease.charge_read(MAX_FRAME_BYTES as u64)?;
        let context = self.authorize(lease).await?;
        let bound = self.invocation.diagnostic();
        let domain = Domain::new(&self.service.storage);
        let witnessed = self.captured.read_sources();
        let mut hits = Vec::new();
        let mut found = BTreeMap::new();
        let mut count = self.proposals.len();
        if witnessed.is_empty() {
            return Ok(json!({"proposals":[],"next":null}));
        }
        let page = domain.review_list(&context, &bound.scope, after).await?;
        let next = page.next;
        for item in page.items {
            if item.proposal_revision > bound.capture_seq
                || !matches!(item.state, ReviewState::Open | ReviewState::Investigating)
            {
                continue;
            }
            let key = (item.memory_id.clone(), item.proposal_revision);
            if found.contains_key(&key) {
                continue;
            }
            let proposal = domain
                .revision(&item.memory_id, item.proposal_revision)?
                .ok_or(Error("DOMAIN_CORRUPT"))?;
            let draft = &proposal.record.draft;
            if draft.source_spans.is_empty()
                || !draft.source_spans.iter().all(|s| s.covered_by(witnessed))
                || !std::iter::once(&draft.claim)
                    .chain(&draft.conditions)
                    .chain(&draft.assumptions)
                    .any(|text| text.to_lowercase().contains(&needle))
            {
                continue;
            }
            let evidence = CandidateEvidence {
                review_id: item.id,
                revision: proposal.revision,
            };
            match domain
                .current_candidate(&context, &bound.scope, &evidence)
                .await
            {
                Ok(_) => {}
                Err(e) if matches!(e.code, "STALE_RECALL" | "READ_DENIED" | "SCOPE_DENIED") => {
                    continue;
                }
                Err(e) => return Err(e.into()),
            }
            if !self.proposals.contains_key(&key) {
                count += 1;
                if count > 32 {
                    return Err(Error("BUDGET_EXHAUSTED"));
                }
            }
            hits.push(
                json!({"memory_id":key.0,"revision":key.1,"authority":proposal.record.authority}),
            );
            found.insert(key, evidence);
        }
        let token = next.as_deref().map(cursor_for);
        *read = Some(ResearchRead::ProposalPage {
            query: needle,
            after: next,
            found,
        });
        Ok(json!({"proposals":hits,"next":token}))
    }

    async fn memory_get(
        &self,
        lease: &RunLease,
        memory_id: String,
        revision: u64,
        read: &mut Option<ResearchRead>,
    ) -> Result<Value> {
        let key = (memory_id.clone(), revision);
        if let Some(proposal) = self.proposals.get(&key) {
            lease.charge_read(MAX_FRAME_BYTES as u64)?;
            let context = self.authorize(lease).await?;
            let candidate = Domain::new(&self.service.storage)
                .current_candidate(&context, &self.invocation.diagnostic().scope, proposal)
                .await?;
            let draft = &candidate.record.draft;
            if !draft
                .source_spans
                .iter()
                .all(|s| s.covered_by(self.captured.read_sources()))
            {
                return Err(Error("READ_DENIED"));
            }
            let sources = CapturedContext::new_read_sources(&self.sources, &draft.source_spans)?;
            let detail = json!({
                "reference":candidate.revision,"scope":draft.scope,"origin":candidate.record.provenance.origin,
                "authority":candidate.record.authority,"lifecycle":candidate.record.lifecycle,
                "claim":draft.claim,"conditions":draft.conditions,"assumptions":draft.assumptions,
                "valid_from_ms":draft.valid_from_ms,"valid_until_ms":draft.valid_until_ms,
                "sources":draft.source_spans,
            });
            *read = Some(ResearchRead::Memory {
                key,
                detail: detail.clone(),
                sources,
                read_seq: candidate.revision.commit_seq,
                truncated: false,
            });
            return Ok(json!({"proposal":detail}));
        }
        if self.handles.get(&memory_id) != Some(&revision) {
            return Err(Error("READ_DENIED"));
        }
        lease.charge_read(MAX_FRAME_BYTES as u64)?;
        let context = self.authorize(lease).await?;
        let domain = Domain::new(&self.service.storage);
        let memory = domain.get(&context, &memory_id, None).await?;
        let MemoryState::Active(active) = &memory.data else {
            return Err(Error("SOURCE_CHANGED"));
        };
        if active.memory.revision.commit_seq != revision {
            return Err(Error("SOURCE_CHANGED"));
        }
        let spans: Vec<_> = active
            .memory
            .record
            .draft
            .source_spans
            .iter()
            .chain(active.memory.record.approval.iter().map(|a| &a.source))
            .cloned()
            .collect();
        let mut remaining = self
            .config
            .resolved()
            .config()
            .retrieval
            .max_original_chars
            .min(16 * 1024);
        let mut originals = Vec::new();
        let mut truncated = false;
        for (i, span) in spans.iter().enumerate() {
            if remaining == 0 {
                truncated = true;
                break;
            }
            lease.charge_read((span.end - span.start).min(remaining * 4) as u64)?;
            let original = domain
                .original_range(
                    &context,
                    &OriginalRange {
                        event_id: span.event_id.clone(),
                        revision: span.revision,
                        start: span.start,
                        end: span.end,
                    },
                    &format!(
                        "consult-original/{}/{}/{i}",
                        hash(self.id.as_bytes()),
                        self.operations
                    ),
                    remaining,
                    self.config.generation(),
                )
                .await?;
            if original.reference.content_sha256 != span.content_sha256 {
                return Err(Error("SOURCE_CHANGED"));
            }
            remaining -= original.output.chars().count();
            truncated |= original.truncated;
            originals.push(original);
        }
        let spans: Vec<_> = originals
            .iter()
            .map(|original| original.reference.clone())
            .collect();
        let sources = CapturedContext::new_read_sources(&self.sources, &spans)?;
        let detail = json!({
            "reference":active.memory.revision, "scope":active.memory.record.draft.scope,
            "authority":active.memory.record.authority, "conditions":active.memory.record.draft.conditions,
            "assumptions":active.memory.record.draft.assumptions,
            "valid_from_ms":active.memory.record.draft.valid_from_ms,
            "valid_until_ms":active.memory.record.draft.valid_until_ms,
        });
        *read = Some(ResearchRead::Memory {
            key,
            detail,
            sources,
            read_seq: memory.read_seq,
            truncated,
        });
        Ok(json!({"memory":memory,"originals":originals,"truncated":truncated}))
    }

    async fn record_code(
        &mut self,
        lease: &RunLease,
        read: &CodeRead,
    ) -> Result<Option<SourceSpan>> {
        if read.changed || read.changed_during_read {
            self.gap("workspace_changed");
        }
        if read.text.is_empty() {
            return Ok(None);
        }
        if self.sources.len() == 64 {
            return Err(Error("BUDGET_EXHAUSTED"));
        }
        let scope = self.invocation.diagnostic().scope.clone();
        let event_id = format!(
            "consult-code/{}",
            hash(&serde_json::to_vec(&(self.id, self.operations, read))?)
        );
        let source = SourceDraft {
            event_id: event_id.clone(),
            scope: scope.clone(),
            text: read.text.clone(),
            reported_role: None,
            event_time_ms: None,
            observed_time_ms: read
                .observed_at_unix_ms
                .try_into()
                .map_err(|_| Error("CLOCK_INVALID"))?,
        };
        let ingress = Ingress::tool("code.read", self.invocation.id(), scope)?;
        let _guard = self.service.admission.lock().await;
        let (_, job) = self.service.job(self.id)?;
        self.service.authorize_job(&job)?;
        lease.check()?;
        let receipt = Domain::new(&self.service.storage)
            .record_source(&event_id, &source, &ingress, self.config.generation())
            .await?;
        let span = SourceSpan {
            event_id,
            revision: receipt.commit_seq,
            content_sha256: hash(read.text.as_bytes()),
            start: 0,
            end: read.text.len(),
        };
        let mut observation = serde_json::to_value(read)?;
        observation
            .as_object_mut()
            .expect("code read object")
            .remove("text");
        observation["source"] = serde_json::to_value(&span)?;
        self.sources.push(span.clone());
        self.code.push(observation);
        self.read_seq = self.read_seq.max(receipt.commit_seq);
        Ok(Some(span))
    }

    async fn dispatch(
        &mut self,
        lease: &RunLease,
        request: ContextRequest,
        bounds: &mut ReadBounds,
        read: &mut Option<ResearchRead>,
    ) -> Result<ContextResponse> {
        let value = match request {
            ContextRequest::Describe {} if self.incident.is_some() => {
                let mut response = self
                    .captured
                    .execute(lease, ContextRequest::Describe {})
                    .await?;
                if let ContextResponse::Description(description) = &mut response {
                    description.investigation = Some(self.incident_description(lease).await?);
                }
                return Ok(response);
            }
            ContextRequest::Read {
                handle,
                start,
                max_bytes,
            } if handle.starts_with("incident:") => {
                return Ok(ContextResponse::Text(
                    self.incident_text(lease, &handle, start, max_bytes, bounds)
                        .await?,
                ));
            }
            ContextRequest::Delegate { hint } => {
                let args = ConsultArgs::new(Some(hint))?;
                let child = {
                    let _guard = self.service.admission.lock().await;
                    let max = self
                        .service
                        .config
                        .pin()
                        .resolved()
                        .config()
                        .runtime
                        .max_concurrent_jobs
                        .min(MAX_RUNNING);
                    if MAX_RUNNING - self.service.runner_slots.available_permits() >= max {
                        return Err(Error("DELEGATION_UNAVAILABLE"));
                    }
                    lease.delegate()?
                };
                serde_json::to_value(self.run(child, args).await?)?
            }
            ContextRequest::MemorySearch {
                query,
                proposals,
                cursor,
            } => {
                if proposals {
                    self.proposal_search(lease, &query, cursor.as_deref(), read)
                        .await?
                } else {
                    if cursor.is_some() {
                        return Err(Error("INVALID_ARGUMENT"));
                    }
                    let found = self.memory_search(lease, query).await?;
                    self.searched = true;
                    found
                }
            }
            ContextRequest::MemoryGet {
                memory_id,
                revision,
            } => self.memory_get(lease, memory_id, revision, read).await?,
            ContextRequest::CodeSearch { paths, query } => {
                let found = self.workspace()?.search(lease, &paths, &query).await?;
                self.searched = true;
                if found
                    .matches
                    .iter()
                    .any(|m| m.changed || m.changed_during_read)
                {
                    self.gap("workspace_changed");
                }
                if found.truncated {
                    self.gap("code_search_budget");
                }
                serde_json::to_value(found)?
            }
            ContextRequest::CodeRead {
                path,
                start,
                max_bytes,
            } => {
                let read = self
                    .workspace()?
                    .read(lease, &path, start, max_bytes)
                    .await?;
                let source = self.record_code(lease, &read).await?;
                let mut value = serde_json::to_value(read)?;
                value["source"] = serde_json::to_value(source)?;
                value
            }
            request => {
                let searched = matches!(request, ContextRequest::Search { .. });
                let response = self
                    .captured
                    .execute_with_bounds(lease, request, Some(bounds))
                    .await?;
                self.searched |= searched;
                return Ok(response);
            }
        };
        Ok(ContextResponse::Research(value))
    }

    fn conclusion(&mut self, text: &str) -> Result<crate::adapter::ResearchConclusion> {
        let conclusion = crate::adapter::ResearchConclusion::parse(text)?;
        if conclusion.sources.iter().any(|s| !self.sources.contains(s))
            || conclusion.memories.iter().any(|m| {
                !self
                    .expanded
                    .contains_key(&(m.memory_id.clone(), m.revision))
            })
        {
            return Err(Error("READ_DENIED"));
        }
        if conclusion.status == RecallStatus::NoHit && !self.searched {
            return Err(Error("PROTOCOL_INVALID"));
        }
        // Descendant limitations survive even when a later parent answer omits them.
        // Later reads can complete a document after a delegated answer returns.
        self.gaps = self.conclusion_gaps(&conclusion, None)?;
        for gap in &conclusion.gaps {
            self.diagnostics.record(Stage::Conclusion, None, gap);
        }
        Ok(conclusion)
    }

    pub fn selected_memories(
        &self,
        conclusion: &crate::adapter::ResearchConclusion,
    ) -> Result<ResearchSelection> {
        let mut selected = ResearchSelection::default();
        for source in &conclusion.sources {
            if !selected.sources.contains(source) {
                selected.sources.push(source.clone());
            }
        }
        let mut seen = BTreeSet::new();
        for memory in &conclusion.memories {
            let key = (memory.memory_id.clone(), memory.revision);
            if !seen.insert(key.clone()) {
                continue;
            }
            let detail = self.expanded.get(&key).ok_or(Error("READ_DENIED"))?;
            if let Some(proposal) = self.proposals.get(&key) {
                selected.proposals.push(proposal.clone());
                let spans: Vec<SourceSpan> = serde_json::from_value(detail["sources"].clone())?;
                for span in spans {
                    if !selected.sources.contains(&span) {
                        selected.sources.push(span);
                    }
                }
            } else {
                selected
                    .adopted
                    .push(serde_json::from_value(detail["reference"].clone())?);
            }
            selected.details.push(detail.clone());
        }
        Ok(selected)
    }
}

fn adapter_error(error: Error) -> AdapterError {
    AdapterError {
        code: serde_json::from_value(json!(error.0)).unwrap_or(ErrorCode::SourceUnavailable),
        source_code: Some(error.0.into()),
    }
}

impl ContextSource for Research<'_> {
    fn execute<'b>(
        &'b mut self,
        lease: &'b RunLease,
        request: ContextRequest,
    ) -> BoxFuture<'b, crate::adapter::Result<ContextResponse>> {
        Box::pin(async move {
            let tool = Some(request.tool());
            let response: crate::adapter::Result<_> = async {
                self.authorize(lease).await.map_err(adapter_error)?;
                self.operations += 1;
                let capture_handle = match &request {
                    ContextRequest::Read { handle, .. } if !handle.starts_with("incident:") => {
                        Some(handle.clone())
                    }
                    _ => None,
                };
                let mut bounds = ReadBounds::default();
                let mut read = None;
                let response = match self.dispatch(lease, request, &mut bounds, &mut read).await {
                    Ok(response) => response,
                    Err(error) => {
                        self.diagnostics.record_reason(
                            Stage::ContextDispatch,
                            tool,
                            error.0,
                            bounds.rejection,
                        );
                        self.gap(error.0);
                        ContextResponse::Error(adapter_error(error))
                    }
                };
                self.authorize(lease).await.map_err(adapter_error)?;
                let bytes = serde_json::to_vec(&response)
                    .map_err(|_| ErrorCode::ProtocolInvalid)?
                    .len();
                if bytes > MAX_FRAME_BYTES {
                    return Err(ErrorCode::OutputLimit.into());
                }
                lease.charge_output(bytes as u64)?;
                match &response {
                    ContextResponse::Page(page) => {
                        for code in &page.gaps {
                            let code = Error::from(AdapterError::from(*code)).0;
                            self.diagnostics.record(Stage::ContextPage, tool, code);
                            self.gap(code);
                        }
                    }
                    ContextResponse::Text(text) => {
                        if let Some(handle) = capture_handle {
                            let sources =
                                CapturedContext::new_read_sources(&self.sources, &text.sources)?;
                            self.captured.confirm_read(&text.sources)?;
                            self.sources.extend(sources);
                            let read =
                                self.captured_reads
                                    .entry(handle)
                                    .or_insert_with(|| CapturedRead {
                                        truncated: false,
                                        total_bytes: text.total_bytes,
                                        ranges: Vec::new(),
                                    });
                            read.truncated |= text.truncated;
                            read.ranges.push((text.start, text.end));
                        }
                    }
                    _ => (),
                }
                match read {
                    Some(ResearchRead::ProposalPage {
                        query,
                        after,
                        found,
                    }) => {
                        for (key, evidence) in found {
                            self.proposals.entry(key).or_insert(evidence);
                        }
                        if let Some(after) = after {
                            self.proposal_cursors.insert(query, after);
                        } else {
                            self.proposal_cursors.remove(&query);
                        }
                        self.searched = true;
                    }
                    Some(ResearchRead::Memory {
                        key,
                        detail,
                        sources,
                        read_seq,
                        truncated,
                    }) => {
                        self.read_seq = self.read_seq.max(read_seq);
                        self.sources.extend(sources);
                        self.expanded.insert(key, detail);
                        if truncated {
                            self.gap("original_budget");
                        }
                    }
                    None => {}
                }
                Ok(response)
            }
            .await;
            if let Err(error) = &response {
                let code = error
                    .source_code
                    .as_deref()
                    .unwrap_or_else(|| Error::from(error.clone()).0);
                self.diagnostics.record(Stage::ContextBoundary, tool, code);
                self.gap(code);
            }
            response
        })
    }
}
