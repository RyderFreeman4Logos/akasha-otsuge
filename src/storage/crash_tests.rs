use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::time::{Duration, Instant};

use super::*;
use crate::config::{Paths, resolve};

mod incremental;
pub(crate) mod native_open;

pub(crate) fn checkpoint(phase: &str) {
    if std::env::var("AKASHA_TEST_CRASH_PHASE").as_deref() != Ok(phase) {
        return;
    }
    let root = PathBuf::from(std::env::var_os("AKASHA_TEST_CRASH_ROOT").unwrap());
    // Publish the phase and readiness together; create alone can expose an empty marker.
    std::fs::write(root.join("reached.pending"), phase).unwrap();
    std::fs::rename(root.join("reached.pending"), root.join("reached")).unwrap();
    loop {
        std::thread::park();
    }
}

fn config(root: &Path) -> ResolvedConfig {
    resolve(
        "",
        None,
        "",
        &Paths {
            data_dir: root.join("data"),
            socket_path: root.join("daemon.sock"),
        },
    )
    .unwrap()
}

fn request(id: &str) -> CommitRequest {
    CommitRequest {
        request_id: id.into(),
        config_generation: 1,
        policy_revision: Some("policy-1".into()),
        changes: [
            RecordKind::Evidence,
            RecordKind::Memory,
            RecordKind::State,
            RecordKind::Job,
        ]
        .into_iter()
        .map(|kind| Change {
            key: RecordKey {
                kind,
                id: id.into(),
            },
            expected_revision: None,
            content: "real atomic content\n".repeat(1024),
        })
        .collect(),
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[test]
fn native_handle_keeps_its_directory_after_storage_owner_is_dropped() {
    let root = std::env::temp_dir().join(format!("akasha-native-owner-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let _fixture = OwnedFixture(root.clone());
    runtime().block_on(async {
        let store = Storage::open(&config(&root)).await.unwrap();
        store.commit(&request("retained")).await.unwrap();
        let tree = store.tree.clone();
        let backup = root.join("native-backup");
        drop(store);
        let (tree, checkpoint) = tokio::task::spawn_blocking(move || {
            let checkpoint = tree.create_checkpoint(backup);
            (tree, checkpoint)
        })
        .await
        .unwrap();
        tree.close().await.unwrap();
        assert!(
            checkpoint.is_ok(),
            "native checkpoint lost its retained directory: {checkpoint:?}"
        );
    });
}

#[test]
#[ignore = "launched only by the final-drop supervisor with isolated descriptor numbers"]
fn native_final_drop_child() {
    use std::os::fd::AsRawFd;
    let root = PathBuf::from(std::env::var_os("AKASHA_TEST_CRASH_ROOT").unwrap());
    runtime().block_on(async {
        let store = Storage::open(&config(&root)).await.unwrap();
        store.commit(&request("drop")).await.unwrap();
        store.checkpoint(&root.join("backup")).await.unwrap();
        let authority = root.join("data/authority");
        let descriptors: Vec<i32> = std::fs::read_dir("/proc/self/fd")
            .unwrap()
            .filter_map(|entry| {
                let entry = entry.ok()?;
                (std::fs::read_link(entry.path()).ok()? == authority)
                    .then(|| entry.file_name().to_str()?.parse().ok())?
            })
            .collect();
        assert_eq!(descriptors.len(), 1);
        let fd = descriptors[0];
        let foreign = root.join("foreign");
        std::fs::create_dir_all(foreign.join("kv/wal")).unwrap();
        let victim = foreign.join("kv/wal/00000000000000000000.wal");
        std::fs::write(&victim, b"foreign WAL").unwrap();
        let foreign = std::fs::File::open(foreign).unwrap();
        drop(store);
        let deadline = Instant::now() + Duration::from_secs(5);
        while PathBuf::from(format!("/proc/self/fd/{fd}")).exists() {
            assert!(
                Instant::now() < deadline,
                "native close retained its directory indefinitely"
            );
            tokio::time::sleep(Duration::from_millis(1)).await;
        }
        // Duplicate only into a free number; never replace a live owner's descriptor.
        let reused = rustix::io::fcntl_dupfd_cloexec(&foreign, fd).unwrap();
        assert_eq!(reused.as_raw_fd(), fd);
        tokio::time::sleep(Duration::from_millis(100)).await;
        assert_eq!(
            std::fs::read(victim).ok().as_deref(),
            Some(b"foreign WAL".as_slice())
        );
    });
}

#[test]
fn final_native_drop_cannot_redirect_cleanup_after_descriptor_reuse() {
    let root = std::env::temp_dir().join(format!("akasha-final-drop-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let _fixture = OwnedFixture(root.clone());
    let log = std::fs::File::create(root.join("child.log")).unwrap();
    let mut child = OwnedChild(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "storage::crash_tests::native_final_drop_child",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("AKASHA_TEST_CRASH_ROOT", &root)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(15);
    let status = loop {
        if let Some(status) = child.0.try_wait().unwrap() {
            break status;
        }
        assert!(Instant::now() < deadline, "final-drop child timed out");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert!(
        status.success(),
        "{}",
        std::fs::read_to_string(root.join("child.log")).unwrap()
    );
}

#[test]
#[ignore = "launched only by the crash supervisor with an isolated database"]
fn crash_child() {
    let root = PathBuf::from(std::env::var_os("AKASHA_TEST_CRASH_ROOT").unwrap());
    runtime().block_on(async {
        let store = Storage::open(&config(&root)).await.unwrap();
        let receipt = store.commit(&request("crash")).await.unwrap();
        assert_eq!(receipt.commit_seq, 2);
        checkpoint("after-reply");
        panic!("requested crash checkpoint was not reached");
    });
}

struct OwnedFixture(PathBuf);

impl Drop for OwnedFixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

struct OwnedChild(Child);

impl Drop for OwnedChild {
    fn drop(&mut self) {
        if self.0.try_wait().ok().flatten().is_none() {
            let _ = self.0.kill();
            let _ = self.0.wait();
        }
    }
}

#[test]
fn killed_transactions_recover_whole_batches_and_retry_exactly_once() {
    for phase in [
        "before-stage",
        "partial-stage",
        "before-commit",
        "commit-pending",
        "after-commit",
        "after-reply",
    ] {
        let root =
            std::env::temp_dir().join(format!("akasha-crash-{}-{phase}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let _fixture = OwnedFixture(root.clone());
        let config = config(&root);
        runtime().block_on(async {
            let store = Storage::open(&config).await.unwrap();
            store.commit(&request("baseline")).await.unwrap();
            store.close().await.unwrap();
        });
        kill_at(&root, phase, "storage::crash_tests::crash_child");
        runtime().block_on(async {
            let store = Storage::open(&config).await.unwrap();
            let receipt = store.receipt("crash").unwrap();
            let seq = store.sequence().unwrap();
            let committed = receipt.is_some();
            assert_eq!(seq, if committed { 2 } else { 1 }, "{phase}");
            if phase.starts_with("after-") {
                assert!(committed, "lost acknowledged commit at {phase}");
            }
            if ["before-stage", "partial-stage", "before-commit"].contains(&phase) {
                assert!(!committed, "{phase}");
            }
            assert_eq!(store.outbox(2).unwrap().is_some(), committed);
            for change in &request("crash").changes {
                let current = store.current(&change.key).unwrap();
                assert_eq!(current.is_some(), committed, "{phase}");
                assert_eq!(store.revision(&change.key, 2).unwrap(), current);
                if let Some(revision) = current {
                    assert_eq!(revision.content, change.content);
                    assert_eq!(revision.content_sha256, hash(change.content.as_bytes()));
                    assert_eq!(revision.previous, None);
                }
            }
            let recovered = store.commit(&request("crash")).await.unwrap();
            if let Some(receipt) = receipt {
                assert_eq!(recovered, receipt);
            }
            assert_eq!(recovered.commit_seq, 2);
            assert_eq!(store.commit(&request("crash")).await.unwrap(), recovered);
            assert_eq!(store.sequence().unwrap(), 2);
            assert!(store.outbox(3).unwrap().is_none());
            for change in &request("baseline").changes {
                assert_eq!(
                    store.current(&change.key).unwrap().unwrap().content,
                    change.content
                );
            }
            store.close().await.unwrap();
            println!(
                "crash checkpoint={phase} recovered_seq={seq} retry_seq=2 all_records_atomic=true"
            );
        });
    }
}

pub(crate) fn kill_at(root: &Path, phase: &str, test: &str) {
    kill_at_with(root, phase, test, || ());
}

fn kill_at_with(root: &Path, phase: &str, test: &str, before_kill: impl FnOnce()) {
    let log = std::fs::File::create(root.join("child.log")).unwrap();
    let mut child = OwnedChild(
        Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                test,
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env("AKASHA_TEST_CRASH_ROOT", root)
            .env("AKASHA_TEST_CRASH_PHASE", phase)
            .stdin(Stdio::null())
            .stdout(log.try_clone().unwrap())
            .stderr(log)
            .spawn()
            .unwrap(),
    );
    let deadline = Instant::now() + Duration::from_secs(30);
    while !root.join("reached").exists() {
        assert!(
            child.0.try_wait().unwrap().is_none(),
            "child exited before {phase}: {}",
            std::fs::read_to_string(root.join("child.log")).unwrap()
        );
        assert!(Instant::now() < deadline, "child did not reach {phase}");
        std::thread::sleep(Duration::from_millis(10));
    }
    assert_eq!(
        std::fs::read_to_string(root.join("reached")).unwrap(),
        phase
    );
    before_kill();
    child.0.kill().unwrap();
    let status = child.0.wait().unwrap();
    #[cfg(unix)]
    {
        use std::os::unix::process::ExitStatusExt;
        assert_eq!(status.signal(), Some(9));
    }
    assert!(!status.success());
}

#[test]
#[ignore = "launched only by the import crash supervisor"]
fn import_crash_child() {
    let root = PathBuf::from(std::env::var_os("AKASHA_TEST_CRASH_ROOT").unwrap());
    runtime().block_on(async {
        Storage::import_archive(&config(&root), &root.join("archive"))
            .await
            .unwrap();
        panic!("requested import checkpoint was not reached");
    });
}

#[test]
fn import_runtime_boundary_rejects_invalid_metadata() {
    let root = std::env::temp_dir().join(format!("akasha-import-boundary-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let _fixture = OwnedFixture(root.clone());
    runtime().block_on(async {
        let store = Storage::open(&config(&root)).await.unwrap();
        assert!(store.import_identity().unwrap().is_none());
        for value in [
            serde_json::json!({"archive_sha256":"a".repeat(64),"commit_seq":1}),
            serde_json::json!({"archive_sha256":"z".repeat(64),"commit_seq":0}),
            serde_json::json!({"archive_sha256":"a".repeat(64),"commit_seq":0,"extra":true}),
            serde_json::json!({"commit_seq":0}),
        ] {
            let mut tx = store.tree.begin().unwrap();
            tx.set(b"meta/archive_import", serde_json::to_vec(&value).unwrap())
                .unwrap();
            tx.commit().await.unwrap();
            assert!(
                matches!(store.import_identity(), Err(error) if error.code == "INVALID_ARCHIVE")
            );
        }
        store.close().await.unwrap();
    });
}

#[test]
fn killed_archive_import_resumes_staging_heads_and_publication_without_duplicates() {
    for phase in [
        "import-staged",
        "import-head",
        "import-before-publish",
        "import-published",
    ] {
        let root = std::env::temp_dir().join(format!(
            "akasha-import-crash-{}-{phase}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let _fixture = OwnedFixture(root.clone());
        let original = request("z-original");
        let memory = original.changes[1].key.clone();
        let (receipts, clocks, views) = runtime().block_on(async {
            let source_config = config(&root.join("source"));
            let source = Storage::open(&source_config).await.unwrap();
            let mut receipts = vec![source.commit(&original).await.unwrap()];
            let mut change = original.clone();
            change.request_id = "a-amendment".into();
            change.changes.retain(|c| c.key.kind == RecordKind::Memory);
            change.changes[0].expected_revision = Some(1);
            change.changes[0].content = "corrected".into();
            receipts.push(source.commit(&change).await.unwrap());
            change.request_id = "m-retired".into();
            change.changes[0].expected_revision = Some(2);
            change.changes[0].content = "retired".into();
            receipts.push(source.commit(&change).await.unwrap());
            let clocks: Vec<_> = (1..=3).map(|s| source.recorded_at_ms(s).unwrap()).collect();
            let views: Vec<_> = (0..=3).map(|s| source.at(&memory, s).unwrap()).collect();
            source.close().await.unwrap();
            Storage::export_archive(&source_config, &root.join("archive"))
                .await
                .unwrap();
            (receipts, clocks, views)
        });
        kill_at(&root, phase, "storage::crash_tests::import_crash_child");
        assert_eq!(
            root.join("data/authority").exists(),
            phase == "import-published"
        );
        if phase != "import-published" {
            assert!(root.join("data/.archive-import/kv").is_dir());
        }
        runtime().block_on(async {
            let target = config(&root);
            if phase == "import-staged" {
                use std::os::unix::fs::PermissionsExt;
                let stage = root.join("data/.archive-import");
                std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o755)).unwrap();
                let error = Storage::import_archive(&target, &root.join("archive"))
                    .await
                    .err()
                    .expect("accepted public staging directory");
                assert_eq!(error.code, "UNSAFE_STORAGE_PATH");
                assert!(!root.join("data/authority").exists());
                std::fs::set_permissions(&stage, std::fs::Permissions::from_mode(0o700)).unwrap();
            }
            for _ in 0..2 {
                Storage::import_archive(&target, &root.join("archive"))
                    .await
                    .unwrap();
                let restored = Storage::open(&target).await.unwrap();
                assert_eq!(restored.sequence().unwrap(), 3);
                for (seq, view) in views.iter().enumerate() {
                    assert_eq!(&restored.at(&memory, seq as u64).unwrap(), view);
                }
                for (index, receipt) in receipts.iter().enumerate() {
                    assert_eq!(
                        restored.receipt(&receipt.request_id).unwrap().as_ref(),
                        Some(receipt)
                    );
                    assert_eq!(
                        restored.recorded_at_ms(receipt.commit_seq).unwrap(),
                        clocks[index]
                    );
                }
                assert_eq!(restored.commit(&original).await.unwrap(), receipts[0]);
                assert_eq!(restored.import_identity().unwrap().unwrap().commit_seq, 3);
                assert!(!restored.locally_created(&original.changes[0].key).unwrap());
                restored.close().await.unwrap();
            }
            assert!(!root.join("data/.archive-import").exists());
        });
        println!("import checkpoint={phase} signal=9 recovered_seq=3 duplicate_commits=0");
    }
}

fn capture_input() -> (
    crate::capture::CaptureBinding,
    crate::capture::EventDraft,
    crate::domain::Ingress,
    crate::capture::Advance,
) {
    use crate::capture::*;
    use crate::domain::{Ingress, Scope};
    let scope = Scope {
        user: "user".into(),
        project: None,
        branch_or_version: None,
        session: None,
    };
    let binding = CaptureBinding::from_source(
        SourceIdentity {
            adapter: "fixture".into(),
            profile: "p".into(),
            session: "s".into(),
            thread: "t".into(),
            agent: "a".into(),
        },
        scope.clone(),
        "log",
        CoverageLevel::AvailablePayloads,
    )
    .unwrap();
    let event = EventDraft {
        event_id: "original".into(),
        source_version: "1".into(),
        runtime_instance: "source-runtime".into(),
        branch: "main".into(),
        parent: None,
        fork_cutoff: None,
        event_time_ms: None,
        observed_time_ms: 1,
        visibility: Visibility::User,
        kind: EventKind::Message {
            message_id: "message".into(),
            phase: MessagePhase::Final,
        },
        text: "精确原文\0\n".repeat(15000),
    };
    let ingress = Ingress::unverified("fixture", "binding", scope).unwrap();
    let advance = Advance {
        expected_revision: None,
        checkpoint: Checkpoint {
            cursor: "source-offset-1".into(),
            coverage: CoverageLevel::AvailablePayloads,
            loss: vec![],
            pending: None,
        },
    };
    (binding, event, ingress, advance)
}

#[test]
#[ignore = "launched only by the capture crash supervisor"]
fn capture_crash_child() {
    let root = PathBuf::from(std::env::var_os("AKASHA_TEST_CRASH_ROOT").unwrap());
    runtime().block_on(async {
        let store = Storage::open(&config(&root)).await.unwrap();
        let (binding, event, ingress, advance) = capture_input();
        crate::capture::Capture::new(&store)
            .ingest(
                &binding,
                &event,
                &vec![0xff; 150000],
                &ingress,
                Some(&advance),
                1,
            )
            .await
            .unwrap();
        panic!("capture crash checkpoint not reached");
    });
}

#[test]
fn killed_capture_reuses_staged_chunks_and_publishes_checkpoint_atomically() {
    use crate::capture::{Capture, EventRef, SourceIdentity};
    for phase in [
        "capture-intent",
        "capture-raw",
        "capture-source",
        "capture-published",
    ] {
        let root = std::env::temp_dir().join(format!(
            "akasha-capture-crash-{}-{phase}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let _fixture = OwnedFixture(root.clone());
        kill_at(&root, phase, "storage::crash_tests::capture_crash_child");
        runtime().block_on(async {
            let store=Storage::open(&config(&root)).await.unwrap();let capture=Capture::new(&store);let (binding,event,ingress,advance)=capture_input();
            let reference=EventRef{source:SourceIdentity{adapter:"fixture".into(),profile:"p".into(),session:"s".into(),thread:"t".into(),agent:"a".into()},event_id:event.event_id.clone()};
            let before=capture.event(&binding,&reference,store.sequence().unwrap()).unwrap();let checkpoint=capture.checkpoint(&binding).unwrap();
            assert_eq!(before.is_some(),phase=="capture-published");assert_eq!(checkpoint.is_some(),before.is_some());
            if let (Some(event),Some(checkpoint))=(&before,&checkpoint) {assert_eq!(event.revision,checkpoint.revision);}
            let raw=vec![0xff;150000];let accepted=capture.ingest(&binding,&event,&raw,&ingress,Some(&advance),1).await.unwrap();
            assert_eq!(capture.checkpoint(&binding).unwrap().unwrap().revision,accepted.revision);
            assert_eq!(capture.raw(&accepted).unwrap(),raw);assert_eq!(capture.text(&accepted).unwrap(),event.text);
            let sequence=store.sequence().unwrap();assert_eq!(capture.ingest(&binding,&event,&raw,&ingress,Some(&advance),1).await.unwrap(),accepted);assert_eq!(store.sequence().unwrap(),sequence);
            store.close().await.unwrap();println!("capture checkpoint={phase} SIGKILL_recovered=true checkpoint_atomic=true retry_unchanged=true");
        });
    }
}

#[test]
fn import_publication_admission_excludes_open_and_restore_before_authority_creation() {
    use std::os::unix::fs::OpenOptionsExt;
    let root = std::env::temp_dir().join(format!("akasha-import-admission-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let _fixture = OwnedFixture(root.clone());
    runtime().block_on(async {
        let source = Storage::open(&config(&root.join("source"))).await.unwrap();
        source.commit(&request("original")).await.unwrap();
        let backup = root.join("backup");
        source.checkpoint(&backup).await.unwrap();
        source.close().await.unwrap();
        let mut missed = Vec::new();
        for entry in ["open", "restore"] {
            let target = root.join(entry);
            let cfg = config(&target);
            let data = target.join("data");
            layout::private_dir(&data, true).unwrap();
            let guard = std::fs::OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .mode(0o600)
                .open(data.join(".archive-import.lock"))
                .unwrap();
            guard.try_lock().unwrap();
            let result = if entry == "open" {
                Storage::open(&cfg).await
            } else {
                Storage::restore(&cfg, &backup).await
            };
            let denied = match result {
                Ok(store) => {
                    store.close().await.unwrap();
                    false
                }
                Err(error) => error.code == "IMPORT_BUSY",
            };
            let untouched = !data.join("authority").exists();
            eprintln!("entry={entry} denied={denied} authority_untouched={untouched}");
            if !denied || !untouched {
                missed.push(entry);
            }
            guard.unlock().unwrap();
            if denied && untouched {
                let store = if entry == "open" {
                    Storage::open(&cfg).await
                } else {
                    Storage::restore(&cfg, &backup).await
                }
                .unwrap();
                store.close().await.unwrap();
            }
        }
        assert!(
            missed.is_empty(),
            "publication admission bypassed: {missed:?}"
        );
    });
}
