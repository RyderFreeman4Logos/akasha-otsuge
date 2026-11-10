use super::*;

async fn prepare(root: &Path) {
    let source_config = config(&root.join("source"));
    let source = Storage::open(&source_config).await.unwrap();
    source.commit(&request("z-original")).await.unwrap();
    source.checkpoint(&root.join("backup")).await.unwrap();
    source.close().await.unwrap();
    Storage::export_archive(&source_config, &root.join("old"))
        .await
        .unwrap();
    Storage::import_archive(&config(root), &root.join("old"))
        .await
        .unwrap();
    let source = Storage::open(&source_config).await.unwrap();
    source.commit(&request("a-incremental")).await.unwrap();
    let mut correction = request("z-original");
    correction.request_id = "m-correction".into();
    correction
        .changes
        .retain(|c| c.key.kind == RecordKind::Memory);
    correction.changes[0].expected_revision = Some(1);
    correction.changes[0].content = "retired; retain original history".into();
    source.commit(&correction).await.unwrap();
    source.close().await.unwrap();
    Storage::export_archive(&source_config, &root.join("archive"))
        .await
        .unwrap();
}

fn assert_archive_equal(a: &Path, b: &Path) {
    for name in ["manifest.json", "commits.jsonl", "blobs.bin"] {
        assert_eq!(
            std::fs::read(a.join(name)).unwrap(),
            std::fs::read(b.join(name)).unwrap(),
            "{name}"
        );
    }
}

