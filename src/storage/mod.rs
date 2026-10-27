//! The sole authority store. Domain authorization belongs to its callers.

mod backup;
#[cfg(test)]
mod crash_tests;
mod history;
mod layout;

use std::collections::BTreeSet;
use std::fmt;
use std::sync::Arc;

use serde::{Deserialize, Serialize, de::DeserializeOwned};
use sha2::{Digest, Sha256};
use surrealkv::{
    CompressionType, Durability, Options, Transaction, Tree, TreeBuilder, VLogChecksumLevel,
    WalRecoveryMode,
};
use tokio::sync::Mutex;

use crate::config::ResolvedConfig;

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum RecordKind {
    Evidence,
    Memory,
    State,
    Job,
}

#[derive(Clone, Debug, Deserialize, Eq, Ord, PartialEq, PartialOrd, Serialize)]
#[serde(deny_unknown_fields)]
pub struct RecordKey {
    pub kind: RecordKind,
    pub id: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub key: RecordKey,
    pub expected_revision: Option<u64>,
    pub content: String,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct CommitRequest {
    pub request_id: String,
    pub config_generation: u64,
    pub policy_revision: Option<String>,
    pub changes: Vec<Change>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct RevisionRef {
    pub key: RecordKey,
    pub commit_seq: u64,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Revision {
    pub key: RecordKey,
    pub commit_seq: u64,
    pub previous: Option<u64>,
    pub content: String,
    pub content_sha256: String,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct CommitReceipt {
    pub request_id: String,
    pub request_sha256: String,
    pub commit_seq: u64,
    pub revisions: Vec<RevisionRef>,
    pub config_generation: u64,
    pub policy_revision: Option<String>,
}

#[derive(Clone, Debug, Deserialize, Eq, PartialEq, Serialize)]
pub struct OutboxEntry {
    pub commit_seq: u64,
    pub revisions: Vec<RevisionRef>,
}

pub struct StorageError {
    pub code: &'static str,
    source: Option<Box<dyn std::error::Error + Send + Sync>>,
}

impl StorageError {
    pub(crate) fn new(code: &'static str) -> Self {
        Self { code, source: None }
    }
}

impl fmt::Debug for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("StorageError")
            .field("code", &self.code)
            .finish_non_exhaustive()
    }
}

impl fmt::Display for StorageError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.code)
    }
}

impl std::error::Error for StorageError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        self.source.as_deref().map(|source| source as _)
    }
}

impl From<surrealkv::Error> for StorageError {
    fn from(source: surrealkv::Error) -> Self {
        Self {
            code: "STORAGE_ERROR",
            source: Some(Box::new(source)),
        }
    }
}

impl From<std::io::Error> for StorageError {
    fn from(source: std::io::Error) -> Self {
        Self {
            code: "STORAGE_IO",
            source: Some(Box::new(source)),
        }
    }
}

impl From<serde_json::Error> for StorageError {
    fn from(source: serde_json::Error) -> Self {
        Self {
            code: "STORAGE_CORRUPT",
            source: Some(Box::new(source)),
        }
    }
}

pub type Result<T> = std::result::Result<T, StorageError>;

const SEQUENCE: &[u8] = b"meta/commit_seq";
const SCHEMA: &[u8] = b"meta/schema_version";
pub const FORMAT_VERSION: u8 = 2;
const MIB: usize = 1024 * 1024;

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 256
}

fn check_id(id: &str) -> Result<()> {
    if valid_id(id) {
        Ok(())
    } else {
        Err(StorageError::new("INVALID_STORAGE_REQUEST"))
    }
}

fn sequence(bytes: Option<Vec<u8>>) -> Result<u64> {
    number(Some(
        bytes.ok_or_else(|| StorageError::new("STORAGE_CORRUPT"))?,
    ))
}

impl CommitRequest {
    fn validate(&self) -> Result<()> {
        let mut keys = BTreeSet::new();
        if !valid_id(&self.request_id)
            || self.config_generation == 0
            || self
                .policy_revision
                .as_ref()
                .is_some_and(|id| !valid_id(id))
            || self.changes.is_empty()
            || self.changes.len() > 64
            || self.changes.iter().any(|change| {
                !valid_id(&change.key.id)
                    || change.expected_revision == Some(0)
                    || change.content.len() > 128 * 1024
                    || !keys.insert(&change.key)
            })
            || self
                .changes
                .iter()
                .map(|change| change.content.len())
                .sum::<usize>()
                > MIB
        {
            return Err(StorageError::new("INVALID_STORAGE_REQUEST"));
        }
        Ok(())
    }
}

pub struct Storage {
    tree: Arc<Tree>,
    writer: Arc<Mutex<()>>,
    root: std::path::PathBuf,
}

impl RecordKey {
    fn encoded(&self) -> String {
        let kind = match self.kind {
            RecordKind::Evidence => "evidence",
            RecordKind::Memory => "memory",
            RecordKind::State => "state",
            RecordKind::Job => "job",
        };
        format!("{kind}/{:04x}/{}", self.id.len(), self.id)
    }

