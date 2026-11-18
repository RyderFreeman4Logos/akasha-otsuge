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
    // Includes native-index initialization.
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
    index.owner.connection.drop_table(TABLE, &[]).await.unwrap();
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
        let candidate = Index::open_generation(
            Arc::new(IndexOwner::open(&config).await.unwrap()),
            pending,
            true,
        )
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
                .owner
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
        assert_eq!(
            Index::start_rebuild(&store, "fresh-key-before-retirement", 1, &|| Ok(()))
                .await
                .unwrap_err()
                .code,
            "INDEX_NOT_READY"
        );
        active
            .owner
            .finish_retirement(&store, 1, &|| Ok(()))
            .await
            .unwrap();
        assert!(pending_rebuild(&store).unwrap().is_none());
        assert_eq!(
            active
                .owner
                .connection
                .table_names()
                .execute()
                .await
                .unwrap(),
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
        let candidate = Index::open_candidate(
            &store,
            &active.owner,
            &snapshot,
            pending,
            "first",
            true,
            &|| Ok(()),
        )
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
        active
            .owner
            .connection
            .drop_table(&name, &[])
            .await
            .unwrap();
        if damaged != "missing lexical" {
            let schema = Arc::new(Schema::new(vec![Field::new(
                "wrong",
                DataType::UInt64,
                false,
            )]));
            let batches =
                RecordBatchIterator::new(vec![Ok(RecordBatch::new_empty(schema.clone()))], schema);
            active
                .owner
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
            &active.owner,
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
            &active.owner,
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
            &Arc::new(IndexOwner::open(&config).await.unwrap()),
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
            .owner
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
        let index = Index::open_generation(
            Arc::new(IndexOwner::open(&config).await.unwrap()),
            pending,
            true,
        )
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

#[tokio::test]
async fn published_and_candidate_share_one_native_session_across_budget_changes() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = Storage::open(&config).await.unwrap();
    let mut cache = IndexCache::default();
    let published = Arc::new(
        Index::open_generation(
            cache.owner(&config).await.unwrap(),
            status(&store).unwrap(),
            false,
        )
        .await
        .unwrap(),
    );
    cache.published = Some(published.clone());
    let pending = Index::start_rebuild(&store, "candidate", 1, &|| Ok(()))
        .await
        .unwrap();
    let candidate = Index::open_generation(cache.owner(&config).await.unwrap(), pending, true)
        .await
        .unwrap();
    assert!(Arc::ptr_eq(&published.owner, &candidate.owner));
    let session =
        |table: Table| async move { table.dataset().unwrap().get().await.unwrap().session() };
    assert!(Arc::ptr_eq(
        &session(published.table.clone()).await,
        &session(candidate.table.clone()).await
    ));
    let new_budget = if published.budget_mib == 512 {
        1024
    } else {
        512
    };
    let changed = resolve(
        &format!("[runtime]\nmemory_budget_mib={new_budget}"),
        None,
        "",
        &config.config().paths,
    )
    .unwrap();
    assert_eq!(
        cache.owner(&changed).await.err().unwrap().code,
        "RESOURCE_BUSY"
    );
    drop(published);
    assert_eq!(
        cache.owner(&changed).await.err().unwrap().code,
        "RESOURCE_BUSY"
    );
    assert_eq!(cache.open_budget_mib(), Some(candidate.budget_mib));
    drop(candidate);
    let owner = cache.owner(&changed).await.unwrap();
    assert_eq!(owner.budget_mib, new_budget);
    store.close().await.unwrap();
}

#[tokio::test]
async fn maintenance_keeps_unindexed_reads_and_authority_progress() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = Storage::open(&config).await.unwrap();
    let mut last = None;
    for i in 0..128 {
        let id = format!("maintenance-{i}");
        let seq = memory(&store, &id, &format!("windowtoken{i}"), None).await;
        last = Some((id, seq));
    }
    let target = store.sequence().unwrap();
    let index = Index::open(&config, &store).await.unwrap();
    let progress = sync(&index, &store, target).await;
    let authority_seq = store.sequence().unwrap();
    let expected = vec![last.unwrap()];
    assert_eq!(
        index.keyword("windowtoken127", &scope(), 20).await.unwrap(),
        expected
    );
    let fragments = index
        .table
        .as_native()
        .unwrap()
        .count_fragments()
        .await
        .unwrap();
    assert!(fragments > 2);
    let version = index.table.version().await.unwrap();
    let previous = index
        .connection
        .open_table(table_name(&index.generation))
        .execute()
        .await
        .unwrap();
    previous.checkout(version).await.unwrap();
    let denied = index
        .maintain_tables(None, &|| Err(error("READ_DENIED")))
        .await;
    assert!(matches!(denied, Err(e) if e.code == "READ_DENIED"));
    assert_eq!(index.table.version().await.unwrap(), version);
    index.maintain_tables(None, &|| Ok(())).await.unwrap();
    assert!(
        index
            .table
            .as_native()
            .unwrap()
            .count_fragments()
            .await
            .unwrap()
            < fragments
    );
    assert_eq!(store.sequence().unwrap(), authority_seq);
    let old_batches: Vec<_> = previous
        .query()
        .select(Select::Columns(vec!["id".into()]))
        .execute()
        .await
        .unwrap()
        .try_collect()
        .await
        .unwrap();
    let old_ids: std::collections::BTreeSet<_> = old_batches
        .iter()
        .flat_map(|batch| {
            let ids = batch
                .column_by_name("id")
                .unwrap()
                .as_any()
                .downcast_ref::<StringArray>()
                .unwrap();
            (0..batch.num_rows())
                .map(|i| ids.value(i).to_owned())
                .collect::<Vec<_>>()
        })
        .collect();
    assert_eq!(
        old_ids,
        (0..128).map(|i| format!("maintenance-{i}")).collect()
    );
    drop(previous);
    let maintained_version = index.table.version().await.unwrap();
    index.maintain_tables(None, &|| Ok(())).await.unwrap();
    assert_eq!(index.table.version().await.unwrap(), maintained_version);
    assert_eq!(
        serde_json::to_value(status(&store).unwrap()).unwrap(),
        serde_json::to_value(progress).unwrap()
    );
    assert_eq!(
        index.keyword("windowtoken127", &scope(), 20).await.unwrap(),
        expected
    );
    drop(index);
    store.close().await.unwrap();
    let store = Storage::open(&config).await.unwrap();
    let index = Index::open(&config, &store).await.unwrap();
    assert_eq!(status(&store).unwrap().lexical_watermark, target);
    assert_eq!(
        index.keyword("windowtoken127", &scope(), 20).await.unwrap(),
        expected
    );
    drop(index);
    store.close().await.unwrap();
}

