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

fn archive_payloads(archive: &Path) -> Vec<Vec<u8>> {
    ["blobs.bin", "commits.jsonl", "manifest.json"]
        .map(|name| std::fs::read(archive.join(name)).unwrap())
        .to_vec()
}

#[test]
fn mempal_conversion_reuses_completed_output_and_recovers_a_partial_prefix() {
    let fixture = Fixture::new();
    let source = fixture.0.join("repeat.db");
    legacy_snapshot(&source, &"original\0原文".repeat(3000));
    let archive = fixture.0.join("repeat-archive");
    let first = convert(&fixture, &source, &archive);
    assert!(first.status.success());
    let original = archive_payloads(&archive);
    let second = convert(&fixture, &source, &archive);
    assert!(
        second.status.success(),
        "{}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert_eq!(first.stdout, second.stdout);
    assert_eq!(archive_payloads(&archive), original);
    std::fs::remove_file(archive.join("manifest.json")).unwrap();
    for (index, name) in ["blobs.bin", "commits.jsonl"].iter().enumerate() {
        std::fs::OpenOptions::new()
            .write(true)
            .open(archive.join(name))
            .unwrap()
            .set_len((original[index].len() / 2 + 7) as u64)
            .unwrap();
    }
    let resumed = convert(&fixture, &source, &archive);
    assert!(
        resumed.status.success(),
        "{}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    assert_eq!(resumed.stdout, first.stdout);
    assert_eq!(archive_payloads(&archive), original);
}

#[test]
fn mempal_conversion_rejects_different_sources_corrupt_output_and_busy_writers() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source.db");
    legacy_snapshot(&source, "retained text");
    let archive = fixture.0.join("conflict-archive");
    assert!(convert(&fixture, &source, &archive).status.success());
    let original = archive_payloads(&archive);
    let different = fixture.0.join("different.db");
    legacy_snapshot(&different, "different source");
    let output = convert(&fixture, &different, &archive);
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "MEMPAL_CONVERSION_CONFLICT");
    assert_eq!(archive_payloads(&archive), original);
    let lock = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open(archive.join(".mempal-convert.lock"))
        .unwrap();
    lock.lock().unwrap();
    let output = convert(&fixture, &source, &archive);
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "MEMPAL_CONVERSION_BUSY");
    assert_eq!(archive_payloads(&archive), original);
    drop(lock);
    let mut corrupted = original[0].clone();
    corrupted[0] ^= 1;
    std::fs::write(archive.join("blobs.bin"), &corrupted).unwrap();
    let before = archive_payloads(&archive);
    let output = convert(&fixture, &source, &archive);
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "MEMPAL_CONVERSION_CONFLICT");
    assert_eq!(archive_payloads(&archive), before);
}

#[test]
fn mempal_conversion_refuses_unsafe_resume_paths_without_replacing_them() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    for mutation in [
        "dangling-manifest",
        "symlink-blob",
        "hardlink-blob",
        "public-directory",
        "public-file",
    ] {
        let fixture = Fixture::new();
        let source = fixture.0.join("source.db");
        legacy_snapshot(&source, "retained text");
        let source_before = std::fs::read(&source).unwrap();
        let archive = fixture.0.join("archive");
        assert!(convert(&fixture, &source, &archive).status.success());
        let external = fixture.0.join("external");
        let original_blob = std::fs::read(archive.join("blobs.bin")).unwrap();
        match mutation {
            "dangling-manifest" => {
                std::fs::remove_file(archive.join("manifest.json")).unwrap();
                symlink(&external, archive.join("manifest.json")).unwrap();
            }
            "symlink-blob" | "hardlink-blob" => {
                std::fs::rename(archive.join("blobs.bin"), &external).unwrap();
                if mutation == "symlink-blob" {
                    symlink(&external, archive.join("blobs.bin")).unwrap();
                } else {
                    std::fs::hard_link(&external, archive.join("blobs.bin")).unwrap();
                }
            }
            "public-directory" => {
                std::fs::set_permissions(&archive, std::fs::Permissions::from_mode(0o755)).unwrap()
            }
            "public-file" => std::fs::set_permissions(
                archive.join("blobs.bin"),
                std::fs::Permissions::from_mode(0o644),
            )
            .unwrap(),
            _ => unreachable!(),
        }
        let output = convert(&fixture, &source, &archive);
        assert!(!output.status.success(), "accepted {mutation}");
        assert_eq!(std::fs::read(&source).unwrap(), source_before);
        assert_eq!(
            std::fs::read(archive.join("blobs.bin")).unwrap(),
            original_blob
        );
        if mutation == "dangling-manifest" {
            assert!(archive.join("manifest.json").is_symlink());
            assert!(!external.exists());
        } else if mutation.ends_with("-blob") {
            assert_eq!(std::fs::read(external).unwrap(), original_blob);
        }
    }
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

