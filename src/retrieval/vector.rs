use super::*;
use crate::config::deadline::Deadline;
use crate::provider::{Call, Client, Input};
use arrow_array::{FixedSizeListArray, types::Float32Type};
use lancedb::{DistanceType, index::vector::IvfFlatIndexBuilder, table::OptimizeAction};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VectorStatus {
    pub generation: String,
    pub model_identity: String,
    pub dimension: u32,
    pub watermark: u64,
    pub indexed_rows: usize,
    pub last_call: Option<serde_json::Value>,
}

fn key(generation: &str) -> RecordKey {
    RecordKey {
        kind: RecordKind::State,
        id: format!("search/vector/{generation}"),
    }
}

pub fn status(storage: &Storage, config: &ResolvedConfig) -> Result<Option<VectorStatus>> {
    let Some(profile) = &config.config().retrieval.embedding else {
        return Ok(None);
    };
    let identity = crate::provider::model_identity(config, profile).map_err(|e| error(e.0))?;
    let dimension = config.config().models[profile]
        .dimension
        .ok_or_else(|| error("CONFIG_INVALID"))?;
    let generation = hash(&serde_json::to_vec(&(
        "claim-4096-byte-mean-cosine-v1",
        &identity,
    ))?);
    let progress = storage
        .current(&key(&generation))?
        .map(|r| serde_json::from_str::<VectorStatus>(&r.content))
        .transpose()?;
    let progress = progress.unwrap_or(VectorStatus {
        generation: generation.clone(),
        model_identity: identity.clone(),
        dimension,
        watermark: 0,
        indexed_rows: 0,
        last_call: None,
    });
    if progress.generation != generation
        || progress.model_identity != identity
        || progress.dimension != dimension
        || progress.watermark > storage.sequence()?
    {
        return Err(error("INDEX_CORRUPT"));
    }
    Ok(Some(progress))
}

pub(crate) struct VectorIndex {
    pub generation: String,
    table: Table,
    schema: Arc<Schema>,
}

