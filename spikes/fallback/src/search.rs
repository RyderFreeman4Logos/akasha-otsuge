//! Corpus baseline only; ordinary-change reconciliation and activation are separate.
use std::{
    collections::{BTreeMap, HashMap, HashSet},
    fs::File,
    io::Read,
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
    time::Instant,
};

use arrow_array::{
    Array, ArrayRef, FixedSizeListArray, Float32Array, RecordBatch, StringArray, UInt8Array,
    UInt64Array, types::Float32Type,
};
use arrow_schema::{DataType, Field, Schema};
use futures::TryStreamExt;
use lance::index::{DatasetIndexExt, IndexParams, vector::VectorIndexParams};
use lance_index::{
    IndexType,
    progress::IndexBuildProgress,
    scalar::{BuiltinIndexType, FullTextSearchQuery, ScalarIndexParams},
    vector::ivf::IvfBuildParams,
};
use lancedb::{
    Connection, ObjectStoreRegistry, Session, Table,
    index::scalar::FtsIndexBuilder,
    query::{ExecutableQuery, QueryBase, Select},
    table::WriteOptions,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use surrealkv::{Durability, LSMIterator, Tree};

use crate::{Error, MIB, checkpoint, load, open_authority};

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Bundle {
    model: String,
    model_fingerprint: String,
    dimension: usize,
    chunker: String,
    schema: u64,
}

impl Bundle {
    fn validate(&self) -> Result<(), Error> {
        if self.model.is_empty()
            || self.model.len() > 128
            || self.model_fingerprint.len() != 64
            || !self
                .model_fingerprint
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
            || self.dimension != 1536
            || self.chunker.is_empty()
            || self.chunker.len() > 128
            || self.schema != 1
        {
            return Err("invalid corpus index bundle".into());
        }
        Ok(())
    }
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Begin {
        generation: u64,
        bundle: Bundle,
    },
    Backfill {
        generation: u64,
        waves: u64,
        hold: Option<String>,
    },
    Build {
        generation: u64,
    },
    Status {
        generation: u64,
    },
    Query {
        generation: u64,
        bundle: Bundle,
        scope: u8,
        term: Option<String>,
        vector: Option<Vec<f32>>,
    },
}

#[derive(Clone, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
struct Wave {
    end: u64,
    vectors: u64,
    hash: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct State {
    generation: u64,
    bundle: Bundle,
    source: String,
    documents: u64,
    vectors: u64,
    cursor: u64,
    vector_rows: u64,
    text_version: u64,
    vector_version: u64,
    pending: Option<Wave>,
    ready: bool,
}

fn key(generation: u64) -> Result<String, Error> {
    if generation == 0 {
        return Err("generation must be positive".into());
    }
    Ok(format!("derived:{generation:020}"))
}

fn state(tree: &Tree, generation: u64) -> Result<State, Error> {
    let value = tree
        .begin()?
        .get(key(generation)?.as_bytes())?
        .ok_or("missing generation")?;
    let value: State = serde_json::from_slice(&value)?;
    value.bundle.validate()?;
    let corpus = load::read_state(&tree.begin()?)?;
    if value.generation != generation
        || value.cursor > value.documents
        || value.vector_rows > value.vectors
        || value.source != corpus.manifest_sha256
        || value.documents != corpus.documents
        || value.vectors != corpus.vectors
        || value
            .pending
            .as_ref()
            .is_some_and(|p| p.end <= value.cursor || p.end > value.documents)
        || (value.ready
            && (value.cursor != value.documents
                || value.vector_rows != value.vectors
                || value.pending.is_some()))
    {
        return Err("invalid generation state".into());
    }
    Ok(value)
}

async fn save(tree: &Tree, state: &State) -> Result<(), Error> {
    let mut tx = tree.begin()?;
    tx.set_durability(Durability::Immediate);
    tx.set(
        key(state.generation)?.as_bytes(),
        serde_json::to_vec(state)?,
    )?;
    tx.commit().await?;
    Ok(())
}

async fn connect(root: &Path) -> Result<Connection, Error> {
    let session = Session::new(16 * MIB, 8 * MIB, Arc::new(ObjectStoreRegistry::default()))
        .with_spill_store(Arc::new(lance_io::spill::LocalSpillStore::with_cap(
            (64 * MIB) as u64,
        )?));
    Ok(lancedb::connect(
        root.join("derived")
            .to_str()
            .ok_or("non-UTF8 derived path")?,
    )
    .session(Arc::new(session))
    .execute()
    .await?)
}

fn table_name(generation: u64, vector: bool) -> String {
    format!("g{generation}_{}", if vector { "vector" } else { "text" })
}

fn batch(rows: &[(u64, load::Document)], vector: bool) -> Result<RecordBatch, Error> {
    let rows: Vec<_> = rows
        .iter()
        .filter(|(_, d)| !vector || d.embedding.is_some())
        .collect();
    let mut fields = vec![
        Field::new("id", DataType::UInt64, false),
        Field::new("scope", DataType::UInt8, false),
        Field::new("revision", DataType::UInt64, false),
    ];
    let mut columns: Vec<ArrayRef> = vec![
        Arc::new(UInt64Array::from_iter_values(
            rows.iter().map(|(id, _)| *id),
        )),
        Arc::new(UInt8Array::from_iter_values(
            rows.iter().map(|(_, d)| d.partition.as_bytes()[1] - b'0'),
        )),
        Arc::new(UInt64Array::from_iter_values(rows.iter().map(|_| 0))),
    ];
    if vector {
        let values = FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
            rows.iter()
                .map(|(_, d)| d.embedding.as_ref().map(|v| v.iter().copied().map(Some))),
            1536,
        );
        fields.push(Field::new("vector", values.data_type().clone(), false));
        columns.push(Arc::new(values));
    } else {
        fields.push(
            Field::new("body", DataType::Utf8, false).with_metadata(HashMap::from([
                ("lance-encoding:compression".into(), "zstd".into()),
                ("lance-encoding:compression-level".into(), "1".into()),
            ])),
        );
        columns.push(Arc::new(StringArray::from_iter_values(
            rows.iter().map(|(_, d)| d.body.as_str()),
        )));
    }
    Ok(RecordBatch::try_new(
        Arc::new(Schema::new(fields)),
        columns,
    )?)
}

fn column<'a, T: Array + 'static>(batch: &'a RecordBatch, name: &str) -> Result<&'a T, Error> {
    batch
        .column_by_name(name)
        .and_then(|c| c.as_any().downcast_ref::<T>())
        .ok_or_else(|| format!("invalid derived column {name}").into())
}

async fn verify_appended(table: &Table, expected: &RecordBatch, start: u64) -> Result<(), Error> {
    let expected_ids = column::<UInt64Array>(expected, "id")?;
    let positions: HashMap<_, _> = expected_ids
        .values()
        .iter()
        .enumerate()
        .map(|(i, id)| (*id, i))
        .collect();
    let mut seen = HashSet::new();
    let mut stream = table
        .take_offsets((start..start + expected.num_rows() as u64).collect())
        .execute()
        .await?;
    while let Some(actual) = stream.try_next().await? {
        for row in 0..actual.num_rows() {
            let id = column::<UInt64Array>(&actual, "id")?.value(row);
            let at = *positions.get(&id).ok_or("unexpected appended identity")?;
            if !seen.insert(id)
                || column::<UInt8Array>(&actual, "scope")?.value(row)
                    != column::<UInt8Array>(expected, "scope")?.value(at)
                || column::<UInt64Array>(&actual, "revision")?.value(row) != 0
            {
                return Err("appended identity/scope mismatch".into());
            }
            if expected.column_by_name("body").is_some() {
                if column::<StringArray>(&actual, "body")?.value(row)
                    != column::<StringArray>(expected, "body")?.value(at)
                {
                    return Err("appended text mismatch".into());
                }
            } else {
                let a = column::<FixedSizeListArray>(&actual, "vector")?.value(row);
                let b = column::<FixedSizeListArray>(expected, "vector")?.value(at);
                if a.as_any()
                    .downcast_ref::<Float32Array>()
                    .ok_or("invalid actual vector")?
                    .values()
                    != b.as_any()
                        .downcast_ref::<Float32Array>()
                        .ok_or("invalid expected vector")?
                        .values()
                {
                    return Err("appended vector mismatch".into());
                }
            }
        }
    }
    if seen.len() != expected.num_rows() {
        return Err("incomplete appended wave".into());
    }
    Ok(())
}

async fn append(
    connection: &Connection,
    name: &str,
    data: RecordBatch,
    before: u64,
    previous_version: u64,
    tag: &str,
) -> Result<u64, Error> {
    if data.num_rows() == 0 {
        return Ok(previous_version);
    }
    let mut options = WriteOptions::default();
    let params = options
        .lance_write_params
        .get_or_insert_with(Default::default);
    params.max_rows_per_group = 256;
    params.max_rows_per_file = 8192;
    params.max_bytes_per_file = 32 * MIB;
    params.transaction_properties = Some(Arc::new(HashMap::from([(
        "akasha.wave".into(),
        tag.into(),
    )])));
    let mut recovered = false;
    let table = match connection.open_table(name).execute().await {
        Ok(table) => {
            let count = table.count_rows(None).await? as u64;
            let version = table.version().await?;
            if count == before && version == previous_version {
                // Explicit native options otherwise retain their default Create mode.
                options
                    .lance_write_params
                    .as_mut()
                    .ok_or("missing write options")?
                    .mode = lance::dataset::WriteMode::Append;
                table
                    .add(data.clone())
                    .write_options(options)
                    .write_parallelism(1)
                    .execute()
                    .await?;
            } else if count == before + data.num_rows() as u64 && version == previous_version + 1 {
                recovered = true;
            } else {
                return Err("derived append count/version conflict".into());
            }
            table
        }
        Err(lancedb::Error::TableNotFound { .. }) if before == 0 && previous_version == 0 => {
            connection
                .create_table(name, data.clone())
                .write_options(options)
                .execute()
                .await?
        }
        Err(error) => return Err(error.into()),
    };
    let dataset = table
        .dataset()
        .ok_or("local dataset required")?
        .get()
        .await?;
    let transaction = dataset
        .read_transaction()
        .await?
        .ok_or("missing derived transaction")?;
    if transaction
        .transaction_properties
        .as_ref()
        .and_then(|p| p.get("akasha.wave"))
        .map(String::as_str)
        != Some(tag)
        || table.count_rows(None).await? as u64 != before + data.num_rows() as u64
    {
        return Err("derived append receipt mismatch".into());
    }
    if recovered {
        verify_appended(&table, &data, before).await?;
    }
    Ok(table.version().await?)
}

async fn backfill(
    tree: &Tree,
    root: &Path,
    state: &mut State,
    waves: u64,
    hold: Option<&str>,
) -> Result<(), Error> {
    if !(1..=1_000_000).contains(&waves) {
        return Err("invalid wave count".into());
    }
    if hold.is_some_and(|h| {
        !matches!(
            h,
            "before-text" | "after-text" | "after-vector" | "after-cursor"
        )
    }) {
        return Err("invalid backfill checkpoint".into());
    }
    let connection = connect(root).await?;
    let started = Instant::now();
    let initial = state.cursor;
    for _ in 0..waves {
        if state.cursor == state.documents {
            break;
        }
        let rows = {
            let tx = tree.begin()?;
            let mut iter = tx.range(
                load::corpus_key(load::FIRST_MEMORY + state.cursor).into_bytes(),
                b"corpus;".to_vec(),
            )?;
            iter.seek_first()?;
            let (mut rows, mut bytes, mut hash) = (Vec::new(), 0, Sha256::new());
            while iter.valid() && rows.len() < 8192 && bytes < 16 * MIB {
                let id = load::FIRST_MEMORY + state.cursor + rows.len() as u64;
                if iter.key().user_key() != load::corpus_key(id).as_bytes() {
                    return Err("noncontiguous backfill corpus".into());
                }
                let raw = load::decode_record(&iter.value()?)?;
                bytes += raw.len();
                hash.update(id.to_be_bytes());
                hash.update(&raw);
                rows.push((id, load::document(&raw)?));
                iter.next()?;
            }
            if rows.is_empty() {
                return Err("incomplete backfill corpus".into());
            }
            let wave = Wave {
                end: state.cursor + rows.len() as u64,
                vectors: rows.iter().filter(|(_, d)| d.embedding.is_some()).count() as u64,
                hash: load::hex(&hash),
            };
            if state.pending.as_ref().is_some_and(|p| p != &wave) {
                return Err("pending backfill source changed".into());
            }
            state.pending = Some(wave);
            rows
        };
        save(tree, state).await?;
        let wave = state.pending.clone().ok_or("missing prepared wave")?;
        let tag = format!(
            "{}:{}:{}:{}",
            state.generation, state.cursor, wave.end, wave.hash
        );
        checkpoint(hold, "before-text").await;
        let text_version = append(
            &connection,
            &table_name(state.generation, false),
            batch(&rows, false)?,
            state.cursor,
            state.text_version,
            &tag,
        )
        .await?;
        checkpoint(hold, "after-text").await;
        let vector_version = append(
            &connection,
            &table_name(state.generation, true),
            batch(&rows, true)?,
            state.vector_rows,
            state.vector_version,
            &tag,
        )
        .await?;
        checkpoint(hold, "after-vector").await;
        state.cursor = wave.end;
        state.vector_rows += wave.vectors;
        state.text_version = text_version;
        state.vector_version = vector_version;
        state.pending = None;
        save(tree, state).await?;
        let elapsed = started.elapsed().as_secs_f64();
        let done = state.cursor - initial;
        load::atomic_json(
            &root.join(format!(
                "generation-{}-backfill-progress.json",
                state.generation
            )),
            &json!({
                "phase":"backfill","completed":state.cursor,"total":state.documents,"elapsed_s":elapsed,
                "rate_per_s":done as f64/elapsed,"eta_s":(state.documents-state.cursor) as f64*elapsed/done as f64,
                "pid":std::process::id(),"state":state
            }),
        )?;
        checkpoint(hold, "after-cursor").await;
    }
    Ok(())
}

#[derive(Debug)]
struct Stage {
    total: Option<u64>,
    completed: u64,
    unit: String,
    started: Instant,
    done: bool,
}
#[derive(Debug)]
struct Progress {
    path: PathBuf,
    stages: Mutex<BTreeMap<String, Stage>>,
}
impl Progress {
    fn write(&self, active: &str, stages: &BTreeMap<String, Stage>) -> lance_core::Result<()> {
        let values:BTreeMap<_,_>=stages.iter().map(|(name,s)|{
            let elapsed=s.started.elapsed().as_secs_f64();
            (name,json!({"total":s.total,"completed":s.completed,"unit":s.unit,"elapsed_s":elapsed,"done":s.done,
                "rate_per_s":s.completed as f64/elapsed,"eta_s":s.total.filter(|_|s.completed>0).map(|t|t.saturating_sub(s.completed) as f64*elapsed/s.completed as f64)}))
        }).collect();
        load::atomic_json(
            &self.path,
            &json!({"phase":active,"pid":std::process::id(),"stages":values}),
        )
        .map_err(|e| lance_core::Error::io(format!("persist index progress: {e}")))
    }
}
#[async_trait::async_trait]
impl IndexBuildProgress for Progress {
    async fn stage_start(
        &self,
        name: &str,
        total: Option<u64>,
        unit: &str,
    ) -> lance_core::Result<()> {
        let mut stages = self
            .stages
            .lock()
            .map_err(|_| lance_core::Error::io("index progress lock poisoned"))?;
        stages.insert(
            name.into(),
            Stage {
                total,
                completed: 0,
                unit: unit.into(),
                started: Instant::now(),
                done: false,
            },
        );
        self.write(name, &stages)
    }
    async fn stage_progress(&self, name: &str, completed: u64) -> lance_core::Result<()> {
        let mut stages = self
            .stages
            .lock()
            .map_err(|_| lance_core::Error::io("index progress lock poisoned"))?;
        stages
            .get_mut(name)
            .ok_or_else(|| lance_core::Error::io("missing progress stage"))?
            .completed = completed;
        self.write(name, &stages)
    }
    async fn stage_complete(&self, name: &str) -> lance_core::Result<()> {
        let mut stages = self
            .stages
            .lock()
            .map_err(|_| lance_core::Error::io("index progress lock poisoned"))?;
        stages
            .get_mut(name)
            .ok_or_else(|| lance_core::Error::io("missing progress stage"))?
            .done = true;
        self.write(name, &stages)
    }
}

async fn index(
    table: &Table,
    root: &Path,
    name: &str,
    column: &str,
    kind: IndexType,
    params: &dyn IndexParams,
    rows: u64,
) -> Result<(), Error> {
    if !table.list_indices().await?.iter().any(|i| i.name == name) {
        let wrapper = table.dataset().ok_or("local dataset required")?;
        let mut dataset = (*wrapper.get().await?).clone();
        dataset
            .create_index_builder(&[column], kind, params)
            .name(name.into())
            .replace(false)
            .progress(Arc::new(Progress {
                path: root.join(format!("{}-{name}-progress.json", table.name())),
                stages: Mutex::new(BTreeMap::new()),
            }))
            .await?;
        wrapper.update(dataset);
    }
    let stats = table
        .index_stats(name)
        .await?
        .ok_or("missing built index")?;
    if stats.num_indexed_rows as u64 != rows || stats.num_unindexed_rows != 0 {
        return Err("incomplete native index coverage".into());
    }
    Ok(())
}

async fn build(tree: &Tree, root: &Path, state: &mut State) -> Result<(), Error> {
    if state.cursor != state.documents
        || state.vector_rows != state.vectors
        || state.pending.is_some()
    {
        return Err("backfill is incomplete".into());
    }
    let connection = connect(root).await?;
    let text = connection
        .open_table(table_name(state.generation, false))
        .execute()
        .await?;
    if text.count_rows(None).await? as u64 != state.documents {
        return Err("text row coverage mismatch".into());
    }
    let bitmap = ScalarIndexParams::for_builtin(BuiltinIndexType::Bitmap);
    index(
        &text,
        root,
        "scope_bitmap",
        "scope",
        IndexType::Bitmap,
        &bitmap,
        state.documents,
    )
    .await?;
    let fts = FtsIndexBuilder::default()
        .memory_limit_mb(32)
        .num_workers(1)
        .with_position(false);
    index(
        &text,
        root,
        "body_fts",
        "body",
        IndexType::Inverted,
        &fts,
        state.documents,
    )
    .await?;
    if state.vectors > 0 {
        let vector = connection
            .open_table(table_name(state.generation, true))
            .execute()
            .await?;
        if vector.count_rows(None).await? as u64 != state.vectors {
            return Err("vector row coverage mismatch".into());
        }
        index(
            &vector,
            root,
            "scope_bitmap",
            "scope",
            IndexType::Bitmap,
            &bitmap,
            state.vectors,
        )
        .await?;
        let partitions = ((state.vectors as f64).sqrt() as usize).clamp(1, 256);
        let mut ivf = IvfBuildParams::new(partitions);
        ivf.sample_rate = 16;
        ivf.max_iters = 20;
        let params =
            VectorIndexParams::with_ivf_flat_params(lancedb::DistanceType::Cosine.into(), ivf);
        index(
            &vector,
            root,
            "vector_ivf",
            "vector",
            IndexType::Vector,
            &params,
            state.vectors,
        )
        .await?;
    }
    state.ready = true;
    save(tree, state).await
}

async fn query(
    root: &Path,
    state: &State,
    bundle: &Bundle,
    scope: u8,
    term: Option<String>,
    vector: Option<Vec<f32>>,
) -> Result<Value, Error> {
    if !state.ready {
        return Err("generation not ready".into());
    }
    if &state.bundle != bundle {
        return Err("query bundle mismatch".into());
    }
    if scope > 7
        || term.is_some() == vector.is_some()
        || term.as_ref().is_some_and(|t| t.is_empty() || t.len() > 512)
        || vector.as_ref().is_some_and(|v| {
            v.len() != 1536 || v.iter().any(|v| !v.is_finite()) || v.iter().all(|v| *v == 0.0)
        })
    {
        return Err("invalid search request".into());
    }
    let connection = connect(root).await?;
    let table = connection
        .open_table(table_name(state.generation, vector.is_some()))
        .execute()
        .await?;
    let base = table
        .query()
        .select(Select::Columns(vec!["id".into(), "revision".into()]))
        .only_if(format!("scope = {scope}"))
        .limit(20);
    let mut stream = if let Some(vector) = vector {
        base.nearest_to(vector)?
            .distance_type(lancedb::DistanceType::Cosine)
            .nprobes(32)
            .execute()
            .await?
    } else {
        base.full_text_search(FullTextSearchQuery::new(term.ok_or("missing term")?))
            .execute()
            .await?
    };
    let mut ids = Vec::new();
    while let Some(batch) = stream.try_next().await? {
        ids.extend_from_slice(column::<UInt64Array>(&batch, "id")?.values());
    }
    Ok(json!({"generation":state.generation,"ids":ids,"corpus_baseline_only":true}))
}

pub async fn run(root: &Path, path: &Path) -> Result<Value, Error> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err("derived request exceeds 1 MiB".into());
    }
    let request: Request = serde_json::from_slice(&bytes)?;
    let tree = open_authority(root)?;
    let corpus = load::read_state(&tree.begin()?)?;
    if !corpus.complete {
        return Err("corpus import is incomplete".into());
    }
    let result = match request {
        Request::Begin { generation, bundle } => {
            bundle.validate()?;
            let existing = tree.begin()?.get(key(generation)?.as_bytes())?;
            let value = if existing.is_some() {
                let value = state(&tree, generation)?;
                if value.bundle != bundle || value.source != corpus.manifest_sha256 {
                    return Err("generation identity conflict".into());
                }
                value
            } else {
                let value = State {
                    generation,
                    bundle,
                    source: corpus.manifest_sha256,
                    documents: corpus.documents,
                    vectors: corpus.vectors,
                    cursor: 0,
                    vector_rows: 0,
                    text_version: 0,
                    vector_version: 0,
                    pending: None,
                    ready: false,
                };
                save(&tree, &value).await?;
                value
            };
            serde_json::to_value(value)?
        }
        Request::Backfill {
            generation,
            waves,
            hold,
        } => {
            let mut value = state(&tree, generation)?;
            backfill(&tree, root, &mut value, waves, hold.as_deref()).await?;
            serde_json::to_value(value)?
        }
        Request::Build { generation } => {
            let mut value = state(&tree, generation)?;
            build(&tree, root, &mut value).await?;
            serde_json::to_value(value)?
        }
        Request::Status { generation } => serde_json::to_value(state(&tree, generation)?)?,
        Request::Query {
            generation,
            bundle,
            scope,
            term,
            vector,
        } => {
            query(
                root,
                &state(&tree, generation)?,
                &bundle,
                scope,
                term,
                vector,
            )
            .await?
        }
    };
    tree.close().await?;
    Ok(result)
}
