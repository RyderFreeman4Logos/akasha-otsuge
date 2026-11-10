use std::collections::{BTreeMap, BTreeSet};
use std::fs::{self, File, Metadata, OpenOptions};
use std::io::{Read, Seek};
use std::os::fd::AsRawFd;
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::{Path, PathBuf};

use rusqlite::{Connection, OpenFlags, limits::Limit, types::ValueRef};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::storage::{Result, StorageError};

const DERIVED: &[&str] = &[
    "conversation_turn_vectors",
    "drawer_vectors",
    "drawer_vectors_auxiliary",
    "drawer_vectors_chunks",
    "drawer_vectors_info",
    "drawer_vectors_rowids",
    "drawer_vectors_vector_chunks00",
    "drawers_fts",
    "drawers_fts_config",
    "drawers_fts_data",
    "drawers_fts_docsize",
    "drawers_fts_idx",
    "sqlite_sequence",
];

fn invalid(_: rusqlite::Error) -> StorageError {
    StorageError::new("INVALID_MEMPAL_DATABASE")
}

fn unsupported() -> StorageError {
    StorageError::new("UNSUPPORTED_MEMPAL_SCHEMA")
}

fn unsafe_snapshot() -> StorageError {
    StorageError::new("UNSAFE_MEMPAL_SNAPSHOT")
}

fn no_sidecars(path: &Path) -> Result<()> {
    for suffix in ["-wal", "-shm", "-journal"] {
        let mut name = path.as_os_str().to_owned();
        name.push(suffix);
        match fs::symlink_metadata(name) {
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
            Ok(_) => return Err(unsafe_snapshot()),
        }
    }
    Ok(())
}

pub(super) fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn file_hash(file: &mut File) -> Result<String> {
    file.rewind()?;
    let mut digest = Sha256::new();
    let mut buffer = [0_u8; 128 * 1024];
    loop {
        let length = file.read(&mut buffer)?;
        if length == 0 {
            break;
        }
        digest.update(&buffer[..length]);
    }
    Ok(hex(&digest.finalize()))
}

fn identity(metadata: &Metadata) -> (u64, u64, u64, i64, i64) {
    (
        metadata.dev(),
        metadata.ino(),
        metadata.len(),
        metadata.mtime(),
        metadata.mtime_nsec(),
    )
}

#[derive(Deserialize, Eq, PartialEq)]
struct Column {
    name: String,
    #[serde(rename = "type")]
    data_type: String,
}

#[derive(Clone, Deserialize, Serialize)]
#[serde(tag = "type", content = "value", rename_all = "snake_case")]
pub(super) enum Cell {
    Null,
    Integer(i64),
    Real(String),
    Text(String),
    Blob(String),
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Row {
    pub rowid: i64,
    pub values: Vec<Cell>,
}

pub(super) struct Snapshot {
    file: File,
    pub(super) path: PathBuf,
    before: Metadata,
    connection: Connection,
    pub sha256: String,
    pub tables: BTreeMap<String, Vec<String>>,
    pub excluded_derived_tables: Vec<String>,
}

impl Snapshot {
    pub(super) fn check_primary_keys(&self) -> Result<()> {
        for table in self.tables.keys() {
            let mut query = self
                .connection
                .prepare("SELECT name FROM pragma_table_xinfo(?1) WHERE pk > 0 ORDER BY pk")
                .map_err(invalid)?;
            let actual: Vec<String> = query
                .query_map([table], |row| row.get(0))
                .map_err(invalid)?
                .collect::<rusqlite::Result<_>>()
                .map_err(invalid)?;
            if actual != primary_key(table) {
                return Err(unsupported());
            }
        }
        Ok(())
    }