impl VectorIndex {
    pub async fn open(connection: &Connection, progress: &VectorStatus) -> Result<Self> {
        let dimension =
            i32::try_from(progress.dimension).map_err(|_| error("INVALID_DIMENSION"))?;
        let schema = Arc::new(Schema::new(vec![
            Field::new("key", DataType::Utf8, false),
            Field::new("id", DataType::Utf8, false),
            Field::new("revision", DataType::UInt64, false),
            Field::new("scope", DataType::Utf8, false),
            Field::new(
                "vector",
                DataType::FixedSizeList(
                    Arc::new(Field::new("item", DataType::Float32, true)),
                    dimension,
                ),
                false,
            ),
        ]));
        let name = format!("memory_vector_{}", progress.generation);
        let table = if connection
            .table_names()
            .execute()
            .await
            .map_err(index_error)?
            .contains(&name)
        {
            connection
                .open_table(&name)
                .execute()
                .await
                .map_err(index_error)?
        } else {
            if progress.watermark != 0 {
                return Err(error("INDEX_MISSING"));
            }
            connection
                .create_table(
                    &name,
                    Box::new(RecordBatchIterator::new(
                        vec![Ok(RecordBatch::new_empty(schema.clone()))],
                        schema.clone(),
                    )) as Box<dyn arrow_array::RecordBatchReader + Send>,
                )
                .execute()
                .await
                .map_err(index_error)?
        };
        if table.schema().await.map_err(index_error)? != schema {
            return Err(error("INDEX_CORRUPT"));
        }
        Ok(Self {
            generation: progress.generation.clone(),
            table,
            schema,
        })
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn sync_batch<F, C>(
        &self,
        storage: &Storage,
        target: u64,
        snapshot: &crate::config::Snapshot<()>,
        client: &Client,
        workflow: &Deadline,
        cancel: C,
        authorize: &(impl Fn() -> Result<()> + Sync),
    ) -> Result<VectorStatus>
    where
        F: std::future::Future<Output = &'static str>,
        C: Fn() -> F,
    {
        let config = snapshot.resolved();
        let mut progress = storage
            .serialized_read(|| {
                authorize()?;
                status(storage, config)?.ok_or_else(|| error("MODEL_NOT_CONFIGURED"))
            })
            .await?;
        if progress.generation != self.generation {
            return Err(error("INDEX_GENERATION_MISMATCH"));
        }
        let before = progress.watermark;
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
                let row_key = hash(reference.key.id.as_bytes());
                let memory = storage
                    .serialized_read(|| {
                        authorize()?;
                        Domain::new(storage).active(&reference.key.id)
                    })
                    .await?;
                let outcome = completion_outcome(reference, memory.as_ref());
                records.push(serde_json::json!({"key_hash":row_key,"revision":reference.commit_seq,"outcome":outcome}));
                if outcome == "superseded" {
                    continue;
                }
                if let Some(memory) = memory {
                    let mut vector = vec![0f32; progress.dimension as usize];
                    let text = &memory.record.draft.claim;
                    let mut start = 0;
                    let mut parts = 0;
                    while start < text.len() {
                        let mut end = (start + 4096).min(text.len());
                        while !text.is_char_boundary(end) {
                            end -= 1;
                        }
                        let outcome = client
                            .call_guarded(
                                config,
                                &Call {
                                    profile: config.config().retrieval.embedding.clone().unwrap(),
                                    input: Input::Embedding {
                                        texts: vec![text[start..end].into()],
                                    },
                                },
                                workflow,
                                cancel(),
                                async {
                                    storage
                                        .serialized_read(authorize)
                                        .await
                                        .map_err(|e| crate::provider::Error(e.code))
                                },
                            )
                            .await;
                        progress.last_call = Some(
                            serde_json::json!({"profile":outcome.profile,"identity":outcome.identity,"error":outcome.error,"trace":outcome.trace}),
                        );
                        if let Some(code) = outcome.error {
                            progress.watermark = before;
                            publish_progress(
                                storage,
                                key(&progress.generation),
                                snapshot.generation(),
                                &progress,
                                vec![],
                                authorize,
                            )
                            .await?;
                            return Err(error(model_error(&code)));
                        }
                        let output = outcome
                            .output
                            .ok_or_else(|| error("PROVIDER_OUTPUT_INVALID"))?;
                        let values = output["vectors"][0]
                            .as_array()
                            .ok_or_else(|| error("PROVIDER_OUTPUT_INVALID"))?;
                        if values.len() != vector.len() {
                            return Err(error("PROVIDER_OUTPUT_INVALID"));
                        }
                        for (sum, value) in vector.iter_mut().zip(values) {
                            let value = value
                                .as_f64()
                                .ok_or_else(|| error("PROVIDER_OUTPUT_INVALID"))?
                                as f32;
                            if !value.is_finite() {
                                return Err(error("PROVIDER_OUTPUT_INVALID"));
                            }
                            *sum += value;
                        }
                        parts += 1;
                        start = end;
                    }
                    for value in &mut vector {
                        *value /= parts as f32;
                    }
                    validate_vector(&vector, progress.dimension)?;
                    let scope = hash(&serde_json::to_vec(&memory.record.draft.scope)?);
                    let batch = RecordBatch::try_new(
                        self.schema.clone(),
                        vec![
                            Arc::new(StringArray::from(vec![row_key.as_str()])),
                            Arc::new(StringArray::from(vec![reference.key.id.as_str()])),
                            Arc::new(UInt64Array::from(vec![memory.revision.commit_seq])),
                            Arc::new(StringArray::from(vec![scope.as_str()])),
                            Arc::new(
                                FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
                                    [Some(vector.into_iter().map(Some))],
                                    progress.dimension as i32,
                                ),
                            ),
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
                        .delete(&format!("key = '{row_key}'"))
                        .await
                        .map_err(index_error)?;
                }
                changed = true;
            }
            completions.push(completion_change(&progress.generation, seq, records)?);
            progress.watermark = seq;
        }
        if progress.watermark == before {
            return Ok(progress);
        }
        progress.indexed_rows = self.table.count_rows(None).await.map_err(index_error)?;
        if changed && progress.indexed_rows > 0 {
            let indices = self.table.list_indices().await.map_err(index_error)?;
            if !indices.iter().any(|i| i.columns == ["scope"]) {
                self.table
                    .create_index(&["scope"], NativeIndex::BTree(BTreeIndexBuilder::default()))
                    .execute()
                    .await
                    .map_err(index_error)?;
            }
            if !indices.iter().any(|i| i.columns == ["vector"]) {
                // Training samples and partition count stay inside this owner's configured budget.
                let bytes = config.config().runtime.memory_budget_mib as usize * 1024 * 1024 / 16;
                let partitions = ((progress.indexed_rows as f64).sqrt() as usize)
                    .clamp(1, 1024)
                    .min((bytes / (progress.dimension as usize * 4 * 16)).max(1));
                self.table
                    .create_index(
                        &["vector"],
                        NativeIndex::IvfFlat(
                            IvfFlatIndexBuilder::default()
                                .distance_type(DistanceType::Cosine)
                                .num_partitions(partitions as u32)
                                .sample_rate(16)
                                .max_iterations(20),
                        ),
                    )
                    .execute()
                    .await
                    .map_err(index_error)?;
            }
            self.table
                .optimize(OptimizeAction::Index(Default::default()))
                .await
                .map_err(index_error)?;
        }
        publish_progress(
            storage,
            key(&progress.generation),
            snapshot.generation(),
            &progress,
            completions,
            authorize,
        )
        .await?;
        Ok(progress)
    }