    fn head(&self) -> String {
        format!("head/{}", self.encoded())
    }

    fn revision(&self, seq: u64) -> String {
        format!("revision/{}/{seq:020}", self.encoded())
    }
}

pub(crate) fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn number(bytes: Option<Vec<u8>>) -> Result<u64> {
    match bytes {
        None => Ok(0),
        Some(bytes) => Ok(u64::from_be_bytes(
            bytes
                .try_into()
                .map_err(|_| StorageError::new("STORAGE_CORRUPT"))?,
        )),
    }
}

fn record<T: DeserializeOwned>(tx: &Transaction, key: &[u8]) -> Result<Option<T>> {
    tx.get(key)?
        .map(|bytes| serde_json::from_slice(&bytes).map_err(Into::into))
        .transpose()
}

fn read_revision(tx: &Transaction, key: &RecordKey, seq: u64) -> Result<Option<Revision>> {
    tx.get(key.revision(seq).as_bytes())?
        .map(|compressed| {
            let bytes = zstd::bulk::decompress(&compressed, MIB)
                .map_err(|_| StorageError::new("STORAGE_CORRUPT"))?;
            let revision: Revision = serde_json::from_slice(&bytes)?;
            if revision.key != *key
                || revision.commit_seq != seq
                || revision.content.len() > 128 * 1024
                || revision
                    .previous
                    .is_some_and(|previous| previous == 0 || previous >= seq)
                || hash(revision.content.as_bytes()) != revision.content_sha256
            {
                return Err(StorageError::new("STORAGE_CORRUPT"));
            }
            Ok(revision)
        })
        .transpose()
}

impl Storage {
    pub async fn open(config: &ResolvedConfig) -> Result<Self> {
        let (root, created) = layout::open_root(&config.config().paths.data_dir)?;
        let store = Self {
            tree: Self::open_tree(config, &root)?,
            writer: Arc::new(Mutex::new(())),
            root,
        };
        let result = async {
            let mut tx = store.tree.begin()?;
            if created {
                tx.set_durability(Durability::Immediate);
                tx.set(SCHEMA, &[FORMAT_VERSION])?;
                tx.set(SEQUENCE, 0_u64.to_be_bytes().as_slice())?;
                tx.commit().await?;
                layout::write_new(&store.root.join("format.json"), layout::FORMAT)?;
                layout::sync_dir(&store.root)?;
            } else if tx.get(SCHEMA)?.as_deref() != Some(&[FORMAT_VERSION]) {
                return Err(StorageError::new("UNSUPPORTED_STORAGE_FORMAT"));
            }
            store.sequence()?;
            Ok(())
        }
        .await;
        if let Err(error) = result {
            let _ = store.close().await;
            return Err(error);
        }
        Ok(store)
    }

    fn open_tree(config: &ResolvedConfig, root: &std::path::Path) -> Result<Arc<Tree>> {
        let (memtable, cache, sst) = if config.config().runtime.memory_budget_mib >= 2048 {
            (64, 128, 64)
        } else {
            (8, 32, 8)
        };
        let options = Options::new()
            .with_path(root.join("kv"))
            .with_max_memtable_size(memtable * MIB)
            .with_block_cache_capacity((cache * MIB) as u64)
            .with_target_file_size((sst * MIB) as u64)
            .with_compression_per_level(vec![CompressionType::ZstdCompression])
            .with_enable_vlog(true)
            .with_vlog_value_threshold(256)
            .with_vlog_max_file_size((16 * MIB) as u64)
            .with_vlog_checksum_verification(VLogChecksumLevel::Full)
            .with_memtable_stall_threshold(2)
            .with_versioning(true, 0)
            .with_wal_recovery_mode(WalRecoveryMode::AbsoluteConsistency)
            .with_flush_on_close(false);
        // Tree::drop closes its shared core even when other native Tree clones exist.
        Ok(Arc::new(TreeBuilder::with_options(options).build()?))
    }

    pub async fn commit(&self, request: &CommitRequest) -> Result<CommitReceipt> {
        request.validate()?;
        self.commit_checked(|| Ok(request.clone())).await
    }

    pub(crate) async fn serialized_read<T>(&self, read: impl FnOnce() -> Result<T>) -> Result<T> {
        let _writer = self.writer.lock().await;
        read()
    }

