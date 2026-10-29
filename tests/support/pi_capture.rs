use super::Fixture;
use akasha_otsuge::{
    adapter::pi::{TRANSCRIPT_STREAM, reconcile_transcript},
    capture::*,
    domain::Scope,
};
use serde_json::Value;

const BEFORE: &str = include_str!("../fixtures/pi/1.4.0/root-before.jsonl");
const TREE: &str = include_str!("../fixtures/pi/1.4.0/root-tree.jsonl");
const FORK: &str = include_str!("../fixtures/pi/1.4.0/fork.jsonl");
fn manifest() -> Value {
    serde_json::from_str(include_str!("../fixtures/pi/1.4.0/manifest.json")).unwrap()
}
fn source(fork: bool) -> SourceIdentity {
    let session = manifest()[if fork { "fork_id" } else { "root_id" }]
        .as_str()
        .unwrap()
        .to_owned();
    SourceIdentity {
        adapter: "pi".into(),
        profile: "private-agent-dir".into(),
        session: session.clone(),
        thread: session,
        agent: "main".into(),
    }
}
fn scope() -> Scope {
    Scope {
        user: "user".into(),
        project: Some("project".into()),
        branch_or_version: None,
        session: None,
    }
}
fn binding(fork: bool) -> CaptureBinding {
    CaptureBinding::from_source(
        source(fork),
        scope(),
        TRANSCRIPT_STREAM,
        CoverageLevel::VisibleText,
    )
    .unwrap()
}
fn reference(fork: bool, name: &str) -> EventRef {
    EventRef {
        source: source(fork),
        event_id: format!("entry:{}", manifest()["ids"][name].as_str().unwrap()),
    }
}

#[tokio::test]
async fn native_tree_keeps_selected_branches_fork_cutoff_edits_and_compaction_originals() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let capture = Capture::new(&store);
    for (fork, text, tip) in [(false, TREE, "final"), (true, FORK, "fork_user")] {
        let path = fixture
            .0
            .join(if fork { "fork.jsonl" } else { "root.jsonl" });
        std::fs::write(&path, text).unwrap();
        let report = reconcile_transcript(&store, &source(fork), scope(), &path, 64, 1)
            .await
            .unwrap();
        assert_eq!(report.processed, text.split_terminator('\n').count());
        assert_eq!(report.checkpoint.checkpoint.pending, None);
        let page = capture
            .lineage(
                &binding(fork),
                &reference(fork, tip),
                store.sequence().unwrap(),
                64,
            )
            .unwrap();
        assert_eq!(page.missing, None);
        assert_eq!(page.next, None);
        let mut texts = Vec::new();
        for event in &page.events {
            let raw = String::from_utf8(capture.raw(event).unwrap()).unwrap();
            assert!(
                !raw.contains("PRIVATE_"),
                "private native content escaped: {}",
                event.event.reference.event_id
            );
            texts.push(capture.text(event).unwrap());
        }
        let joined = texts.join("\n");
        assert!(joined.contains("Original intent α\u{2028}retained"));
        assert!(joined.contains("Original public answer"));
        assert!(joined.contains("Original tool result"));
        assert!(joined.contains("A native compaction summary"));
        assert!(!joined.contains("SIBLING_"));
        assert!(!joined.contains("OTHER_ROOT_ONLY"));
        assert!(!joined.contains("LATER_PARENT_ONLY"));
        let compact = capture
            .event(
                &binding(fork),
                &reference(fork, "compaction"),
                store.sequence().unwrap(),
            )
            .unwrap()
            .unwrap();
        assert!(matches!(
            compact.event.metadata.kind,
            EventKind::Compaction { .. }
        ));
        let original = capture
            .event(
                &binding(fork),
                &reference(fork, "user"),
                store.sequence().unwrap(),
            )
            .unwrap()
            .unwrap();
        assert_eq!(
            capture.text(&original).unwrap(),
            "Original intent α\u{2028}retained"
        );
        let edit = capture
            .event(
                &binding(fork),
                &reference(fork, "edit"),
                store.sequence().unwrap(),
            )
            .unwrap()
            .unwrap();
        let edit: Value = serde_json::from_slice(&capture.raw(&edit).unwrap()).unwrap();
        assert_eq!(
            edit["replacement"]["content"],
            "Replacement for future model context only"
        );
        let omitted = capture
            .event(
                &binding(fork),
                &reference(fork, "omission"),
                store.sequence().unwrap(),
            )
            .unwrap()
            .unwrap();
        let omitted: Value = serde_json::from_slice(&capture.raw(&omitted).unwrap()).unwrap();
        assert!(omitted["replacement"].is_null());
        let state = capture
            .message(&binding(fork), &reference(fork, "assistant").event_id)
            .unwrap()
            .unwrap();
        assert_eq!(state.delta_count, 0);
        assert_eq!(state.final_event, Some(reference(fork, "assistant")));
        let replay = reconcile_transcript(&store, &source(fork), scope(), &path, 64, 1)
            .await
            .unwrap();
        assert_eq!(replay.processed, 0);
        if fork {
            let header = page.events.last().unwrap();
            assert_eq!(header.event.metadata.kind, EventKind::Fork);
            let raw: Value = serde_json::from_slice(&capture.raw(header).unwrap()).unwrap();
            assert_eq!(raw["parentSession"], manifest()["root_file"]);
        }
    }
}