#[tokio::test]
async fn maintenance_keeps_the_writer_generation_when_a_reader_replaces_the_cache() {
    use arrow_array::{FixedSizeListArray, types::Float32Type};
    use lancedb::{DistanceType, index::vector::IvfFlatIndexBuilder};

    let fixture = Fixture::new();
    let config = fixture.config();
    let store = Storage::open(&config).await.unwrap();
    let index = Index::open(&config, &store).await.unwrap();
    let configs = ["reader", "writer"].map(|model| {
        resolve(
            &format!("[retrieval]\nembedding='embed'\n[models.embed]\nkind='embedding'\nendpoint='http://127.0.0.1:1/v1'\nmodel='{model}'\ndimension=2\nlocal=true\ntimeout='2s'"),
            None,
            "",
            &Paths { data_dir: fixture.0.join("data"), socket_path: fixture.0.join("run/socket") },
        ).unwrap()
    });
    for config in &configs {
        let vector = index.vector_index(&store, config).await.unwrap();
        let schema = vector.table.schema().await.unwrap();
        for wave in 0..4 {
            let ids = (wave * 16..(wave + 1) * 16)
                .map(|i| format!("vector-{i}"))
                .collect::<Vec<_>>();
            let batch = RecordBatch::try_new(
                schema.clone(),
                vec![
                    Arc::new(StringArray::from(
                        ids.iter().map(|id| hash(id.as_bytes())).collect::<Vec<_>>(),
                    )),
                    Arc::new(StringArray::from(ids)),
                    Arc::new(UInt64Array::from(vec![1u64; 16])),
                    Arc::new(StringArray::from(vec![
                        hash(
                            &serde_json::to_vec(&scope()).unwrap()
                        );
                        16
                    ])),
                    Arc::new(
                        FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
                            (wave * 16..(wave + 1) * 16)
                                .map(|i| Some(vec![Some(1.0), Some(i as f32 / 64.0)])),
                            2,
                        ),
                    ),
                ],
            )
            .unwrap();
            vector
                .table
                .add(
                    Box::new(RecordBatchIterator::new(vec![Ok(batch)], schema.clone()))
                        as Box<dyn arrow_array::RecordBatchReader + Send>,
                )
                .execute()
                .await
                .unwrap();
            if wave == 0 {
                vector
                    .table
                    .create_index(
                        &["vector"],
                        NativeIndex::IvfFlat(
                            IvfFlatIndexBuilder::default()
                                .distance_type(DistanceType::Cosine)
                                // Later waves concentrate above the original training range.
                                .target_partition_size(8)
                                .sample_rate(8),
                        ),
                    )
                    .execute()
                    .await
                    .unwrap();
            }
        }
    }
    let writer = index.vector_index(&store, &configs[1]).await.unwrap();
    let reader = index.vector_index(&store, &configs[0]).await.unwrap();
    assert_ne!(writer.generation, reader.generation);
    assert!(Arc::ptr_eq(
        index.vector.lock().await.as_ref().unwrap(),
        &reader
    ));
    let reader_version = reader.table.version().await.unwrap();
    let native_index = writer.table.list_indices().await.unwrap()[0].name.clone();
    let before = writer
        .table
        .index_stats(&native_index)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(before.num_unindexed_rows, 48);
    index
        .maintain_tables(Some(writer.as_ref()), &|| Ok(()))
        .await
        .unwrap();
    let after = writer
        .table
        .index_stats(&native_index)
        .await
        .unwrap()
        .unwrap();
    let reader_after = reader.table.version().await.unwrap();
    let reader_rows = reader
        .search(&[1.0, 0.0], &scope(), 64, || Ok(()))
        .await
        .unwrap()
        .len();
    let writer_rows = writer
        .search(&[1.0, 0.0], &scope(), 64, || Ok(()))
        .await
        .unwrap()
        .len();
    let sequence = store.sequence().unwrap();
    drop(writer);
    drop(reader);
    drop(index);
    store.close().await.unwrap();
    assert_eq!(
        after.num_unindexed_rows, 0,
        "writer generation missed maintenance"
    );
    assert_eq!(after.num_indexed_rows, 64);
    assert_eq!(before.num_indices, Some(1));
    assert_eq!(
        after.num_indices,
        Some(2),
        "crossing the IVF split threshold must preserve the base segment"
    );
    assert_eq!(
        reader_after, reader_version,
        "reader generation was maintained instead"
    );
    assert_eq!((reader_rows, writer_rows, sequence), (64, 64, 0));
}

