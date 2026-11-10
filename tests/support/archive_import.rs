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
async fn archive_import_replay_must_complete_a_failed_publication_parent_sync() {
    let source = Fixture::new();
    let store = Storage::open(&source.config()).await.unwrap();
    store.commit(&request()).await.unwrap();
    store.close().await.unwrap();
    let archive = source.0.join("archive");
    assert!(source.export(&archive).status.success());
    let target = Fixture::new();
    assert!(target.import(&archive).status.success());
    let data = target.0.join("data");
    // Recreate a fully written unpublished stage without racing the native writer.
    std::fs::rename(data.join("authority"), data.join(".archive-import")).unwrap();
    let interposer = target.0.join("fail-sync.so");
    let source_file = target.0.join("fail-sync.c");
    let observed = target.0.join("sync-faults");
    std::fs::write(
        &source_file,
        r#"
#define _GNU_SOURCE
#include <dlfcn.h>
#include <errno.h>
#include <fcntl.h>
#include <stdlib.h>
#include <sys/stat.h>
#include <unistd.h>
int fsync(int fd) {
    const char *directory = getenv("AKASHA_FAIL_SYNC_DIRECTORY");
    const char *published = getenv("AKASHA_FAIL_SYNC_PUBLISHED");
    const char *observed = getenv("AKASHA_FAIL_SYNC_OBSERVED");
    struct stat wanted, actual, complete;
    if (directory && published && observed && !stat(directory, &wanted)
        && !stat(published, &complete) && !fstat(fd, &actual)
        && wanted.st_dev == actual.st_dev && wanted.st_ino == actual.st_ino) {
        int log = open(observed, O_WRONLY | O_APPEND | O_CREAT, 0600);
        if (log >= 0) { (void)write(log, "x", 1); close(log); }
        errno = EIO;
        return -1;
    }
    int (*original)(int) = dlsym(RTLD_NEXT, "fsync");
    return original(fd);
}
"#,
    )
    .unwrap();
    assert!(
        Command::new("cc")
            .args(["-shared", "-fPIC", "-Wall", "-Wextra", "-Werror"])
            .arg(&source_file)
            .args(["-ldl", "-o"])
            .arg(&interposer)
            .status()
            .unwrap()
            .success()
    );
    for attempt in 0..2 {
        let output = Command::new(env!("CARGO_BIN_EXE_akasha-otsuge"))
            .arg("--config")
            .arg(target.0.join("config.toml"))
            .args(["archive", "import"])
            .arg(&archive)
            .env("LD_PRELOAD", &interposer)
            .env("AKASHA_FAIL_SYNC_DIRECTORY", &data)
            .env("AKASHA_FAIL_SYNC_PUBLISHED", data.join("authority"))
            .env("AKASHA_FAIL_SYNC_OBSERVED", &observed)
            .output()
            .unwrap();
        assert!(
            !output.status.success(),
            "attempt {attempt} skipped the outstanding parent sync"
        );
        assert!(output.stdout.is_empty());
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
            "STORAGE_IO"
        );
        assert!(data.join("authority").is_dir());
        assert!(!data.join(".archive-import").exists());
    }
    assert_eq!(std::fs::read(&observed).unwrap(), b"xx");
    assert!(target.import(&archive).status.success());
    let store = Storage::open(&target.config()).await.unwrap();
    assert_eq!(store.sequence().unwrap(), 1);
    assert_eq!(store.commit(&request()).await.unwrap().commit_seq, 1);
    store.close().await.unwrap();
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
async fn incremental_archive_keeps_common_history_and_replays_old_prefixes() {
    let source = Fixture::new();
    let store = Storage::open(&source.config()).await.unwrap();
    let initial = request();
    let first = store.commit(&initial).await.unwrap();
    let key = initial.changes[1].key.clone();
    store.close().await.unwrap();
    let old_archive = source.0.join("old-archive");
    assert!(source.export(&old_archive).status.success());
    let target = Fixture::new();
    assert!(target.import(&old_archive).status.success());

    let store = Storage::open(&source.config()).await.unwrap();
    let mut change = initial.clone();
    // New request sorts before the old request; byte-prefix comparison is insufficient.
    change.request_id = "a-incremental".into();
    change.changes.retain(|c| c.key == key);
    change.changes[0].expected_revision = Some(first.commit_seq);
    change.changes[0].content = "new claim; original stays historical".into();
    let second = store.commit(&change).await.unwrap();
    change.request_id = "m-retirement".into();
    change.changes[0].expected_revision = Some(second.commit_seq);
    change.changes[0].content = "retired; all prior bytes retained".into();
    let third = store.commit(&change).await.unwrap();
    let expected: Vec<_> = (1..=3)
        .map(|seq| {
            (
                store.at(&key, seq).unwrap(),
                store.recorded_at_ms(seq).unwrap(),
            )
        })
        .collect();
    store.close().await.unwrap();
    let archive = source.0.join("new-archive");
    assert!(source.export(&archive).status.success());
    let output = target.import(&archive);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(target.import(&archive).status.success());
    assert!(target.import(&old_archive).status.success());
    let restored = Storage::open(&target.config()).await.unwrap();
    assert_eq!(restored.sequence().unwrap(), third.commit_seq);
    for (index, (revision, clock)) in expected.iter().enumerate() {
        let seq = index as u64 + 1;
        assert_eq!(&restored.at(&key, seq).unwrap(), revision);
        assert_eq!(&restored.recorded_at_ms(seq).unwrap(), clock);
    }
    for receipt in [first, second, third] {
        assert_eq!(
            restored.receipt(&receipt.request_id).unwrap(),
            Some(receipt)
        );
    }
    restored.close().await.unwrap();
    let exported = target.0.join("exported");
    assert!(target.export(&exported).status.success());
    for name in ["manifest.json", "commits.jsonl", "blobs.bin"] {
        assert_eq!(
            std::fs::read(exported.join(name)).unwrap(),
            std::fs::read(archive.join(name)).unwrap(),
            "{name}"
        );
    }
}