#[tokio::test]
async fn native_capture_resumes_partial_tail_reopen_and_rotation_without_duplicate_finals() {
    let fixture = Fixture::new();
    let path = fixture.0.join("session.jsonl");
    let store = fixture.open().await;
    std::fs::write(&path, BEFORE.trim_end_matches('\n')).unwrap();
    let partial = reconcile_transcript(&store, &source(false), scope(), &path, 64, 1)
        .await
        .unwrap();
    assert_eq!(
        partial.checkpoint.checkpoint.pending,
        Some(PendingReason::PartialTail)
    );
    store.close().await.unwrap();
    drop(store);
    let store = fixture.open().await;
    std::fs::write(&path, TREE).unwrap();
    let resumed = reconcile_transcript(&store, &source(false), scope(), &path, 2, 1)
        .await
        .unwrap();
    assert_eq!(resumed.processed, 2);
    assert_eq!(
        resumed.checkpoint.checkpoint.pending,
        Some(PendingReason::Backlog)
    );
    let done = reconcile_transcript(&store, &source(false), scope(), &path, 64, 1)
        .await
        .unwrap();
    assert_eq!(done.checkpoint.checkpoint.pending, None);
    std::fs::rename(&path, fixture.0.join("rotated.jsonl")).unwrap();
    std::fs::write(&path, TREE).unwrap();
    let rotated = reconcile_transcript(&store, &source(false), scope(), &path, 64, 1)
        .await
        .unwrap();
    assert_eq!(rotated.deduplicated, TREE.split_terminator('\n').count());
    assert_eq!(
        rotated.checkpoint.checkpoint.loss,
        vec![LossReason::Rotation]
    );
    assert_eq!(rotated.checkpoint.checkpoint.pending, None);
    let capture = Capture::new(&store);
    assert_eq!(
        capture
            .message(&binding(false), &reference(false, "assistant").event_id)
            .unwrap()
            .unwrap()
            .delta_count,
        0
    );
}

#[tokio::test]
async fn foreign_headers_conflicting_ids_and_unsupported_records_never_advance_as_complete() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let path = fixture.0.join("session.jsonl");
    std::fs::write(&path, FORK).unwrap();
    assert_eq!(
        reconcile_transcript(&store, &source(false), scope(), &path, 64, 1)
            .await
            .unwrap_err()
            .code,
        "BINDING_CONFLICT"
    );
    std::fs::write(&path, BEFORE).unwrap();
    reconcile_transcript(&store, &source(false), scope(), &path, 64, 1)
        .await
        .unwrap();
    let mut assistant: Value = BEFORE
        .split_terminator('\n')
        .map(|v| serde_json::from_str::<Value>(v).unwrap())
        .find(|v| v["id"] == manifest()["ids"]["assistant"])
        .unwrap();
    assistant["message"]["content"] = serde_json::json!([{ "type":"text", "text":"CONFLICT" }]);
    std::fs::write(&path, format!("{BEFORE}{assistant}\n")).unwrap();
    let conflict = reconcile_transcript(&store, &source(false), scope(), &path, 64, 1)
        .await
        .unwrap();
    assert_eq!(
        conflict.checkpoint.checkpoint.pending,
        Some(PendingReason::Conflict)
    );
    std::fs::write(&path, format!("{BEFORE}{{\"type\":\"future_native_entry\",\"id\":\"unknown\",\"parentId\":null,\"timestamp\":\"now\"}}\n")).unwrap();
    let unknown = reconcile_transcript(&store, &source(false), scope(), &path, 64, 1)
        .await
        .unwrap();
    assert_eq!(
        unknown.checkpoint.checkpoint.pending,
        Some(PendingReason::InvalidFrame)
    );
    std::fs::write(&path, BEFORE.replacen("\"version\":3", "\"version\":4", 1)).unwrap();
    assert_eq!(
        reconcile_transcript(&store, &source(false), scope(), &path, 64, 1)
            .await
            .unwrap_err()
            .code,
        "ADAPTER_UNSUPPORTED"
    );
}
