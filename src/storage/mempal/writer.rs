use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::storage::archive::{ArchiveCommit, ArchiveManifest, BlobRevision, Sink};
use crate::storage::{
    Change, CommitReceipt, CommitRequest, RecordKey, RecordKind, Result, RevisionRef, StorageError,
    hash, layout,
};

pub(super) struct Writer {
    destination: PathBuf,
    commits: Sink,
    blobs: Sink,
    source: String,
    pub observed_at_ms: u64,
    sequence: u64,
    revisions: u64,
    pending: Vec<Change>,
    pending_bytes: usize,
    _lock: File,
    completed_manifest: Option<Vec<u8>>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    mapping_version: u32,
    source_sha256: String,
    observed_at_ms: u64,
    uid: u32,
}

fn conflict() -> StorageError {
    StorageError::new("MEMPAL_CONVERSION_CONFLICT")
}

fn private_file(path: &Path, create: bool) -> Result<File> {
    let file = OpenOptions::new()
        .read(true)
        .write(true)
        .create(create)
        .truncate(false)
        .mode(0o600)
        .custom_flags((rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32)
        .open(path)?;
    let metadata = file.metadata()?;
    if !metadata.is_file()
        || metadata.nlink() != 1
        || metadata.uid() != rustix::process::geteuid().as_raw()
        || metadata.mode() & 0o777 != 0o600
    {
        return Err(StorageError::new("UNSAFE_STORAGE_PATH"));
    }
    Ok(file)
}

fn small_file(path: &Path) -> Result<Vec<u8>> {
    let file = private_file(path, false)?;
    let mut bytes = Vec::new();
    file.take(64 * 1024 + 1).read_to_end(&mut bytes)?;
    if bytes.len() > 64 * 1024 {
        return Err(conflict());
    }
    Ok(bytes)
}

fn present(path: &Path) -> Result<bool> {
    match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_symlink() => Err(StorageError::new("UNSAFE_STORAGE_PATH")),
        Ok(_) => Ok(true),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(false),
        Err(error) => Err(error.into()),
    }
}

fn publish(root: &Path, name: &str, bytes: &[u8]) -> Result<()> {
    let temporary = root.join(format!("{name}.tmp"));
    let mut file = private_file(&temporary, true)?;
    file.set_len(0)?;
    file.write_all(bytes)?;
    file.sync_all()?;
    fs::rename(temporary, root.join(name))?;
    #[cfg(test)]
    if name == "manifest.json" {
        crate::storage::crash_tests::checkpoint("mempal-manifest-renamed");
    }
    layout::sync_dir(root)?;
    layout::sync_dir(root.parent().ok_or_else(conflict)?)
}

fn sink(path: &Path, complete: bool) -> Result<Sink> {
    let file = private_file(path, !complete)?;
    let prefix = private_file(path, false)?;
    let a = file.metadata()?;
    let b = prefix.metadata()?;
    if (a.dev(), a.ino()) != (b.dev(), b.ino()) {
        return Err(conflict());
    }
    Sink::resume(file, prefix, complete)
}

