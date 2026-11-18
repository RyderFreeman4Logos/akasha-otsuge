use super::*;
use crate::config::{Paths, ReviewMode, resolve};
use crate::domain::*;
use std::path::PathBuf;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "akasha-index-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
        ));
        crate::daemon::private_dir(&root).unwrap();
        Self(root)
    }
    fn config(&self) -> ResolvedConfig {
        resolve(
            "",
            None,
            "",
            &Paths {
                data_dir: self.0.join("data"),
                socket_path: self.0.join("run/socket"),
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
fn scope() -> Scope {
    Scope {
        user: "fixture".into(),
        project: Some("alpha".into()),
        branch_or_version: None,
        session: None,
    }
}
async fn memory(store: &Storage, id: &str, claim: &str, previous: Option<u64>) -> u64 {
    let domain = Domain::new(store);
    let ingress = Ingress::tool("fixture", "fixture", scope()).unwrap();
    let source_id = format!("{id}/{}", hash(claim.as_bytes()));
    let receipt = domain
        .record_source(
            &format!("source/{source_id}"),
            &SourceDraft {
                event_id: source_id.clone(),
                scope: scope(),
                text: claim.into(),
                reported_role: None,
                event_time_ms: None,
                observed_time_ms: 1,
            },
            &ingress,
            1,
        )
        .await
        .unwrap();
    let source = domain.source(&source_id, receipt.commit_seq).unwrap();
    let draft = MemoryDraft {
        kind: MemoryKind::Fact,
        scope: scope(),
        claim: claim.into(),
        rationale: "fixture".into(),
        conditions: vec![],
        assumptions: vec![],
        source_spans: vec![SourceSpan {
            event_id: source_id.clone(),
            revision: receipt.commit_seq,
            content_sha256: source.content_sha256,
            start: 0,
            end: claim.len(),
        }],
        valid_from_ms: None,
        valid_until_ms: None,
    };
    domain
        .propose(
            &MemoryWrite {
                request_id: format!("write/{source_id}"),
                memory_id: id.into(),
                expected_revision: previous,
                draft,
            },
            &ingress,
            &Policy {
                revision: "fixture".into(),
                config_generation: 1,
                mode: ReviewMode::Automatic,
                scope: scope(),
                adopt_explicit_decisions: false,
            },
        )
        .await
        .unwrap()
        .commit_seq
}
async fn sync(index: &Index, store: &Storage, target: u64) -> Status {
    loop {
        let status = index
            .sync_batch(store, target, 1, &|| Ok(()))
            .await
            .unwrap();
        if status.lexical_watermark >= target {
            return status;
        }
    }
}

#[tokio::test]
async fn lexical_sync_rechecks_each_read_and_progress_publication() {
    for deny_at in [3, 4] {
        let fixture = Fixture::new();
        let config = fixture.config();
        let store = Storage::open(&config).await.unwrap();
        memory(&store, "first", "first keyword", None).await;
        let target = memory(&store, "second", "second keyword", None).await;
        let index = Index::open(&config, &store).await.unwrap();
        let checks = std::sync::atomic::AtomicUsize::new(0);
        let authorize = || {
            if checks.fetch_add(1, std::sync::atomic::Ordering::SeqCst) + 1 >= deny_at {
                Err(error("READ_DENIED"))
            } else {
                Ok(())
            }
        };
        let result = index.sync_batch(&store, target, 1, &authorize).await;
        assert!(matches!(result, Err(e) if e.code == "READ_DENIED"));
        assert_eq!(status(&store).unwrap().lexical_watermark, 0);
        let progress = sync(&index, &store, target).await;
        assert_eq!(progress.indexed_rows, 2);
        drop(index);
        store.close().await.unwrap();
    }
}

#[tokio::test]
async fn one_authority_batch_bounds_native_versions_and_keeps_every_lexical_row() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = Storage::open(&config).await.unwrap();
    let mut expected = std::collections::BTreeSet::new();
    for i in 0..16 {
        let id = format!("batch-{i}");
        let seq = memory(&store, &id, "nativebatch searchable body", None).await;
        expected.insert((id, seq));
    }
    let target = store.sequence().unwrap();
    assert_eq!(target, 32);
    let index = Index::open(&config, &store).await.unwrap();
    let before = index.table.version().await.unwrap();
    let progress = index
        .sync_batch(&store, target, 1, &|| Ok(()))
        .await
        .unwrap();
    assert_eq!(progress.lexical_watermark, target);
    assert_eq!(progress.indexed_rows, expected.len());
    assert_eq!(
        index
            .keyword("nativebatch", &scope(), 100)
            .await
            .unwrap()
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        expected
    );
    let versions = index.table.version().await.unwrap() - before;
    // Includes native-index initialization and the first optimization.
    assert!(
        versions <= 8,
        "one bounded authority batch created {versions} native versions"
    );
    drop(index);
    store.close().await.unwrap();
}

#[tokio::test]
async fn native_batches_keep_last_operations_and_flush_bounded_buffers() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = Storage::open(&config).await.unwrap();
    let index = Index::open(&config, &store).await.unwrap();
    let row = |id: &str, body: &str| {
        RecordBatch::try_new(
            index.schema.clone(),
            vec![
                Arc::new(StringArray::from(vec![hash(id.as_bytes())])),
                Arc::new(StringArray::from(vec![id])),
                Arc::new(UInt64Array::from(vec![1u64])),
                Arc::new(StringArray::from(vec!["scope"])),
                Arc::new(StringArray::from(vec![body])),
            ],
        )
        .unwrap()
    };
    let first = hash(b"first");
    let second = hash(b"second");
    let mut pending = NativeBatch::new(512);
    pending
        .push(&index.table, first.clone(), Some(row("first", "old")))
        .await
        .unwrap();
    pending.push(&index.table, first, None).await.unwrap();
    pending
        .push(&index.table, second.clone(), None)
        .await
        .unwrap();
    pending
        .push(&index.table, second.clone(), Some(row("second", "latest")))
        .await
        .unwrap();
    pending.flush(&index.table).await.unwrap();
    let stored: Vec<RecordBatch> = index
        .table
        .query()
        .execute()
        .await
        .unwrap()
        .try_collect()
        .await
        .unwrap();
    assert_eq!(stored.iter().map(RecordBatch::num_rows).sum::<usize>(), 1);
    let body = stored[0]
        .column_by_name("body")
        .unwrap()
        .as_any()
        .downcast_ref::<StringArray>()
        .unwrap();
    assert_eq!(body.value(0), "latest");
    pending.push(&index.table, second, None).await.unwrap();
    pending.flush(&index.table).await.unwrap();
    assert_eq!(index.table.count_rows(None).await.unwrap(), 0);

    let mut pending = NativeBatch::new(4);
    let large = "x".repeat(96 * 1024);
    pending
        .push(&index.table, hash(b"large-1"), Some(row("large-1", &large)))
        .await
        .unwrap();
    assert_eq!(index.table.count_rows(None).await.unwrap(), 0);
    pending
        .push(&index.table, hash(b"large-2"), Some(row("large-2", &large)))
        .await
        .unwrap();
    assert_eq!(index.table.count_rows(None).await.unwrap(), 2);
    index.table.delete("true").await.unwrap();

    let mut pending = NativeBatch::new(512);
    for i in 0..255 {
        let id = format!("tiny-{i}");
        pending
            .push(&index.table, hash(id.as_bytes()), Some(row(&id, "tiny")))
            .await
            .unwrap();
    }
    assert_eq!(index.table.count_rows(None).await.unwrap(), 0);
    pending
        .push(
            &index.table,
            hash(b"tiny-255"),
            Some(row("tiny-255", "tiny")),
        )
        .await
        .unwrap();
    assert_eq!(index.table.count_rows(None).await.unwrap(), 256);
    pending.flush(&index.table).await.unwrap();
    assert_eq!(index.table.count_rows(None).await.unwrap(), 256);
    drop(index);
    store.close().await.unwrap();
}

#[tokio::test]
async fn contiguous_prefix_records_superseded_and_not_required_completions() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = Storage::open(&config).await.unwrap();
    let old = memory(&store, "setting", "old setting", None).await;
    let new = memory(&store, "setting", "new setting", Some(old)).await;
    let index = Index::open(&config, &store).await.unwrap();
    assert_eq!(sync(&index, &store, new).await.lexical_watermark, new);
    for (seq, reason) in [(1, "not_required"), (old, "superseded"), (new, "indexed")] {
        let key = RecordKey {
            kind: RecordKind::State,
            id: format!("search/completion/{GENERATION}/{seq:020}"),
        };
        let record = store
            .current(&key)
            .unwrap()
            .expect("watermark requires durable completion reasons for every covered sequence");
        let value: serde_json::Value = serde_json::from_str(&record.content).unwrap();
        assert_eq!(value["seq"], seq);
        if reason == "not_required" {
            assert_eq!(value["reason"], reason);
        } else {
            assert_eq!(value["records"][0]["outcome"], reason);
        }
    }
    assert_eq!(
        index.keyword("new", &scope(), 20).await.unwrap(),
        vec![("setting".into(), new)]
    );
    assert!(index.keyword("old", &scope(), 20).await.unwrap().is_empty());
    drop(index);
    store.close().await.unwrap();
    let store = Storage::open(&config).await.unwrap();
    let index = Index::open(&config, &store).await.unwrap();
    assert_eq!(status(&store).unwrap().lexical_watermark, new);
    assert_eq!(
        index.keyword("new", &scope(), 20).await.unwrap(),
        vec![("setting".into(), new)]
    );
    drop(index);
    store.close().await.unwrap();
}

