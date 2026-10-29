use std::{
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

#[path = "support/hermes_caller.rs"]
mod caller;

#[path = "support/hermes_runner.rs"]
mod runner;

#[path = "support/hermes_install.rs"]
mod installation;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "akasha-hermes-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    async fn open(&self) -> akasha_otsuge::storage::Storage {
        use akasha_otsuge::config::{Paths, resolve};
        akasha_otsuge::storage::Storage::open(
            &resolve(
                "",
                None,
                "",
                &Paths {
                    data_dir: self.0.join("data"),
                    socket_path: self.0.join("service.sock"),
                },
            )
            .unwrap(),
        )
        .await
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn doctor_observes_hermes_without_model_calls_or_a_daemon() {
    let fixture = Fixture::new();
    let binary = fixture.0.join("hermes");
    std::fs::write(
        &binary,
        "#!/bin/sh\n[ \"$#\" = 1 ] && [ \"$1\" = --version ] || exit 78\nprintf 'Hermes Agent v0.22.5+1914.gfdb04a0 (2026.9.24) · stable · local fdb04a00c162\\n'\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_akasha-otsuge"))
        .args(["doctor", "--host", "hermes", "--binary"])
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["data"]["host"], "hermes");
    assert_eq!(report["data"]["version"], "0.22.5+1914.gfdb04a0");
    assert_eq!(report["data"]["self_test"]["status"], "not_run");
    assert_eq!(report["coverage"]["model_auth"], "not_checked");
    assert_eq!(report["coverage"]["version_supported"], true);
    assert!(output.stderr.is_empty());
}

#[tokio::test]
async fn qualified_git_build_labels_do_not_admit_other_older_versions() {
    let fixture = Fixture::new();
    let binary = fixture.0.join("hermes");
    for (version, supported) in [
        ("git.4960e41", true),
        ("0.21.5+18146.g4960e41", true),
        ("0.21.5+18146.g0000000", false),
        ("0.21.5", false),
    ] {
        std::fs::write(
            &binary,
            format!("#!/bin/sh\nprintf 'Hermes Agent v{version}\\n'\n"),
        )
        .unwrap();
        std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
        let report = akasha_otsuge::adapter::hermes::probe(&binary)
            .await
            .unwrap();
        assert_eq!(report.version.as_deref(), Some(version));
        assert_eq!(
            akasha_otsuge::adapter::hermes::supported_version(report.version.as_deref()),
            supported
        );
    }
}

#[tokio::test]
async fn snapshot_backfill_resumes_and_preserves_old_fences_across_native_rewrites() {
    use akasha_otsuge::{
        adapter::hermes::capture::{EXPORT_STREAM, reconcile_export},
        capture::*,
        domain::Scope,
    };
    let fixture = Fixture::new();
    let source = SourceIdentity {
        adapter: "hermes".into(),
        profile: "registered-home".into(),
        session: "20261029_021600_a00001".into(),
        thread: "20261029_021600_a00001".into(),
        agent: "main".into(),
    };
    let scope = Scope {
        user: "user".into(),
        project: Some("project".into()),
        branch_or_version: None,
        session: None,
    };
    let binding = CaptureBinding::from_source(
        source.clone(),
        scope.clone(),
        EXPORT_STREAM,
        CoverageLevel::VisibleText,
    )
    .unwrap();
    let before = include_bytes!("fixtures/hermes/0.22.5/root-before.jsonl");
    let after = include_bytes!("fixtures/hermes/0.22.5/root-compacted.jsonl");
    let store = fixture.open().await;
    let partial = reconcile_export(&store, &source, scope.clone(), "custom", before, 2, 1)
        .await
        .unwrap();
    assert_eq!(partial.capture.processed, 2);
    assert_eq!(
        partial.capture.checkpoint.checkpoint.pending,
        Some(PendingReason::Backlog)
    );
    store.close().await.unwrap();
    drop(store);
    let store = fixture.open().await;
    let complete = reconcile_export(&store, &source, scope.clone(), "custom", before, 64, 1)
        .await
        .unwrap();
    assert_eq!(complete.capture.processed, 4);
    assert_eq!(complete.capture.checkpoint.checkpoint.pending, None);
    let old_tip = complete.tip.unwrap();
    let old_seq = store.sequence().unwrap();
    let replay = reconcile_export(&store, &source, scope.clone(), "custom", before, 64, 1)
        .await
        .unwrap();
    assert_eq!(replay.capture.processed, 0);
    assert_eq!(store.sequence().unwrap(), old_seq);
    let compacted = reconcile_export(&store, &source, scope.clone(), "custom", after, 64, 1)
        .await
        .unwrap();
    assert_eq!(compacted.capture.processed, 8);
    assert_eq!(compacted.capture.deduplicated, 6);
    assert_eq!(compacted.capture.checkpoint.checkpoint.pending, None);
    let capture = Capture::new(&store);
    let historical = capture.lineage(&binding, &old_tip, old_seq, 64).unwrap();
    assert_eq!(historical.events.len(), 6);
    let current_tip = compacted.tip.unwrap();
    let current = capture
        .lineage(&binding, &current_tip, store.sequence().unwrap(), 64)
        .unwrap();
    assert_eq!(current.events.len(), 8);
    assert_eq!(current.missing, None);
    let text = current
        .events
        .iter()
        .map(|e| capture.text(e).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Original public preference"));
    assert!(text.contains("Summary of earlier public turns"));
    assert_eq!(text.matches("Answer before interruption.").count(), 1);
    let saved = capture.checkpoint(&binding).unwrap();
    assert!(
        reconcile_export(&store, &source, scope.clone(), "custom", b"{", 64, 1)
            .await
            .is_err()
    );
    assert_eq!(capture.checkpoint(&binding).unwrap(), saved);
    let mut undone: serde_json::Value = serde_json::from_slice(after).unwrap();
    undone["messages"][0]["compacted"] = 0.into();
    let changed = reconcile_export(
        &store,
        &source,
        scope,
        "custom",
        &serde_json::to_vec(&undone).unwrap(),
        64,
        1,
    )
    .await
    .unwrap();
    let page = capture
        .lineage(
            &binding,
            &changed.tip.unwrap(),
            store.sequence().unwrap(),
            64,
        )
        .unwrap();
    assert_eq!(page.events.len(), 7);
    assert!(!page.events.iter().any(|e| {
        capture
            .text(e)
            .unwrap()
            .contains("Original public preference")
    }));
    assert_eq!(
        capture
            .lineage(&binding, &current_tip, store.sequence().unwrap(), 64)
            .unwrap()
            .events
            .len(),
        8
    );
    store.close().await.unwrap();
}
