use super::*;
use crate::{
    capture::{Capture, CoverageLevel, EventRef, SourceIdentity, Visibility},
    domain::{Domain, SourceSpan},
    storage::{Storage, hash},
};
use futures::future::BoxFuture;
use std::collections::BTreeMap;

pub const RESEARCH_TOOLS: [&str; 9] = [
    "code_read",
    "code_search",
    "context_describe",
    "context_list",
    "context_read",
    "context_search",
    "delegate",
    "memory_get",
    "memory_search",
];

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum ContextRequest {
    Describe {},
    List {
        cursor: Option<String>,
        limit: usize,
    },
    Read {
        handle: String,
        start: usize,
        max_bytes: usize,
    },
    Search {
        cursor: Option<String>,
        query: String,
        limit: usize,
    },
    MemorySearch {
        query: String,
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        proposals: bool,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cursor: Option<String>,
    },
    MemoryGet {
        memory_id: String,
        revision: u64,
    },
    CodeSearch {
        paths: Vec<String>,
        query: String,
    },
    CodeRead {
        path: String,
        start: usize,
        max_bytes: usize,
    },
    Delegate {
        hint: String,
    },
}

impl ContextRequest {
    pub fn from_tool(name: &str, arguments: serde_json::Value) -> Result<Self> {
        if !RESEARCH_TOOLS.contains(&name) {
            return Err(ErrorCode::ToolDenied.into());
        }
        let mut args = arguments
            .as_object()
            .ok_or(ErrorCode::InvalidArgument)?
            .clone();
        let operation = name.strip_prefix("context_").unwrap_or(name);
        if args.insert("operation".into(), operation.into()).is_some() {
            return Err(ErrorCode::InvalidArgument.into());
        }
        serde_json::from_value(serde_json::Value::Object(args))
            .map_err(|_| ErrorCode::InvalidArgument.into())
    }
    pub fn tool(&self) -> &'static str {
        match self {
            Self::Describe {} => "context.describe",
            Self::List { .. } => "context.list",
            Self::Read { .. } => "context.read",
            Self::Search { .. } => "context.search",
            Self::MemorySearch { .. } => "memory.search",
            Self::MemoryGet { .. } => "memory.get",
            Self::CodeSearch { .. } => "code.search",
            Self::CodeRead { .. } => "code.read",
            Self::Delegate { .. } => "delegate",
        }
    }
}
#[derive(Clone, Debug, Serialize)]
pub struct ContextEvent {
    pub handle: String,
    pub reference: EventRef,
    pub revision: u64,
    pub visibility: Visibility,
}
#[derive(Clone, Debug, Serialize)]
pub struct ContextPage {
    pub events: Vec<ContextEvent>,
    pub next: Option<String>,
    pub gaps: Vec<ErrorCode>,
    pub read_seq: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct ContextText {
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub total_bytes: usize,
    pub sha256: String,
    pub truncated: bool,
    pub sources: Vec<crate::domain::SourceSpan>,
}
#[derive(Debug, Serialize)]
pub struct ContextPagination {
    pub tools: [&'static str; 2],
    pub min_limit: usize,
    pub max_limit: usize,
    pub initial_cursor: Option<&'static str>,
    pub next_cursor_field: &'static str,
}
const PAGINATION: ContextPagination = ContextPagination {
    tools: ["context_list", "context_search"],
    min_limit: 1,
    max_limit: 16,
    initial_cursor: None,
    next_cursor_field: "next",
};
#[derive(Clone, Debug, Serialize)]
pub struct ContextDescription {
    pub invocation_id: String,
    pub source: SourceIdentity,
    pub binding_level: BindingLevel,
    pub coverage: FenceCoverage,
    pub requested_cursor: u64,
    pub available_cursor: u64,
    pub read_seq: u64,
    pub pagination: &'static ContextPagination,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub investigation: Option<crate::incident::Context>,
}
#[derive(Clone, Debug, Serialize)]
#[serde(tag = "kind", content = "value", rename_all = "snake_case")]
pub enum ContextResponse {
    Description(ContextDescription),
    Page(ContextPage),
    Text(ContextText),
    Research(serde_json::Value),
    Error(AdapterError),
}

#[derive(Clone, Copy, Eq, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ReadRangeRejection {
    MaxBytesZero,
    MaxBytesAboveLimit,
    StartPastEnd,
    StartNotUtf8Boundary,
    EmptyIncidentRange,
}

#[derive(Default)]
pub(crate) struct ReadBounds {
    pub(crate) rejection: Option<ReadRangeRejection>,
}

impl ReadBounds {
    pub(crate) fn reject(&mut self, reason: ReadRangeRejection) -> AdapterError {
        self.rejection = Some(reason);
        ErrorCode::InvalidArgument.into()
    }

    pub(crate) fn check_size(&mut self, max_bytes: usize) -> Result<()> {
        match max_bytes {
            0 => Err(self.reject(ReadRangeRejection::MaxBytesZero)),
            1..=16384 => Ok(()),
            _ => Err(self.reject(ReadRangeRejection::MaxBytesAboveLimit)),
        }
    }