    pub(crate) async fn commit_checked(
        &self,
        prepare: impl FnOnce() -> Result<CommitRequest>,
    ) -> Result<CommitReceipt> {
        let writer = self.writer.clone().lock_owned().await;
        let mut request = prepare()?;
        request.validate()?;
        request.changes.sort_by(|a, b| a.key.cmp(&b.key));
        let request_hash = hash(&serde_json::to_vec(&request)?);
        let mut tx = self.tree.begin()?;
        let receipt_key = format!("receipt/{}", request.request_id);
        if let Some(bytes) = tx.get_for_update(receipt_key.as_bytes())? {
            let receipt: CommitReceipt = serde_json::from_slice(&bytes)?;
            return if receipt.request_sha256 == request_hash {
                Ok(receipt)
            } else {
                Err(StorageError::new("IDEMPOTENCY_CONFLICT"))
            };
        }
        let seq = sequence(tx.get_for_update(SEQUENCE)?)?
            .checked_add(1)
            .filter(|seq| *seq <= i64::MAX as u64)
            .ok_or_else(|| StorageError::new("SEQUENCE_EXHAUSTED"))?;
        let mut revisions = Vec::with_capacity(request.changes.len());
        tx.set_durability(Durability::Immediate);
        #[cfg(test)]
        crash_tests::checkpoint("before-stage");
        for change in &request.changes {
            let previous = number(tx.get_for_update(change.key.head().as_bytes())?)?;
            if (previous != 0).then_some(previous) != change.expected_revision {
                return Err(StorageError::new("REVISION_CONFLICT"));
            }
            if change.key.kind == RecordKind::Evidence && previous != 0 {
                return Err(StorageError::new("IMMUTABLE_EVIDENCE"));
            }
            let revision = Revision {
                key: change.key.clone(),
                commit_seq: seq,
                previous: (previous != 0).then_some(previous),
                content: change.content.clone(),
                content_sha256: hash(change.content.as_bytes()),
            };
            tx.set(
                change.key.revision(seq).as_bytes(),
                zstd::bulk::compress(&serde_json::to_vec(&revision)?, 1)?,
            )?;
            #[cfg(test)]
            crash_tests::checkpoint("partial-stage");
            tx.set_at(
                change.key.head().as_bytes(),
                seq.to_be_bytes().as_slice(),
                seq + 1,
            )?;
            revisions.push(RevisionRef {
                key: change.key.clone(),
                commit_seq: seq,
            });
        }
        let receipt = CommitReceipt {
            request_id: request.request_id.clone(),
            request_sha256: request_hash,
            commit_seq: seq,
            revisions,
            config_generation: request.config_generation,
            policy_revision: request.policy_revision.clone(),
        };
        tx.set(receipt_key.as_bytes(), serde_json::to_vec(&receipt)?)?;
        tx.set(
            format!("outbox/{seq:020}").as_bytes(),
            serde_json::to_vec(&OutboxEntry {
                commit_seq: seq,
                revisions: receipt.revisions.clone(),
            })?,
        )?;
        tx.set(SEQUENCE, seq.to_be_bytes().as_slice())?;
        let recorded_at_ms = u64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| StorageError::new("INVALID_SYSTEM_CLOCK"))?
                .as_millis(),
        )
        .map_err(|_| StorageError::new("INVALID_SYSTEM_CLOCK"))?;
        tx.set(
            format!("clock/{seq:020}").as_bytes(),
            recorded_at_ms.to_be_bytes().as_slice(),
        )?;
        let tree = self.tree.clone();
        // Once admitted, the transaction and its serialization lease outlive a cancelled caller.
        tokio::spawn(async move {
            let _tree = tree;
            let _writer = writer;
            #[cfg(test)]
            {
                use std::future::{Future, poll_fn};
                crash_tests::checkpoint("before-commit");
                let mut commit = std::pin::pin!(tx.commit());
                poll_fn(|cx| {
                    let result = commit.as_mut().poll(cx);
                    if result.is_pending() {
                        crash_tests::checkpoint("commit-pending");
                    }
                    result
                })
                .await?;
                crash_tests::checkpoint("after-commit");
            }
            #[cfg(not(test))]
            tx.commit().await?;
            Ok::<_, surrealkv::Error>(())
        })
        .await
        .map_err(|source| StorageError {
            code: "COMMIT_RESULT_UNKNOWN",
            source: Some(Box::new(source)),
        })??;
        Ok(receipt)
    }

    pub fn sequence(&self) -> Result<u64> {
        sequence(self.tree.begin()?.get(SEQUENCE)?)
    }

    pub fn receipt(&self, request_id: &str) -> Result<Option<CommitReceipt>> {
        check_id(request_id)?;
        record(
            &self.tree.begin()?,
            format!("receipt/{request_id}").as_bytes(),
        )
    }

    pub fn outbox(&self, seq: u64) -> Result<Option<OutboxEntry>> {
        record(&self.tree.begin()?, format!("outbox/{seq:020}").as_bytes())
    }

    pub fn current(&self, key: &RecordKey) -> Result<Option<Revision>> {
        check_id(&key.id)?;
        let tx = self.tree.begin()?;
        let seq = number(tx.get(key.head().as_bytes())?)?;
        if seq == 0 {
            return Ok(None);
        }
        read_revision(&tx, key, seq)?
            .map(Some)
            .ok_or_else(|| StorageError::new("STORAGE_CORRUPT"))
    }

    pub fn revision(&self, key: &RecordKey, seq: u64) -> Result<Option<Revision>> {
        check_id(&key.id)?;
        read_revision(&self.tree.begin()?, key, seq)
    }

    pub async fn close(&self) -> Result<()> {
        let _writer = self.writer.lock().await;
        Ok(self.tree.close().await?)
    }
}
