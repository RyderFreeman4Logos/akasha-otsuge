use super::*;

impl Fixture {
    fn import(&self, archive: &Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_akasha-otsuge"))
            .arg("--config")
            .arg(self.0.join("config.toml"))
            .args(["archive", "import"])
            .arg(archive)
            .output()
            .unwrap()
    }
}

fn request() -> CommitRequest {
    CommitRequest {
        request_id: "z-original".into(),
        config_generation: 8,
        policy_revision: Some("original-policy".into()),
        changes: [
            (RecordKind::Evidence, "source/原文", "exact\n原文\u{0}é"),
            (RecordKind::Memory, "memory/one", "original memory"),
            (RecordKind::State, "recall/one", "original receipt output"),
            (RecordKind::Job, "job/one", "archived job state"),
        ]
        .into_iter()
        .map(|(kind, id, content)| Change {
            key: RecordKey {
                kind,
                id: id.into(),
            },
            expected_revision: None,
            content: content.into(),
        })
        .collect(),
    }
}

#[tokio::test]
async fn archive_import_empty_locked_existing_and_source_symlink_boundaries() {
    let source = Fixture::new();
    let store = Storage::open(&source.config()).await.unwrap();
    store.close().await.unwrap();
    let archive = source.0.join("empty");
    assert!(source.export(&archive).status.success());
    let foreign = Fixture::new();
    let stage = foreign.0.join("data/.archive-import");
    std::fs::create_dir_all(&stage).unwrap();
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o700)).unwrap();
    std::fs::write(stage.join("unrelated"), b"preserve this file").unwrap();
    let output = foreign.import(&archive);
    assert!(
        !output.status.success(),
        "adopted an unowned staging directory"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
        "IMPORT_CONFLICT"
    );
    assert_eq!(
        std::fs::read(stage.join("unrelated")).unwrap(),
        b"preserve this file"
    );
    assert!(!stage.join("kv").exists());
    let target = Fixture::new();
    assert!(target.import(&archive).status.success());
    let restored = Storage::open(&target.config()).await.unwrap();
    assert_eq!(restored.sequence().unwrap(), 0);
    assert!(
        !target.import(&archive).status.success(),
        "accepted a locked destination"
    );
    restored.close().await.unwrap();
    let unrelated = Fixture::new();
    let store = Storage::open(&unrelated.config()).await.unwrap();
    let receipt = store.commit(&request()).await.unwrap();
    store.close().await.unwrap();
    let output = unrelated.import(&archive);
    assert!(!output.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
        "IMPORT_CONFLICT"
    );
    let store = Storage::open(&unrelated.config()).await.unwrap();
    assert_eq!(store.receipt(&receipt.request_id).unwrap(), Some(receipt));
    assert_eq!(store.sequence().unwrap(), 1);
    store.close().await.unwrap();
    std::fs::rename(archive.join("blobs.bin"), archive.join("real-blobs")).unwrap();
    std::os::unix::fs::symlink("real-blobs", archive.join("blobs.bin")).unwrap();
    let target = Fixture::new();
    assert!(!target.import(&archive).status.success());
    assert!(!target.0.join("data/authority").exists());
}

