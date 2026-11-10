use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

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
    observed_at_ms: u64,
    sequence: u64,
    revisions: u64,
    pending: Vec<Change>,
    pending_bytes: usize,
}

impl Writer {
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
        layout::private_dir(&destination, false)?;
        Ok(Self {
            commits: Sink::new(&destination.join("commits.jsonl"))?,
            blobs: Sink::new(&destination.join("blobs.bin"))?,
            destination,
            source: source.into(),
            observed_at_ms,
            sequence: 0,
            revisions: 0,
            pending: Vec::new(),
            pending_bytes: 0,
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
            policy_revision: Some("mempal-schema21-fork28/raw-v1".into()),
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
        layout::write_new(
            &self.destination.join("manifest.tmp"),
            &serde_json::to_vec(&manifest)?,
        )?;
        fs::rename(
            self.destination.join("manifest.tmp"),
            self.destination.join("manifest.json"),
        )?;
        layout::sync_dir(&self.destination)?;
        layout::sync_dir(
            self.destination
                .parent()
                .ok_or_else(|| StorageError::new("UNSAFE_STORAGE_PATH"))?,
        )?;
        Ok(manifest)
    }
}