    pub(crate) fn end(&mut self, text: &str, start: usize, max_bytes: usize) -> Result<usize> {
        if start > text.len() {
            return Err(self.reject(ReadRangeRejection::StartPastEnd));
        }
        if !text.is_char_boundary(start) {
            return Err(self.reject(ReadRangeRejection::StartNotUtf8Boundary));
        }
        let mut end = start.saturating_add(max_bytes).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        Ok(end)
    }
}

pub trait ContextSource: Send {
    fn execute<'a>(
        &'a mut self,
        lease: &'a RunLease,
        request: ContextRequest,
    ) -> BoxFuture<'a, Result<ContextResponse>>;
}
pub struct CapturedContext<'a> {
    storage: &'a Storage,
    invocation: Invocation,
    handles: BTreeMap<String, EventRef>,
    read_sources: Vec<crate::domain::SourceSpan>,
}
impl<'a> CapturedContext<'a> {
    pub fn new(storage: &'a Storage, invocation: Invocation) -> Result<Self> {
        invocation.read_context()?;
        invocation.capture_binding()?;
        Ok(Self {
            storage,
            invocation,
            handles: BTreeMap::new(),
            read_sources: Vec::new(),
        })
    }
    pub(crate) fn read_sources(&self) -> &[crate::domain::SourceSpan] {
        &self.read_sources
    }
    pub(crate) fn new_read_sources(
        current: &[SourceSpan],
        incoming: &[SourceSpan],
    ) -> Result<Vec<SourceSpan>> {
        if current.len() > 64 {
            return Err(ErrorCode::BudgetExhausted.into());
        }
        let mut additions = Vec::new();
        for source in incoming {
            if !current.contains(source) && !additions.contains(source) {
                if current.len() + additions.len() >= 64 {
                    return Err(ErrorCode::BudgetExhausted.into());
                }
                additions.push(source.clone());
            }
        }
        Ok(additions)
    }

