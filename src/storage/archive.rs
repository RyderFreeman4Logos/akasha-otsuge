use std::collections::BTreeMap;
use std::fs::{self, File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::Path;

use serde::Serialize;
use sha2::{Digest, Sha256};
use surrealkv::LSMIterator;

use super::{
    Change, CommitReceipt, CommitRequest, OutboxEntry, RecordKey, Result, Storage, StorageError,
    hash, layout, read_revision, record, sequence,
};
use crate::config::ResolvedConfig;

#[derive(Serialize)]
pub struct ArchiveManifest {
    archive_version: u32,
    record_format_version: u8,
    commit_seq: u64,
    commits: u64,
    revisions: u64,
    files: BTreeMap<String, ArchiveFile>,
}

#[derive(Serialize)]
struct ArchiveFile {
    bytes: u64,
    sha256: String,
}

#[derive(Serialize)]
struct BlobRevision {
    key: RecordKey,
    commit_seq: u64,
    previous: Option<u64>,
    content_sha256: String,
    blob_offset: u64,
    blob_bytes: u64,
}

#[derive(Serialize)]
struct ArchiveCommit {
    receipt: CommitReceipt,
    recorded_at_ms: u64,
    revisions: Vec<BlobRevision>,
}

struct Sink {
    file: BufWriter<File>,
    digest: Sha256,
    bytes: u64,
}

impl Sink {
    fn new(path: &Path) -> Result<Self> {
        let mut options = OpenOptions::new();
        options.write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        Ok(Self {
            file: BufWriter::new(options.open(path)?),
            digest: Sha256::new(),
            bytes: 0,
        })
    }

    fn write(&mut self, bytes: &[u8]) -> Result<()> {
        self.bytes = self
            .bytes
            .checked_add(bytes.len() as u64)
            .ok_or_else(corrupt)?;
        self.file.write_all(bytes)?;
        self.digest.update(bytes);
        Ok(())
    }

    fn finish(mut self) -> Result<ArchiveFile> {
        self.file.flush()?;
        self.file.get_ref().sync_all()?;
        Ok(ArchiveFile {
            bytes: self.bytes,
            sha256: self
                .digest
                .finalize()
                .iter()
                .map(|b| format!("{b:02x}"))
                .collect(),
        })
    }
}

fn corrupt() -> StorageError {
    StorageError::new("STORAGE_CORRUPT")
}

impl Storage {
    /// Offline logical export: the native store lock excludes a running daemon.
    pub async fn export_archive(
        config: &ResolvedConfig,
        destination: &Path,
    ) -> Result<ArchiveManifest> {
        let root = config.config().paths.data_dir.join("authority");
        layout::directory(&root)?;
        layout::check_format(&root)?;
        let store = Self::open(config).await?;
        let result = store.write_archive(destination);
        let closed = store.close().await;
        let manifest = result?;
        closed?;
        Ok(manifest)
    }

    fn write_archive(&self, destination: &Path) -> Result<ArchiveManifest> {
        let unsafe_path = || StorageError::new("UNSAFE_STORAGE_PATH");
        let parent = destination
            .parent()
            .filter(|path| !path.as_os_str().is_empty())
            .unwrap_or_else(|| Path::new("."))
            .canonicalize()?;
        let destination = parent.join(destination.file_name().ok_or_else(unsafe_path)?);
        let source = self.root.canonicalize()?;
        if destination.starts_with(&source) || source.starts_with(&destination) {
            return Err(unsafe_path());
        }
        layout::private_dir(&destination, false)?;
        let tx = self.tree.begin()?;
        let commit_seq = sequence(tx.get(super::SEQUENCE)?)?;
        let mut commits = Sink::new(&destination.join("commits.jsonl"))?;
        let mut blobs = Sink::new(&destination.join("blobs.bin"))?;
        let mut count = 0_u64;
        let mut revision_count = 0_u64;
        let mut iter = tx.range(b"receipt/".to_vec(), b"receipt0".to_vec())?;
        iter.seek_first()?;
        while iter.valid() {
            let bytes = iter.value()?;
            if bytes.len() > super::MIB {
                return Err(corrupt());
            }
            let receipt: CommitReceipt = serde_json::from_slice(&bytes)?;
            if receipt.commit_seq == 0
                || receipt.commit_seq > commit_seq
                || receipt.revisions.is_empty()
                || receipt.revisions.len() > 64
                || iter.key().user_key() != format!("receipt/{}", receipt.request_id).as_bytes()
            {
                return Err(corrupt());
            }
            let outbox: OutboxEntry =
                record(&tx, format!("outbox/{:020}", receipt.commit_seq).as_bytes())?
                    .ok_or_else(corrupt)?;
            if outbox.commit_seq != receipt.commit_seq || outbox.revisions != receipt.revisions {
                return Err(corrupt());
            }
            let mut changes = Vec::with_capacity(receipt.revisions.len());
            let mut revisions = Vec::with_capacity(receipt.revisions.len());
            for reference in &receipt.revisions {
                if reference.commit_seq != receipt.commit_seq {
                    return Err(corrupt());
                }
                let revision = read_revision(&tx, &reference.key, reference.commit_seq)?
                    .ok_or_else(corrupt)?;
                let blob_offset = blobs.bytes;
                blobs.write(revision.content.as_bytes())?;
                revisions.push(BlobRevision {
                    key: revision.key.clone(),
                    commit_seq: revision.commit_seq,
                    previous: revision.previous,
                    content_sha256: revision.content_sha256,
                    blob_offset,
                    blob_bytes: revision.content.len() as u64,
                });
                changes.push(Change {
                    key: revision.key,
                    expected_revision: revision.previous,
                    content: revision.content,
                });
            }
            let mut request = CommitRequest {
                request_id: receipt.request_id.clone(),
                config_generation: receipt.config_generation,
                policy_revision: receipt.policy_revision.clone(),
                changes,
            };
            request.validate()?;
            request.changes.sort_by(|a, b| a.key.cmp(&b.key));
            if hash(&serde_json::to_vec(&request)?) != receipt.request_sha256 {
                return Err(corrupt());
            }
            revision_count = revision_count
                .checked_add(revisions.len() as u64)
                .ok_or_else(corrupt)?;
            let entry = ArchiveCommit {
                recorded_at_ms: sequence(
                    tx.get(format!("clock/{:020}", receipt.commit_seq).as_bytes())?,
                )?,
                receipt,
                revisions,
            };
            let mut bytes = serde_json::to_vec(&entry)?;
            if bytes.len() >= super::MIB {
                return Err(corrupt());
            }
            bytes.push(b'\n');
            commits.write(&bytes)?;
            count = count.checked_add(1).ok_or_else(corrupt)?;
            iter.next()?;
        }
        if count != commit_seq {
            return Err(corrupt());
        }
        let manifest = ArchiveManifest {
            archive_version: 1,
            record_format_version: super::FORMAT_VERSION,
            commit_seq,
            commits: count,
            revisions: revision_count,
            files: BTreeMap::from([
                ("blobs.bin".into(), blobs.finish()?),
                ("commits.jsonl".into(), commits.finish()?),
            ]),
        };
        // The marker appears only after both data files have reached durable storage.
        layout::write_new(
            &destination.join("manifest.tmp"),
            &serde_json::to_vec(&manifest)?,
        )?;
        fs::rename(
            destination.join("manifest.tmp"),
            destination.join("manifest.json"),
        )?;
        layout::sync_dir(&destination)?;
        layout::sync_dir(&parent)?;
        Ok(manifest)
    }
}
