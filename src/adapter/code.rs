use super::*;
use crate::{
    config::ConfigManager,
    domain::Domain,
    storage::{Storage, hash},
};
use rustix::fs::{Mode, OFlags, ResolveFlags};
use std::{
    collections::BTreeMap,
    fs::File,
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

const MAX_FILE_BYTES: u64 = 1024 * 1024;

#[derive(Clone, Debug, Serialize)]
pub struct CodeRead {
    pub path: String,
    pub text: String,
    pub start: usize,
    pub end: usize,
    pub total_bytes: usize,
    pub sha256: String,
    pub changed: bool,
    pub changed_during_read: bool,
    pub observed_at_unix_ms: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct CodeMatch {
    pub path: String,
    pub line: usize,
    pub start: usize,
    pub end: usize,
    pub sha256: String,
    pub changed: bool,
    pub changed_during_read: bool,
    pub observed_at_unix_ms: u64,
}
#[derive(Clone, Debug, Serialize)]
pub struct CodeSearch {
    pub matches: Vec<CodeMatch>,
    pub truncated: bool,
}
pub struct BoundWorkspace<'a, R> {
    store: &'a Storage,
    invocation: Invocation,
    config: &'a ConfigManager<R>,
    root: File,
    path: PathBuf,
    observations: BTreeMap<String, String>,
}
impl<'a, R: Send + Sync + 'static> BoundWorkspace<'a, R> {
    pub fn new(
        store: &'a Storage,
        invocation: Invocation,
        config: &'a ConfigManager<R>,
    ) -> Result<Self> {
        let path = invocation
            .diagnostic()
            .workspace
            .canonicalize()
            .map_err(|_| ErrorCode::ReadDenied)?;
        let root = File::from(
            rustix::fs::open(
                &path,
                OFlags::RDONLY | OFlags::DIRECTORY | OFlags::CLOEXEC,
                Mode::empty(),
            )
            .map_err(|_| ErrorCode::ReadDenied)?,
        );
        let result = Self {
            store,
            invocation,
            config,
            root,
            path,
            observations: BTreeMap::new(),
        };
        result.allowed()?;
        Ok(result)
    }
    fn allowed(&self) -> Result<()> {
        let named = std::fs::metadata(&self.path).map_err(|_| ErrorCode::ReadDenied)?;
        let pinned = self.root.metadata().map_err(|_| ErrorCode::ReadDenied)?;
        if named.dev() != pinned.dev()
            || named.ino() != pinned.ino()
            || !self
                .config
                .pin()
                .resolved()
                .config()
                .access
                .read_roots
                .iter()
                .filter_map(|p| p.canonicalize().ok())
                .any(|root| self.path.starts_with(root))
        {
            return Err(ErrorCode::ReadDenied.into());
        }
        Ok(())
    }
    async fn observe(&mut self, lease: &RunLease, path: &str) -> Result<CodeRead> {
        if path.len() > 4096
            || path.is_empty()
            || Path::new(path)
                .components()
                .any(|p| !matches!(p, Component::Normal(_)))
        {
            return Err(ErrorCode::ReadDenied.into());
        }
        if !self.observations.contains_key(path) && self.observations.len() >= 1024 {
            return Err(ErrorCode::BudgetExhausted.into());
        }
        let result: Result<CodeRead> = self
            .store
            .serialized_read(|| {
                Ok((|| {
                    if lease.origin() != self.invocation.id()
                        || lease.fence() != self.invocation.fence_hash()
                    {
                        return Err(ErrorCode::ReadDenied.into());
                    }
                    lease.check()?;
                    self.allowed()?;
                    Domain::new(self.store).permission(
                        &self.invocation.read_context()?,
                        Some(&self.invocation.diagnostic().scope),
                    )?;
                    let fd = rustix::fs::openat2(
                        &self.root,
                        path,
                        OFlags::RDONLY | OFlags::CLOEXEC | OFlags::NONBLOCK,
                        Mode::empty(),
                        ResolveFlags::BENEATH | ResolveFlags::NO_MAGICLINKS,
                    )
                    .map_err(|e| {
                        if e == rustix::io::Errno::NOENT {
                            ErrorCode::SourceUnavailable
                        } else {
                            ErrorCode::ReadDenied
                        }
                    })?;
                    let mut file = File::from(fd);
                    let before = file.metadata().map_err(|_| ErrorCode::SourceUnavailable)?;
                    if !before.is_file() {
                        return Err(ErrorCode::ReadDenied.into());
                    }
                    if before.len() > MAX_FILE_BYTES {
                        return Err(ErrorCode::OutputLimit.into());
                    }
                    lease.charge_read(before.len() + 1)?;
                    let mut bytes = Vec::new();
                    (&mut file)
                        .take(before.len() + 1)
                        .read_to_end(&mut bytes)
                        .map_err(|_| ErrorCode::SourceUnavailable)?;
                    let after = file.metadata().map_err(|_| ErrorCode::SourceUnavailable)?;
                    if bytes.len() as u64 != before.len() || after.len() != before.len() {
                        return Err(ErrorCode::SourceChanged.into());
                    }
                    let sha256 = hash(&bytes);
                    let text =
                        String::from_utf8(bytes).map_err(|_| ErrorCode::SourceUnavailable)?;
                    self.allowed()?;
                    lease.check()?;
                    Ok(CodeRead {
                        path: path.into(),
                        end: text.len(),
                        total_bytes: text.len(),
                        sha256: sha256.clone(),
                        text,
                        start: 0,
                        changed: self
                            .observations
                            .get(path)
                            .is_some_and(|old| old != &sha256),
                        changed_during_read: before.mtime() != after.mtime()
                            || before.mtime_nsec() != after.mtime_nsec(),
                        observed_at_unix_ms: SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .map_err(|_| ErrorCode::SourceUnavailable)?
                            .as_millis()
                            .try_into()
                            .map_err(|_| ErrorCode::SourceUnavailable)?,
                    })
                })())
            })
            .await?;
        let result = result?;
        self.observations.insert(path.into(), result.sha256.clone());
        Ok(result)
    }
    pub async fn read(
        &mut self,
        lease: &RunLease,
        path: &str,
        start: usize,
        max_bytes: usize,
    ) -> Result<CodeRead> {
        if max_bytes == 0 || max_bytes > 16 * 1024 {
            return Err(ErrorCode::InvalidArgument.into());
        }
        let mut result = self.observe(lease, path).await?;
        if start > result.text.len() || !result.text.is_char_boundary(start) {
            return Err(ErrorCode::InvalidArgument.into());
        }
        let mut end = start.saturating_add(max_bytes).min(result.text.len());
        while !result.text.is_char_boundary(end) {
            end -= 1;
        }
        result.text = result.text[start..end].into();
        result.start = start;
        result.end = end;
        charge(lease, &result)?;
        Ok(result)
    }
    pub async fn search(
        &mut self,
        lease: &RunLease,
        paths: &[String],
        query: &str,
    ) -> Result<CodeSearch> {
        if paths.is_empty()
            || paths.len() > 16
            || query.trim().is_empty()
            || query.chars().count() > 512
        {
            return Err(ErrorCode::InvalidArgument.into());
        }
        let mut result = CodeSearch {
            matches: vec![],
            truncated: false,
        };
        for path in paths {
            let file = self.observe(lease, path).await?;
            let mut offset = 0;
            for (line, text) in file.text.split_inclusive('\n').enumerate() {
                if text.contains(query) {
                    if result.matches.len() == 64 {
                        result.truncated = true;
                        break;
                    }
                    result.matches.push(CodeMatch {
                        path: path.clone(),
                        line: line + 1,
                        start: offset,
                        end: offset + text.len(),
                        sha256: file.sha256.clone(),
                        changed: file.changed,
                        changed_during_read: file.changed_during_read,
                        observed_at_unix_ms: file.observed_at_unix_ms,
                    });
                }
                offset += text.len();
            }
            if result.truncated {
                break;
            }
        }
        charge(lease, &result)?;
        Ok(result)
    }
}

fn charge(lease: &RunLease, result: &impl Serialize) -> Result<()> {
    let bytes = serde_json::to_vec(result)
        .map_err(|_| ErrorCode::ProtocolInvalid)?
        .len();
    if bytes > MAX_FRAME_BYTES {
        return Err(ErrorCode::OutputLimit.into());
    }
    lease.charge_output(bytes as u64)
}