#[tokio::test]
async fn appended_index_retains_mixed_language_symbols_and_versions_after_reopen() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = Storage::open(&config).await.unwrap();
    memory(
        &store,
        "first",
        "离线 offline query load_config v2.3.1",
        None,
    )
    .await;
    let index = Index::open(&config, &store).await.unwrap();
    sync(&index, &store, store.sequence().unwrap()).await;
    memory(
        &store,
        "second",
        "在线 online publish send_message v3.4.2",
        None,
    )
    .await;
    sync(&index, &store, store.sequence().unwrap()).await;
    drop(index);
    store.close().await.unwrap();
    let store = Storage::open(&config).await.unwrap();
    let index = Index::open(&config, &store).await.unwrap();
    for (query, expected) in [
        ("离线", "first"),
        ("offline", "first"),
        ("load_config", "first"),
        ("v2.3.1", "first"),
        ("在线", "second"),
        ("send_message", "second"),
        ("v3.4.2", "second"),
    ] {
        let found = index.keyword(query, &scope(), 20).await.unwrap();
        assert!(
            found.first().is_some_and(|(id, _)| id == expected),
            "{query}: {found:?}"
        );
    }
    drop(index);
    store.close().await.unwrap();
}

#[tokio::test]
#[ignore = "launched only by the crash supervisor with an isolated database"]
async fn crash_child() {
    let root = PathBuf::from(std::env::var_os("AKASHA_TEST_CRASH_ROOT").unwrap());
    let fixture = std::mem::ManuallyDrop::new(Fixture(root));
    let config = fixture.config();
    let store = Storage::open(&config).await.unwrap();
    let index = Index::open(&config, &store).await.unwrap();
    sync(&index, &store, store.sequence().unwrap()).await;
    panic!("index crash checkpoint not reached");
}