#[tokio::test]
async fn mempal_projects_sources_and_candidates_without_approving_or_reviving_legacy_rows() {
    use akasha_otsuge::domain::{
        Actor, Authority, Domain, Lifecycle, MemoryKind, Origin, ReadContext,
    };

    let fixture = Fixture::new();
    let source = fixture.0.join("legacy.db");
    let text = "  imported source\0原文\n";
    legacy_snapshot(&source, text);
    let source_hash = digest(&std::fs::read(&source).unwrap());
    let archive = fixture.0.join("mapped-archive");
    let output = convert(&fixture, &source, &archive);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Storage::import_archive(&fixture.config(), &archive)
        .await
        .unwrap();
    let store = Storage::open(&fixture.config()).await.unwrap();
    let domain = Domain::new(&store);
    let memory_id = format!("mempal/{source_hash}/drawers/1/0");
    let memory = domain
        .memory(&memory_id)
        .unwrap()
        .expect("legacy memory candidate is missing");
    assert_eq!(memory.record.authority, Authority::Candidate);
    assert_eq!(memory.record.lifecycle, Lifecycle::Candidate);
    assert_eq!(memory.record.draft.kind, MemoryKind::Fact);
    assert!(memory.record.approval.is_none());
    assert!(matches!(
        memory.record.provenance.actor,
        Actor::Unverified(_)
    ));
    assert_eq!(memory.record.provenance.origin, Origin::Unverified);
    assert_eq!(memory.record.draft.claim, text);
    assert_eq!(
        memory.record.draft.scope.project.as_deref(),
        Some("project-a")
    );
    assert_eq!(
        memory.record.draft.scope.user,
        format!("uid:{}", rustix::process::geteuid().as_raw())
    );
    assert!(domain.active(&memory_id).unwrap().is_none());
    assert_eq!(memory.record.draft.source_spans.len(), 1);
    let span = &memory.record.draft.source_spans[0];
    let original = domain.source(&span.event_id, span.revision).unwrap();
    assert_eq!(original.payload.text, text);
    assert_eq!(original.payload.event_time_ms, Some(1_767_323_045_000));
    assert_eq!(span.start, 0);
    assert_eq!(span.end, text.len());
    assert_eq!(span.content_sha256, digest(text.as_bytes()));
    assert!(
        domain
            .memory(&format!("mempal/{source_hash}/drawers/2/0"))
            .unwrap()
            .is_none()
    );
    domain
        .set_read_grant(
            "migration-review-read",
            "migration-reader",
            Some(&memory.record.draft.scope),
            None,
            1,
        )
        .await
        .unwrap();
    let context = ReadContext::from_verified_binding(
        "migration-reader",
        "controlled-fixture",
        "import-review",
    )
    .unwrap();
    let pending = domain
        .review_list(&context, &memory.record.draft.scope, None)
        .await
        .unwrap();
    assert_eq!(pending.items.len(), 1);
    assert_eq!(pending.items[0].memory_id, memory_id);
    store.close().await.unwrap();
}

fn projected_source(store: &Storage, event: &str) -> akasha_otsuge::domain::SourceRecord {
    let revision = store
        .current(&RecordKey {
            kind: RecordKind::Evidence,
            id: format!("source/{}", digest(event.as_bytes())),
        })
        .unwrap()
        .expect("projected original source is missing");
    akasha_otsuge::domain::Domain::new(store)
        .source(event, revision.commit_seq)
        .unwrap()
}

#[tokio::test]
async fn mempal_projects_knowledge_cards_without_elevating_source_claims() {
    use akasha_otsuge::domain::{Authority, Domain, Lifecycle};
    let fixture = Fixture::new();
    let source = fixture.0.join("cards.db");
    legacy_snapshot(&source, "drawer source");
    let db = rusqlite::Connection::open(&source).unwrap();
    for (id, status, statement) in [
        ("card-one", "canonical", "Card statement"),
        ("card-two", "retired", "Retired card"),
    ] {
        db.execute("INSERT INTO knowledge_cards(id,statement,content,tier,status,domain,anchor_kind,anchor_id,created_at,updated_at,source_drawer_ids) VALUES(?1,?2,'Card context','qi',?3,'global','global','global','2026-01-02 03:04:05','2026-01-02 03:04:05','[\"legacy-one\"]')", rusqlite::params![id,statement,status]).unwrap();
    }
    db.close().unwrap();
    let sha = digest(&std::fs::read(&source).unwrap());
    let archive = fixture.0.join("cards-archive");
    let output = convert(&fixture, &source, &archive);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Storage::import_archive(&fixture.config(), &archive)
        .await
        .unwrap();
    let store = Storage::open(&fixture.config()).await.unwrap();
    let domain = Domain::new(&store);
    let memory = domain
        .memory(&format!("mempal/{sha}/knowledge_cards/1/0"))
        .unwrap()
        .expect("knowledge card candidate missing");
    assert_eq!(memory.record.draft.claim, "Card statement");
    assert_eq!(memory.record.authority, Authority::Candidate);
    assert_eq!(memory.record.lifecycle, Lifecycle::Candidate);
    assert!(memory.record.approval.is_none());
    assert!(memory.record.draft.scope.project.is_none());
    let original = projected_source(
        &store,
        &format!("mempal/{sha}/knowledge_cards/1/content/0/source"),
    );
    assert_eq!(original.payload.text, "Card context");
    assert!(
        domain
            .memory(&format!("mempal/{sha}/knowledge_cards/2/0"))
            .unwrap()
            .is_none()
    );
    let (_, rows) = table_rows(&store, &sha, "knowledge_cards");
    assert_eq!(rows.len(), 2);
    store.close().await.unwrap();
}

