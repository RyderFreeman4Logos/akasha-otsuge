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
        let status = index.sync_batch(store, target, 1).await.unwrap();
        if status.lexical_watermark >= target {
            return status;
        }
    }
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