#[tokio::test]
async fn killed_index_updates_replay_without_duplicates_or_prefix_holes() {
    for phase in ["index-before-progress", "index-after-progress"] {
        let fixture = Fixture::new();
        let config = fixture.config();
        let store = Storage::open(&config).await.unwrap();
        let target = memory(&store, "durable", "durable searchable text", None).await;
        store.close().await.unwrap();
        crate::storage::crash_tests::kill_at(&fixture.0, phase, "retrieval::tests::crash_child");
        let store = Storage::open(&config).await.unwrap();
        let index = Index::open(&config, &store).await.unwrap();
        let published = phase == "index-after-progress";
        assert_eq!(
            status(&store).unwrap().lexical_watermark,
            if published { target } else { 0 }
        );
        assert_eq!(index.table.count_rows(None).await.unwrap(), 1);
        let version = index.table.version().await.unwrap();
        let seq = store.sequence().unwrap();
        let progress = sync(&index, &store, target).await;
        assert_eq!(progress.lexical_watermark, target);
        assert_eq!(progress.indexed_rows, 1);
        assert_eq!(
            index.keyword("durable", &scope(), 20).await.unwrap(),
            vec![("durable".into(), target)]
        );
        for seq in 1..=target {
            assert!(
                store
                    .current(&RecordKey {
                        kind: RecordKind::State,
                        id: format!("search/completion/{GENERATION}/{seq:020}"),
                    })
                    .unwrap()
                    .is_some()
            );
        }
        if published {
            assert_eq!(store.sequence().unwrap(), seq);
            assert_eq!(index.table.version().await.unwrap(), version);
        }
        drop(index);
        store.close().await.unwrap();
    }
}

