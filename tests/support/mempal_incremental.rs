use super::*;

fn extend(fixture: &Fixture, source: &Path, destination: &Path, previous: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_akasha-otsuge"))
        .arg("--config")
        .arg(fixture.0.join("config.toml"))
        .args(["migrate", "mempal"])
        .arg(source)
        .arg(destination)
        .arg("--previous")
        .arg(previous)
        .stdin(std::process::Stdio::null())
        .output()
        .unwrap()
}

#[tokio::test]
async fn changed_snapshot_preserves_logical_identity_and_original_source_history() {
    use akasha_otsuge::domain::{Authority, Domain, Lifecycle};

    let fixture = Fixture::new();
    let source = fixture.0.join("source.db");
    legacy_snapshot(&source, "old body");
    let initial_hash = digest(&std::fs::read(&source).unwrap());
    let previous = fixture.0.join("previous");
    assert!(convert(&fixture, &source, &previous).status.success());
    let previous_bytes = archive_payloads(&previous);
    Storage::import_archive(&fixture.config(), &previous)
        .await
        .unwrap();
    let store = Storage::open(&fixture.config()).await.unwrap();
    let id = format!("mempal/{initial_hash}/drawers/1/0");
    let original = Domain::new(&store).memory(&id).unwrap().unwrap();
    let original_sequence = store.sequence().unwrap();
    let last_commit: Value = serde_json::from_str(
        std::str::from_utf8(&previous_bytes[1])
            .unwrap()
            .lines()
            .last()
            .unwrap(),
    )
    .unwrap();
    let original_receipt = store
        .receipt(last_commit["receipt"]["request_id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    let original_revision = store
        .revision(&original.revision.key, original.revision.commit_seq)
        .unwrap();
    store.close().await.unwrap();

    let db = rusqlite::Connection::open(&source).unwrap();
    db.execute(
        "UPDATE drawers SET rowid=500,content='new body' WHERE id='legacy-one'",
        [],
    )
    .unwrap();
    db.close().unwrap();
    let source_bytes = std::fs::read(&source).unwrap();
    let destination = fixture.0.join("extended");
    let converted = extend(&fixture, &source, &destination, &previous);
    assert!(
        converted.status.success(),
        "changed-snapshot conversion failed: {}",
        String::from_utf8_lossy(&converted.stderr)
    );
    assert_eq!(std::fs::read(&source).unwrap(), source_bytes);
    assert_eq!(archive_payloads(&previous), previous_bytes);
    let extended_bytes = archive_payloads(&destination);
    let replay = extend(&fixture, &source, &destination, &previous);
    assert!(replay.status.success());
    assert_eq!(replay.stdout, converted.stdout);
    assert_eq!(archive_payloads(&destination), extended_bytes);

    use std::os::unix::fs::DirBuilderExt;
    std::fs::DirBuilder::new()
        .mode(0o700)
        .create(destination.join(".mempal-index"))
        .unwrap();
    let recovered_index = extend(&fixture, &source, &destination, &previous);
    assert!(
        recovered_index.status.success(),
        "{}",
        String::from_utf8_lossy(&recovered_index.stderr)
    );
    assert_eq!(recovered_index.stdout, converted.stdout);
    assert!(!destination.join(".mempal-index").exists());

    std::fs::remove_file(destination.join("manifest.json")).unwrap();
    for (index, name) in ["blobs.bin", "commits.jsonl"].iter().enumerate() {
        std::fs::OpenOptions::new()
            .write(true)
            .open(destination.join(name))
            .unwrap()
            .set_len((extended_bytes[index].len() / 2 + 3) as u64)
            .unwrap();
    }
    let resumed = extend(&fixture, &source, &destination, &previous);
    assert!(
        resumed.status.success(),
        "{}",
        String::from_utf8_lossy(&resumed.stderr)
    );
    assert_eq!(resumed.stdout, converted.stdout);
    assert_eq!(archive_payloads(&destination), extended_bytes);

    Storage::import_archive(&fixture.config(), &destination)
        .await
        .unwrap();
    let store = Storage::open(&fixture.config()).await.unwrap();
    let domain = Domain::new(&store);
    let updated = domain.memory(&id).unwrap().unwrap();
    assert_eq!(updated.record.draft.claim, "new body");
    assert_eq!(updated.record.authority, Authority::Candidate);
    assert_eq!(updated.record.lifecycle, Lifecycle::Candidate);
    assert!(updated.record.approval.is_none());
    assert!(domain.active(&id).unwrap().is_none());
    assert!(store.sequence().unwrap() > original_sequence);
    assert_eq!(
        store
            .revision(&original.revision.key, original.revision.commit_seq)
            .unwrap(),
        original_revision
    );
    assert_eq!(
        store.receipt(&original_receipt.request_id).unwrap(),
        Some(original_receipt.clone())
    );
    let old_span = &original.record.draft.source_spans[0];
    let new_span = &updated.record.draft.source_spans[0];
    assert_ne!(old_span.event_id, new_span.event_id);
    assert_eq!(
        domain
            .source(&old_span.event_id, old_span.revision)
            .unwrap()
            .payload
            .text,
        "old body"
    );
    assert_eq!(
        domain
            .source(&new_span.event_id, new_span.revision)
            .unwrap()
            .payload
            .text,
        "new body"
    );
    store.close().await.unwrap();
    let exported = fixture.0.join("exported");
    assert!(fixture.export(&exported).status.success());
    assert_eq!(archive_payloads(&exported), extended_bytes);
    let fresh = Fixture::new();
    let imported = Command::new(env!("CARGO_BIN_EXE_akasha-otsuge"))
        .arg("--config")
        .arg(fresh.0.join("config.toml"))
        .args(["archive", "import"])
        .arg(&exported)
        .output()
        .unwrap();
    assert!(
        imported.status.success(),
        "{}",
        String::from_utf8_lossy(&imported.stderr)
    );
    let store = Storage::open(&fresh.config()).await.unwrap();
    assert_eq!(Domain::new(&store).memory(&id).unwrap().unwrap(), updated);
    assert_eq!(
        store.receipt(&original_receipt.request_id).unwrap(),
        Some(original_receipt)
    );
    store.close().await.unwrap();
}

fn review_state(store: &Storage, memory: &str, request: &str) -> Value {
    let id = digest(&serde_json::to_vec(&(memory, request)).unwrap());
    let revision = store
        .current(&RecordKey {
            kind: RecordKind::State,
            id: format!("review/{id}"),
        })
        .unwrap()
        .unwrap();
    serde_json::from_str(&revision.content).unwrap()
}

fn assert_sorted_receipts(archive: &Path) {
    let requests: Vec<String> = std::fs::read_to_string(archive.join("commits.jsonl"))
        .unwrap()
        .lines()
        .map(|line| {
            let entry: Value = serde_json::from_str(line).unwrap();
            entry["receipt"]["request_id"].as_str().unwrap().to_owned()
        })
        .collect();
    assert!(requests.windows(2).all(|pair| pair[0] < pair[1]));
}

#[test]
fn unchanged_snapshot_reuses_archive_history_without_new_revisions() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source.db");
    legacy_snapshot(&source, "same body");
    let previous = fixture.0.join("previous");
    let original = convert(&fixture, &source, &previous);
    assert!(original.status.success());
    let destination = fixture.0.join("unchanged");
    let extended = extend(&fixture, &source, &destination, &previous);
    assert!(
        extended.status.success(),
        "{}",
        String::from_utf8_lossy(&extended.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&extended.stdout).unwrap()["data"],
        serde_json::from_slice::<Value>(&original.stdout).unwrap()["data"]
    );
    assert_eq!(archive_payloads(&destination), archive_payloads(&previous));
    assert!(!destination.join(".mempal-index").exists());
}

#[tokio::test]
async fn snapshot_lifecycle_delta_matches_primary_key_fixture_oracle() {
    use akasha_otsuge::domain::{
        Authority, Domain, Lifecycle, PageBudget, ReadContext, ReviewOpen, ReviewState,
        ReviewUpdate,
    };

    let fixture = Fixture::new();
    let source = fixture.0.join("source.db");
    legacy_snapshot(&source, "unused");
    let db = rusqlite::Connection::open(&source).unwrap();
    db.execute("DELETE FROM drawers", []).unwrap();
    for (rowid, key, text) in [
        (10, "rowid", "unchanged".to_owned()),
        (20, "update", "old body".into()),
        (30, "delete", "deleted body".into()),
        (40, "shorten", "x".repeat(32769)),
        (50, "scope", "scope body".into()),
        (60, "retire", "retired body".into()),
        (70, "return", "return body".into()),
        (80, "duplicate", "unchanged".into()),
    ] {
        db.execute("INSERT INTO drawers(rowid,id,content,wing,added_at,project_id,status) VALUES(?1,?2,?3,'test','2026-01-02 03:04:05','project-a','active')", rusqlite::params![rowid,key,text]).unwrap();
    }
    db.close().unwrap();
    let initial_hash = digest(&std::fs::read(&source).unwrap());
    let id = |rowid, part| format!("mempal/{initial_hash}/drawers/{rowid}/{part}");
    let previous = fixture.0.join("previous");
    assert!(convert(&fixture, &source, &previous).status.success());
    Storage::import_archive(&fixture.config(), &previous)
        .await
        .unwrap();
    let store = Storage::open(&fixture.config()).await.unwrap();
    let unchanged = Domain::new(&store).memory(&id(10, 0)).unwrap().unwrap();
    let original = Domain::new(&store).memory(&id(70, 0)).unwrap().unwrap();
    store.close().await.unwrap();
    let db = rusqlite::Connection::open(&source).unwrap();
    db.execute_batch("UPDATE drawers SET rowid=1000 WHERE id='rowid';
        UPDATE drawers SET content='new body' WHERE id='update';
        DELETE FROM drawers WHERE id IN ('delete','return');
        UPDATE drawers SET content='short' WHERE id='shorten';
        UPDATE drawers SET project_id='project-b' WHERE id='scope';
        UPDATE drawers SET status='retired' WHERE id='retire';
        INSERT INTO drawers(rowid,id,content,wing,added_at,project_id,status) VALUES(90,'insert','unchanged','test','2026-01-02 03:04:05','project-a','active');").unwrap();
    db.close().unwrap();
    let second = fixture.0.join("second");
    let output = extend(&fixture, &source, &second, &previous);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(!second.join(".mempal-index").exists());
    assert_sorted_receipts(&second);
    Storage::import_archive(&fixture.config(), &second)
        .await
        .unwrap();
    let store = Storage::open(&fixture.config()).await.unwrap();
    let domain = Domain::new(&store);
    assert_eq!(domain.memory(&id(10, 0)).unwrap().unwrap(), unchanged);
    assert_eq!(
        review_state(&store, &id(10, 0), &id(10, 0))["state"],
        "open"
    );
    for (row, part) in [(30, 0), (70, 0), (60, 0), (40, 1), (40, 2)] {
        let memory = domain.memory(&id(row, part)).unwrap().unwrap();
        assert_eq!(
            serde_json::to_value(memory.record.lifecycle).unwrap(),
            "withdrawn",
            "row={row} part={part}"
        );
        assert_eq!(memory.record.authority, Authority::Candidate);
        assert!(memory.record.approval.is_none());
        assert!(domain.active(&id(row, part)).unwrap().is_none());
        assert_eq!(
            review_state(&store, &id(row, part), &id(row, part))["state"],
            "withdrawn"
        );
    }
    for row in [20, 40, 50] {
        assert_eq!(
            review_state(&store, &id(row, 0), &id(row, 0))["state"],
            "withdrawn"
        );
        assert_eq!(
            domain
                .memory(&id(row, 0))
                .unwrap()
                .unwrap()
                .record
                .lifecycle,
            Lifecycle::Candidate
        );
    }
    assert_eq!(
        domain
            .memory(&id(50, 0))
            .unwrap()
            .unwrap()
            .record
            .draft
            .scope
            .project
            .as_deref(),
        Some("project-b")
    );
    let inserted_key = digest(
        &serde_json::to_vec(&(
            "drawers",
            vec![serde_json::json!({"type":"text","value":"insert"})],
        ))
        .unwrap(),
    );
    let inserted = format!("mempal/{initial_hash}/keys/{inserted_key}/0");
    for memory in [id(10, 0), id(80, 0), inserted] {
        assert_eq!(
            domain.memory(&memory).unwrap().unwrap().record.draft.claim,
            "unchanged"
        );
    }
    let withdrawn = domain.memory(&id(70, 0)).unwrap().unwrap();
    store.close().await.unwrap();
    let db = rusqlite::Connection::open(&source).unwrap();
    db.execute("INSERT INTO drawers(rowid,id,content,wing,added_at,project_id,status) VALUES(3000,'return','return body v2','test','2026-01-02 03:04:05','project-a','active')", []).unwrap();
    db.close().unwrap();
    let third = fixture.0.join("third");
    let output = extend(&fixture, &source, &third, &second);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_sorted_receipts(&third);
    Storage::import_archive(&fixture.config(), &third)
        .await
        .unwrap();
    let store = Storage::open(&fixture.config()).await.unwrap();
    let domain = Domain::new(&store);
    let returned = domain.memory(&id(70, 0)).unwrap().unwrap();
    assert_eq!(returned.record.draft.claim, "return body v2");
    assert_eq!(returned.record.lifecycle, Lifecycle::Candidate);
    assert_eq!(returned.previous, Some(withdrawn.revision.commit_seq));
    assert_eq!(
        domain
            .revision(&id(70, 0), original.revision.commit_seq)
            .unwrap()
            .unwrap(),
        original
    );
    assert_eq!(
        domain
            .revision(&id(70, 0), withdrawn.revision.commit_seq)
            .unwrap()
            .unwrap(),
        withdrawn
    );
    assert!(returned.record.approval.is_none());
    assert!(domain.active(&id(70, 0)).unwrap().is_none());
    let deleted = domain.memory(&id(30, 0)).unwrap().unwrap();
    domain
        .set_read_grant(
            "migration-review-read",
            "migration-reader",
            Some(&deleted.record.draft.scope),
            None,
            1,
        )
        .await
        .unwrap();
    let context =
        ReadContext::from_verified_binding("migration-reader", "fixture", "delta-review").unwrap();
    let history = domain
        .history(
            &context,
            &id(30, 0),
            1..=store.sequence().unwrap(),
            None,
            PageBudget {
                limit: 10,
                max_bytes: 128 * 1024,
            },
        )
        .await
        .unwrap();
    assert_eq!(history.data.items.len(), 2);
    assert_eq!(
        serde_json::to_value(history.data.items[1].status).unwrap(),
        "withdrawn"
    );
    let opened = domain
        .review_open(
            &context,
            "reopen-withdrawn-source",
            &ReviewOpen {
                memory_id: id(30, 0),
                expected_revision: deleted.revision.commit_seq,
                reason: "ordinary follow-up".into(),
            },
            1,
        )
        .await;
    assert_eq!(opened.unwrap_err().code, "INVALID_REVIEW_TRANSITION");
    let review_id = digest(&serde_json::to_vec(&(id(30, 0), id(30, 0))).unwrap());
    let review = domain
        .review_show(&context, &review_id, None)
        .await
        .unwrap();
    let reopened = domain
        .review_update(
            &context,
            "reopen-withdrawn-review",
            &ReviewUpdate {
                id: review_id,
                expected_revision: review.item.revision,
                state: ReviewState::Open,
                reason: "ordinary follow-up".into(),
            },
            1,
        )
        .await;
    assert_eq!(reopened.unwrap_err().code, "INVALID_REVIEW_TRANSITION");
    store.close().await.unwrap();
}

#[test]
fn ambiguous_legacy_primary_key_is_refused_without_changing_sources() {
    let fixture = Fixture::new();
    let source = fixture.0.join("source.db");
    legacy_snapshot(&source, "original");
    let previous = fixture.0.join("previous");
    assert!(convert(&fixture, &source, &previous).status.success());
    let original_archive = archive_payloads(&previous);
    let db = rusqlite::Connection::open(&source).unwrap();
    db.execute("UPDATE drawers SET id=NULL WHERE id='legacy-one'", [])
        .unwrap();
    db.close().unwrap();
    let original_source = std::fs::read(&source).unwrap();
    let destination = fixture.0.join("ambiguous");
    let output = extend(&fixture, &source, &destination, &previous);
    assert!(!output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
        "INVALID_MEMPAL_KEY"
    );
    assert_eq!(std::fs::read(source).unwrap(), original_source);
    assert_eq!(archive_payloads(&previous), original_archive);
    assert!(!destination.join("manifest.json").exists());
    assert!(!destination.join(".mempal-index").exists());
}