#[tokio::test]
async fn segmented_ann_preserves_unindexed_rows_and_snapshot_during_native_updates() {
    use arrow_array::{FixedSizeListArray, types::Float32Type};
    use lancedb::{
        DistanceType,
        index::vector::IvfFlatIndexBuilder,
        table::{CompactionOptions, OptimizeAction, OptimizeOptions},
    };

    let fixture = Fixture::new();
    let config = |budget| {
        resolve(
            &format!("[runtime]\nmemory_budget_mib={budget}\n[retrieval]\nembedding='embed'\n[models.embed]\nkind='embedding'\nendpoint='http://127.0.0.1:1/v1'\nmodel='s'\ndimension=2\nlocal=true"),
            None, "", &Paths { data_dir: fixture.0.join("data"), socket_path: fixture.0.join("run/socket") },
        ).unwrap()
    };
    let cfg = config(512);
    let storage = Storage::open(&cfg).await.unwrap();
    let owner = Index::open(&cfg, &storage).await.unwrap();
    let index = owner.vector_index(&storage, &cfg).await.unwrap();
    let table = &index.table;
    let schema = table.schema().await.unwrap();
    let key = hash(&serde_json::to_vec(&scope()).unwrap());
    let query = [1.0, 0.0];
    let records = |start: usize, end: usize| {
        let ids: Vec<_> = (start..end).map(|i| format!("r{i:03}")).collect();
        let data = RecordBatch::try_new(
            schema.clone(),
            vec![
                Arc::new(StringArray::from(ids.clone())),
                Arc::new(StringArray::from(ids)),
                Arc::new(UInt64Array::from(
                    (start..end).map(|i| i as u64 + 1).collect::<Vec<_>>(),
                )),
                Arc::new(StringArray::from(
                    (start..end)
                        .map(|i| if i % 2 == 0 { key.as_str() } else { "excluded" })
                        .collect::<Vec<_>>(),
                )),
                Arc::new(
                    FixedSizeListArray::from_iter_primitive::<Float32Type, _, _>(
                        (start..end).map(|i| Some(vec![Some(1.0), Some((321 - i) as f32 / 100.0)])),
                        2,
                    ),
                ),
            ],
        )
        .unwrap();
        Box::new(RecordBatchIterator::new(vec![Ok(data)], schema.clone()))
            as Box<dyn arrow_array::RecordBatchReader + Send>
    };
    let hits = |end: usize, excluded: &[usize]| {
        (0..end)
            .rev()
            .filter(|i| i % 2 == 0 && !excluded.contains(i))
            .take(8)
            .map(|i| (format!("r{i:03}"), i as u64 + 1))
            .collect::<Vec<_>>()
    };
    table.add(records(0, 32)).execute().await.unwrap();
    assert_eq!(
        index.search(&query, &scope(), 8, || Ok(())).await.unwrap(),
        hits(32, &[])
    );
    table
        .create_index(&["scope"], NativeIndex::BTree(BTreeIndexBuilder::default()))
        .execute()
        .await
        .unwrap();
    table
        .create_index(
            &["vector"],
            NativeIndex::IvfFlat(
                IvfFlatIndexBuilder::default()
                    .distance_type(DistanceType::Cosine)
                    .num_partitions(3),
            ),
        )
        .execute()
        .await
        .unwrap();
    for wave in 1..9 {
        table
            .add(records(32 * wave, 32 * (wave + 1)))
            .execute()
            .await
            .unwrap();
        table
            .optimize(OptimizeAction::Index(OptimizeOptions::append()))
            .await
            .unwrap();
    }
    let s = table.index_stats("vector_idx").await.unwrap().unwrap();
    assert_eq!(
        (s.num_indices, s.num_indexed_rows, s.num_unindexed_rows),
        (Some(9), 288, 0)
    );
    table.add(records(288, 294)).execute().await.unwrap();
    let s = table.index_stats("vector_idx").await.unwrap().unwrap();
    assert_eq!(s.num_unindexed_rows, 6);
    let snapshot = table.query_snapshot().await.unwrap();
    let previous = snapshot.dataset().unwrap().get().await.unwrap();
    let previous_version = previous.version().version;
    let filter = format!("scope = '{key}'");
    // Three ANN groups, tail discovery, one flat group, then the final row fetch.
    for (stop, code) in [(2, "CANCELLED"), (5, "WORKFLOW_TIMEOUT"), (6, "CANCELLED")] {
        let calls = std::cell::Cell::new(0);
        let stopped = vector::segments::search(&previous, &query, &filter, 8, &|| {
            calls.set(calls.get() + 1);
            if calls.get() == stop {
                Err(error(code))
            } else {
                Ok(())
            }
        })
        .await
        .unwrap_err();
        assert_eq!(stopped.code, code);
        assert_eq!(calls.get(), stop);
    }

    // A reopened index uses the live owner budget even when reading an old model generation.
    let high = Index::open(&config(2048), &storage).await.unwrap();
    let old_generation = high.vector_index(&storage, &cfg).await.unwrap();
    assert_eq!(old_generation.generation, index.generation);
    let calls = std::cell::Cell::new(0);
    let native_hits = old_generation
        .search(&query, &scope(), 8, || {
            calls.set(calls.get() + 1);
            Ok(())
        })
        .await
        .unwrap();
    assert_eq!(native_hits, hits(294, &[]));
    assert_eq!(
        calls.get(),
        1,
        "ample budget should dispatch the ordinary scan"
    );
    drop(old_generation);
    drop(high);
    let old_query = async {
        vector::segments::search(&previous, &query, &filter, 8, &|| Ok(()))
            .await
            .unwrap()
            .unwrap()
    };
    let change = async {
        // Remove a winner from each of the indexed and unindexed inputs.
        table.delete("id IN ('r286','r292')").await.unwrap();
        table.add(records(294, 300)).execute().await.unwrap();
        table
            .optimize(OptimizeAction::Compact {
                options: CompactionOptions {
                    target_rows_per_fragment: 128,
                    ..Default::default()
                },
                remap_options: None,
            })
            .await
            .unwrap();
    };
    let (old, ()) = tokio::join!(old_query, change);
    assert_eq!(old, hits(294, &[]));
    assert!(table.version().await.unwrap() > previous_version);
    assert_eq!(
        vector::segments::search(&previous, &query, &filter, 8, &|| Ok(()))
            .await
            .unwrap()
            .unwrap(),
        old
    );
    assert_eq!(
        index.search(&query, &scope(), 8, || Ok(())).await.unwrap(),
        hits(300, &[286, 292])
    );
    table
        .optimize(OptimizeAction::Index(OptimizeOptions::append()))
        .await
        .unwrap();
    assert_eq!(
        index.search(&query, &scope(), 8, || Ok(())).await.unwrap(),
        hits(300, &[286, 292])
    );
    // An unrelated scalar index whose segments name different columns cannot be
    // described. The scoped vector scan itself can still use its healthy indices.
    use lance::{Dataset, dataset::transaction::Operation, index::DatasetIndexExt};
    for column in ["id", "key"] {
        table
            .create_index(&[column], NativeIndex::BTree(BTreeIndexBuilder::default()))
            .execute()
            .await
            .unwrap();
    }
    let native = table.dataset().unwrap();
    let current = native.get().await.unwrap();
    let metadata = current.load_indices().await.unwrap();
    let id = metadata.iter().find(|i| i.name == "id_idx").unwrap();
    let key = metadata.iter().find(|i| i.name == "key_idx").unwrap();
    let mut mixed = key.clone();
    mixed.name.clone_from(&id.name);
    let legacy = Dataset::commit(
        current.uri(),
        Operation::CreateIndex {
            new_indices: vec![id.clone(), mixed],
            removed_indices: vec![id.clone(), key.clone()],
        },
        Some(current.version().version),
        None,
        None,
        current.session(),
        false,
    )
    .await
    .unwrap();
    assert!(
        legacy
            .describe_indices(None)
            .await
            .err()
            .unwrap()
            .to_string()
            .contains("Index fields")
    );
    native.update(legacy);
    assert_eq!(
        index.search(&query, &scope(), 8, || Ok(())).await.unwrap(),
        hits(300, &[286, 292])
    );
    assert_eq!(
        index
            .search(&[1.0, 0.0, 0.0], &scope(), 8, || Ok(()))
            .await
            .unwrap_err()
            .code,
        "INDEX_IO"
    );
    drop(index);
    drop(owner);
    storage.close().await.unwrap();
}
