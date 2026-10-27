use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use akasha_otsuge::config::{Paths, ResolvedConfig, resolve};
use akasha_otsuge::storage::{Change, CommitRequest, RecordKey, RecordKind, Storage};

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "akasha-storage-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }

    fn config(&self) -> ResolvedConfig {
        resolve(
            "",
            None,
            "",
            &Paths {
                data_dir: self.0.join("data"),
                socket_path: self.0.join("service.sock"),
            },
        )
        .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn request() -> CommitRequest {
    CommitRequest {
        request_id: "request-1".into(),
        config_generation: 1,
        policy_revision: Some("policy-1".into()),
        changes: [
            (RecordKind::Evidence, "event-1", "原始证据\n"),
            (RecordKind::Memory, "memory-1", "first memory"),
            (RecordKind::State, "status-1", "active"),
            (RecordKind::Job, "job-1", "pending"),
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
async fn commits_the_complete_batch_with_one_receipt_and_durable_outbox() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = Storage::open(&config).await.unwrap();
    let request = request();
    let receipt = store.commit(&request).await.unwrap();
    assert_eq!(receipt.commit_seq, 1);
    assert_eq!(receipt.request_id, request.request_id);
    assert_eq!(receipt.config_generation, 1);
    assert_eq!(receipt.policy_revision.as_deref(), Some("policy-1"));
    assert_eq!(receipt.revisions.len(), 4);
    store.close().await.unwrap();

    let reopened = Storage::open(&config).await.unwrap();
    assert_eq!(reopened.sequence().unwrap(), 1);
    assert_eq!(
        reopened.receipt("request-1").unwrap(),
        Some(receipt.clone())
    );
    assert_eq!(
        reopened.outbox(1).unwrap().unwrap().revisions,
        receipt.revisions
    );
    for change in &request.changes {
        let revision = reopened.current(&change.key).unwrap().unwrap();
        assert_eq!(revision.content, change.content);
        assert_eq!(revision.commit_seq, 1);
        assert_eq!(revision.previous, None);
        assert_eq!(reopened.revision(&change.key, 1).unwrap(), Some(revision));
    }
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn retries_reuse_the_receipt_across_restart_and_conflicting_keys_have_no_effect() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = Storage::open(&config).await.unwrap();
    let mut request = request();
    let first = store.commit(&request).await.unwrap();
    let (left, right) = tokio::join!(store.commit(&request), store.commit(&request));
    assert_eq!(left.unwrap(), first);
    assert_eq!(right.unwrap(), first);
    store.close().await.unwrap();
    let store = Storage::open(&config).await.unwrap();
    request.changes.reverse();
    assert_eq!(store.commit(&request).await.unwrap(), first);
    request.changes[0].content.push_str("changed");
    assert_eq!(
        store.commit(&request).await.unwrap_err().code,
        "IDEMPOTENCY_CONFLICT"
    );
    assert_eq!(store.sequence().unwrap(), 1);
    assert!(store.outbox(2).unwrap().is_none());
    assert_eq!(store.receipt("request-1").unwrap(), Some(first));
    store.close().await.unwrap();
}

#[tokio::test]
async fn stale_batches_are_atomic_and_corrections_preserve_history_and_original_evidence() {
    let fixture = Fixture::new();
    let store = Storage::open(&fixture.config()).await.unwrap();
    let original = request();
    let receipt = store.commit(&original).await.unwrap();
    let memory = original.changes[1].key.clone();
    let before = store.current(&memory).unwrap().unwrap();
    let mut stale = original.clone();
    stale.request_id = "stale".into();
    stale.changes[0].key.id = "new-event".into();
    stale.changes[1].content = "must not be visible".into();
    assert_eq!(
        store.commit(&stale).await.unwrap_err().code,
        "REVISION_CONFLICT"
    );
    assert!(store.current(&stale.changes[0].key).unwrap().is_none());
    assert!(store.receipt("stale").unwrap().is_none());
    assert!(store.outbox(2).unwrap().is_none());
    assert_eq!(store.current(&memory).unwrap(), Some(before.clone()));

    let mut correction = original.clone();
    correction.request_id = "correction".into();
    correction
        .changes
        .retain(|change| change.key.kind != RecordKind::Evidence);
    for change in &mut correction.changes {
        change.expected_revision = Some(1);
        change.content = "corrected".into();
    }
    assert_eq!(store.commit(&correction).await.unwrap().commit_seq, 2);
    assert_eq!(store.current(&memory).unwrap().unwrap().previous, Some(1));
    assert_eq!(store.revision(&memory, 1).unwrap(), Some(before));
    assert_eq!(store.receipt("request-1").unwrap(), Some(receipt));
    let mut evidence_overwrite = original;
    evidence_overwrite.request_id = "evidence-overwrite".into();
    evidence_overwrite.changes.truncate(1);
    evidence_overwrite.changes[0].expected_revision = Some(1);
    evidence_overwrite.changes[0].content = "forged original".into();
    assert_eq!(
        store.commit(&evidence_overwrite).await.unwrap_err().code,
        "IMMUTABLE_EVIDENCE"
    );
    assert_eq!(store.sequence().unwrap(), 2);

    correction.request_id = "race-a".into();
    for change in &mut correction.changes {
        change.expected_revision = Some(2);
    }
    let mut competitor = correction.clone();
    competitor.request_id = "race-b".into();
    competitor.changes[0].content = "competitor".into();
    let (a, b) = tokio::join!(store.commit(&correction), store.commit(&competitor));
    assert_eq!(usize::from(a.is_ok()) + usize::from(b.is_ok()), 1);
    assert_eq!(a.err().or(b.err()).unwrap().code, "REVISION_CONFLICT");
    assert_eq!(store.sequence().unwrap(), 3);
    store.close().await.unwrap();
}

#[tokio::test]
async fn malformed_and_oversized_batches_are_rejected_before_any_effect() {
    let fixture = Fixture::new();
    let store = Storage::open(&fixture.config()).await.unwrap();
    let mut cases = Vec::new();
    for id in [String::new(), "x".repeat(257)] {
        let mut r = request();
        r.request_id = id;
        cases.push(r);
    }
    let mut r = request();
    r.changes.clear();
    cases.push(r);
    let mut r = request();
    r.config_generation = 0;
    cases.push(r);
    let mut r = request();
    r.policy_revision = Some(String::new());
    cases.push(r);
    let mut r = request();
    r.changes[0].key.id.clear();
    cases.push(r);
    let mut r = request();
    r.changes[0].expected_revision = Some(0);
    cases.push(r);
    let mut r = request();
    r.changes.push(r.changes[0].clone());
    cases.push(r);
    let mut r = request();
    r.changes[0].content = "x".repeat(128 * 1024 + 1);
    cases.push(r);
    let mut r = request();
    r.changes = (0..65)
        .map(|id| Change {
            key: RecordKey {
                kind: RecordKind::State,
                id: id.to_string(),
            },
            expected_revision: None,
            content: "x".into(),
        })
        .collect();
    cases.push(r.clone());
    r.changes.truncate(9);
    for change in &mut r.changes {
        change.content = "x".repeat(128 * 1024);
    }
    cases.push(r);
    for (index, invalid) in cases.into_iter().enumerate() {
        assert_eq!(
            store.commit(&invalid).await.unwrap_err().code,
            "INVALID_STORAGE_REQUEST",
            "case {index}"
        );
        assert_eq!(store.sequence().unwrap(), 0);
    }
    let mut valid = request();
    valid.changes[0].content = "\0".repeat(128 * 1024);
    store.commit(&valid).await.unwrap();
    assert_eq!(
        store
            .current(&valid.changes[0].key)
            .unwrap()
            .unwrap()
            .content,
        valid.changes[0].content
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn content_hash_and_revision_identity_are_checked_on_read() {
    for corruption in ["content", "key", "seq"] {
        let fixture = Fixture::new();
        let config = fixture.config();
        let store = Storage::open(&config).await.unwrap();
        let request = request();
        store.commit(&request).await.unwrap();
        let key = request.changes[1].key.clone();
        let mut revision = store.current(&key).unwrap().unwrap();
        store.close().await.unwrap();
        match corruption {
            "content" => revision.content.push_str("tampered"),
            "key" => revision.key.id = "wrong-memory".into(),
            "seq" => revision.commit_seq = 99,
            _ => unreachable!(),
        }
        let raw = surrealkv::TreeBuilder::with_options(
            surrealkv::Options::new()
                .with_path(config.config().paths.data_dir.join("authority/kv"))
                .with_enable_vlog(true)
                .with_versioning(true, 0),
        )
        .build()
        .unwrap();
        let mut tx = raw.begin().unwrap();
        tx.set_durability(surrealkv::Durability::Immediate);
        tx.set(
            b"revision/memory/0008/memory-1/00000000000000000001",
            zstd::bulk::compress(&serde_json::to_vec(&revision).unwrap(), 1).unwrap(),
        )
        .unwrap();
        tx.commit().await.unwrap();
        raw.close().await.unwrap();
        let reopened = Storage::open(&config).await.unwrap();
        assert_eq!(
            reopened.current(&key).unwrap_err().code,
            "STORAGE_CORRUPT",
            "{corruption}"
        );
        reopened.close().await.unwrap();
    }
}

fn files(root: &std::path::Path) -> std::collections::BTreeMap<PathBuf, Vec<u8>> {
    let mut result = std::collections::BTreeMap::new();
    for entry in std::fs::read_dir(root).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            result.extend(files(&path));
        } else {
            result.insert(path.clone(), std::fs::read(&path).unwrap());
        }
    }
    result
}

#[tokio::test]
async fn unsupported_or_missing_format_is_refused_without_touching_database_files() {
    for marker in [Some(b"{\"schema_version\":999}".as_slice()), None] {
        let fixture = Fixture::new();
        let config = fixture.config();
        let store = Storage::open(&config).await.unwrap();
        store.commit(&request()).await.unwrap();
        store.close().await.unwrap();
        let root = config.config().paths.data_dir.join("authority");
        let format = root.join("format.json");
        match marker {
            Some(bytes) => std::fs::write(&format, bytes).unwrap(),
            None => {
                if format.exists() {
                    std::fs::remove_file(&format).unwrap();
                }
            }
        }
        let before = files(&root);
        assert_eq!(
            Storage::open(&config).await.err().unwrap().code,
            "UNSUPPORTED_STORAGE_FORMAT"
        );
        assert_eq!(files(&root), before);
    }
}

#[cfg(unix)]
#[tokio::test]
async fn store_is_private_and_refuses_authority_symlink_aliases() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = Storage::open(&config).await.unwrap();
    store.close().await.unwrap();
    let root = config.config().paths.data_dir.join("authority");
    assert_eq!(
        std::fs::metadata(&root).unwrap().permissions().mode() & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(root.join("format.json"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    let moved = fixture.0.join("moved");
    std::fs::rename(&root, &moved).unwrap();
    symlink(&moved, &root).unwrap();
    let before = files(&moved);
    assert_eq!(
        Storage::open(&config).await.err().unwrap().code,
        "UNSAFE_STORAGE_PATH"
    );
    assert_eq!(files(&moved), before);
}

#[tokio::test]
async fn native_checkpoint_restores_exact_revisions_hashes_receipts_and_outbox() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = Storage::open(&config).await.unwrap();
    let original = request();
    let first = store.commit(&original).await.unwrap();
    let mut update = original.clone();
    update.request_id = "second".into();
    update
        .changes
        .retain(|change| change.key.kind != RecordKind::Evidence);
    for change in &mut update.changes {
        change.expected_revision = Some(1);
        change.content = "second version".into();
    }
    let second = store.commit(&update).await.unwrap();
    let backup = fixture.0.join("backup");
    assert_eq!(store.checkpoint(&backup).await.unwrap(), 2);
    update.request_id = "third".into();
    for change in &mut update.changes {
        change.expected_revision = Some(2);
    }
    store.commit(&update).await.unwrap();
    let destination = Fixture::new();
    let restored = Storage::restore(&destination.config(), &backup)
        .await
        .unwrap();
    assert_eq!(restored.sequence().unwrap(), 2);
    assert_eq!(restored.receipt("request-1").unwrap(), Some(first));
    assert_eq!(restored.receipt("second").unwrap(), Some(second));
    assert_eq!(restored.receipt("third").unwrap(), None);
    assert_eq!(restored.outbox(2).unwrap(), store.outbox(2).unwrap());
    for change in &original.changes {
        for seq in 1..=2 {
            assert_eq!(
                restored.revision(&change.key, seq).unwrap(),
                store.revision(&change.key, seq).unwrap()
            );
        }
    }
    restored.close().await.unwrap();
    let reopened = Storage::open(&destination.config()).await.unwrap();
    assert_eq!(reopened.sequence().unwrap(), 2);
    assert_eq!(reopened.commit(&original).await.unwrap().commit_seq, 1);
    reopened.close().await.unwrap();
    store.close().await.unwrap();
}

#[tokio::test]
async fn missing_native_data_cannot_silently_become_a_new_empty_store() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = Storage::open(&config).await.unwrap();
    store.commit(&request()).await.unwrap();
    store.close().await.unwrap();
    let root = config.config().paths.data_dir.join("authority");
    std::fs::remove_dir_all(root.join("kv")).unwrap();
    let before = files(&root);
    assert!(
        Storage::open(&config).await.is_err(),
        "missing data was accepted as a fresh database"
    );
    assert_eq!(files(&root), before);
}

#[tokio::test]
async fn invalid_backups_and_existing_destinations_never_overwrite_live_data() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = Storage::open(&config).await.unwrap();
    store.commit(&request()).await.unwrap();
    for damage in ["content", "manifest", "missing", "extra", "path"] {
        let backup = fixture.0.join(damage);
        store.checkpoint(&backup).await.unwrap_or_else(|error| {
            panic!(
                "{damage}: {error:?}; source={:?}",
                std::error::Error::source(&error)
            )
        });
        let manifest_path = backup.join("backup.json");
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        match damage {
            "content" => {
                let path = manifest["files"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|item| item["path"].as_str().unwrap().starts_with("kv/"))
                    .unwrap()["path"]
                    .as_str()
                    .unwrap();
                std::fs::write(backup.join(path), b"corrupt").unwrap();
            }
            "manifest" => {
                std::fs::write(&manifest_path, b"{").unwrap();
            }
            "missing" => {
                std::fs::remove_file(&manifest_path).unwrap();
            }
            "extra" => {
                std::fs::write(backup.join("unlisted"), b"extra").unwrap();
            }
            "path" => {
                manifest["files"][0]["path"] = serde_json::json!("../../outside");
                std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
            }
            _ => unreachable!(),
        }
        let destination = Fixture::new();
        assert!(
            Storage::restore(&destination.config(), &backup)
                .await
                .is_err(),
            "{damage}"
        );
        assert!(
            !destination
                .config()
                .config()
                .paths
                .data_dir
                .join("authority")
                .exists()
        );
    }
    let backup = fixture.0.join("good");
    store.checkpoint(&backup).await.unwrap();
    let unchanged = files(&backup);
    assert!(store.checkpoint(&backup).await.is_err());
    assert_eq!(files(&backup), unchanged);
    assert!(
        store
            .checkpoint(&config.config().paths.data_dir.join("authority/nested"))
            .await
            .is_err()
    );
    assert!(Storage::restore(&config, &backup).await.is_err());
    assert_eq!(store.sequence().unwrap(), 1);
    assert_eq!(
        store
            .current(&request().changes[0].key)
            .unwrap()
            .unwrap()
            .content,
        "原始证据\n"
    );
    store.close().await.unwrap();
}

#[cfg(unix)]
#[tokio::test]
async fn backup_and_restore_files_do_not_share_writable_inodes_with_each_other() {
    use std::os::unix::fs::MetadataExt;
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = Storage::open(&config).await.unwrap();
    store.commit(&request()).await.unwrap();
    let backup = fixture.0.join("backup");
    store.checkpoint(&backup).await.unwrap();
    for path in files(&backup).keys() {
        let meta = std::fs::metadata(path).unwrap();
        assert_eq!(
            meta.nlink(),
            1,
            "backup still shares an inode: {}",
            path.display()
        );
    }
    let restored_fixture = Fixture::new();
    let restored = Storage::restore(&restored_fixture.config(), &backup)
        .await
        .unwrap();
    restored.close().await.unwrap();
    for path in files(&backup).keys() {
        assert_eq!(
            std::fs::metadata(path).unwrap().nlink(),
            1,
            "restoring linked the backup to writable data"
        );
    }
    let sst = files(&backup)
        .keys()
        .find(|path| path.to_string_lossy().contains("sstables"))
        .unwrap()
        .clone();
    std::fs::write(sst, b"corrupt backup table").unwrap();
    store.close().await.unwrap();
    for config in [config, restored_fixture.config()] {
        let reopened = Storage::open(&config).await.unwrap();
        assert_eq!(
            reopened
                .current(&request().changes[1].key)
                .unwrap()
                .unwrap()
                .content,
            "first memory"
        );
        reopened.close().await.unwrap();
    }
}

#[tokio::test]
async fn missing_commit_metadata_is_corruption_and_invalid_lookup_ids_are_bounded() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = Storage::open(&config).await.unwrap();
    store.commit(&request()).await.unwrap();
    let invalid = RecordKey {
        kind: RecordKind::Memory,
        id: String::new(),
    };
    assert_eq!(
        store.current(&invalid).unwrap_err().code,
        "INVALID_STORAGE_REQUEST"
    );
    assert_eq!(
        store.revision(&invalid, 1).unwrap_err().code,
        "INVALID_STORAGE_REQUEST"
    );
    assert_eq!(
        store.receipt(&"x".repeat(257)).unwrap_err().code,
        "INVALID_STORAGE_REQUEST"
    );
    store.close().await.unwrap();
    let raw = surrealkv::TreeBuilder::with_options(
        surrealkv::Options::new()
            .with_path(config.config().paths.data_dir.join("authority/kv"))
            .with_enable_vlog(true)
            .with_versioning(true, 0),
    )
    .build()
    .unwrap();
    let mut tx = raw.begin().unwrap();
    tx.set_durability(surrealkv::Durability::Immediate);
    tx.delete(b"meta/commit_seq").unwrap();
    tx.commit().await.unwrap();
    raw.close().await.unwrap();
    assert_eq!(
        Storage::open(&config).await.err().unwrap().code,
        "STORAGE_CORRUPT"
    );
}

#[cfg(unix)]
#[tokio::test]
async fn backup_symlinks_are_rejected_and_native_ownership_is_exclusive() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = Storage::open(&config).await.unwrap();
    assert!(Storage::open(&config).await.is_err());
    store.commit(&request()).await.unwrap();
    let backup = fixture.0.join("backup");
    store.checkpoint(&backup).await.unwrap();
    let original_files = files(&backup);
    assert!(original_files.iter().all(|(path, bytes)| {
        !path.to_string_lossy().to_lowercase().contains("sqlite")
            && !bytes.starts_with(b"SQLite format 3\0")
    }));
    let file = backup.join("kv/CHECKPOINT_METADATA");
    let outside = fixture.0.join("outside");
    std::fs::rename(&file, &outside).unwrap();
    std::os::unix::fs::symlink(&outside, &file).unwrap();
    let destination = Fixture::new();
    assert!(
        Storage::restore(&destination.config(), &backup)
            .await
            .is_err()
    );
    assert!(
        !destination
            .config()
            .config()
            .paths
            .data_dir
            .join("authority")
            .exists()
    );
    assert_eq!(std::fs::read(outside).unwrap(), original_files[&file]);
    store.close().await.unwrap();
}

#[tokio::test]
async fn a_canceled_submission_cannot_publish_an_inconsistent_checkpoint() {
    use std::future::{Future, poll_fn};
    use std::task::Poll;
    for attempt in 0..8 {
        let fixture = Fixture::new();
        let config = fixture.config();
        let store = Storage::open(&config).await.unwrap();
        let request = request();
        let mut submission = Box::pin(store.commit(&request));
        poll_fn(|cx| {
            let _ = submission.as_mut().poll(cx);
            Poll::Ready(())
        })
        .await;
        drop(submission);
        let backup = fixture.0.join("backup");
        let seq = store.checkpoint(&backup).await.unwrap();
        let destination = Fixture::new();
        let restored = Storage::restore(&destination.config(), &backup)
            .await
            .unwrap();
        assert_eq!(restored.sequence().unwrap(), seq, "attempt {attempt}");
        assert_eq!(restored.receipt("request-1").unwrap().is_some(), seq == 1);
        for change in &request.changes {
            assert_eq!(restored.current(&change.key).unwrap().is_some(), seq == 1);
        }
        restored.close().await.unwrap();
        store.close().await.unwrap();
    }
}