    pub fn open(path: &Path) -> Result<Self> {
        let mut file = OpenOptions::new()
            .read(true)
            .custom_flags(
                (rustix::fs::OFlags::NOFOLLOW | rustix::fs::OFlags::NONBLOCK).bits() as i32,
            )
            .open(path)?;
        let before = file.metadata()?;
        if !before.is_file() {
            return Err(unsafe_snapshot());
        }
        let path = path.canonicalize()?;
        if identity(&fs::metadata(&path)?) != identity(&before) {
            return Err(unsafe_snapshot());
        }
        no_sidecars(&path)?;
        let mut header = [0; 16];
        if file.read_exact(&mut header).is_err() || &header != b"SQLite format 3\0" {
            return Err(StorageError::new("INVALID_MEMPAL_DATABASE"));
        }
        let sha256 = file_hash(&mut file)?;
        // The retained descriptor pins the caller's snapshot inode, not a later path replacement.
        let uri = format!(
            "file:/proc/self/fd/{}?mode=ro&immutable=1",
            file.as_raw_fd()
        );
        let connection = Connection::open_with_flags(
            uri,
            OpenFlags::SQLITE_OPEN_READ_ONLY
                | OpenFlags::SQLITE_OPEN_URI
                | OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .map_err(invalid)?;
        connection
            .set_limit(Limit::SQLITE_LIMIT_LENGTH, 16 * 1024 * 1024)
            .map_err(invalid)?;
        connection
            .set_limit(Limit::SQLITE_LIMIT_ATTACHED, 0)
            .map_err(invalid)?;
        connection.execute_batch("PRAGMA query_only=ON; PRAGMA trusted_schema=OFF; PRAGMA cache_size=-2048; PRAGMA temp_store=MEMORY; BEGIN;").map_err(invalid)?;
        let version: i64 = connection
            .query_row("PRAGMA user_version", [], |row| row.get(0))
            .map_err(invalid)?;
        if version != 21 {
            return Err(unsupported());
        }
        let extension: String = connection
            .query_row(
                "SELECT value FROM fork_ext_meta WHERE key='fork_ext_version'",
                [],
                |row| row.get(0),
            )
            .map_err(|_| unsupported())?;
        if extension != "28" {
            return Err(unsupported());
        }
        let expected: BTreeMap<String, Vec<Column>> =
            serde_json::from_str(include_str!("schema.json"))?;
        let mut tables = BTreeMap::new();
        let mut excluded_derived_tables = Vec::new();
        {
            let mut statement = connection.prepare("SELECT type,name,sql FROM sqlite_schema WHERE type IN ('table','view') ORDER BY name LIMIT 1001").map_err(invalid)?;
            let mut rows = statement.query([]).map_err(invalid)?;
            let mut seen = BTreeSet::new();
            while let Some(row) = rows.next().map_err(invalid)? {
                let kind: String = row.get(0).map_err(invalid)?;
                let name: String = row.get(1).map_err(invalid)?;
                let sql: String = row.get(2).map_err(invalid)?;
                if !seen.insert(name.clone()) || seen.len() > 1000 {
                    return Err(unsupported());
                }
                if DERIVED.contains(&name.as_str()) {
                    excluded_derived_tables.push(name);
                    continue;
                }
                if kind != "table"
                    || sql
                        .trim_start()
                        .to_ascii_uppercase()
                        .starts_with("CREATE VIRTUAL TABLE")
                {
                    return Err(unsupported());
                }
                let columns = expected.get(&name).ok_or_else(unsupported)?;
                let mut info = connection
                    .prepare("SELECT name,type,hidden FROM pragma_table_xinfo(?1) ORDER BY cid")
                    .map_err(invalid)?;
                let actual: Vec<(Column, i64)> = info
                    .query_map([&name], |r| {
                        Ok((
                            Column {
                                name: r.get(0)?,
                                data_type: r.get::<_, String>(1)?.to_ascii_uppercase(),
                            },
                            r.get(2)?,
                        ))
                    })
                    .map_err(invalid)?
                    .collect::<rusqlite::Result<_>>()
                    .map_err(invalid)?;
                if actual.len() != columns.len()
                    || actual
                        .iter()
                        .zip(columns)
                        .any(|((column, hidden), expected)| *hidden != 0 || column != expected)
                {
                    return Err(unsupported());
                }
                tables.insert(
                    name,
                    columns.iter().map(|column| column.name.clone()).collect(),
                );
            }
            if expected
                .keys()
                .any(|name| name != "historical_rejudge_work_items" && !tables.contains_key(name))
            {
                return Err(unsupported());
            }
        }
        let check: String = connection
            .query_row("PRAGMA quick_check", [], |row| row.get(0))
            .map_err(invalid)?;
        if check != "ok" {
            return Err(StorageError::new("INVALID_MEMPAL_DATABASE"));
        }
        Ok(Self {
            file,
            path,
            before,
            connection,
            sha256,
            tables,
            excluded_derived_tables,
        })
    }

    pub fn file_identity(&self) -> (u64, u64) {
        (self.before.dev(), self.before.ino())
    }

    pub fn bytes(&self) -> u64 {
        self.before.len()
    }

    pub fn rows(&self, table: &str, mut consume: impl FnMut(Row) -> Result<()>) -> Result<()> {
        let columns = self.tables.get(table).ok_or_else(unsupported)?;
        let names = columns
            .iter()
            .map(|name| format!("\"{name}\""))
            .collect::<Vec<_>>()
            .join(",");
        let mut statement = self
            .connection
            .prepare(&format!(
                "SELECT rowid,{names} FROM \"{table}\" ORDER BY rowid"
            ))
            .map_err(invalid)?;
        let mut rows = statement.query([]).map_err(invalid)?;
        while let Some(row) = rows.next().map_err(invalid)? {
            let mut values = Vec::with_capacity(columns.len());
            for index in 1..=columns.len() {
                values.push(match row.get_ref(index).map_err(invalid)? {
                    ValueRef::Null => Cell::Null,
                    ValueRef::Integer(value) => Cell::Integer(value),
                    ValueRef::Real(value) => Cell::Real(format!("{:016x}", value.to_bits())),
                    ValueRef::Text(value) => Cell::Text(
                        std::str::from_utf8(value)
                            .map_err(|_| StorageError::new("INVALID_MEMPAL_DATABASE"))?
                            .into(),
                    ),
                    ValueRef::Blob(value) => Cell::Blob(hex(value)),
                });
            }
            consume(Row {
                rowid: row.get(0).map_err(invalid)?,
                values,
            })?;
        }
        Ok(())
    }

    pub fn verify_unchanged(&mut self) -> Result<()> {
        no_sidecars(&self.path)?;
        if identity(&self.file.metadata()?) != identity(&self.before)
            || identity(&fs::symlink_metadata(&self.path)?) != identity(&self.before)
            || file_hash(&mut self.file)? != self.sha256
        {
            return Err(unsafe_snapshot());
        }
        Ok(())
    }
}

pub(super) fn primary_key(table: &str) -> &'static [&'static str] {
    match table {
        "failure_events" => &["event_id"],
        "foresights" => &["drawer_id"],
        "fork_ext_meta" => &["key"],
        "historical_rejudge_work_items" => &["run_id", "drawer_rowid"],
        "leases" => &["resource_path"],
        "patterns" => &["pattern_id"],
        "pending_message_completions" => &["message_id"],
        "reindex_progress" => &["source_path"],
        "runtime_writer_lease_generations" | "runtime_writer_leases" => &["name"],
        "skills" => &["skill_id"],
        "taxonomy" => &["wing", "room"],
        _ => &["id"],
    }
}

pub(super) fn row_key(table: &str, columns: &[String], row: &Row) -> Result<String> {
    if columns.len() != row.values.len() {
        return Err(StorageError::new("INVALID_MEMPAL_VALUE"));
    }
    let values = primary_key(table)
        .iter()
        .map(|name| {
            let index = columns
                .iter()
                .position(|column| column == name)
                .ok_or_else(|| StorageError::new("UNSUPPORTED_MEMPAL_SCHEMA"))?;
            let value = &row.values[index];
            if matches!(value, Cell::Null) {
                return Err(StorageError::new("INVALID_MEMPAL_KEY"));
            }
            Ok(value)
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(crate::storage::hash(&serde_json::to_vec(&(table, values))?))
}