#[tokio::test]
async fn archive_import_roundtrip_preserves_all_points_clocks_receipts_and_original_bytes() {
    let source = Fixture::new();
    let store = Storage::open(&source.config()).await.unwrap();
    let initial = request();
    let first = store.commit(&initial).await.unwrap();
    let mut amendment = initial.clone();
    amendment.request_id = "a-amendment".into();
    amendment.config_generation = 9;
    amendment
        .changes
        .retain(|c| c.key.kind == RecordKind::Memory);
    amendment.changes[0].expected_revision = Some(1);
    amendment.changes[0].content = "corrected memory".into();
    let second = store.commit(&amendment).await.unwrap();
    amendment.request_id = "m-retirement".into();
    amendment.changes[0].expected_revision = Some(2);
    amendment.changes[0].content = "retired; preserve earlier content".into();
    let third = store.commit(&amendment).await.unwrap();
    let receipts = [first, second, third];
    let clocks: Vec<_> = (1..=3)
        .map(|seq| store.recorded_at_ms(seq).unwrap())
        .collect();
    let views: Vec<Vec<_>> = (0..=3)
        .map(|seq| {
            initial
                .changes
                .iter()
                .map(|c| store.at(&c.key, seq).unwrap())
                .collect()
        })
        .collect();
    store.close().await.unwrap();
    let archive = source.0.join("archive");
    assert!(source.export(&archive).status.success());
    let originals: Vec<_> = ["manifest.json", "commits.jsonl", "blobs.bin"]
        .map(|name| (name, std::fs::read(archive.join(name)).unwrap()))
        .into_iter()
        .collect();
    let target = Fixture::new();
    let output = target.import(&archive);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let restored = Storage::open(&target.config()).await.unwrap();
    assert_eq!(restored.sequence().unwrap(), 3);
    for (seq, expected) in views.iter().enumerate() {
        for (change, expected) in initial.changes.iter().zip(expected) {
            assert_eq!(&restored.at(&change.key, seq as u64).unwrap(), expected);
            if seq == 3 {
                assert_eq!(&restored.current(&change.key).unwrap(), expected);
            }
        }
    }
    for (index, receipt) in receipts.iter().enumerate() {
        assert_eq!(
            restored.receipt(&receipt.request_id).unwrap(),
            Some(receipt.clone())
        );
        assert_eq!(
            restored.recorded_at_ms(receipt.commit_seq).unwrap(),
            clocks[index]
        );
        assert_eq!(
            restored
                .outbox(receipt.commit_seq)
                .unwrap()
                .unwrap()
                .revisions,
            receipt.revisions
        );
    }
    // Ordinary idempotent requests and new writes must still work after restoration.
    assert_eq!(restored.commit(&initial).await.unwrap(), receipts[0]);
    amendment.request_id = "local-after-import".into();
    amendment.changes[0].expected_revision = Some(3);
    amendment.changes[0].content = "local new content".into();
    assert_eq!(restored.commit(&amendment).await.unwrap().commit_seq, 4);
    assert_eq!(
        restored.at(&amendment.changes[0].key, 3).unwrap(),
        views[3][1]
    );
    restored.close().await.unwrap();
    for (name, bytes) in originals {
        assert_eq!(std::fs::read(archive.join(name)).unwrap(), bytes);
    }
    // Repeating the same completed import does not replace subsequent local work.
    assert!(target.import(&archive).status.success());
    let restored = Storage::open(&target.config()).await.unwrap();
    assert_eq!(restored.sequence().unwrap(), 4);
    assert_eq!(
        restored
            .current(&amendment.changes[0].key)
            .unwrap()
            .unwrap()
            .content,
        "local new content"
    );
    restored.close().await.unwrap();
}