    pub(crate) fn confirm_read(&mut self, spans: &[SourceSpan]) -> Result<()> {
        let additions = Self::new_read_sources(&self.read_sources, spans)?;
        self.read_sources.extend(additions);
        Ok(())
    }
    fn check(&self, lease: &RunLease) -> Result<()> {
        if lease.origin() != self.invocation.id() || lease.fence() != self.invocation.fence_hash() {
            return Err(ErrorCode::ReadDenied.into());
        }
        lease.check()?;
        Domain::new(self.storage).permission(
            &self.invocation.read_context()?,
            Some(&self.invocation.diagnostic().scope),
        )?;
        Ok(())
    }
    async fn guarded<T>(&self, lease: &RunLease, read: impl FnOnce() -> Result<T>) -> Result<T> {
        self.storage
            .serialized_read(|| {
                Ok((|| {
                    self.check(lease)?;
                    read()
                })())
            })
            .await?
    }
    fn handle(&self, reference: &EventRef) -> Result<String> {
        Ok(hash(
            &serde_json::to_vec(&(
                self.invocation.id(),
                self.invocation.fence_hash(),
                reference,
            ))
            .map_err(|_| ErrorCode::ProtocolInvalid)?,
        ))
    }
    async fn page(
        &mut self,
        lease: &RunLease,
        cursor: Option<String>,
        limit: usize,
        query: Option<&str>,
    ) -> Result<ContextPage> {
        if !(PAGINATION.min_limit..=PAGINATION.max_limit).contains(&limit) {
            return Err(ErrorCode::InvalidArgument.into());
        }
        let tip = match cursor {
            Some(cursor) => Some(
                self.handles
                    .get(&cursor)
                    .ok_or(ErrorCode::ReadDenied)?
                    .clone(),
            ),
            None => self.invocation.diagnostic().tip.clone(),
        };
        lease.charge_read((limit * 2048) as u64)?;
        let (page, handles) = self
            .guarded(lease, || {
                let mut page = ContextPage {
                    events: vec![],
                    next: None,
                    gaps: vec![],
                    read_seq: self.invocation.diagnostic().capture_seq,
                };
                let mut handles = Vec::new();
                if self.invocation.coverage() != FenceCoverage::CompleteToFence {
                    page.gaps.push(ErrorCode::HistoryGap);
                }
                let Some(tip) = tip else {
                    return Ok((page, handles));
                };
                let captured = Capture::new(self.storage).lineage(
                    &self.invocation.capture_binding()?,
                    &tip,
                    page.read_seq,
                    limit,
                )?;
                if captured.missing.is_some() && page.gaps.is_empty() {
                    page.gaps.push(ErrorCode::HistoryGap);
                }
                for event in captured.events {
                    Domain::new(self.storage)
                        .permission(&self.invocation.read_context()?, Some(&event.event.scope))?;
                    let handle = self.handle(&event.event.reference)?;
                    handles.push((handle.clone(), event.event.reference.clone()));
                    if let Some(query) = query {
                        if matches!(
                            event.event.coverage,
                            CoverageLevel::Unavailable | CoverageLevel::MetadataOnly
                        ) {
                            if page.gaps.is_empty() {
                                page.gaps.push(ErrorCode::HistoryGap);
                            }
                            continue;
                        }
                        lease.charge_read(event.event.text_bytes as u64)?;
                        if !Capture::new(self.storage).text(&event)?.contains(query) {
                            continue;
                        }
                    }
                    page.events.push(ContextEvent {
                        handle,
                        reference: event.event.reference,
                        revision: event.revision,
                        visibility: event.event.metadata.visibility,
                    });
                }
                if let Some(next) = captured.next {
                    let handle = self.handle(&next)?;
                    page.next = Some(handle.clone());
                    handles.push((handle, next));
                }
                Ok((page, handles))
            })
            .await?;
        if self.handles.len()
            + handles
                .iter()
                .filter(|(key, _)| !self.handles.contains_key(key))
                .count()
            > 4096
        {
            return Err(ErrorCode::BudgetExhausted.into());
        }
        self.handles.extend(handles);
        Ok(page)
    }
    async fn text(
        &self,
        lease: &RunLease,
        handle: &str,
        start: usize,
        max_bytes: usize,
        bounds: &mut ReadBounds,
    ) -> Result<ContextText> {
        bounds.check_size(max_bytes)?;
        let reference = self.handles.get(handle).ok_or(ErrorCode::ReadDenied)?;
        self.guarded(lease, || {
            let event = Capture::new(self.storage)
                .event(
                    &self.invocation.capture_binding()?,
                    reference,
                    self.invocation.diagnostic().capture_seq,
                )?
                .ok_or(ErrorCode::HistoryGap)?;
            Domain::new(self.storage)
                .permission(&self.invocation.read_context()?, Some(&event.event.scope))?;
            if matches!(
                event.event.coverage,
                CoverageLevel::Unavailable | CoverageLevel::MetadataOnly
            ) {
                return Err(ErrorCode::HistoryGap.into());
            }
            // Capture verifies the whole immutable original before slicing it.
            lease.charge_read(event.event.text_bytes as u64)?;
            let text = Capture::new(self.storage).text(&event)?;
            let end = bounds.end(&text, start, max_bytes)?;
            let mut offset = 0;
            let sources = event
                .event
                .normalized
                .into_iter()
                .filter_map(|mut source| {
                    let length = source.end - source.start;
                    let a = start.max(offset);
                    let b = end.min(offset + length);
                    let begin = offset;
                    offset += length;
                    if a >= b {
                        return None;
                    }
                    source.start += a - begin;
                    source.end = source.start + b - a;
                    Some(source)
                })
                .collect();
            Ok(ContextText {
                text: text[start..end].into(),
                start,
                end,
                total_bytes: text.len(),
                sha256: event.event.text_sha256,
                truncated: end < text.len(),
                sources,
            })
        })
        .await
    }
    pub(crate) fn execute_with_bounds<'b>(
        &'b mut self,
        lease: &'b RunLease,
        request: ContextRequest,
        observed: Option<&'b mut ReadBounds>,
    ) -> BoxFuture<'b, Result<ContextResponse>> {
        Box::pin(async move {
            let mut unused = ReadBounds::default();
            let bounds = observed.unwrap_or(&mut unused);
            let response = match request {
                ContextRequest::Describe {} => {
                    lease.charge_read(2048)?;
                    self.guarded(lease, || {
                        let b = self.invocation.diagnostic();
                        Ok(ContextResponse::Description(ContextDescription {
                            invocation_id: b.invocation_id.clone(),
                            source: b.caller.clone(),
                            binding_level: b.binding_level(),
                            coverage: self.invocation.coverage(),
                            requested_cursor: b.requested_cursor,
                            available_cursor: b.available_cursor,
                            read_seq: b.capture_seq,
                            pagination: &PAGINATION,
                            investigation: None,
                        }))
                    })
                    .await?
                }
                ContextRequest::List { cursor, limit } => {
                    ContextResponse::Page(self.page(lease, cursor, limit, None).await?)
                }
                ContextRequest::Read {
                    handle,
                    start,
                    max_bytes,
                } => ContextResponse::Text(
                    self.text(lease, &handle, start, max_bytes, bounds).await?,
                ),
                ContextRequest::Search {
                    cursor,
                    query,
                    limit,
                } => {
                    if query.trim().is_empty() || query.chars().count() > 512 {
                        return Err(ErrorCode::InvalidArgument.into());
                    }
                    ContextResponse::Page(self.page(lease, cursor, limit, Some(&query)).await?)
                }
                _ => return Err(ErrorCode::ToolDenied.into()),
            };
            if let ContextResponse::Text(text) = &response
                && self.read_sources.len() + text.sources.len() > 64
            {
                Self::new_read_sources(&self.read_sources, &text.sources)?;
            }
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

impl ContextSource for CapturedContext<'_> {
    fn execute<'a>(
        &'a mut self,
        lease: &'a RunLease,
        request: ContextRequest,
    ) -> BoxFuture<'a, Result<ContextResponse>> {
        Box::pin(async move {
            let response = self.execute_with_bounds(lease, request, None).await?;
            if let ContextResponse::Text(text) = &response {
                self.confirm_read(&text.sources)?;
            }
            Ok(response)
        })
    }
}
