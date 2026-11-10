mod index;

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use super::reader::{Row, Snapshot, hex, row_key};
use super::writer::{ProjectionContext, Writer};
use super::{MempalArchive, Migration, Table, emit_block, projection};
use crate::storage::{RecordKey, RecordKind, Result, Storage, StorageError, hash};
use index::{Index, RowState, Scratch};

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Lineage {
    root: String,
    previous: String,
    previous_archive_sha256: String,
    generation: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Delta {
    rows: u64,
    bytes: u64,
    sha256: String,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct DeltaRow {
    key: String,
    origin: String,
    row: Option<Row>,
}

fn invalid() -> StorageError {
    StorageError::new("INVALID_MEMPAL_LINEAGE")
}

fn candidate_parts(table: &str, columns: &[String], row: &Row) -> Result<u64> {
    let column = match table {
        "drawers" => "content",
        "knowledge_cards" => "statement",
        _ => return Ok(0),
    };
    let position = columns
        .iter()
        .position(|name| name == column)
        .ok_or_else(invalid)?;
    let super::reader::Cell::Text(text) = row.values.get(position).ok_or_else(invalid)? else {
        return Err(invalid());
    };
    let mut parts = 0;
    let mut consumed = 0;
    while consumed < text.len() {
        let mut end = (consumed + 16 * 1024).min(text.len());
        while !text.is_char_boundary(end) {
            end -= 1;
        }
        consumed = end;
        parts += 1;
    }
    Ok(parts)
}

fn withdraw(
    writer: &mut Writer,
    store: &Storage,
    state: &RowState,
    retained: &std::collections::BTreeSet<String>,
) -> Result<()> {
    for part in 0..state.parts {
        let id = format!("{}/{part}", state.origin);
        if retained.contains(&id) {
            continue;
        }
        let changes = crate::domain::migrated_withdrawal(store, &id)?;
        if !changes.is_empty() {
            writer.group(|_| Ok(changes.clone()))?;
        }
    }
    Ok(())
}

fn evidence(store: &Storage, id: &str) -> Result<crate::storage::Revision> {
    let value = store
        .current(&RecordKey {
            kind: RecordKind::Evidence,
            id: id.into(),
        })?
        .ok_or_else(invalid)?;
    if value.previous.is_some() {
        return Err(invalid());
    }
    Ok(value)
}

fn lineage(store: &Storage) -> Result<Vec<(String, Migration)>> {
    let mut sequence = store.sequence()?;
    let last = store.outbox(sequence)?.ok_or_else(invalid)?;
    let ids: Vec<_> = last
        .revisions
        .iter()
        .filter(|r| {
            r.key.kind == RecordKind::Evidence
                && r.key.id.starts_with("mempal/")
                && r.key.id.ends_with("/migration")
        })
        .collect();
    if ids.len() != 1 {
        return Err(invalid());
    }
    let mut id = ids[0].key.id.clone();
    let mut result = Vec::new();
    let mut metadata_bytes = 0;
    loop {
        let revision = evidence(store, &id)?;
        metadata_bytes += revision.content.len();
        if metadata_bytes > 32 * crate::storage::MIB {
            return Err(StorageError::new("MEMPAL_LINEAGE_TOO_LARGE"));
        }
        if revision.commit_seq > sequence {
            return Err(invalid());
        }
        let migration: Migration = serde_json::from_str(&revision.content)?;
        if migration.source_mapping_commit != "4593a5c5e6a9ea1140256cd42fa5838511a24e14"
            || migration.schema_axes != [21, 28]
            || migration.source_sha256.len() != 64
            || migration.observed_at_ms > i64::MAX as u64
        {
            return Err(invalid());
        }
        let previous = match (&migration.incremental, migration.mapping_version) {
            (None, 2) if id == format!("mempal/{}/migration", migration.source_sha256) => None,
            (Some(chain), 3)
                if id == format!("mempal/{}/migration", chain.generation)
                    && chain.generation
                        == hash(&serde_json::to_vec(&(
                            &chain.previous_archive_sha256,
                            &migration.source_sha256,
                        ))?) =>
            {
                Some(chain.previous.clone())
            }
            _ => return Err(invalid()),
        };
        sequence = revision.commit_seq.checked_sub(1).ok_or_else(invalid)?;
        result.push((id, migration));
        match previous {
            Some(previous) => id = previous,
            None => break,
        }
    }
    result.reverse();
    let root = &result[0].1.source_sha256;
    if result.iter().any(|(_, m)| {
        m.incremental
            .as_ref()
            .is_some_and(|chain| &chain.root != root)
    }) {
        return Err(invalid());
    }
    Ok(result)
}

fn read_rows(
    store: &Storage,
    generation: &str,
    name: &str,
    table: &Table,
    mut consume: impl FnMut(&str) -> Result<()>,
) -> Result<()> {
    #[derive(Deserialize)]
    #[serde(deny_unknown_fields)]
    struct Block {
        table: String,
        start_byte: u64,
        data: String,
    }
    let mut bytes = 0_u64;
    let mut rows = 0_u64;
    let mut digest = Sha256::new();
    let mut pending = String::new();
    for block in 0..table.blocks {
        let value = evidence(
            store,
            &format!("mempal/{generation}/table/{name}/{block:020}"),
        )?;
        let block: Block = serde_json::from_str(&value.content)?;
        if block.table != name || block.start_byte != bytes || block.data.len() > 32 * 1024 {
            return Err(invalid());
        }
        bytes += block.data.len() as u64;
        digest.update(block.data.as_bytes());
        pending.push_str(&block.data);
        let mut consumed = 0;
        while let Some(end) = pending[consumed..].find('\n') {
            consume(&pending[consumed..consumed + end])?;
            consumed += end + 1;
            rows += 1;
        }
        pending.drain(..consumed);
        if pending.len() > 128 * crate::storage::MIB {
            return Err(invalid());
        }
    }
    let (expected_rows, expected_bytes, expected_hash) = table
        .delta
        .as_ref()
        .map_or((table.rows, table.bytes, &table.sha256), |d| {
            (d.rows, d.bytes, &d.sha256)
        });
    if !pending.is_empty()
        || rows != expected_rows
        || bytes != expected_bytes
        || hex(&digest.finalize()) != *expected_hash
    {
        return Err(invalid());
    }
    Ok(())
}

fn rebuild(index: &mut Index<'_>, chain: &[(String, Migration)]) -> Result<()> {
    let schema: BTreeMap<String, Vec<serde_json::Value>> =
        serde_json::from_str(include_str!("schema.json"))?;
    for (_, migration) in chain {
        let generation = migration
            .incremental
            .as_ref()
            .map_or(migration.source_sha256.as_str(), |c| c.generation.as_str());
        for (name, table) in &migration.tables {
            let expected = schema.get(name).ok_or_else(invalid)?;
            if table.columns
                != expected
                    .iter()
                    .map(|v| v["name"].as_str().unwrap_or("").to_owned())
                    .collect::<Vec<_>>()
                || table.delta.is_some() != migration.incremental.is_some()
            {
                return Err(invalid());
            }
            let store = index.store.clone();
            read_rows(&store, generation, name, table, |line| {
                let (key, origin, row) = if migration.incremental.is_some() {
                    let delta: DeltaRow = serde_json::from_str(line)?;
                    (delta.key, delta.origin, delta.row)
                } else {
                    let row: Row = serde_json::from_str(line)?;
                    let key = row_key(name, &table.columns, &row)?;
                    let origin = format!("mempal/{}/{name}/{}", migration.source_sha256, row.rowid);
                    (key, origin, Some(row))
                };
                if key.len() != 64 || !key.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err(invalid());
                }
                let previous = index.get(name, &key)?;
                if migration.incremental.is_none() && previous.is_some() {
                    return Err(StorageError::new("INVALID_MEMPAL_KEY"));
                }
                if let Some(previous) = &previous {
                    if previous.origin != origin {
                        return Err(invalid());
                    }
                } else if let Some(chain) = &migration.incremental
                    && origin != format!("mempal/{}/keys/{key}", chain.root)
                {
                    return Err(invalid());
                }
                let state = if let Some(row) = row {
                    if row_key(name, &table.columns, &row)? != key {
                        return Err(invalid());
                    }
                    RowState {
                        origin,
                        fingerprint: hash(&serde_json::to_vec(&row.values)?),
                        rowid: row.rowid,
                        present: true,
                        seen: false,
                        parts: candidate_parts(name, &table.columns, &row)?,
                    }
                } else {
                    let mut state = previous.filter(|s| s.present).ok_or_else(invalid)?;
                    state.present = false;
                    state
                };
                index.set(name, &key, state)
            })?;
        }
    }
    index.flush()
}

struct Rows {
    pending: String,
    table: Table,
    digest: Sha256,
}

impl Rows {
    fn new(columns: Vec<String>) -> Self {
        Self {
            pending: String::new(),
            digest: Sha256::new(),
            table: Table {
                columns,
                rows: 0,
                blocks: 0,
                bytes: 0,
                sha256: String::new(),
                delta: None,
            },
        }
    }

    fn push(
        &mut self,
        writer: &mut Writer,
        prefix: &str,
        name: &str,
        row: &DeltaRow,
    ) -> Result<()> {
        self.pending.push_str(&serde_json::to_string(row)?);
        self.pending.push('\n');
        self.table.rows += 1;
        let mut consumed = 0;
        while self.pending.len() - consumed >= 32 * 1024 {
            let mut end = consumed + 32 * 1024;
            while !self.pending.is_char_boundary(end) {
                end -= 1;
            }
            emit_block(
                writer,
                prefix,
                name,
                &self.pending[consumed..end],
                &mut self.table,
                &mut self.digest,
            )?;
            consumed = end;
        }
        self.pending.drain(..consumed);
        Ok(())
    }

    fn finish(mut self, writer: &mut Writer, prefix: &str, name: &str) -> Result<Table> {
        if !self.pending.is_empty() {
            emit_block(
                writer,
                prefix,
                name,
                &self.pending,
                &mut self.table,
                &mut self.digest,
            )?;
        }
        self.table.sha256 = hex(&self.digest.finalize());
        Ok(self.table)
    }
}

fn convert(
    snapshot: &mut Snapshot,
    writer: &mut Writer,
    index: &mut Index<'_>,
    previous_hash: &str,
) -> Result<Migration> {
    let mut chain = lineage(&index.store)?;
    let root = chain[0].1.source_sha256.clone();
    let previous = chain.last().ok_or_else(invalid)?.0.clone();
    let generation = hash(&serde_json::to_vec(&(previous_hash, &snapshot.sha256))?);
    writer.seed(&index.store, &generation)?;
    if chain
        .last()
        .is_some_and(|(_, m)| m.source_sha256 == snapshot.sha256)
    {
        snapshot.verify_unchanged()?;
        return Ok(chain.pop().ok_or_else(invalid)?.1);
    }
    rebuild(index, &chain)?;
    let observed_at_ms = writer.observed_at_ms;
    let mut projection = projection::Summary::default();
    let mut tables = BTreeMap::new();
    let mut columns_by_table = snapshot.tables.clone();
    for (_, migration) in &chain {
        for (name, table) in &migration.tables {
            columns_by_table
                .entry(name.clone())
                .or_insert_with(|| table.columns.clone());
        }
    }
    for (name, columns) in columns_by_table {
        let prefix = format!("mempal/{generation}/table/{name}");
        let mut delta = Rows::new(columns.clone());
        let mut row_count = 0;
        let mut byte_count = 0;
        let mut digest = Sha256::new();
        if snapshot.tables.contains_key(&name) {
            snapshot.rows(&name, |row| {
                let key = row_key(&name, &columns, &row)?;
                let previous = index.get(&name, &key)?;
                if previous.as_ref().is_some_and(|s| s.seen) {
                    return Err(StorageError::new("INVALID_MEMPAL_KEY"));
                }
                let origin = previous
                    .as_ref()
                    .map_or_else(|| format!("mempal/{root}/keys/{key}"), |s| s.origin.clone());
                let fingerprint = hash(&serde_json::to_vec(&row.values)?);
                let semantic_change = previous
                    .as_ref()
                    .is_none_or(|s| !s.present || s.fingerprint != fingerprint);
                let changed =
                    semantic_change || previous.as_ref().is_some_and(|s| s.rowid != row.rowid);
                let mut bytes = serde_json::to_vec(&row)?;
                bytes.push(b'\n');
                digest.update(&bytes);
                byte_count += bytes.len() as u64;
                row_count += 1;
                if semantic_change {
                    writer.projection = Some(ProjectionContext {
                        store: index.store.clone(),
                        from: format!("mempal/{}/{name}/{}", snapshot.sha256, row.rowid),
                        origin: origin.clone(),
                        generation: generation.clone(),
                        retained: Default::default(),
                    });
                    projection::row(
                        &name,
                        &row,
                        &columns,
                        &snapshot.sha256,
                        observed_at_ms,
                        writer,
                        &mut projection,
                    )?;
                    let context = writer.projection.take().ok_or_else(invalid)?;
                    if let Some(previous) = &previous {
                        withdraw(writer, &index.store, previous, &context.retained)?;
                    }
                }
                index.set(
                    &name,
                    &key,
                    RowState {
                        origin: origin.clone(),
                        fingerprint,
                        rowid: row.rowid,
                        present: true,
                        seen: true,
                        parts: candidate_parts(&name, &columns, &row)?,
                    },
                )?;
                if changed {
                    delta.push(
                        writer,
                        &prefix,
                        &name,
                        &DeltaRow {
                            key,
                            origin,
                            row: Some(row),
                        },
                    )?;
                }
                Ok(())
            })?;
        }
        let store = index.store.clone();
        index.missing(&name, |key, state| {
            withdraw(writer, &store, state, &Default::default())?;
            delta.push(
                writer,
                &prefix,
                &name,
                &DeltaRow {
                    key: key.into(),
                    origin: state.origin.clone(),
                    row: None,
                },
            )
        })?;
        let mut table = delta.finish(writer, &prefix, &name)?;
        table.delta = Some(Delta {
            rows: table.rows,
            bytes: table.bytes,
            sha256: table.sha256,
        });
        table.rows = row_count;
        table.bytes = byte_count;
        table.sha256 = hex(&digest.finalize());
        writer.push(prefix, &table)?;
        tables.insert(name, table);
    }
    snapshot.verify_unchanged()?;
    let migration = Migration {
        mapping_version: 3,
        source_mapping_commit: "4593a5c5e6a9ea1140256cd42fa5838511a24e14".into(),
        schema_axes: [21, 28],
        source_sha256: snapshot.sha256.clone(),
        source_bytes: snapshot.bytes(),
        observed_at_ms,
        tables,
        excluded_derived_tables: snapshot.excluded_derived_tables.clone(),
        projection,
        incremental: Some(Lineage {
            root,
            previous,
            previous_archive_sha256: previous_hash.into(),
            generation: generation.clone(),
        }),
    };
    writer.push(format!("mempal/{generation}/migration"), &migration)?;
    Ok(migration)
}

/// Extend a completed mempal lineage using actual typed primary keys, retaining original receipts.
pub fn migrate_mempal_previous(
    source: &Path,
    destination: &Path,
    previous: &Path,
) -> Result<MempalArchive> {
    // The legacy reader is synchronous; its disk index uses a separate bounded runtime.
    std::thread::scope(|scope| {
        scope
            .spawn(|| run(source, destination, previous))
            .join()
            .map_err(|_| StorageError::new("MEMPAL_CONVERSION_FAILED"))?
    })
}

fn run(source: &Path, destination: &Path, previous: &Path) -> Result<MempalArchive> {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()?;
    let mut snapshot = Snapshot::open(source)?;
    snapshot.check_primary_keys()?;
    let previous = previous.canonicalize()?;
    let parent = destination
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
        .canonicalize()?;
    let output = parent.join(destination.file_name().ok_or_else(invalid)?);
    if previous.starts_with(&output) || output.starts_with(&previous) {
        return Err(StorageError::new("UNSAFE_STORAGE_PATH"));
    }
    let mut manifest = Vec::new();
    crate::storage::import::regular_file(&previous.join("manifest.json"))?
        .take(64 * 1024 + 1)
        .read_to_end(&mut manifest)?;
    if manifest.len() > 64 * 1024 {
        return Err(StorageError::new("INVALID_ARCHIVE"));
    }
    let previous_hash = hash(&manifest);
    let observed_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StorageError::new("INVALID_CLOCK"))?
        .as_millis()
        .try_into()
        .map_err(|_| StorageError::new("INVALID_CLOCK"))?;
    let mut writer = Writer::open(
        destination,
        &snapshot,
        observed_at_ms,
        Some(previous_hash.clone()),
    )?;
    let scratch = Scratch::open(
        &writer.index_path(),
        &hash(&serde_json::to_vec(&(&previous_hash, &snapshot.sha256))?),
    )?;
    let config = scratch.config()?;
    let imported = runtime.block_on(Storage::import_archive(&config, &previous));
    if let Err(error) = imported {
        scratch.clean()?;
        return Err(error);
    }
    let store = Arc::new(runtime.block_on(Storage::open(&config))?);
    let result = (|| {
        if store
            .import_identity()?
            .is_none_or(|identity| identity.archive_sha256 != previous_hash)
        {
            return Err(invalid());
        }
        let mut index = Index::new(store.clone(), &runtime);
        convert(&mut snapshot, &mut writer, &mut index, &previous_hash)
    })();
    writer.projection = None;
    let closed = runtime.block_on(store.close());
    drop(store);
    closed?;
    scratch.clean()?;
    let migration = result?;
    Ok(MempalArchive {
        archive: writer.finish()?,
        migration,
    })
}