#[tokio::test]
async fn missing_native_index_does_not_reuse_a_completed_watermark() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = Storage::open(&config).await.unwrap();
    let target = memory(&store, "durable", "durable searchable text", None).await;
    let index = Index::open(&config, &store).await.unwrap();
    sync(&index, &store, target).await;
    index.connection.drop_table(TABLE, &[]).await.unwrap();
    drop(index);
    assert!(matches!(Index::open(&config, &store).await, Err(e) if e.code == "INDEX_MISSING"));
    store.close().await.unwrap();
}

#[tokio::test]
#[ignore = "launched only by the crash supervisor with an isolated database"]
async fn rebuild_activation_child() {
    let root = PathBuf::from(std::env::var_os("AKASHA_TEST_CRASH_ROOT").unwrap());
    let fixture = std::mem::ManuallyDrop::new(Fixture(root));
    let store = Storage::open(&fixture.config()).await.unwrap();
    let pending = pending_rebuild(&store).unwrap().unwrap();
    Index::publish_rebuild(&store, &pending, 1, pending.lexical_watermark, &|| Ok(()))
        .await
        .unwrap();
    panic!("rebuild activation checkpoint not reached");
}

#[tokio::test]
async fn interrupted_rebuild_preserves_complete_generation_on_each_side_of_publication() {
    for phase in ["rebuild-before-activation", "rebuild-after-activation"] {
        let fixture = Fixture::new();
        let config = fixture.config();
        let store = Storage::open(&config).await.unwrap();
        let orig = memory(&store, "original", "marmalade original", None).await;
        let active = Index::open(&config, &store).await.unwrap();
        sync(&active, &store, orig).await;
        let added = memory(&store, "later", "watercress later", None).await;
        let pending = Index::start_rebuild(&store, "replacement", 1, &|| Ok(()))
            .await
            .unwrap();
        assert_eq!(status(&store).unwrap().generation, GENERATION);
        assert_eq!(
            Index::remaining_sync(&store, &config, added, false).unwrap(),
            added
        );
        assert_eq!(
            Index::publish_rebuild(&store, &pending, 1, added, &|| Ok(()))
                .await
                .unwrap_err()
                .code,
            "INDEX_NOT_READY"
        );
        let candidate = Index::open_generation(&config, pending, true)
            .await
            .unwrap();
        let ready = sync(&candidate, &store, added).await;
        assert_eq!(
            Index::remaining_sync(&store, &config, added, true).unwrap(),
            added
        );
        assert_eq!(
            Index::remaining_sync(&store, &config, added, false).unwrap(),
            0
        );
        assert_eq!(
            active.keyword("marmalade", &scope(), 10).await.unwrap(),
            [("original".into(), orig)]
        );
        assert!(
            active
                .keyword("watercress", &scope(), 10)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            candidate.keyword("watercress", &scope(), 10).await.unwrap(),
            [("later".into(), added)]
        );
        drop((active, candidate));
        store.close().await.unwrap();
        crate::storage::crash_tests::kill_at(
            &fixture.0,
            phase,
            "retrieval::tests::rebuild_activation_child",
        );
        let store = Storage::open(&config).await.unwrap();
        let published = phase == "rebuild-after-activation";
        let active = Index::open(&config, &store).await.unwrap();
        assert_eq!(active.generation == ready.generation, published);
        assert_eq!(
            active.keyword("marmalade", &scope(), 10).await.unwrap(),
            [("original".into(), orig)]
        );
        assert_eq!(
            active
                .keyword("watercress", &scope(), 10)
                .await
                .unwrap()
                .len(),
            usize::from(published)
        );
        assert_eq!(
            active
                .connection
                .table_names()
                .execute()
                .await
                .unwrap()
                .len(),
            2
        );
        drop(active);
        if !published {
            let resumed = pending_rebuild(&store).unwrap().unwrap();
            assert_eq!(resumed.lexical_watermark, added);
            Index::publish_rebuild(&store, &resumed, 1, added, &|| Ok(()))
                .await
                .unwrap();
        }
        let active = Index::open(&config, &store).await.unwrap();
        active
            .retire_previous_tables(None, &|| Ok(()))
            .await
            .unwrap();
        Index::finish_retirement(&store, 1, &|| Ok(()))
            .await
            .unwrap();
        assert!(pending_rebuild(&store).unwrap().is_none());
        assert_eq!(
            active.connection.table_names().execute().await.unwrap(),
            [table_name(&ready.generation)]
        );
        assert_eq!(
            active.keyword("watercress", &scope(), 10).await.unwrap(),
            [("later".into(), added)]
        );
        assert_eq!(
            Domain::new(&store)
                .memory("original")
                .unwrap()
                .unwrap()
                .revision
                .commit_seq,
            orig
        );
        drop(active);
        store.close().await.unwrap();
    }
}

