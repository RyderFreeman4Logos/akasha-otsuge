use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Read};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use surrealkv::Durability;
use tokio::sync::Mutex;

use super::archive::{ArchiveCommit, ArchiveFile, ArchiveManifest};
use super::import_publish::{Replacement, Root};
use super::{
    Change, CommitRequest, OutboxEntry, RecordKind, Result, Revision, RevisionRef, Storage,
    StorageError, hash, layout, number, read_revision,
};
use crate::config::ResolvedConfig;

const IMPORT: &[u8] = b"meta/archive_import";

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImportIdentity {
    pub(crate) archive_sha256: String,
    pub(crate) commit_seq: u64,
}

impl ImportIdentity {
    pub(super) fn valid(&self) -> bool {
        self.commit_seq <= i64::MAX as u64
            && self.archive_sha256.len() == 64
            && self
                .archive_sha256
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    }
}

fn invalid() -> StorageError {
    StorageError::new("INVALID_ARCHIVE")
}

fn regular_file(path: &Path) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(path)?;
    if !file.metadata()?.is_file() {
        return Err(invalid());
    }
    Ok(file)
}

fn prepare_stage(root: &Path, identity: &[u8]) -> Result<()> {
    match fs::symlink_metadata(root) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
            layout::private_dir(root, false)?;
            layout::write_new(&root.join("import.json"), identity)?;
            layout::sync_dir(root)?;
            layout::sync_dir(root.parent().ok_or_else(invalid)?)?;
        }
        Err(error) => return Err(error.into()),
        Ok(metadata) => {
            layout::directory(root)?;
            if metadata.uid() != rustix::process::geteuid().as_raw()
                || metadata.mode() & 0o777 != 0o700
            {
                return Err(StorageError::new("UNSAFE_STORAGE_PATH"));
            }
            let mut stored = Vec::new();
            regular_file(&root.join("import.json"))
                .map_err(|_| StorageError::new("IMPORT_CONFLICT"))?
                .take(identity.len() as u64 + 1)
                .read_to_end(&mut stored)?;
            if stored != identity {
                return Err(StorageError::new("IMPORT_CONFLICT"));
            }
        }
    }
    Ok(())
}

struct Input {
    file: BufReader<File>,
    digest: Sha256,
    bytes: u64,
}

impl Input {
    fn open(path: &Path, expected: &ArchiveFile) -> Result<Self> {
        let file = regular_file(path)?;
        if file.metadata()?.len() != expected.bytes {
            return Err(invalid());
        }
        Ok(Self {
            file: BufReader::new(file),
            digest: Sha256::new(),
            bytes: 0,
        })
    }

    fn account(&mut self, bytes: &[u8]) -> Result<()> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(invalid)?;
        self.digest.update(bytes);
        Ok(())
    }

    fn finish(mut self, expected: &ArchiveFile) -> Result<()> {
        if !self.file.fill_buf()?.is_empty()
            || self.bytes != expected.bytes
            || self
                .digest
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect::<String>()
                != expected.sha256
        {
            return Err(invalid());
        }
        Ok(())
    }
}

impl Storage {
    pub(crate) fn import_identity(&self) -> Result<Option<ImportIdentity>> {
        let Some(bytes) = self.tree.begin()?.get(IMPORT)? else {
            return Ok(None);
        };
        let identity: ImportIdentity = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if identity.commit_seq > self.sequence()? || !identity.valid() {
            return Err(invalid());
        }
        Ok(Some(identity))
    }

    pub(crate) fn locally_written(&self, revision: u64) -> Result<bool> {
        Ok(revision > self.import_identity()?.map_or(0, |i| i.commit_seq))
    }

    pub(crate) fn locally_created(&self, key: &super::RecordKey) -> Result<bool> {
        match self.import_identity()? {
            Some(identity) => Ok(self.at(key, identity.commit_seq)?.is_none()),
            None => Ok(true),
        }
    }

    pub(crate) fn runtime_token(&self, token: &str) -> Result<String> {
        Ok(match self.import_identity()? {
            Some(identity) => hash(format!("{}/{token}", identity.archive_sha256).as_bytes()),
            None => token.into(),
        })
    }