impl Writer {
    pub fn group(&mut self, build: impl Fn(u64) -> Result<Vec<Change>>) -> Result<()> {
        let next = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| StorageError::new("INVALID_STORAGE_REQUEST"))?;
        let mut changes = build(next)?;
        let mut bytes = changes
            .iter()
            .map(|change| change.content.len())
            .sum::<usize>();
        if self.pending.len() + changes.len() > 64 || self.pending_bytes + bytes > 1024 * 1024 {
            self.flush()?;
            changes = build(
                self.sequence
                    .checked_add(1)
                    .ok_or_else(|| StorageError::new("INVALID_STORAGE_REQUEST"))?,
            )?;
            bytes = changes.iter().map(|change| change.content.len()).sum();
        }
        if changes.len() > 64 || bytes > 1024 * 1024 {
            return Err(StorageError::new("INVALID_STORAGE_REQUEST"));
        }
        self.pending_bytes += bytes;
        self.pending.extend(changes);
        Ok(())
    }

    pub fn new(destination: &Path, source: &str, observed_at_ms: u64) -> Result<Self> {
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .canonicalize()?;
        let destination = parent.join(
            destination
                .file_name()
                .ok_or_else(|| StorageError::new("UNSAFE_STORAGE_PATH"))?,
        );
        match fs::symlink_metadata(&destination) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {
                layout::private_dir(&destination, false)?
            }
            Err(error) => return Err(error.into()),
            Ok(metadata) => {
                if !metadata.is_dir()
                    || metadata.uid() != rustix::process::geteuid().as_raw()
                    || metadata.mode() & 0o777 != 0o700
                {
                    return Err(StorageError::new("UNSAFE_STORAGE_PATH"));
                }
            }
        }
        let lock = private_file(&destination.join(".mempal-convert.lock"), true)?;
        lock.try_lock()
            .map_err(|_| StorageError::new("MEMPAL_CONVERSION_BUSY"))?;
        for entry in fs::read_dir(&destination)? {
            if !matches!(
                entry?.file_name().to_str(),
                Some(
                    ".mempal-convert.lock"
                        | "conversion.json"
                        | "conversion.json.tmp"
                        | "commits.jsonl"
                        | "blobs.bin"
                        | "manifest.json"
                        | "manifest.json.tmp"
                )
            ) {
                return Err(conflict());
            }
        }
        let identity_path = destination.join("conversion.json");
        let identity = if present(&identity_path)? {
            let identity: Identity =
                serde_json::from_slice(&small_file(&identity_path)?).map_err(|_| conflict())?;
            if identity.mapping_version != super::MAPPING_VERSION
                || identity.source_sha256 != source
                || identity.uid != rustix::process::geteuid().as_raw()
                || identity.observed_at_ms > i64::MAX as u64
            {
                return Err(conflict());
            }
            identity
        } else {
            for name in [
                "blobs.bin",
                "commits.jsonl",
                "manifest.json",
                "manifest.json.tmp",
            ] {
                if fs::symlink_metadata(destination.join(name)).is_ok() {
                    return Err(conflict());
                }
            }
            let identity = Identity {
                mapping_version: super::MAPPING_VERSION,
                source_sha256: source.into(),
                observed_at_ms,
                uid: rustix::process::geteuid().as_raw(),
            };
            publish(
                &destination,
                "conversion.json",
                &serde_json::to_vec(&identity)?,
            )?;
            identity
        };
        let completed_manifest = present(&destination.join("manifest.json"))?
            .then(|| small_file(&destination.join("manifest.json")))
            .transpose()?;
        #[cfg(test)]
        crate::storage::crash_tests::checkpoint("mempal-identity");
        Ok(Self {
            commits: sink(
                &destination.join("commits.jsonl"),
                completed_manifest.is_some(),
            )?,
            blobs: sink(&destination.join("blobs.bin"), completed_manifest.is_some())?,
            destination,
            source: source.into(),
            observed_at_ms: identity.observed_at_ms,
            sequence: 0,
            revisions: 0,
            pending: Vec::new(),
            pending_bytes: 0,
            _lock: lock,
            completed_manifest,
        })
    }

    pub fn push(&mut self, id: String, value: &impl serde::Serialize) -> Result<()> {
        let content = serde_json::to_string(value)?;
        if self.pending.len() == 64 || self.pending_bytes + content.len() > 1024 * 1024 {
            self.flush()?;
        }
        self.pending_bytes += content.len();
        self.pending.push(Change {
            key: RecordKey {
                kind: RecordKind::Evidence,
                id,
            },
            expected_revision: None,
            content,
        });
        Ok(())
    }

    fn flush(&mut self) -> Result<()> {
        if self.pending.is_empty() {
            return Ok(());
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or_else(|| StorageError::new("INVALID_STORAGE_REQUEST"))?;
        let mut request = CommitRequest {
            request_id: format!("mempal/{}/commit/{:020}", self.source, self.sequence),
            config_generation: 1,
            policy_revision: Some("mempal-unverified-v1".into()),
            changes: std::mem::take(&mut self.pending),
        };
        request.validate()?;
        request.changes.sort_by(|a, b| a.key.cmp(&b.key));
        let request_sha256 = hash(&serde_json::to_vec(&request)?);
        let mut revisions = Vec::with_capacity(request.changes.len());
        for change in &request.changes {
            let offset = self.blobs.bytes;
            self.blobs.write(change.content.as_bytes())?;
            revisions.push(BlobRevision {
                key: change.key.clone(),
                commit_seq: self.sequence,
                previous: None,
                content_sha256: hash(change.content.as_bytes()),
                blob_offset: offset,
                blob_bytes: change.content.len() as u64,
            });
        }
        self.revisions += revisions.len() as u64;
        #[cfg(test)]
        crate::storage::crash_tests::checkpoint("mempal-partial");
        let receipt = CommitReceipt {
            request_id: request.request_id,
            request_sha256,
            commit_seq: self.sequence,
            config_generation: request.config_generation,
            policy_revision: request.policy_revision,
            revisions: revisions
                .iter()
                .map(|revision| RevisionRef {
                    key: revision.key.clone(),
                    commit_seq: revision.commit_seq,
                })
                .collect(),
        };
        let entry = ArchiveCommit {
            receipt,
            recorded_at_ms: self.observed_at_ms,
            revisions,
        };
        let mut bytes = serde_json::to_vec(&entry)?;
        bytes.push(b'\n');
        self.commits.write(&bytes)?;
        self.pending_bytes = 0;
        Ok(())
    }

    pub fn finish(mut self) -> Result<ArchiveManifest> {
        self.flush()?;
        let manifest = ArchiveManifest {
            archive_version: 1,
            record_format_version: u32::from(crate::storage::FORMAT_VERSION),
            commit_seq: self.sequence,
            commits: self.sequence,
            revisions: self.revisions,
            files: BTreeMap::from([
                ("blobs.bin".into(), self.blobs.finish()?),
                ("commits.jsonl".into(), self.commits.finish()?),
            ]),
        };
        let bytes = serde_json::to_vec(&manifest)?;
        #[cfg(test)]
        crate::storage::crash_tests::checkpoint("mempal-before-publish");
        match self.completed_manifest {
            Some(stored) if stored != bytes => return Err(conflict()),
            Some(_) => {
                layout::sync_dir(&self.destination)?;
                layout::sync_dir(self.destination.parent().ok_or_else(conflict)?)?;
            }
            None => publish(&self.destination, "manifest.json", &bytes)?,
        }
        #[cfg(test)]
        crate::storage::crash_tests::checkpoint("mempal-published");
        Ok(manifest)
    }
}