#[tokio::test]
async fn damaged_candidate_cleanup_resumes_without_touching_the_published_table() {
    for damaged in ["missing lexical", "corrupt lexical", "corrupt vector"] {
        let fixture = Fixture::new();
        let paths = Paths {
            data_dir: fixture.0.join("data"),
            socket_path: fixture.0.join("socket"),
        };
        let config = resolve("[retrieval]\nembedding='e'\n[models.e]\nkind='embedding'\nmodel='fixture'\nendpoint='http://127.0.0.1:1/v1'\ndimension=2\nlocal=true", None, "", &paths).unwrap();
        let snapshot = crate::config::ConfigManager::new(1, config.clone(), |_| Ok(()))
            .unwrap()
            .pin();
        let store = Storage::open(&config).await.unwrap();
        let at = memory(&store, "original", "marmalade published", None).await;
        let active = Index::open(&config, &store).await.unwrap();
        sync(&active, &store, at).await;
        let pending = Index::start_rebuild(&store, "first", 1, &|| Ok(()))
            .await
            .unwrap();
        let old = pending.generation.clone();
        let candidate =
            Index::open_candidate(&store, &config, &snapshot, pending, "first", true, &|| {
                Ok(())
            })
            .await
            .unwrap();
        sync(&candidate, &store, at).await;
        drop(candidate);
        let pending = Index::start_rebuild(&store, "another-key", 1, &|| Ok(()))
            .await
            .unwrap();
        assert_eq!(pending.generation, old);
        assert_eq!(pending.lexical_watermark, at);
        let name = if damaged == "corrupt vector" {
            format!(
                "memory_vector_{}",
                pending.candidate_vector.as_ref().unwrap()
            )
        } else {
            table_name(&old)
        };
        active.connection.drop_table(&name, &[]).await.unwrap();
        if damaged != "missing lexical" {
            let schema = Arc::new(Schema::new(vec![Field::new(
                "wrong",
                DataType::UInt64,
                false,
            )]));
            let batches =
                RecordBatchIterator::new(vec![Ok(RecordBatch::new_empty(schema.clone()))], schema);
            active
                .connection
                .create_table(
                    &name,
                    Box::new(batches) as Box<dyn arrow_array::RecordBatchReader + Send>,
                )
                .execute()
                .await
                .unwrap();
        }
        let err = Index::open_candidate(
            &store,
            &config,
            &snapshot,
            pending.clone(),
            "sync",
            false,
            &|| Ok(()),
        )
        .await
        .err()
        .unwrap();
        assert_eq!(
            err.code,
            if damaged == "missing lexical" {
                "INDEX_MISSING"
            } else {
                "INDEX_CORRUPT"
            }
        );
        // Interrupt after the first candidate table is dropped, before replacement publication.
        let drops = std::sync::atomic::AtomicUsize::new(0);
        let interrupted = || {
            if pending_rebuild(&store)?.is_some_and(|p| p.discarding)
                && drops.fetch_add(1, std::sync::atomic::Ordering::Relaxed) >= 1
            {
                return Err(error("CANCELLED"));
            }
            Ok(())
        };
        let err = Index::open_candidate(
            &store,
            &config,
            &snapshot,
            pending,
            "repair",
            true,
            &interrupted,
        )
        .await
        .err()
        .unwrap();
        assert_eq!(err.code, "CANCELLED");
        assert_eq!(
            active.keyword("marmalade", &scope(), 20).await.unwrap(),
            [("original".into(), at)]
        );
        drop(active);
        store.close().await.unwrap();
        let store = Storage::open(&config).await.unwrap();
        let pending = pending_rebuild(&store).unwrap().unwrap();
        assert!(pending.discarding);
        let replacement = Index::open_candidate(
            &store,
            &config,
            &snapshot,
            pending,
            "reopen",
            false,
            &|| Ok(()),
        )
        .await
        .unwrap();
        assert_ne!(replacement.generation, old);
        let new_vector = vector::status(&store, &config, &replacement.generation)
            .unwrap()
            .unwrap();
        let names: std::collections::BTreeSet<_> = replacement
            .connection
            .table_names()
            .execute()
            .await
            .unwrap()
            .into_iter()
            .collect();
        assert_eq!(
            names,
            [
                TABLE.to_owned(),
                table_name(&replacement.generation),
                format!("memory_vector_{}", new_vector.generation)
            ]
            .into_iter()
            .collect()
        );
        assert_eq!(status(&store).unwrap().generation, GENERATION);
        drop(replacement);
        store.close().await.unwrap();
    }
}

