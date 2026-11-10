//! A one-way legacy reader; imported rows never become execution authority.

mod projection;
mod reader;
mod writer;

use std::collections::BTreeMap;
use std::path::Path;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Serialize;
use sha2::{Digest, Sha256};

use super::{ArchiveManifest, Result, StorageError};
use reader::Snapshot;
use writer::Writer;

#[derive(Serialize)]
pub struct MempalArchive {
    archive: ArchiveManifest,
    migration: Migration,
}

#[derive(Serialize)]
struct Migration {
    mapping_version: u32,
    source_mapping_commit: &'static str,
    schema_axes: [u32; 2],
    source_sha256: String,
    source_bytes: u64,
    observed_at_ms: u64,
    tables: BTreeMap<String, Table>,
    excluded_derived_tables: Vec<String>,
    projection: projection::Summary,
}

#[derive(Serialize)]
struct Table {
    columns: Vec<String>,
    rows: u64,
    blocks: u64,
    bytes: u64,
    sha256: String,
}

#[derive(Serialize)]
struct Block<'a> {
    table: &'a str,
    start_byte: u64,
    data: &'a str,
}

fn emit_block(
    writer: &mut Writer,
    prefix: &str,
    name: &str,
    data: &str,
    table: &mut Table,
    digest: &mut Sha256,
) -> Result<()> {
    writer.push(
        format!("{prefix}/{:020}", table.blocks),
        &Block {
            table: name,
            start_byte: table.bytes,
            data,
        },
    )?;
    digest.update(data.as_bytes());
    table.bytes += data.len() as u64;
    table.blocks += 1;
    Ok(())
}

/// Convert an independently prepared immutable SQLite snapshot to a neutral archive.
pub fn migrate_mempal(source: &Path, destination: &Path) -> Result<MempalArchive> {
    let mut snapshot = Snapshot::open(source)?;
    let observed_at_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| StorageError::new("INVALID_CLOCK"))?
        .as_millis()
        .try_into()
        .map_err(|_| StorageError::new("INVALID_CLOCK"))?;
    let mut writer = Writer::new(destination, &snapshot.sha256, observed_at_ms)?;
    let mut tables = BTreeMap::new();
    let mut projection = projection::Summary::default();
    for (name, columns) in &snapshot.tables {
        let mut table = Table {
            columns: columns.clone(),
            rows: 0,
            blocks: 0,
            bytes: 0,
            sha256: String::new(),
        };
        let prefix = format!("mempal/{}/table/{name}", snapshot.sha256);
        let mut digest = Sha256::new();
        let mut pending = String::new();
        snapshot.rows(name, |row| {
            let mut line = serde_json::to_string(&row)?;
            line.push('\n');
            pending.push_str(&line);
            let mut consumed = 0;
            while pending.len() - consumed >= 32 * 1024 {
                let mut end = consumed + 32 * 1024;
                while !pending.is_char_boundary(end) {
                    end -= 1;
                }
                emit_block(
                    &mut writer,
                    &prefix,
                    name,
                    &pending[consumed..end],
                    &mut table,
                    &mut digest,
                )?;
                consumed = end;
            }
            pending.drain(..consumed);
            projection::row(
                name,
                &row,
                columns,
                &snapshot.sha256,
                observed_at_ms,
                &mut writer,
                &mut projection,
            )?;
            table.rows += 1;
            Ok(())
        })?;
        if !pending.is_empty() {
            emit_block(
                &mut writer,
                &prefix,
                name,
                &pending,
                &mut table,
                &mut digest,
            )?;
        }
        table.sha256 = reader::hex(&digest.finalize());
        writer.push(prefix, &table)?;
        tables.insert(name.clone(), table);
    }
    snapshot.verify_unchanged()?;
    let migration = Migration {
        mapping_version: 2,
        source_mapping_commit: "4593a5c5e6a9ea1140256cd42fa5838511a24e14",
        schema_axes: [21, 28],
        source_sha256: snapshot.sha256.clone(),
        source_bytes: snapshot.bytes(),
        observed_at_ms,
        tables,
        excluded_derived_tables: snapshot.excluded_derived_tables,
        projection,
    };
    writer.push(format!("mempal/{}/migration", snapshot.sha256), &migration)?;
    Ok(MempalArchive {
        archive: writer.finish()?,
        migration,
    })
}
