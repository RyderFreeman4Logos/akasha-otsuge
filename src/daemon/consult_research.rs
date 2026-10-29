use super::*;
use crate::adapter::{
    AdapterError, BoundWorkspace, CodeRead, ContextRequest, ContextResponse, ContextSource,
    ErrorCode, MAX_FRAME_BYTES, RunLease,
};
use crate::domain::{Ingress, MemoryState, OriginalRange, SourceDraft, SourceSpan};
use crate::storage::RevisionRef;
use futures::future::BoxFuture;
use std::collections::BTreeMap;
#[path = "consult_run.rs"]
mod runs;

pub(super) struct Research<'a> {
    service: &'a Service,
    id: &'a str,
    invocation: Invocation,
    config: Arc<crate::config::Snapshot<()>>,
    workflow: &'a Deadline,
    captured: CapturedContext<'a>,
    workspace: Option<BoundWorkspace<'a, ()>>,
    handles: BTreeMap<String, u64>,
    operations: u64,
    native: NativeExecutor,
    budget: RunBudget,
    runs: Vec<Value>,
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
        native: NativeExecutor,
        budget: RunBudget,
    ) -> Result<Self> {
        Ok(Self {
            captured: CapturedContext::new(&service.storage, invocation.clone())?,
            read_seq: invocation.diagnostic().capture_seq,
            service,
            id,
            invocation,
            config,
            workflow,
            workspace: None,
            handles: BTreeMap::new(),
            operations: 0,
            native,
            budget,
            runs: vec![],
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

    async fn memory_get(
        &mut self,
        lease: &RunLease,
        memory_id: String,
        revision: u64,
    ) -> Result<Value> {
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
        self.read_seq = self.read_seq.max(memory.read_seq);
        if truncated {
            self.gap("original_budget");
        }
        Ok(json!({"memory":memory,"originals":originals,"truncated":truncated}))
    }

    async fn record_code(&mut self, lease: &RunLease, read: &CodeRead) -> Result<()> {
        if read.changed || read.changed_during_read {
            self.gap("workspace_changed");
        }
        if read.text.is_empty() {
            return Ok(());
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
        self.sources.push(span);
        self.code.push(observation);
        self.read_seq = self.read_seq.max(receipt.commit_seq);
        Ok(())
    }

    async fn dispatch(
        &mut self,
        lease: &RunLease,
        request: ContextRequest,
    ) -> Result<ContextResponse> {
        let value = match request {
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
                json!({"answer":self.run(child, args).await?})
            }
            ContextRequest::MemorySearch { query } => self.memory_search(lease, query).await?,
            ContextRequest::MemoryGet {
                memory_id,
                revision,
            } => self.memory_get(lease, memory_id, revision).await?,
            ContextRequest::CodeSearch { paths, query } => {
                let found = self.workspace()?.search(lease, &paths, &query).await?;
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
                self.record_code(lease, &read).await?;
                serde_json::to_value(read)?
            }
            request => {
                let response = self.captured.execute(lease, request).await?;
                for source in self.captured.read_sources() {
                    if !self.sources.contains(source) {
                        if self.sources.len() == 64 {
                            return Err(Error("BUDGET_EXHAUSTED"));
                        }
                        self.sources.push(source.clone());
                    }
                }
                return Ok(response);
            }
        };
        Ok(ContextResponse::Research(value))
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
            self.authorize(lease).await.map_err(adapter_error)?;
            self.operations += 1;
            let response = match self.dispatch(lease, request).await {
                Ok(response) => response,
                Err(error) => {
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
            Ok(response)
        })
    }
}
