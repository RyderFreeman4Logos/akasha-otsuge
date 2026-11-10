use std::path::{Path, PathBuf};
use std::process::{Command, Output};
use std::sync::atomic::{AtomicU64, Ordering};

use akasha_otsuge::config::{Paths, ResolvedConfig, resolve};
use akasha_otsuge::storage::{Change, CommitRequest, RecordKey, RecordKind, Storage};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

#[path = "support/archive_import.rs"]
mod import;

#[path = "support/mempal.rs"]
mod mempal;

struct Fixture(PathBuf);

impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "akasha-archive-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        let text = format!(
            "[paths]\ndata_dir={}\nsocket_path={}\n",
            json!(root.join("data")),
            json!(root.join("service.sock"))
        );
        std::fs::write(root.join("config.toml"), text).unwrap();
        Self(root)
    }

    fn config(&self) -> ResolvedConfig {
        resolve(
            &std::fs::read_to_string(self.0.join("config.toml")).unwrap(),
            None,
            "",
            &Paths {
                data_dir: self.0.join("data"),
                socket_path: self.0.join("service.sock"),
            },
        )
        .unwrap()
    }

    fn export(&self, destination: &Path) -> Output {
        Command::new(env!("CARGO_BIN_EXE_akasha-otsuge"))
            .arg("--config")
            .arg(self.0.join("config.toml"))
            .args(["archive", "export"])
            .arg(destination)
            .output()
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[tokio::test]
async fn archive_export_preserves_original_bytes_revisions_relations_clocks_and_receipts() {
    let fixture = Fixture::new();
    let store = Storage::open(&fixture.config()).await.unwrap();
    let mut request = CommitRequest {
        request_id: "z-original".into(),
        config_generation: 7,
        policy_revision: Some("policy-original".into()),
        changes: [
            (RecordKind::Evidence, "source/original", "原文\n\u{0}é"),
            (RecordKind::Memory, "memory/one", "first claim"),
            (RecordKind::State, "recall/one", "original receipt body"),
            (
                RecordKind::Job,
                "daemon-job/one",
                "pending job, archive only",
            ),
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
    };
    let first = store.commit(&request).await.unwrap();
    request.request_id = "a-correction".into();
    request.config_generation = 9;
    request
        .changes
        .retain(|change| change.key.kind == RecordKind::Memory);
    request.changes[0].expected_revision = Some(first.commit_seq);
    request.changes[0].content = "corrected claim — old text remains".into();
    let second = store.commit(&request).await.unwrap();
    let receipts = [first, second];
    let clocks: Vec<_> = receipts
        .iter()
        .map(|r| store.recorded_at_ms(r.commit_seq).unwrap())
        .collect();
    let revisions: Vec<Vec<_>> = receipts
        .iter()
        .map(|receipt| {
            receipt
                .revisions
                .iter()
                .map(|r| store.revision(&r.key, r.commit_seq).unwrap().unwrap())
                .collect()
        })
        .collect();
    store.close().await.unwrap();

    let destination = fixture.0.join("export");
    let output = fixture.export(&destination);
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: Value = serde_json::from_slice(&output.stdout).unwrap();
    let manifest: Value =
        serde_json::from_slice(&std::fs::read(destination.join("manifest.json")).unwrap()).unwrap();
    assert_eq!(report["data"], manifest);
    assert_eq!(manifest["archive_version"], 1);
    assert_eq!(manifest["record_format_version"], 2);
    assert_eq!(manifest["commit_seq"], 2);
    assert_eq!(manifest["commits"], 2);
    assert_eq!(manifest["revisions"], 5);
    assert_eq!(manifest["files"].as_object().unwrap().len(), 2);
    let blobs = std::fs::read(destination.join("blobs.bin")).unwrap();
    let commits = std::fs::read(destination.join("commits.jsonl")).unwrap();
    for (name, bytes) in [("blobs.bin", &blobs), ("commits.jsonl", &commits)] {
        assert_eq!(manifest["files"][name]["bytes"], bytes.len() as u64);
        assert_eq!(manifest["files"][name]["sha256"], digest(bytes));
    }
    let lines: Vec<Value> = std::str::from_utf8(&commits)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(lines.len(), 2);
    // Request-key order differs from commit order: every relation carries its own identity.
    assert_eq!(lines[0]["receipt"]["request_id"], "a-correction");
    let mut offset = 0;
    for line in &lines {
        let index = line["receipt"]["commit_seq"].as_u64().unwrap() as usize - 1;
        assert_eq!(
            line["receipt"],
            serde_json::to_value(&receipts[index]).unwrap()
        );
        assert_eq!(line["recorded_at_ms"], clocks[index]);
        let exported = line["revisions"].as_array().unwrap();
        assert_eq!(exported.len(), revisions[index].len());
        for (row, original) in exported.iter().zip(&revisions[index]) {
            assert_eq!(row["key"], serde_json::to_value(&original.key).unwrap());
            assert_eq!(row["commit_seq"], original.commit_seq);
            assert_eq!(row["previous"], json!(original.previous));
            assert_eq!(row["content_sha256"], original.content_sha256);
            assert_eq!(row["blob_offset"], offset as u64);
            let length = row["blob_bytes"].as_u64().unwrap() as usize;
            assert_eq!(&blobs[offset..offset + length], original.content.as_bytes());
            offset += length;
        }
    }
    assert_eq!(offset, blobs.len());
    let before = std::fs::read(destination.join("manifest.json")).unwrap();
    assert!(!fixture.export(&destination).status.success());
    assert_eq!(
        std::fs::read(destination.join("manifest.json")).unwrap(),
        before
    );
    let reopened = Storage::open(&fixture.config()).await.unwrap();
    assert_eq!(reopened.sequence().unwrap(), 2);
    for (index, receipt) in receipts.iter().enumerate() {
        assert_eq!(
            reopened.receipt(&receipt.request_id).unwrap(),
            Some(receipt.clone())
        );
        assert_eq!(
            reopened.recorded_at_ms(receipt.commit_seq).unwrap(),
            clocks[index]
        );
    }
    reopened.close().await.unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        assert_eq!(
            std::fs::metadata(&destination)
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o700
        );
        for name in ["manifest.json", "commits.jsonl", "blobs.bin"] {
            assert_eq!(
                std::fs::metadata(destination.join(name))
                    .unwrap()
                    .permissions()
                    .mode()
                    & 0o777,
                0o600
            );
        }
    }
}

#[tokio::test]
async fn archive_export_requires_existing_exclusive_source_and_disjoint_new_destination() {
    let fixture = Fixture::new();
    let destination = fixture.0.join("export");
    assert!(!fixture.export(&destination).status.success());
    assert!(!fixture.0.join("data").exists());
    assert!(!destination.exists());
    let store = Storage::open(&fixture.config()).await.unwrap();
    assert!(!fixture.export(&destination).status.success());
    assert!(!destination.exists());
    store.close().await.unwrap();
    let nested = fixture.0.join("data/authority/export");
    let rejected = fixture.export(&nested);
    assert!(!rejected.status.success());
    let error: Value = serde_json::from_slice(&rejected.stderr).unwrap();
    assert_eq!(error["error"]["code"], "UNSAFE_STORAGE_PATH");
    assert!(!nested.exists());
    let empty = fixture.export(&destination);
    assert!(
        empty.status.success(),
        "{}",
        String::from_utf8_lossy(&empty.stderr)
    );
    let report: Value = serde_json::from_slice(&empty.stdout).unwrap();
    assert_eq!(report["data"]["commits"], 0);
    assert_eq!(report["data"]["files"]["blobs.bin"]["bytes"], 0);
    assert_eq!(report["data"]["files"]["commits.jsonl"]["bytes"], 0);
}

#[tokio::test]
async fn archive_export_accepts_a_relative_new_directory() {
    let fixture = Fixture::new();
    Storage::open(&fixture.config())
        .await
        .unwrap()
        .close()
        .await
        .unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_akasha-otsuge"))
        .current_dir(&fixture.0)
        .arg("--config")
        .arg(fixture.0.join("config.toml"))
        .args(["archive", "export", "relative-export"])
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(fixture.0.join("relative-export/manifest.json").is_file());
}

#[cfg(target_os = "linux")]
#[tokio::test]
async fn archive_export_does_not_publish_a_manifest_when_a_blob_write_fails() {
    let fixture = Fixture::new();
    let store = Storage::open(&fixture.config()).await.unwrap();
    let original = CommitRequest {
        request_id: "write-failure-control".into(),
        config_generation: 1,
        policy_revision: None,
        changes: vec![Change {
            key: RecordKey {
                kind: RecordKind::Evidence,
                id: "large-original".into(),
            },
            expected_revision: None,
            content: "原文".repeat(10_000),
        }],
    };
    let receipt = store.commit(&original).await.unwrap();
    store.close().await.unwrap();
    let destination = fixture.0.join("interrupted");
    // This is a real EFBIG write failure, not a claim to simulate ENOSPC or power loss.
    let output = Command::new("python3")
        .args(["-c", "import os,resource,signal,sys; signal.signal(signal.SIGXFSZ,signal.SIG_IGN); resource.setrlimit(resource.RLIMIT_FSIZE,(4096,4096)); os.execv(sys.argv[1],sys.argv[1:])"])
        .arg(env!("CARGO_BIN_EXE_akasha-otsuge"))
        .arg("--config")
        .arg(fixture.0.join("config.toml"))
        .args(["archive", "export"])
        .arg(&destination)
        .output().unwrap();
    assert_eq!(output.status.code(), Some(1), "{output:?}");
    let error: Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "STORAGE_IO");
    assert!(destination.join("blobs.bin").is_file());
    assert!(!destination.join("manifest.json").exists());
    let reopened = Storage::open(&fixture.config()).await.unwrap();
    assert_eq!(
        reopened.receipt(&original.request_id).unwrap(),
        Some(receipt)
    );
    assert_eq!(
        reopened
            .current(&original.changes[0].key)
            .unwrap()
            .unwrap()
            .content,
        original.changes[0].content
    );
    reopened.close().await.unwrap();
}
