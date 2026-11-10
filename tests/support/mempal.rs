use super::*;

fn convert(fixture: &Fixture, source: &Path, destination: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_akasha-otsuge"))
        .arg("--config")
        .arg(fixture.0.join("config.toml"))
        .args(["migrate", "mempal"])
        .arg(source)
        .arg(destination)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

fn legacy_snapshot(path: &Path, text: &str) {
    let db = rusqlite::Connection::open(path).unwrap();
    db.execute_batch(include_str!("../fixtures/mempal21-28.sql"))
        .unwrap();
    db.execute(
        "INSERT INTO drawers(id,content,wing,added_at,project_id,provenance) VALUES(?1,?2,'test','2026-01-02 03:04:05','project-a','human')",
        rusqlite::params!["legacy-one", text],
    )
    .unwrap();
    db.execute(
        "INSERT INTO drawers(id,content,wing,added_at,project_id,deleted_at,status) VALUES('deleted-two','retained deleted text','test','2026-01-02 03:04:05','project-b','2026-02-01 00:00:00','retired')",
        [],
    )
    .unwrap();
    db.execute(
        "INSERT INTO fork_ext_meta(key,value) VALUES('custom-binary',?1)",
        [vec![0_u8, 255, 0, 1]],
    )
    .unwrap();
    db.close().unwrap();
}

fn stored_json(store: &Storage, id: &str) -> Value {
    let revision = store
        .current(&RecordKey {
            kind: RecordKind::Evidence,
            id: id.into(),
        })
        .unwrap()
        .unwrap();
    serde_json::from_str(&revision.content).unwrap()
}

fn table_rows(store: &Storage, source: &str, table: &str) -> (Value, Vec<Value>) {
    let prefix = format!("mempal/{source}/table/{table}");
    let descriptor = stored_json(store, &prefix);
    let mut rows = String::new();
    for index in 0..descriptor["blocks"].as_u64().unwrap() {
        let block = stored_json(store, &format!("{prefix}/{index:020}"));
        assert_eq!(block["table"], table);
        assert_eq!(block["start_byte"], rows.len());
        rows.push_str(block["data"].as_str().unwrap());
    }
    assert_eq!(descriptor["bytes"], rows.len());
    assert_eq!(descriptor["sha256"], digest(rows.as_bytes()));
    let values: Vec<_> = rows
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(descriptor["rows"], values.len());
    (descriptor, values)
}

fn refused(fixture: &Fixture, source: &Path, code: &str) {
    let destination = fixture.0.join("refused-archive");
    let original = std::fs::read(source).unwrap();
    let output = convert(fixture, source, &destination);
    assert!(!output.status.success());
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], code);
    assert!(!destination.exists());
    assert_eq!(std::fs::read(source).unwrap(), original);
}

#[test]
fn mempal_refuses_unknown_source_versions() {
    let fixture = Fixture::new();
    let source = fixture.0.join("future.db");
    legacy_snapshot(&source, "source stays unchanged");
    let db = rusqlite::Connection::open(&source).unwrap();
    db.execute_batch("PRAGMA user_version=22").unwrap();
    db.close().unwrap();
    refused(&fixture, &source, "UNSUPPORTED_MEMPAL_SCHEMA");
}

#[test]
fn mempal_refuses_snapshot_with_pending_wal() {
    let fixture = Fixture::new();
    let source = fixture.0.join("pending.db");
    legacy_snapshot(&source, "source stays unchanged");
    std::fs::write(fixture.0.join("pending.db-wal"), b"unconsumed WAL").unwrap();
    refused(&fixture, &source, "UNSAFE_MEMPAL_SNAPSHOT");
    assert_eq!(
        std::fs::read(fixture.0.join("pending.db-wal")).unwrap(),
        b"unconsumed WAL"
    );
}

#[test]
fn mempal_refuses_corrupt_sqlite_input() {
    let fixture = Fixture::new();
    let source = fixture.0.join("corrupt.db");
    std::fs::write(&source, b"not a SQLite database").unwrap();
    refused(&fixture, &source, "INVALID_MEMPAL_DATABASE");
}

#[tokio::test]
async fn mempal_conversion_preserves_typed_rows_large_fields_and_source_bytes() {
    let fixture = Fixture::new();
    let source = fixture.0.join("legacy.db");
    let text = "原文\0é\n".repeat(12_000);
    legacy_snapshot(&source, &text);
    let original = std::fs::read(&source).unwrap();
    let source_hash = digest(&original);
    let archive = fixture.0.join("legacy-archive");
    let output = convert(&fixture, &source, &archive);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["data"]["migration"]["source_sha256"], source_hash);
    assert_eq!(report["data"]["migration"]["schema_axes"], json!([21, 28]));
    Storage::import_archive(&fixture.config(), &archive)
        .await
        .unwrap();
    let store = Storage::open(&fixture.config()).await.unwrap();
    let (descriptor, rows) = table_rows(&store, &source_hash, "drawers");
    assert_eq!(rows.len(), 2);
    assert!(descriptor["blocks"].as_u64().unwrap() > 1);
    let columns = descriptor["columns"].as_array().unwrap();
    let cell = |row: usize, name: &str| {
        &rows[row]["values"][columns.iter().position(|column| column == name).unwrap()]
    };
    assert_eq!(cell(0, "id"), &json!({"type":"text","value":"legacy-one"}));
    assert_eq!(cell(0, "content"), &json!({"type":"text","value":text}));
    assert_eq!(cell(0, "room"), &json!({"type":"null"}));
    assert_eq!(
        cell(0, "confidence"),
        &json!({"type":"real","value":"3fe0000000000000"})
    );
    assert_eq!(cell(0, "merge_count"), &json!({"type":"integer","value":0}));
    assert_eq!(cell(1, "status"), &json!({"type":"text","value":"retired"}));
    let (_, metadata) = table_rows(&store, &source_hash, "fork_ext_meta");
    assert_eq!(
        metadata[1]["values"][1],
        json!({"type":"blob","value":"00ff0001"})
    );
    store.close().await.unwrap();
    assert_eq!(std::fs::read(&source).unwrap(), original);
    for suffix in ["-wal", "-shm", "-journal"] {
        assert!(!fixture.0.join(format!("legacy.db{suffix}")).exists());
    }
}