#[tokio::test]
async fn incremental_archive_refuses_divergent_local_and_corrupt_successors_without_loss() {
    for scenario in ["divergent", "local", "corrupt"] {
        let source = Fixture::new();
        let store = Storage::open(&source.config()).await.unwrap();
        let original = request();
        store.commit(&original).await.unwrap();
        store.close().await.unwrap();
        let archive = source.0.join("old");
        assert!(source.export(&archive).status.success());
        let target = Fixture::new();
        assert!(target.import(&archive).status.success());
        if scenario == "local" {
            let local = Storage::open(&target.config()).await.unwrap();
            let mut change = original.clone();
            change.request_id = "local-correction".into();
            change.changes.retain(|c| c.key.kind == RecordKind::Memory);
            change.changes[0].expected_revision = Some(1);
            change.changes[0].content = "keep this later local decision".into();
            local.commit(&change).await.unwrap();
            local.close().await.unwrap();
        }
        let before = target.0.join("before");
        assert!(target.export(&before).status.success());
        let different = Fixture::new();
        let producer = if scenario == "divergent" {
            &different
        } else {
            &source
        };
        let store = Storage::open(&producer.config()).await.unwrap();
        if scenario == "divergent" {
            let mut other = original.clone();
            other.changes[1].content = "different first history".into();
            store.commit(&other).await.unwrap();
        }
        let mut change = original.clone();
        change.request_id = "a-external-update".into();
        change.changes.retain(|c| c.key.kind == RecordKind::Memory);
        change.changes[0].expected_revision = Some(1);
        change.changes[0].content = "next external revision".into();
        store.commit(&change).await.unwrap();
        store.close().await.unwrap();
        let next = producer.0.join("next");
        assert!(producer.export(&next).status.success());
        if scenario == "corrupt" {
            let path = next.join("blobs.bin");
            let mut bytes = std::fs::read(&path).unwrap();
            bytes[0] ^= 1;
            std::fs::write(path, bytes).unwrap();
        }
        assert!(
            !target.import(&next).status.success(),
            "accepted {scenario}"
        );
        let after = target.0.join("after");
        assert!(target.export(&after).status.success());
        for name in ["manifest.json", "commits.jsonl", "blobs.bin"] {
            assert_eq!(
                std::fs::read(before.join(name)).unwrap(),
                std::fs::read(after.join(name)).unwrap(),
                "{scenario}: {name}"
            );
        }
    }
}