#[tokio::test]
async fn archive_import_rejects_unknown_corrupt_and_forged_records_without_publishing() {
    let source = Fixture::new();
    let store = Storage::open(&source.config()).await.unwrap();
    store.commit(&request()).await.unwrap();
    store.close().await.unwrap();
    let original = source.0.join("original");
    assert!(source.export(&original).status.success());
    for mutation in [
        "archive-version",
        "record-version",
        "blob-hash",
        "previous",
        "request-hash",
        "references",
        "unknown-receipt-field",
        "offset",
        "oversize-line",
    ] {
        let copy = source.0.join(mutation);
        std::fs::create_dir(&copy).unwrap();
        for name in ["manifest.json", "commits.jsonl", "blobs.bin"] {
            std::fs::copy(original.join(name), copy.join(name)).unwrap();
        }
        let mut manifest: Value =
            serde_json::from_slice(&std::fs::read(copy.join("manifest.json")).unwrap()).unwrap();
        let mut row: Value =
            serde_json::from_slice(&std::fs::read(copy.join("commits.jsonl")).unwrap()).unwrap();
        match mutation {
            "archive-version" => manifest["archive_version"] = json!(999),
            "record-version" => manifest["record_format_version"] = json!(999),
            "blob-hash" => {
                let mut bytes = std::fs::read(copy.join("blobs.bin")).unwrap();
                bytes[0] ^= 1;
                std::fs::write(copy.join("blobs.bin"), bytes).unwrap();
            }
            "previous" => row["revisions"][0]["previous"] = json!(999),
            "request-hash" => row["receipt"]["request_sha256"] = json!("0".repeat(64)),
            "references" => row["receipt"]["revisions"][0]["key"]["id"] = json!("missing"),
            "unknown-receipt-field" => row["receipt"]["unexpected_authority"] = json!(true),
            "offset" => row["revisions"][0]["blob_offset"] = json!(u64::MAX),
            "oversize-line" => {}
            _ => unreachable!(),
        }
        let bytes = if mutation == "oversize-line" {
            vec![b' '; 1024 * 1024 + 1]
        } else {
            let mut bytes = serde_json::to_vec(&row).unwrap();
            bytes.push(b'\n');
            bytes
        };
        std::fs::write(copy.join("commits.jsonl"), &bytes).unwrap();
        manifest["files"]["commits.jsonl"]["bytes"] = json!(bytes.len());
        manifest["files"]["commits.jsonl"]["sha256"] = json!(digest(&bytes));
        std::fs::write(
            copy.join("manifest.json"),
            serde_json::to_vec(&manifest).unwrap(),
        )
        .unwrap();
        let target = Fixture::new();
        let output = target.import(&copy);
        assert!(!output.status.success(), "accepted {mutation}");
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        let expected = if mutation.ends_with("version") {
            "UNSUPPORTED_ARCHIVE_FORMAT"
        } else {
            "INVALID_ARCHIVE"
        };
        assert_eq!(error["error"]["code"], expected, "{mutation}");
        assert!(
            !target.0.join("data/authority").exists(),
            "published {mutation}"
        );
        assert!(
            !target.0.join("data/.archive-import").exists(),
            "retained rejected archive {mutation}"
        );
    }
}

#[tokio::test]
async fn archive_import_remains_offline_until_runtime_authority_is_reinitialized() {
    use std::process::Stdio;
    use std::time::{Duration, Instant};
    let source = Fixture::new();
    let store = Storage::open(&source.config()).await.unwrap();
    store.commit(&request()).await.unwrap();
    store.close().await.unwrap();
    let archive = source.0.join("archive");
    assert!(source.export(&archive).status.success());
    let target = Fixture::new();
    assert!(target.import(&archive).status.success());
    // Give the daemon a valid private runtime directory so this exercises the import boundary.
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&target.0, std::fs::Permissions::from_mode(0o700)).unwrap();
    for _ in 0..2 {
        let mut child = Command::new(env!("CARGO_BIN_EXE_akasha-otsuge"))
            .arg("--config")
            .arg(target.0.join("config.toml"))
            .args(["daemon", "run"])
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        let deadline = Instant::now() + Duration::from_secs(3);
        let mut timed_out = false;
        while child.try_wait().unwrap().is_none() {
            if Instant::now() >= deadline {
                child.kill().unwrap();
                timed_out = true;
                break;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        let output = child.wait_with_output().unwrap();
        assert!(!timed_out, "imported authority started an active daemon");
        assert!(!output.status.success());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "IMPORTED_RUNTIME_DISABLED");
        assert!(!target.0.join("service.sock").exists());
        let store = Storage::open(&target.config()).await.unwrap();
        assert_eq!(
            store.sequence().unwrap(),
            1,
            "startup mutated imported records"
        );
        store.close().await.unwrap();
    }
}
