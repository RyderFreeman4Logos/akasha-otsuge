//! Rebuildable disk search; authority remains in Storage and Domain.
#[cfg(test)]
mod tests;
mod vector;
use std::sync::Arc;
pub use vector::VectorStatus;
pub(crate) use vector::model_error;
pub(crate) use vector::{activate_vector, active_vector};

use arrow_array::{Array, RecordBatch, RecordBatchIterator, StringArray, UInt64Array};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lancedb::{
    Connection, ObjectStoreRegistry, Session, Table,
    index::{
        Index as NativeIndex,
        scalar::{BTreeIndexBuilder, FtsIndexBuilder},
    },
    query::{ExecutableQuery, QueryBase, Select},
};
use serde::{Deserialize, Serialize};

use crate::config::ResolvedConfig;
use crate::domain::{Domain, Scope};
use crate::storage::{
    Change, CommitRequest, RecordKey, RecordKind, Result, Storage, StorageError, hash,
};

const GENERATION: &str = "lexical-icu-v1";
const TABLE: &str = "memory_lexical_v1";
fn error(code: &'static str) -> StorageError {
    StorageError::new(code)
}
fn index_error<E>(_: E) -> StorageError {
    error("INDEX_IO")
}
fn state_key() -> RecordKey {
    RecordKey {
        kind: RecordKind::State,
        id: "search/lexical-v1".into(),
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum Request {
    Sync,
    Search { query: String, scope: Scope },
}
impl Request {
    pub fn validate(&self) -> Result<()> {
        if let Self::Search { query, scope } = self
            && (query.trim().is_empty() || query.len() > 2048 || !scope.contains(scope))
        {
            return Err(error("INVALID_SEARCH"));
        }
        Ok(())
    }

    pub fn workflow_budget(
        &self,
        config: &ResolvedConfig,
        vector_config: &ResolvedConfig,
        pending: u64,
    ) -> Result<std::time::Duration> {
        if config.config().timeouts.workflow != "auto" {
            return Ok(config.timeouts().workflow);
        }
        let batches = if matches!(self, Self::Sync) {
            pending.div_ceil(32).max(1)
        } else {
            1
        };
        let mut budget = std::time::Duration::from_millis(
            u64::try_from(
                config
                    .timeouts()
                    .tools
                    .as_millis()
                    .checked_mul(batches as u128)
                    .ok_or_else(|| error("CONFIG_INVALID"))?,
            )
            .map_err(|_| error("CONFIG_INVALID"))?,
        );
        let embedding_calls = match self {
            Self::Search { .. } => 1,
            // Each authority transaction has at most64 changes; a32KiB claim has at most9 UTF-8 chunks.
            Self::Sync => pending
                .checked_mul(64 * 9)
                .ok_or_else(|| error("CONFIG_INVALID"))?,
        };
        let rerank_calls = if matches!(self, Self::Search { .. }) {
            config.config().retrieval.max_candidates as u64
        } else {
            0
        };
        for (model_config, profile, calls) in [
            (
                vector_config,
                &vector_config.config().retrieval.embedding,
                embedding_calls,
            ),
            (config, &config.config().retrieval.reranker, rerank_calls),
        ] {
            if let Some(profile) = profile {
                let milliseconds = model_config.timeouts().providers[profile]
                    .as_millis()
                    .checked_add(model_config.timeouts().queue.as_millis())
                    .and_then(|v| v.checked_mul(calls as u128))
                    .and_then(|v| u64::try_from(v).ok())
                    .ok_or_else(|| error("CONFIG_INVALID"))?;
                budget = budget
                    .checked_add(std::time::Duration::from_millis(milliseconds))
                    .ok_or_else(|| error("CONFIG_INVALID"))?;
            }
        }
        Ok(budget)
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Status {
    pub generation: String,
    pub lexical_watermark: u64,
    pub indexed_rows: usize,
    #[serde(default)]
    pub vector: Option<VectorStatus>,
}

pub fn status(storage: &Storage) -> Result<Status> {
    let status = storage
        .current(&state_key())?
        .map(|r| serde_json::from_str::<Status>(&r.content))
        .transpose()?;
    let status = status.unwrap_or(Status {
        generation: GENERATION.into(),
        lexical_watermark: 0,
        indexed_rows: 0,
        vector: None,
    });
    if status.generation != GENERATION || status.lexical_watermark > storage.sequence()? {
        return Err(error("INDEX_CORRUPT"));
    }
    Ok(status)
}

pub struct Index {
    pub budget_mib: u32,
    connection: Connection,
    table: Table,
    schema: Arc<Schema>,
    vector: tokio::sync::Mutex<Option<Arc<vector::VectorIndex>>>,
}
impl Index {
    pub async fn open(config: &ResolvedConfig, storage: &Storage) -> Result<Self> {
        let root = config.config().paths.data_dir.join("search");
        crate::daemon::private_dir(&root).map_err(|e| error(e.0))?;
        let budget_mib = config.config().runtime.memory_budget_mib;
        let bytes = budget_mib as usize * 1024 * 1024;
        let session = Session::new(
            bytes / 32,
            bytes / 64,
            Arc::new(ObjectStoreRegistry::default()),
        )
        .with_spill_store(Arc::new(
            lance_io::spill::LocalSpillStore::with_cap((bytes / 8) as u64).map_err(index_error)?,
        ));
        let connection = lancedb::connect(root.to_str().ok_or_else(|| error("INVALID_PATH"))?)
            .session(Arc::new(session))
            .execute()
            .await
            .map_err(index_error)?;
        let schema = Arc::new(Schema::new(vec![
            Field::new("key", DataType::Utf8, false),
            Field::new("id", DataType::Utf8, false),
            Field::new("revision", DataType::UInt64, false),
            Field::new("scope", DataType::Utf8, false),
            Field::new("body", DataType::Utf8, false),
        ]));
        let table = if connection
            .table_names()
            .execute()
            .await
            .map_err(index_error)?
            .iter()
            .any(|n| n == TABLE)
        {
            connection
                .open_table(TABLE)
                .execute()
                .await
                .map_err(index_error)?
        } else {
            if status(storage)?.lexical_watermark > 0 {
                return Err(error("INDEX_MISSING"));
            }
            let batch = RecordBatch::new_empty(schema.clone());
            connection
                .create_table(
                    TABLE,
                    Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema.clone()))
                        as Box<dyn arrow_array::RecordBatchReader + Send>,
                )
                .execute()
                .await
                .map_err(index_error)?
        };
        if table.schema().await.map_err(index_error)? != schema {
            return Err(error("INDEX_CORRUPT"));
        }
        Ok(Self {
            budget_mib,
            connection,
            table,
            schema,
            vector: tokio::sync::Mutex::new(None),
        })
    }

    pub(crate) async fn vector_index(
        &self,
        storage: &Storage,
        config: &ResolvedConfig,
    ) -> Result<Arc<vector::VectorIndex>> {
        let progress =
            vector::status(storage, config)?.ok_or_else(|| error("MODEL_NOT_CONFIGURED"))?;
        let mut vector = self.vector.lock().await;
        if vector
            .as_ref()
            .is_none_or(|v| v.generation != progress.generation)
        {
            *vector = Some(Arc::new(
                vector::VectorIndex::open(&self.connection, &progress).await?,
            ));
        }
        Ok(vector.as_ref().unwrap().clone())
    }

    pub fn status(storage: &Storage, config: &ResolvedConfig) -> Result<Status> {
        let mut progress = status(storage)?;
        progress.vector = vector::status(storage, config)?;
        Ok(progress)
    }

    /// Apply at most 32 complete authority transactions. Replays replace the same primary keys.
    pub async fn sync_batch(
        &self,
        storage: &Storage,
        target: u64,
        config_generation: u64,
        authorize: &(impl Fn() -> Result<()> + Sync),
    ) -> Result<Status> {
        let mut progress = storage
            .serialized_read(|| {
                authorize()?;
                status(storage)
            })
            .await?;
        let before = progress.lexical_watermark;
        let end = target.min(before.saturating_add(32));
        let mut changed = false;
        let mut completions = Vec::new();
        for seq in before + 1..=end {
            let entry = storage.outbox(seq)?.ok_or_else(|| error("OUTBOX_GAP"))?;
            let mut records = Vec::new();
            for reference in entry
                .revisions
                .iter()
                .filter(|r| r.key.kind == RecordKind::Memory)
            {
                let key = hash(reference.key.id.as_bytes());
                let memory = storage
                    .serialized_read(|| {
                        authorize()?;
                        Domain::new(storage).active(&reference.key.id)
                    })
                    .await?;
                let outcome = completion_outcome(reference, memory.as_ref());
                records.push(serde_json::json!({"key_hash":key,"revision":reference.commit_seq,"outcome":outcome}));
                if outcome == "superseded" {
                    continue;
                }
                if let Some(memory) = memory {
                    let body = format!(
                        "{}\n{}\n{}\n{}",
                        memory.record.draft.claim,
                        memory.record.draft.rationale,
                        memory.record.draft.conditions.join("\n"),
                        memory.record.draft.assumptions.join("\n")
                    );
                    let scope = hash(&serde_json::to_vec(&memory.record.draft.scope)?);
                    let batch = RecordBatch::try_new(
                        self.schema.clone(),
                        vec![
                            Arc::new(StringArray::from(vec![key.as_str()])),
                            Arc::new(StringArray::from(vec![reference.key.id.as_str()])),
                            Arc::new(UInt64Array::from(vec![memory.revision.commit_seq])),
                            Arc::new(StringArray::from(vec![scope.as_str()])),
                            Arc::new(StringArray::from(vec![body.as_str()])),
                        ],
                    )
                    .map_err(index_error)?;
                    let mut merge = self.table.merge_insert(&["key"]);
                    merge
                        .when_matched_update_all(None)
                        .when_not_matched_insert_all();
                    merge
                        .execute(Box::new(RecordBatchIterator::new(
                            vec![Ok(batch)],
                            self.schema.clone(),
                        )))
                        .await
                        .map_err(index_error)?;
                } else {
                    self.table
                        .delete(&format!("key = '{key}'"))
                        .await
                        .map_err(index_error)?;
                }
                changed = true;
            }
            completions.push(completion_change(GENERATION, seq, records)?);
            progress.lexical_watermark = seq;
        }
        if progress.lexical_watermark == before {
            return Ok(progress);
        }
        if changed || before == 0 {
            let indices = self.table.list_indices().await.map_err(index_error)?;
            if !indices.iter().any(|i| i.columns == ["scope"]) {
                self.table
                    .create_index(&["scope"], NativeIndex::BTree(BTreeIndexBuilder::default()))
                    .execute()
                    .await
                    .map_err(index_error)?;
            }
            if !indices.iter().any(|i| i.columns == ["body"]) {
                self.table
                    .create_index(
                        &["body"],
                        NativeIndex::FTS(
                            FtsIndexBuilder::default()
                                .base_tokenizer("icu".into())
                                .stem(false)
                                .remove_stop_words(false)
                                .memory_limit_mb((self.budget_mib as u64 / 16).clamp(1, 32))
                                .num_workers(1)
                                .with_position(false),
                        ),
                    )
                    .execute()
                    .await
                    .map_err(index_error)?;
            }
            self.table
                .optimize(lancedb::table::OptimizeAction::Index(Default::default()))
                .await
                .map_err(index_error)?;
        }
        progress.indexed_rows = self.table.count_rows(None).await.map_err(index_error)?;
        #[cfg(test)]
        crate::storage::crash_tests::checkpoint("index-before-progress");
        publish_progress(
            storage,
            state_key(),
            config_generation,
            &progress,
            completions,
            authorize,
        )
        .await?;
        #[cfg(test)]
        crate::storage::crash_tests::checkpoint("index-after-progress");
        Ok(progress)
    }

    pub async fn keyword(
        &self,
        query: &str,
        scope: &Scope,
        limit: usize,
    ) -> Result<Vec<(String, u64)>> {
        if !(1..=100).contains(&limit) {
            return Err(error("INVALID_SEARCH_BUDGET"));
        }
        let filter = format!("scope = '{}'", hash(&serde_json::to_vec(scope)?));
        let batches = self
            .table
            .query()
            .only_if(filter)
            .select(Select::Columns(vec!["id".into(), "revision".into()]))
            .limit(limit)
            .full_text_search(
                lance_index::scalar::FullTextSearchQuery::new(query.into())
                    .with_column("body".into())
                    .map_err(index_error)?,
            )
            .execute()
            .await
            .map_err(index_error)?;
        candidates(batches, limit).await
    }
}

async fn candidates(
    mut batches: lancedb::arrow::SendableRecordBatchStream,
    limit: usize,
) -> Result<Vec<(String, u64)>> {
    let mut result = Vec::new();
    while let Some(batch) = batches.try_next().await.map_err(index_error)? {
        let ids = batch
            .column_by_name("id")
            .and_then(|a| a.as_any().downcast_ref::<StringArray>())
            .ok_or_else(|| error("INDEX_CORRUPT"))?;
        let revisions = batch
            .column_by_name("revision")
            .and_then(|a| a.as_any().downcast_ref::<UInt64Array>())
            .ok_or_else(|| error("INDEX_CORRUPT"))?;
        for row in 0..batch.num_rows() {
            if ids.is_null(row) || revisions.is_null(row) || result.len() == limit {
                return Err(error("INDEX_CORRUPT"));
            }
            result.push((ids.value(row).to_owned(), revisions.value(row)));
        }
    }
    Ok(result)
}

fn completion_outcome(
    reference: &crate::storage::RevisionRef,
    memory: Option<&crate::domain::MemoryView>,
) -> &'static str {
    match memory {
        Some(memory) if memory.revision != *reference => "superseded",
        Some(_) => "indexed",
        None => "inactive",
    }
}
fn completion_change(
    generation: &str,
    seq: u64,
    records: Vec<serde_json::Value>,
) -> Result<Change> {
    let reason = if records.is_empty() {
        "not_required"
    } else {
        "processed"
    };
    Ok(Change {
        key: RecordKey {
            kind: RecordKind::State,
            id: format!("search/completion/{generation}/{seq:020}"),
        },
        expected_revision: None,
        content: serde_json::to_string(
            &serde_json::json!({"generation":generation,"seq":seq,"reason":reason,"records":records}),
        )?,
    })
}
async fn publish_progress(
    storage: &Storage,
    key: RecordKey,
    config_generation: u64,
    progress: &impl Serialize,
    mut completions: Vec<Change>,
    authorize: &(impl Fn() -> Result<()> + Sync),
) -> Result<()> {
    let current = storage.current(&key)?.map(|r| r.commit_seq);
    let content = serde_json::to_string(progress)?;
    let request_id = format!(
        "index-progress/{}/{}/{}",
        hash(key.id.as_bytes()),
        current.unwrap_or(0),
        hash(content.as_bytes())
    );
    completions.push(Change {
        key,
        expected_revision: current,
        content,
    });
    storage
        .commit_checked(|| {
            authorize()?;
            Ok(CommitRequest {
                request_id,
                config_generation,
                policy_revision: None,
                changes: completions,
            })
        })
        .await?;
    Ok(())
}