#[tokio::test]
async fn candidate_manifest_detects_missing_data_and_truncated_index_files() {
    use lance::index::DatasetIndexExt;
    for damage_index in [false, true] {
        let fixture = Fixture::new();
        let config = fixture.config();
        let store = Storage::open(&config).await.unwrap();
        let at = memory(&store, "first", "purple orchard", None).await;
        let pending = Index::start_rebuild(&store, "candidate", 1, &|| Ok(()))
            .await
            .unwrap();
        let table = table_name(&pending.generation);
        let index = Index::open_generation(&config, pending, true)
            .await
            .unwrap();
        sync(&index, &store, at).await;
        index.check_candidate_files(&|| Ok(())).await.unwrap();
        let root = config
            .config()
            .paths
            .data_dir
            .join("search")
            .join(format!("{table}.lance"));
        let dataset = index.table.dataset().unwrap().get().await.unwrap();
        if damage_index {
            let indices = dataset.load_indices().await.unwrap();
            let segment = &indices[0];
            let file = &segment.files.as_ref().unwrap()[0];
            assert!(file.size_bytes > 1);
            std::fs::OpenOptions::new()
                .write(true)
                .open(
                    root.join("_indices")
                        .join(segment.uuid.to_string())
                        .join(&file.path),
                )
                .unwrap()
                .set_len(1)
                .unwrap();
        } else {
            let file = &dataset.iter_fragments().next().unwrap().files[0];
            std::fs::remove_file(root.join("data").join(&file.path)).unwrap();
        }
        assert!(index.check_candidate_files(&|| Ok(())).await.is_err());
        assert_eq!(status(&store).unwrap().generation, GENERATION);
        drop(index);
        store.close().await.unwrap();
    }
}