#[test]
fn incremental_sigkill_preserves_complete_history_and_owner_exclusion() {
    for phase in [
        "import-staged",
        "import-head",
        "incremental-validated",
        "import-before-publish",
        "incremental-marker",
        "incremental-exchanged",
        "incremental-synced",
        "incremental-cleaned",
        "import-published",
    ] {
        let root =
            std::env::temp_dir().join(format!("akasha-incremental-{}-{phase}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let _fixture = OwnedFixture(root.clone());
        runtime().block_on(prepare(&root));
        kill_at_with(
            &root,
            phase,
            "storage::crash_tests::import_crash_child",
            || {
                runtime().block_on(async {
                    let target = config(&root);
                    for result in [
                        Storage::open(&target).await,
                        Storage::restore(&target, &root.join("backup")).await,
                    ] {
                        assert!(matches!(result, Err(e) if e.code == "IMPORT_BUSY"));
                    }
                    assert!(matches!(
                        Storage::import_archive(&target, &root.join("archive")).await,
                        Err(e) if e.code == "IMPORT_BUSY"
                    ));
                });
            },
        );
        let published = [
            "incremental-exchanged",
            "incremental-synced",
            "incremental-cleaned",
            "import-published",
        ]
        .contains(&phase);
        runtime().block_on(async {
            let target = config(&root);
            let before = Storage::open(&target).await.unwrap();
            let expected = if published { 3 } else { 1 };
            assert_eq!(before.sequence().unwrap(), expected);
            assert_eq!(
                before.import_identity().unwrap().unwrap().commit_seq,
                expected
            );
            assert_eq!(
                before.receipt("a-incremental").unwrap().is_some(),
                published
            );
            before.close().await.unwrap();
            for archive in ["archive", "archive", "old"] {
                Storage::import_archive(&target, &root.join(archive))
                    .await
                    .unwrap();
            }
            let store = Storage::open(&target).await.unwrap();
            assert_eq!(store.sequence().unwrap(), 3);
            assert_eq!(store.import_identity().unwrap().unwrap().commit_seq, 3);
            let original = request("z-original");
            let memory = &original.changes[1].key;
            for seq in [1, 2] {
                assert_eq!(
                    store.at(memory, seq).unwrap().unwrap().content,
                    original.changes[1].content
                );
            }
            assert_eq!(
                store.current(memory).unwrap().unwrap().content,
                "retired; retain original history"
            );
            assert!(!store.locally_written(2).unwrap());
            assert!(!store.locally_written(3).unwrap());
            assert!(
                !store
                    .locally_created(&request("a-incremental").changes[3].key)
                    .unwrap()
            );
            store.close().await.unwrap();
            Storage::export_archive(&target, &root.join("exported"))
                .await
                .unwrap();
            assert_archive_equal(&root.join("archive"), &root.join("exported"));
            assert!(!root.join("data/.archive-import").exists());
            assert!(!root.join("data/.archive-replace.json").exists());
        });
        println!(
            "incremental checkpoint={phase} signal=9 recovered_seq=3 duplicate_commits=0 excluded_entries=3"
        );
    }
}

#[test]
fn incremental_recovery_preserves_local_writes_on_either_side_of_exchange() {
    for (phase, published) in [
        ("incremental-marker", false),
        ("incremental-exchanged", true),
    ] {
        let root = std::env::temp_dir().join(format!(
            "akasha-incremental-local-{}-{phase}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let _fixture = OwnedFixture(root.clone());
        runtime().block_on(prepare(&root));
        kill_at(&root, phase, "storage::crash_tests::import_crash_child");
        runtime().block_on(async {
            let target = config(&root);
            let store = Storage::open(&target).await.unwrap();
            let receipt = store.commit(&request("local-after-crash")).await.unwrap();
            let boundary = store.import_identity().unwrap().unwrap().commit_seq;
            store.close().await.unwrap();
            Storage::export_archive(&target, &root.join("before"))
                .await
                .unwrap();
            let result = Storage::import_archive(&target, &root.join("archive")).await;
            if published {
                assert!(result.is_ok());
            } else {
                assert!(matches!(result, Err(e) if e.code == "IMPORT_CONFLICT"));
            }
            let store = Storage::open(&target).await.unwrap();
            assert_eq!(store.sequence().unwrap(), receipt.commit_seq);
            assert_eq!(store.receipt("local-after-crash").unwrap(), Some(receipt));
            assert_eq!(
                store.import_identity().unwrap().unwrap().commit_seq,
                boundary
            );
            store.close().await.unwrap();
            Storage::export_archive(&target, &root.join("after"))
                .await
                .unwrap();
            assert_archive_equal(&root.join("before"), &root.join("after"));
        });
        println!("incremental local_after={phase} preserved=true published={published}");
    }
}

#[test]
fn incremental_recovery_preserves_foreign_stage_and_resumes_after_owner_restored() {
    for phase in ["incremental-marker", "incremental-exchanged"] {
        let root = std::env::temp_dir().join(format!(
            "akasha-incremental-foreign-{}-{phase}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let _fixture = OwnedFixture(root.clone());
        runtime().block_on(prepare(&root));
        kill_at(&root, phase, "storage::crash_tests::import_crash_child");
        let stage = root.join("data/.archive-import");
        let parked = root.join("parked");
        std::fs::rename(&stage, &parked).unwrap();
        layout::private_dir(&stage, false).unwrap();
        std::fs::write(stage.join("foreign"), b"do not remove").unwrap();
        runtime().block_on(async {
            let target = config(&root);
            Storage::export_archive(&target, &root.join("before")).await.unwrap();
            assert!(matches!(Storage::import_archive(&target, &root.join("archive")).await, Err(e) if e.code == "IMPORT_CONFLICT"));
            assert_eq!(std::fs::read(stage.join("foreign")).unwrap(), b"do not remove");
            Storage::export_archive(&target, &root.join("after")).await.unwrap();
            assert_archive_equal(&root.join("before"), &root.join("after"));
            std::fs::remove_file(stage.join("foreign")).unwrap();
            std::fs::remove_dir(&stage).unwrap();
            std::fs::rename(&parked, &stage).unwrap();
            Storage::import_archive(&target, &root.join("archive")).await.unwrap();
            Storage::export_archive(&target, &root.join("recovered")).await.unwrap();
            assert_archive_equal(&root.join("archive"), &root.join("recovered"));
        });
        println!("incremental foreign_stage_after={phase} preserved=true recovered=true");
    }
}

#[test]
fn admission_survives_renamed_lock_and_rejects_changed_directory() {
    let root = std::env::temp_dir().join(format!("akasha-admission-owner-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let _fixture = OwnedFixture(root.clone());
    let data = root.join("data");
    let admission = layout::Admission::acquire(&data, true).unwrap();
    std::fs::rename(data.join(".archive-import.lock"), data.join("parked-lock")).unwrap();
    assert!(matches!(layout::Admission::acquire(&data, false), Err(e) if e.code == "IMPORT_BUSY"));
    assert!(admission.validate().is_err());
    std::fs::rename(data.join("parked-lock"), data.join(".archive-import.lock")).unwrap();
    admission.validate().unwrap();
    std::fs::rename(&data, root.join("parked-data")).unwrap();
    layout::private_dir(&data, false).unwrap();
    std::fs::write(data.join("foreign"), b"keep").unwrap();
    assert!(admission.sync().is_err());
    assert_eq!(std::fs::read(data.join("foreign")).unwrap(), b"keep");
}

#[test]
fn incremental_recovery_checks_native_identity_before_deleting_old_history() {
    let root = std::env::temp_dir().join(format!(
        "akasha-incremental-identity-{}",
        std::process::id()
    ));
    std::fs::create_dir(&root).unwrap();
    let _fixture = OwnedFixture(root.clone());
    runtime().block_on(prepare(&root));
    kill_at(
        &root,
        "incremental-exchanged",
        "storage::crash_tests::import_crash_child",
    );
    runtime().block_on(async {
        let target = config(&root);
        let store = Storage::open(&target).await.unwrap();
        let mut tx = store.tree.begin().unwrap();
        let original = tx.get(b"meta/archive_import").unwrap().unwrap();
        tx.set(
            b"meta/archive_import",
            br#"{"archive_sha256":"invalid","commit_seq":3}"#,
        )
        .unwrap();
        tx.commit().await.unwrap();
        store.close().await.unwrap();
        assert!(
            Storage::import_archive(&target, &root.join("archive"))
                .await
                .is_err()
        );
        assert!(
            root.join("data/.archive-import/kv").is_dir(),
            "deleted old history before checking the replacement database"
        );
        assert!(root.join("data/.archive-replace.json").is_file());
        assert!(matches!(Storage::open(&target).await, Err(e) if e.code == "INVALID_ARCHIVE"));
        // Repair injected metadata through the fixture's native handle; public opens must refuse it.
        let admission = layout::Admission::acquire(&root.join("data"), false).unwrap();
        let authority = super::super::import_publish::Root::open(&admission, "authority").unwrap();
        let tree = Storage::open_tree(&target, &authority).unwrap();
        let mut tx = tree.begin().unwrap();
        tx.set(b"meta/archive_import", &original).unwrap();
        tx.commit().await.unwrap();
        tree.close().await.unwrap();
        drop(tree);
        drop(authority);
        drop(admission);
        Storage::import_archive(&target, &root.join("archive"))
            .await
            .unwrap();
        Storage::export_archive(&target, &root.join("recovered"))
            .await
            .unwrap();
        assert_archive_equal(&root.join("archive"), &root.join("recovered"));
    });
}

#[test]
fn incremental_recovery_rejects_untrusted_marker_without_changing_either_store() {
    use std::os::unix::fs::PermissionsExt;
    let root =
        std::env::temp_dir().join(format!("akasha-incremental-marker-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    let _fixture = OwnedFixture(root.clone());
    runtime().block_on(prepare(&root));
    kill_at(
        &root,
        "incremental-marker",
        "storage::crash_tests::import_crash_child",
    );
    let marker = root.join("data/.archive-replace.json");
    let original = std::fs::read(&marker).unwrap();
    let value: serde_json::Value = serde_json::from_slice(&original).unwrap();
    let mut unknown = value.clone();
    unknown["unexpected"] = true.into();
    let mut backwards = value.clone();
    backwards["new"]["identity"]["commit_seq"] = 0.into();
    let mut foreign = value;
    foreign["old"]["directory"]["inode"] = 0.into();
    runtime().block_on(async {
        let target = config(&root);
        Storage::export_archive(&target, &root.join("before"))
            .await
            .unwrap();
        for (number, bytes) in [
            serde_json::to_vec(&unknown).unwrap(),
            serde_json::to_vec(&backwards).unwrap(),
            serde_json::to_vec(&foreign).unwrap(),
            vec![b' '; 2049],
        ]
        .into_iter()
        .enumerate()
        {
            std::fs::write(&marker, &bytes).unwrap();
            assert!(
                Storage::import_archive(&target, &root.join("archive"))
                    .await
                    .is_err()
            );
            assert_eq!(std::fs::read(&marker).unwrap(), bytes);
            let after = root.join(format!("after-{number}"));
            assert!(Storage::export_archive(&target, &after).await.is_err());
            assert!(!after.exists());
            assert_eq!(std::fs::read(&marker).unwrap(), bytes);
            std::fs::write(&marker, &original).unwrap();
            Storage::export_archive(&target, &after).await.unwrap();
            assert_archive_equal(&root.join("before"), &after);
            assert!(root.join("data/.archive-import/kv").is_dir());
        }
        std::fs::write(&marker, &original).unwrap();
        std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o644)).unwrap();
        assert!(
            Storage::import_archive(&target, &root.join("archive"))
                .await
                .is_err()
        );
        assert!(Storage::open(&target).await.is_err());
        std::fs::set_permissions(&marker, std::fs::Permissions::from_mode(0o600)).unwrap();
        let saved = root.join("saved-marker");
        std::fs::rename(&marker, &saved).unwrap();
        std::os::unix::fs::symlink(&saved, &marker).unwrap();
        assert!(
            Storage::import_archive(&target, &root.join("archive"))
                .await
                .is_err()
        );
        assert!(Storage::open(&target).await.is_err());
        assert_eq!(std::fs::read(&saved).unwrap(), original);
        std::fs::remove_file(&marker).unwrap();
        std::fs::rename(&saved, &marker).unwrap();
        Storage::import_archive(&target, &root.join("archive"))
            .await
            .unwrap();
        Storage::export_archive(&target, &root.join("recovered"))
            .await
            .unwrap();
        assert_archive_equal(&root.join("archive"), &root.join("recovered"));
    });
}
