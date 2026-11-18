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
    index.connection.drop_table(TABLE, &[]).await.unwrap();
    drop(index);
    assert!(matches!(Index::open(&config, &store).await, Err(e) if e.code == "INDEX_MISSING"));
    store.close().await.unwrap();
}

#[tokio::test]
async fn rebuild_generation_survives_restart_and_reclaims_only_old_native_tables() {
    let fixture = Fixture::new();
    let config = fixture.config();
    let store = Storage::open(&config).await.unwrap();
    let target = memory(&store, "original", "rebuild searchable text", None).await;
    let original = Domain::new(&store).memory("original").unwrap().unwrap();
    let index = Index::open(&config, &store).await.unwrap();
    sync(&index, &store, target).await;
    drop(index);
    Index::begin_rebuild(&store, "rebuild-1", 1, &|| Ok(()))
        .await
        .unwrap();
    let pending = status(&store).unwrap();
    assert_eq!(pending.lexical_watermark, 0);
    assert_ne!(pending.generation, GENERATION);
    store.close().await.unwrap();
    let store = Storage::open(&config).await.unwrap();
    assert_eq!(status(&store).unwrap().generation, pending.generation);
    let index = Index::open(&config, &store).await.unwrap();
    assert_eq!(
        index
            .connection
            .table_names()
            .execute()
            .await
            .unwrap()
            .len(),
        2
    );
    index.retire_previous_tables(&|| Ok(())).await.unwrap();
    assert_eq!(
        index.connection.table_names().execute().await.unwrap(),
        [table_name(&pending.generation)]
    );
    let progress = sync(&index, &store, target).await;
    assert_eq!(progress.indexed_rows, 1);
    assert_eq!(
        index.keyword("rebuild", &scope(), 10).await.unwrap(),
        [("original".into(), original.revision.commit_seq)]
    );
    assert_eq!(
        Domain::new(&store)
            .memory("original")
            .unwrap()
            .unwrap()
            .revision,
        original.revision
    );
    let sequence = store.sequence().unwrap();
    Index::begin_rebuild(&store, "rebuild-1", 1, &|| Ok(()))
        .await
        .unwrap();
    assert_eq!(store.sequence().unwrap(), sequence);
    assert_eq!(status(&store).unwrap().lexical_watermark, target);
    drop(index);
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
        .search(&[1.0, 0.0], &scope(), 64)
        .await
        .unwrap()
        .len();
    let writer_rows = writer
        .search(&[1.0, 0.0], &scope(), 64)
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