#[tokio::test]
async fn mempal_projects_conversation_sources_without_merging_profiles() {
    use akasha_otsuge::domain::{Actor, Domain, Origin};
    let fixture = Fixture::new();
    let source = fixture.0.join("turns.db");
    legacy_snapshot(&source, "drawer source");
    let db = rusqlite::Connection::open(&source).unwrap();
    for (index, profile, text) in [
        (0, "profile-a", "original A\0"),
        (1, "profile-b", "original B"),
    ] {
        db.execute("INSERT INTO conversation_turns(id,session_id,tool,turn_index,role,content,timestamp_epoch,project_path,git_branch,hermes_profile,message_id) VALUES(?1,'shared-session','hermes',?2,'user',?3,1767323045.125,'/repo','main',?4,'same-message-id')",rusqlite::params![format!("turn-{index}"),index,text,profile]).unwrap();
    }
    db.close().unwrap();
    let sha = digest(&std::fs::read(&source).unwrap());
    let archive = fixture.0.join("turns-archive");
    let output = convert(&fixture, &source, &archive);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Storage::import_archive(&fixture.config(), &archive)
        .await
        .unwrap();
    let store = Storage::open(&fixture.config()).await.unwrap();
    let first = projected_source(
        &store,
        &format!("mempal/{sha}/conversation_turns/1/0/source"),
    );
    let second = projected_source(
        &store,
        &format!("mempal/{sha}/conversation_turns/2/0/source"),
    );
    assert_eq!(first.payload.text, "original A\0");
    assert_eq!(second.payload.text, "original B");
    assert_eq!(first.payload.event_time_ms, Some(1_767_323_045_125));
    assert_eq!(first.payload.scope.project.as_deref(), Some("/repo"));
    assert_eq!(
        first.payload.scope.branch_or_version.as_deref(),
        Some("main")
    );
    assert!(first.payload.scope.session.is_some());
    assert_ne!(first.payload.scope.session, second.payload.scope.session);
    assert!(matches!(first.provenance.actor, Actor::Unverified(_)));
    assert_eq!(first.provenance.origin, Origin::Unverified);
    assert!(
        Domain::new(&store)
            .memory(&format!("mempal/{sha}/conversation_turns/1/0"))
            .unwrap()
            .is_none()
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn mempal_projection_keeps_chunk_evidence_and_reviews_across_commit_boundaries() {
    use akasha_otsuge::domain::{Domain, ReadContext};
    let fixture = Fixture::new();
    let source = fixture.0.join("batches.db");
    let original = "原文\0é\n".repeat(12_000);
    legacy_snapshot(&source, &original);
    let db = rusqlite::Connection::open(&source).unwrap();
    for index in 0..65 {
        db.execute("INSERT INTO drawers(id,content,wing,added_at,project_id) VALUES(?1,?2,'test','2026-01-02T03:04:05Z','project-a')", rusqlite::params![format!("batch-{index}"),format!("claim {index}")]).unwrap();
    }
    db.close().unwrap();
    let sha = digest(&std::fs::read(&source).unwrap());
    let archive = fixture.0.join("batches-archive");
    let output = convert(&fixture, &source, &archive);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Storage::import_archive(&fixture.config(), &archive)
        .await
        .unwrap();
    let store = Storage::open(&fixture.config()).await.unwrap();
    let domain = Domain::new(&store);
    let first = domain
        .memory(&format!("mempal/{sha}/drawers/1/0"))
        .unwrap()
        .unwrap();
    domain
        .set_read_grant(
            "read-batches",
            "reader",
            Some(&first.record.draft.scope),
            None,
            1,
        )
        .await
        .unwrap();
    let context = ReadContext::from_verified_binding("reader", "fixture", "batches").unwrap();
    let mut after = None;
    let mut seen = std::collections::BTreeSet::new();
    let mut commit_sequences = std::collections::BTreeSet::new();
    loop {
        let page = domain
            .review_list(&context, &first.record.draft.scope, after.as_deref())
            .await
            .unwrap();
        for item in page.items {
            let memory = domain.memory(&item.memory_id).unwrap().unwrap();
            assert_eq!(item.proposal_revision, memory.revision.commit_seq);
            assert!(seen.insert(item.memory_id));
            for span in &memory.record.draft.source_spans {
                let source = domain.source(&span.event_id, span.revision).unwrap();
                assert_eq!(source.payload.text, memory.record.draft.claim);
                assert_eq!(source.content_sha256, span.content_sha256);
                assert_eq!(span.end, source.payload.text.len());
                commit_sequences.insert(span.revision);
            }
        }
        after = page.next;
        if after.is_none() {
            break;
        }
    }
    assert!(commit_sequences.len() > 2);
    let mut reassembled = String::new();
    let mut parts = 0;
    while let Some(memory) = domain
        .memory(&format!("mempal/{sha}/drawers/1/{parts}"))
        .unwrap()
    {
        reassembled.push_str(&memory.record.draft.claim);
        parts += 1;
    }
    assert_eq!(reassembled, original);
    assert_eq!(seen.len(), 65 + parts);
    store.close().await.unwrap();
}

#[tokio::test]
async fn mempal_projection_preserves_validity_and_does_not_broaden_unmapped_constraints() {
    use akasha_otsuge::domain::Domain;
    let fixture = Fixture::new();
    let source = fixture.0.join("scopes.db");
    legacy_snapshot(&source, "original");
    let db = rusqlite::Connection::open(&source).unwrap();
    for index in 0..5 {
        db.execute("INSERT INTO drawers(id,content,wing,added_at,project_id) VALUES(?1,'Scoped claim','test','unknown date','project-a')", [format!("scope-{index}")]).unwrap();
    }
    db.execute(
        "UPDATE drawers SET scope_constraints=?1 WHERE id='scope-0'",
        ["{\"requires\":\"external condition\"}"],
    )
    .unwrap();
    db.execute_batch("UPDATE drawers SET valid_from='unparseable' WHERE id='scope-1'; UPDATE drawers SET valid_from='2020-01-01 00:00:00',valid_until='2021-01-01 00:00:00' WHERE id='scope-2'; UPDATE drawers SET anchor_kind='worktree',anchor_id='/repo/branch' WHERE id='scope-3'; UPDATE drawers SET project_id=NULL WHERE id='scope-4';").unwrap();
    db.close().unwrap();
    let sha = digest(&std::fs::read(&source).unwrap());
    let archive = fixture.0.join("scopes-archive");
    let output = convert(&fixture, &source, &archive);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    Storage::import_archive(&fixture.config(), &archive)
        .await
        .unwrap();
    let store = Storage::open(&fixture.config()).await.unwrap();
    let domain = Domain::new(&store);
    for rowid in [3, 4] {
        assert!(
            domain
                .memory(&format!("mempal/{sha}/drawers/{rowid}/0"))
                .unwrap()
                .is_none()
        );
        assert_eq!(
            projected_source(&store, &format!("mempal/{sha}/drawers/{rowid}/0/source"))
                .payload
                .text,
            "Scoped claim"
        );
    }
    let expired = domain
        .memory(&format!("mempal/{sha}/drawers/5/0"))
        .unwrap()
        .unwrap();
    assert!(expired.record.draft.valid_at(1_600_000_000_000));
    assert!(!expired.record.draft.valid_at(1_767_323_045_000));
    let worktree = domain
        .memory(&format!("mempal/{sha}/drawers/6/0"))
        .unwrap()
        .unwrap();
    assert!(
        worktree
            .record
            .draft
            .scope
            .branch_or_version
            .as_deref()
            .unwrap()
            .starts_with("mempal-worktree/")
    );
    let isolated = domain
        .memory(&format!("mempal/{sha}/drawers/7/0"))
        .unwrap()
        .unwrap();
    assert!(
        isolated
            .record
            .draft
            .scope
            .project
            .as_deref()
            .unwrap()
            .starts_with("mempal-scope/")
    );
    let metadata = stored_json(&store, &format!("mempal/{sha}/migration"));
    assert_eq!(metadata["projection"]["unmapped_constraints_rows"], 2);
    assert_eq!(metadata["projection"]["unknown_event_time_rows"], 5);
    let (_, rows) = table_rows(&store, &sha, "drawers");
    assert_eq!(rows.len(), 7);
    store.close().await.unwrap();
}