    /// Restore original revisions into a private stage, then publish without replacement.
    pub async fn import_archive(config: &ResolvedConfig, source: &Path) -> Result<ArchiveManifest> {
        layout::directory(source)?;
        let source = source.canonicalize()?;
        let mut bytes = Vec::new();
        regular_file(&source.join("manifest.json"))?
            .take(64 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 64 * 1024 {
            return Err(invalid());
        }
        let manifest: ArchiveManifest = serde_json::from_slice(&bytes).map_err(|_| invalid())?;
        if manifest.archive_version != 1
            || manifest.record_format_version != u32::from(super::FORMAT_VERSION)
        {
            return Err(StorageError::new("UNSUPPORTED_ARCHIVE_FORMAT"));
        }
        if manifest.commits != manifest.commit_seq
            || manifest.commit_seq > i64::MAX as u64
            || manifest.files.len() != 2
            || !manifest.files.contains_key("commits.jsonl")
            || !manifest.files.contains_key("blobs.bin")
            || manifest.files.values().any(|f| {
                f.sha256.len() != 64
                    || !f
                        .sha256
                        .bytes()
                        .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            })
        {
            return Err(invalid());
        }
        let identity = ImportIdentity {
            archive_sha256: hash(&bytes),
            commit_seq: manifest.commit_seq,
        };
        let data = &config.config().paths.data_dir;
        layout::private_dir(data, true)?;
        layout::directory(data)?;
        let canonical = data.canonicalize()?;
        if source.starts_with(&canonical) || canonical.starts_with(&source) {
            return Err(StorageError::new("UNSAFE_STORAGE_PATH"));
        }
        let admission = layout::Admission::acquire(data, true)?;
        let marker = Replacement::load(&admission)?;
        let published = marker
            .as_ref()
            .map(|marker| marker.published(&admission, &identity))
            .transpose()?
            .unwrap_or(false);
        let destination = admission.root().join("authority");
        let previous_root = match fs::symlink_metadata(&destination) {
            Ok(_) => Some(Root::open(&admission, "authority")?),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => return Err(e.into()),
        };
        let previous = if let Some(root) = &previous_root {
            layout::check_format(&root.path())?;
            layout::directory(&root.path().join("kv"))?;
            Some(Self::import_store(config, root)?)
        } else {
            None
        };
        let prepared = async {
            if let Some(marker) = &marker {
                let current = previous
                    .as_ref()
                    .ok_or_else(|| StorageError::new("IMPORT_CONFLICT"))?
                    .import_identity()?
                    .ok_or_else(|| StorageError::new("IMPORT_CONFLICT"))?;
                marker.validate_identity(&current, published)?;
            }
            Self::prepare_archive(
                config,
                &source,
                &manifest,
                &identity,
                &admission,
                previous.as_ref().zip(previous_root.as_ref()),
            )
            .await
        }
        .await;
        let closed = if let Some(store) = &previous {
            store.close().await
        } else {
            Ok(())
        };
        drop(previous);
        let prepared = prepared?;
        closed?;
        if let Some((root, replacement)) = prepared {
            #[cfg(test)]
            super::crash_tests::checkpoint("import-before-publish");
            root.validate(&admission, ".archive-import")?;
            if let Some(replacement) = replacement {
                replacement.publish(&admission)?;
            } else {
                rustix::fs::renameat_with(
                    &admission.directory,
                    ".archive-import",
                    &admission.directory,
                    "authority",
                    rustix::fs::RenameFlags::NOREPLACE,
                )
                .map_err(std::io::Error::from)?;
                root.validate(&admission, "authority")?;
            }
        } else if let Some(root) = &previous_root {
            root.validate(&admission, "authority")?;
        }
        if published && let Some(marker) = marker {
            marker.cleanup(&admission)?;
        }
        // Replay also finishes publication durability after a previous command failed.
        admission.sync()?;
        #[cfg(test)]
        super::crash_tests::checkpoint("import-published");
        Ok(manifest)
    }

    fn import_store(config: &ResolvedConfig, root: &Root) -> Result<Self> {
        Ok(Self {
            tree: Self::open_tree(config, &root.path())?,
            writer: Arc::new(Mutex::new(())),
            root: root.path(),
            budget_mib: config.config().runtime.memory_budget_mib,
        })
    }

    async fn prepare_archive(
        config: &ResolvedConfig,
        source: &Path,
        manifest: &ArchiveManifest,
        identity: &ImportIdentity,
        admission: &layout::Admission,
        previous: Option<(&Self, &Root)>,
    ) -> Result<Option<(Root, Option<Replacement>)>> {
        let old_identity = if let Some((store, root)) = previous {
            if store.tree.begin()?.get(super::SCHEMA)?.as_deref() != Some(&[super::FORMAT_VERSION])
            {
                return Err(StorageError::new("UNSUPPORTED_STORAGE_FORMAT"));
            }
            let old = store
                .import_identity()?
                .ok_or_else(|| StorageError::new("IMPORT_CONFLICT"))?;
            if identity.commit_seq <= old.commit_seq {
                store.read_archive(source, manifest, true, None).await?;
                root.validate(admission, "authority")?;
                return Ok(None);
            }
            if store.sequence()? != old.commit_seq {
                return Err(StorageError::new("IMPORT_CONFLICT"));
            }
            Some(old)
        } else {
            None
        };
        let encoded = serde_json::to_vec(identity)?;
        prepare_stage(&admission.root().join(".archive-import"), &encoded)?;
        let root = Root::open(admission, ".archive-import")?;
        prepare_stage(&root.path(), &encoded)?;
        root.validate(admission, ".archive-import")?;
        let store = Self::import_store(config, &root)?;
        let result = async {
            {
                let mut tx = store.tree.begin()?;
                match tx.get(IMPORT)? {
                    Some(stored) if stored == encoded => (),
                    Some(_) => return Err(StorageError::new("IMPORT_CONFLICT")),
                    None => {
                        if tx.get(super::SCHEMA)?.is_some() {
                            return Err(StorageError::new("IMPORT_CONFLICT"));
                        }
                        tx.set_durability(Durability::Immediate);
                        tx.set(IMPORT, &encoded)?;
                        tx.set(super::SCHEMA, &[super::FORMAT_VERSION])?;
                        tx.set(super::SEQUENCE, &0_u64.to_be_bytes())?;
                        tx.commit().await?;
                    }
                }
            }
            let common = previous
                .zip(old_identity.as_ref())
                .map(|((old, _), id)| (old, id.commit_seq));
            store.read_archive(source, manifest, false, common).await?;
            store.import_heads(manifest.commit_seq).await?;
            Ok(())
        }
        .await;
        let closed = store.close().await;
        drop(store);
        if closed.is_ok()
            && result.as_ref().is_err_and(|e| e.code == "INVALID_ARCHIVE")
            && Replacement::load(admission)?.is_none()
        {
            root.remove(admission, ".archive-import")?;
            admission.sync()?;
        }
        result?;
        closed?;
        if root.path().join("format.json").try_exists()? {
            layout::check_format(&root.path())?;
        } else {
            layout::write_new(&root.path().join("format.json"), layout::FORMAT)?;
        }
        layout::sync_dir(&root.path())?;
        root.validate(admission, ".archive-import")?;
        let replacement = previous.zip(old_identity).map(|((_, old), old_identity)| {
            Replacement::new(old, old_identity, &root, identity.clone())
        });
        #[cfg(test)]
        super::crash_tests::checkpoint("incremental-validated");
        Ok(Some((root, replacement)))
    }

    async fn read_archive(
        &self,
        source: &Path,
        manifest: &ArchiveManifest,
        existing: bool,
        common: Option<(&Self, u64)>,
    ) -> Result<()> {
        let mut commits = Input::open(
            &source.join("commits.jsonl"),
            &manifest.files["commits.jsonl"],
        )?;
        let mut blobs = Input::open(&source.join("blobs.bin"), &manifest.files["blobs.bin"])?;
        let mut previous_request = None;
        let mut count = 0_u64;
        let mut revision_count = 0_u64;
        loop {
            let mut line = Vec::new();
            (&mut commits.file)
                .take(super::MIB as u64 + 1)
                .read_until(b'\n', &mut line)?;
            if line.is_empty() {
                break;
            }
            if line.len() > super::MIB || line.last() != Some(&b'\n') {
                return Err(invalid());
            }
            commits.account(&line)?;
            let entry: ArchiveCommit = serde_json::from_slice(&line).map_err(|_| invalid())?;
            let seq = entry.receipt.commit_seq;
            if seq == 0
                || seq > manifest.commit_seq
                || entry.revisions.is_empty()
                || entry.revisions.len() > 64
                || previous_request
                    .as_ref()
                    .is_some_and(|id| id >= &entry.receipt.request_id)
            {
                return Err(invalid());
            }
            previous_request = Some(entry.receipt.request_id.clone());
            let mut changes = Vec::with_capacity(entry.revisions.len());
            let mut total = 0_u64;
            for r in &entry.revisions {
                total = total.checked_add(r.blob_bytes).ok_or_else(invalid)?;
                if r.commit_seq != seq
                    || r.blob_bytes > 128 * 1024
                    || total > super::MIB as u64
                    || r.blob_offset != blobs.bytes
                    || r.previous.is_some_and(|p| p == 0 || p >= seq)
                    || (r.key.kind == RecordKind::Evidence && r.previous.is_some())
                {
                    return Err(invalid());
                }
                let mut content = vec![0; r.blob_bytes as usize];
                blobs.file.read_exact(&mut content).map_err(|e| {
                    if e.kind() == std::io::ErrorKind::UnexpectedEof {
                        invalid()
                    } else {
                        e.into()
                    }
                })?;
                if hash(&content) != r.content_sha256 {
                    return Err(invalid());
                }
                blobs.account(&content)?;
                changes.push(Change {
                    key: r.key.clone(),
                    expected_revision: r.previous,
                    content: String::from_utf8(content).map_err(|_| invalid())?,
                });
            }
            let mut request = CommitRequest {
                request_id: entry.receipt.request_id.clone(),
                config_generation: entry.receipt.config_generation,
                policy_revision: entry.receipt.policy_revision.clone(),
                changes,
            };
            request.validate().map_err(|_| invalid())?;
            request.changes.sort_by(|a, b| a.key.cmp(&b.key));
            let references: Vec<_> = request
                .changes
                .iter()
                .map(|c| RevisionRef {
                    key: c.key.clone(),
                    commit_seq: seq,
                })
                .collect();
            if entry.receipt.revisions != references
                || hash(&serde_json::to_vec(&request)?) != entry.receipt.request_sha256
            {
                return Err(invalid());
            }
            if let Some((old, boundary)) = common
                && seq <= boundary
            {
                old.stage_import(&entry, &request, true)
                    .await
                    .map_err(|e| {
                        if e.code == "INVALID_ARCHIVE" {
                            StorageError::new("IMPORT_CONFLICT")
                        } else {
                            e
                        }
                    })?;
            }
            self.stage_import(&entry, &request, existing).await?;
            count = count.checked_add(1).ok_or_else(invalid)?;
            revision_count = revision_count
                .checked_add(references.len() as u64)
                .ok_or_else(invalid)?;
        }
        if count != manifest.commits || revision_count != manifest.revisions {
            return Err(invalid());
        }
        commits.finish(&manifest.files["commits.jsonl"])?;
        blobs.finish(&manifest.files["blobs.bin"])?;
        Ok(())
    }

    async fn stage_import(
        &self,
        entry: &ArchiveCommit,
        request: &CommitRequest,
        existing: bool,
    ) -> Result<()> {
        let seq = entry.receipt.commit_seq;
        let mut tx = self.tree.begin()?;
        let receipt_key = format!("receipt/{}", request.request_id);
        let receipt = serde_json::to_vec(&entry.receipt)?;
        let stored = tx.get(receipt_key.as_bytes())?;
        let resumed = stored.is_some();
        let outbox_key = format!("outbox/{seq:020}");
        let outbox = serde_json::to_vec(&OutboxEntry {
            commit_seq: seq,
            revisions: entry.receipt.revisions.clone(),
        })?;
        let clock_key = format!("clock/{seq:020}");
        let clock = entry.recorded_at_ms.to_be_bytes();
        if resumed {
            if stored.as_deref() != Some(&receipt)
                || tx.get(outbox_key.as_bytes())?.as_deref() != Some(&outbox)
                || tx.get(clock_key.as_bytes())?.as_deref() != Some(&clock)
            {
                return Err(invalid());
            }
        } else if existing || tx.get(outbox_key.as_bytes())?.is_some() {
            return Err(invalid());
        }
        for change in &request.changes {
            let revision = Revision {
                key: change.key.clone(),
                commit_seq: seq,
                previous: change.expected_revision,
                content: change.content.clone(),
                content_sha256: hash(change.content.as_bytes()),
            };
            if resumed {
                if read_revision(&tx, &change.key, seq)? != Some(revision) {
                    return Err(invalid());
                }
            } else {
                tx.set(
                    change.key.revision(seq).as_bytes(),
                    zstd::bulk::compress(&serde_json::to_vec(&revision)?, 1)?,
                )?;
            }
        }
        if !resumed {
            tx.set_durability(Durability::Immediate);
            tx.set(receipt_key.as_bytes(), &receipt)?;
            tx.set(outbox_key.as_bytes(), &outbox)?;
            tx.set(clock_key.as_bytes(), &clock)?;
            tx.commit().await?;
            #[cfg(test)]
            super::crash_tests::checkpoint("import-staged");
        }
        Ok(())
    }

    async fn import_heads(&self, last: u64) -> Result<()> {
        // Request-key order differs from commit order; rebuild timestamped heads in sequence.
        for seq in self.sequence()?.checked_add(1).ok_or_else(invalid)?..=last {
            let entry = self.outbox(seq)?.ok_or_else(invalid)?;
            let mut tx = self.tree.begin()?;
            tx.set_durability(Durability::Immediate);
            for reference in entry.revisions {
                let revision = read_revision(&tx, &reference.key, seq)?.ok_or_else(invalid)?;
                let previous = number(tx.get(reference.key.head().as_bytes())?)?;
                if revision.previous != (previous != 0).then_some(previous) {
                    return Err(invalid());
                }
                tx.set_at(reference.key.head().as_bytes(), &seq.to_be_bytes(), seq + 1)?;
            }
            tx.set(super::SEQUENCE, &seq.to_be_bytes())?;
            tx.commit().await?;
            #[cfg(test)]
            super::crash_tests::checkpoint("import-head");
        }
        Ok(())
    }
}