    pub async fn search(
        &self,
        vector: &[f32],
        scope: &Scope,
        limit: usize,
    ) -> Result<Vec<(String, u64)>> {
        let filter = format!("scope = '{}'", hash(&serde_json::to_vec(scope)?));
        let stream = self
            .table
            .query()
            .only_if(filter)
            .select(Select::Columns(vec!["id".into(), "revision".into()]))
            .limit(limit)
            .nearest_to(vector)
            .map_err(index_error)?
            .distance_type(DistanceType::Cosine)
            .nprobes(16)
            .execute()
            .await
            .map_err(index_error)?;
        candidates(stream, limit).await
    }
}

pub(super) fn validate_vector(vector: &[f32], dimension: u32) -> Result<()> {
    if vector.len() != dimension as usize
        || vector.iter().any(|v| !v.is_finite())
        || !vector.iter().any(|v| *v != 0.0)
    {
        return Err(error("PROVIDER_OUTPUT_INVALID"));
    }
    Ok(())
}

pub(crate) fn model_error(code: &str) -> &'static str {
    match code {
        "READ_DENIED" => "READ_DENIED",
        "CANCELLED" => "CANCELLED",
        "PROVIDER_INTERRUPTED" => "PROVIDER_INTERRUPTED",
        "WORKFLOW_TIMEOUT" => "WORKFLOW_TIMEOUT",
        "QUEUE_TIMEOUT" => "QUEUE_TIMEOUT",
        "PROVIDER_TIMEOUT" => "PROVIDER_TIMEOUT",
        "PROVIDER_OUTPUT_INVALID" => "PROVIDER_OUTPUT_INVALID",
        "PROVIDER_RATE_LIMITED" => "PROVIDER_RATE_LIMITED",
        _ => "MODEL_UNAVAILABLE",
    }
}

#[derive(Deserialize, Serialize, Eq, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct ActiveVector {
    pub generation: String,
    pub config_generation: u64,
}
fn active_key() -> RecordKey {
    RecordKey {
        kind: RecordKind::State,
        id: "search/vector-active".into(),
    }
}
pub(crate) fn active_vector(storage: &Storage) -> Result<Option<ActiveVector>> {
    let active = storage
        .current(&active_key())?
        .map(|r| serde_json::from_str::<ActiveVector>(&r.content))
        .transpose()?;
    if active.as_ref().is_some_and(|v| {
        v.config_generation == 0
            || v.generation.len() != 64
            || !v.generation.bytes().all(|b| b.is_ascii_hexdigit())
    }) {
        return Err(error("INDEX_CORRUPT"));
    }
    Ok(active)
}
pub(crate) async fn activate_vector(
    storage: &Storage,
    progress: &Status,
    snapshot: &crate::config::Snapshot<()>,
    target: u64,
    authorize: &(impl Fn() -> Result<()> + Sync),
) -> Result<()> {
    let Some(vector) = &progress.vector else {
        return Ok(());
    };
    if vector.watermark < target || progress.lexical_watermark < target {
        return Err(error("INDEX_NOT_READY"));
    }
    let candidate = ActiveVector {
        generation: vector.generation.clone(),
        config_generation: snapshot.generation(),
    };
    if active_vector(storage)?
        .is_some_and(|v| v == candidate || v.config_generation > candidate.config_generation)
    {
        return Ok(());
    }
    let current = storage.current(&active_key())?;
    storage
        .commit_checked(|| {
            authorize()?;
            Ok(CommitRequest {
                request_id: format!(
                    "vector-active/{}/{}",
                    candidate.config_generation, candidate.generation
                ),
                config_generation: snapshot.generation(),
                policy_revision: None,
                changes: vec![Change {
                    key: active_key(),
                    expected_revision: current.map(|r| r.commit_seq),
                    content: serde_json::to_string(&candidate)?,
                }],
            })
        })
        .await?;
    Ok(())
}
