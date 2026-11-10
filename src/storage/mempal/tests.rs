use std::path::PathBuf;

use super::*;

struct Fixture(PathBuf);

impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
#[ignore = "launched only by the mempal conversion crash supervisor"]
fn mempal_crash_child() {
    let root = PathBuf::from(std::env::var_os("AKASHA_TEST_CRASH_ROOT").unwrap());
    migrate_mempal(&root.join("source.db"), &root.join("archive")).unwrap();
    panic!("requested mempal conversion checkpoint was not reached");
}

#[test]
fn killed_mempal_conversion_reconciles_output_without_duplicate_candidates() {
    for phase in [
        "mempal-identity",
        "mempal-partial",
        "mempal-before-publish",
        "mempal-manifest-renamed",
        "mempal-published",
    ] {
        let root = std::env::temp_dir().join(format!(
            "akasha-mempal-crash-{}-{phase}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let _fixture = Fixture(root.clone());
        let source = root.join("source.db");
        let db = rusqlite::Connection::open(&source).unwrap();
        db.execute_batch(include_str!("../../../tests/fixtures/mempal21-28.sql"))
            .unwrap();
        let original = "original\0原文".repeat(100);
        for index in 0..100 {
            db.execute("INSERT INTO drawers(id,content,wing,added_at,project_id) VALUES(?1,?2,'test','2026-01-01 00:00:00','project')", rusqlite::params![format!("row-{index}"),original]).unwrap();
        }
        db.close().unwrap();
        let source_hash = crate::storage::hash(&std::fs::read(&source).unwrap());
        crate::storage::crash_tests::kill_at(
            &root,
            phase,
            "storage::mempal::tests::mempal_crash_child",
        );
        let archive = root.join("archive");
        assert_eq!(
            archive.join("manifest.json").exists(),
            matches!(phase, "mempal-manifest-renamed" | "mempal-published")
        );
        if phase == "mempal-partial" {
            assert!(std::fs::metadata(archive.join("blobs.bin")).unwrap().len() > 0);
        }
        let first = migrate_mempal(&source, &archive).unwrap();
        let again = migrate_mempal(&source, &archive).unwrap();
        assert_eq!(
            serde_json::to_value(&first).unwrap(),
            serde_json::to_value(&again).unwrap()
        );
        assert_eq!(first.migration.tables["drawers"].rows, 100);
        assert_eq!(first.migration.projection.candidate_memories, 100);
        assert_eq!(first.migration.projection.source_chunks, 100);
        let config = crate::config::resolve(
            "",
            None,
            "",
            &crate::config::Paths {
                data_dir: root.join("data"),
                socket_path: root.join("daemon.sock"),
            },
        )
        .unwrap();
        tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .unwrap()
            .block_on(async {
                crate::storage::Storage::import_archive(&config, &archive)
                    .await
                    .unwrap();
                let store = crate::storage::Storage::open(&config).await.unwrap();
                let domain = crate::domain::Domain::new(&store);
                for rowid in 1..=100 {
                    let id = format!("mempal/{source_hash}/drawers/{rowid}/0");
                    let candidate = domain.memory(&id).unwrap().unwrap();
                    assert_eq!(candidate.record.draft.claim, original);
                    assert!(domain.active(&id).unwrap().is_none());
                    let span = &candidate.record.draft.source_spans[0];
                    assert_eq!(
                        domain
                            .source(&span.event_id, span.revision)
                            .unwrap()
                            .payload
                            .text,
                        original
                    );
                }
                store.close().await.unwrap();
            });
        assert_eq!(
            crate::storage::hash(&std::fs::read(&source).unwrap()),
            source_hash
        );
        println!(
            "mempal checkpoint={phase} signal=9 recovered_candidates=100 duplicate_candidates=0 source_unchanged=true"
        );
    }
}

#[test]
fn moved_snapshot_inode_cannot_be_adopted_by_conversion_output() {
    use std::os::unix::fs::PermissionsExt;
    for name in [
        "conversion.json.tmp",
        "manifest.json.tmp",
        "blobs.bin",
        "commits.jsonl",
        ".mempal-convert.lock",
    ] {
        let root =
            std::env::temp_dir().join(format!("akasha-mempal-moved-{}-{name}", std::process::id()));
        std::fs::create_dir(&root).unwrap();
        let _fixture = Fixture(root.clone());
        let source = root.join("source.db");
        let db = rusqlite::Connection::open(&source).unwrap();
        db.execute_batch(include_str!("../../../tests/fixtures/mempal21-28.sql"))
            .unwrap();
        db.close().unwrap();
        std::fs::set_permissions(&source, std::fs::Permissions::from_mode(0o600)).unwrap();
        let before = std::fs::read(&source).unwrap();
        let snapshot = Snapshot::open(&source).unwrap();
        let destination = root.join("archive");
        std::fs::create_dir(&destination).unwrap();
        std::fs::set_permissions(&destination, std::fs::Permissions::from_mode(0o700)).unwrap();
        if name != "conversion.json.tmp" {
            drop(Writer::new(&destination, &snapshot, 1).unwrap());
        }
        let moved = destination.join(name);
        // Deterministically move the pinned source after reader admission.
        std::fs::rename(&source, &moved).unwrap();
        let result = Writer::new(&destination, &snapshot, 1).and_then(Writer::finish);
        assert_eq!(
            result.err().map(|error| error.code),
            Some("UNSAFE_MEMPAL_SNAPSHOT"),
            "{name}"
        );
        assert_eq!(std::fs::read(&moved).unwrap(), before, "{name}");
    }
}
