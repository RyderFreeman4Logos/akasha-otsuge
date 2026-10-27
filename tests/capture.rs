use akasha_otsuge::capture::*;
use akasha_otsuge::config::{Paths, resolve};
use akasha_otsuge::domain::{Ingress, Scope};
use akasha_otsuge::storage::Storage;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "akasha-capture-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    async fn open(&self) -> Storage {
        Storage::open(
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
fn scope() -> Scope {
    Scope {
        user: "user".into(),
        project: Some("project".into()),
        branch_or_version: None,
        session: None,
    }
}
fn identity() -> SourceIdentity {
    SourceIdentity {
        adapter: "fixture-v1".into(),
        profile: "profile".into(),
        session: "session".into(),
        thread: "thread".into(),
        agent: "agent".into(),
    }
}
fn binding(stream: &str) -> CaptureBinding {
    CaptureBinding::from_source(
        identity(),
        scope(),
        stream,
        CoverageLevel::AvailablePayloads,
    )
    .unwrap()
}
fn ingress() -> Ingress {
    Ingress::unverified("fixture", "archive", scope()).unwrap()
}
fn reference(id: &str) -> EventRef {
    EventRef {
        source: identity(),
        event_id: id.into(),
    }
}
fn event(id: &str, text: &str) -> EventDraft {
    EventDraft {
        event_id: id.into(),
        source_version: "1.0".into(),
        runtime_instance: "host-runtime".into(),
        branch: "main".into(),
        parent: None,
        fork_cutoff: None,
        event_time_ms: Some(10),
        observed_time_ms: 20,
        visibility: Visibility::User,
        kind: EventKind::Message {
            message_id: id.into(),
            phase: MessagePhase::Final,
        },
        text: text.into(),
    }
}
fn frame(event: EventDraft, raw: &[u8]) -> Vec<u8> {
    let mut b = serde_json::to_vec(&CaptureFrame {
        schema_version: 1,
        event,
        raw_hex: raw.iter().map(|b| format!("{b:02x}")).collect(),
    })
    .unwrap();
    b.push(b'\n');
    b
}

#[tokio::test]
async fn checkpoint_retry_is_scoped_to_the_stream_and_stale_publication_is_atomic() {
    let f = Fixture::new();
    let store = f.open().await;
    let c = Capture::new(&store);
    let advance = Advance {
        expected_revision: None,
        checkpoint: Checkpoint {
            cursor: "opaque-host-cursor".into(),
            coverage: CoverageLevel::AvailablePayloads,
            loss: vec![],
            pending: None,
        },
    };
    let a = event("same", "text");
    let one = binding("one");
    let two = binding("two");
    let first = c
        .ingest(&one, &a, b"original", &ingress(), Some(&advance), 1)
        .await
        .unwrap();
    assert_eq!(
        c.checkpoint(&one).unwrap().unwrap().revision,
        first.revision
    );
    let second = c
        .ingest(&two, &a, b"original", &ingress(), Some(&advance), 1)
        .await
        .unwrap();
    assert_eq!(first, second);
    assert!(
        c.checkpoint(&two).unwrap().is_some(),
        "same cursor on a different source stream must advance its own checkpoint"
    );
    let cp = c.checkpoint(&one).unwrap().unwrap();
    let pending = event("new", "new text");
    assert_eq!(
        c.ingest(&one, &pending, b"new", &ingress(), Some(&advance), 1)
            .await
            .unwrap_err()
            .code,
        "REVISION_CONFLICT"
    );
    assert!(
        c.event(&one, &reference("new"), store.sequence().unwrap())
            .unwrap()
            .is_none()
    );
    assert_eq!(c.checkpoint(&one).unwrap().unwrap(), cp);
    let fixed = Advance {
        expected_revision: Some(cp.revision),
        checkpoint: Checkpoint {
            cursor: "next".into(),
            ..advance.checkpoint.clone()
        },
    };
    let committed = c
        .ingest(&one, &pending, b"new", &ingress(), Some(&fixed), 1)
        .await
        .unwrap();
    assert_eq!(
        committed.revision,
        c.checkpoint(&one).unwrap().unwrap().revision
    );
    let seq = store.sequence().unwrap();
    assert_eq!(
        c.ingest(&one, &pending, b"new", &ingress(), Some(&fixed), 2)
            .await
            .unwrap(),
        committed
    );
    assert_eq!(store.sequence().unwrap(), seq);
    store.close().await.unwrap();
}

#[tokio::test]
async fn cross_branch_edges_are_rejected_even_at_a_one_event_page_boundary() {
    let f = Fixture::new();
    let store = f.open().await;
    let c = Capture::new(&store);
    let b = binding("stream");
    c.ingest(&b, &event("a", "a"), b"a", &ingress(), None, 1)
        .await
        .unwrap();
    let mut bad = event("bad", "bad");
    bad.branch = "other".into();
    bad.parent = Some(reference("a"));
    c.ingest(&b, &bad, b"bad", &ingress(), None, 1)
        .await
        .unwrap();
    assert_eq!(
        c.lineage(&b, &reference("bad"), store.sequence().unwrap(), 1)
            .unwrap_err()
            .code,
        "CAPTURE_BRANCH_CONFLICT"
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn invalid_ingress_and_oversized_metadata_leave_no_partial_archive() {
    let f = Fixture::new();
    let store = f.open().await;
    let c = Capture::new(&store);
    let b = binding("stream");
    let mut wrong = scope();
    wrong.user = "different-user".into();
    let ingress = Ingress::unverified("fixture", "archive", wrong).unwrap();
    let a = event("a", "text");
    assert_eq!(
        c.ingest(&b, &a, b"raw", &ingress, None, 1)
            .await
            .unwrap_err()
            .code,
        "SCOPE_DENIED"
    );
    assert_eq!(
        store.sequence().unwrap(),
        0,
        "scope must be checked before admitting evidence"
    );
    let mut too_big = event("oversized", "");
    let mut long = reference(&"x".repeat(256));
    long.source.agent = "a".repeat(256);
    long.source.thread = "t".repeat(256);
    long.source.session = "s".repeat(256);
    too_big.kind = EventKind::Compaction {
        first: long.clone().into(),
        last: long.clone().into(),
        originals: vec![long; 64],
    };
    let trusted = Ingress::unverified("fixture", "archive", scope()).unwrap();
    assert_eq!(
        c.ingest(&b, &too_big, b"", &trusted, None, 1)
            .await
            .unwrap_err()
            .code,
        "INVALID_CAPTURE_EVENT"
    );
    assert_eq!(store.sequence().unwrap(), 0);
    store.close().await.unwrap();
}

#[tokio::test]
async fn out_of_order_backfill_and_concurrent_replay_keep_originals_and_missing_links() {
    use akasha_otsuge::domain::{Actor, Domain};
    let f = Fixture::new();
    let store = f.open().await;
    let c = Capture::new(&store);
    let b = binding("stream");
    let channel = ingress();
    let mut child = event("child", "中文\0\n".repeat(25000).as_str());
    child.parent = Some(reference("late"));
    let raw = (0..200000).map(|n| (n % 256) as u8).collect::<Vec<_>>();
    let (one, two) = tokio::join!(
        c.ingest(&b, &child, &raw, &channel, None, 1),
        c.ingest(&b, &child, &raw, &channel, None, 1)
    );
    let one = one.unwrap();
    assert_eq!(one, two.unwrap());
    let before = store.sequence().unwrap();
    assert_eq!(
        c.lineage(&b, &reference("child"), before, 64)
            .unwrap()
            .missing,
        Some(reference("late"))
    );
    assert!(one.event.raw.len() > 1 && one.event.normalized.len() > 1);
    assert_eq!(c.raw(&one).unwrap(), raw);
    assert_eq!(c.text(&one).unwrap(), child.text);
    let source = &one.event.normalized[0];
    assert!(matches!(
        Domain::new(&store)
            .source(&source.event_id, source.revision)
            .unwrap()
            .provenance
            .actor,
        Actor::Unverified(_)
    ));
    c.ingest(
        &b,
        &event("late", "late original"),
        b"late",
        &channel,
        None,
        1,
    )
    .await
    .unwrap();
    assert!(
        c.lineage(&b, &reference("child"), before, 64)
            .unwrap()
            .missing
            .is_some()
    );
    let current = c
        .lineage(&b, &reference("child"), store.sequence().unwrap(), 64)
        .unwrap();
    assert!(current.missing.is_none());
    assert_eq!(current.events.len(), 2);
    let mut compact = event("compacted", "summary");
    compact.parent = Some(reference("child"));
    compact.kind = EventKind::Compaction {
        first: reference("late").into(),
        last: reference("child").into(),
        originals: vec![reference("unavailable-original")],
    };
    let compact = c
        .ingest(&b, &compact, b"compact", &channel, None, 1)
        .await
        .unwrap();
    assert_eq!(
        c.missing_references(&b, &compact, store.sequence().unwrap())
            .unwrap(),
        vec![reference("unavailable-original")]
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn invalid_frames_missing_files_and_offline_backfill_never_claim_complete() {
    let f = Fixture::new();
    let store = f.open().await;
    let c = Capture::new(&store);
    let b = binding("live");
    let path = f.0.join("events.jsonl");
    assert_eq!(
        c.reconcile_jsonl(&b, &path, 64, 1)
            .await
            .unwrap()
            .checkpoint
            .checkpoint
            .pending,
        Some(PendingReason::SourceMissing)
    );
    std::fs::write(&path, b"not json\n").unwrap();
    let bad = c.reconcile_jsonl(&b, &path, 64, 1).await.unwrap();
    assert_eq!(
        bad.checkpoint.checkpoint.pending,
        Some(PendingReason::InvalidFrame)
    );
    let mut bytes = frame(event("a", "first"), b"first");
    bytes.extend(frame(event("b", "second"), b"second"));
    std::fs::write(&path, &bytes).unwrap();
    let bounded = c.reconcile_jsonl(&b, &path, 1, 1).await.unwrap();
    assert_eq!(bounded.processed, 1);
    assert_eq!(
        bounded.checkpoint.checkpoint.pending,
        Some(PendingReason::Backlog)
    );
    let end = c.reconcile_jsonl(&b, &path, 64, 1).await.unwrap();
    assert_eq!(end.processed, 1);
    assert_eq!(end.checkpoint.checkpoint.pending, None);
    let backfill = binding("offline");
    let replay = c.reconcile_jsonl(&backfill, &path, 64, 2).await.unwrap();
    assert_eq!((replay.processed, replay.deduplicated), (2, 2));
    assert_eq!(replay.checkpoint.checkpoint.pending, None);
    let mut conflicting = event("a", "altered source");
    conflicting.source_version = "2".into();
    bytes.extend(frame(conflicting, b"replacement"));
    std::fs::write(&path, &bytes).unwrap();
    let conflict = c.reconcile_jsonl(&b, &path, 64, 1).await.unwrap();
    assert_eq!(
        conflict.checkpoint.checkpoint.pending,
        Some(PendingReason::Conflict)
    );
    assert_eq!(conflict.processed, 0);
    assert_eq!(
        c.text(
            &c.event(&b, &reference("a"), store.sequence().unwrap())
                .unwrap()
                .unwrap()
        )
        .unwrap(),
        "first"
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn duplicate_final_delivery_has_one_logical_final_and_a_conflict_cannot_overwrite_it() {
    let f = Fixture::new();
    let store = f.open().await;
    let c = Capture::new(&store);
    let b = binding("stream");
    let mut first = event("delta", "part");
    first.kind = EventKind::Message {
        message_id: "message".into(),
        phase: MessagePhase::Delta { index: 0 },
    };
    c.ingest(&b, &first, b"delta", &ingress(), None, 1)
        .await
        .unwrap();
    assert!(
        c.message(&b, "message")
            .unwrap()
            .unwrap()
            .final_event
            .is_none()
    );
    let mut final_event = event("final-1", "complete");
    final_event.kind = EventKind::Message {
        message_id: "message".into(),
        phase: MessagePhase::Final,
    };
    c.ingest(&b, &final_event, b"final1", &ingress(), None, 1)
        .await
        .unwrap();
    final_event.event_id = "final-2".into();
    c.ingest(&b, &final_event, b"final2", &ingress(), None, 1)
        .await
        .unwrap();
    assert_eq!(
        c.message(&b, "message").unwrap().unwrap().final_event,
        Some(reference("final-1"))
    );
    final_event.event_id = "conflicting-final".into();
    final_event.text = "changed".into();
    assert_eq!(
        c.ingest(&b, &final_event, b"conflict", &ingress(), None, 1)
            .await
            .unwrap_err()
            .code,
        "CAPTURE_MESSAGE_CONFLICT"
    );
    assert_eq!(
        c.message(&b, "message").unwrap().unwrap().final_event,
        Some(reference("final-1"))
    );
    let mut resume = event("resume", "");
    resume.kind = EventKind::Resume;
    resume.parent = Some(reference("final-1"));
    c.ingest(&b, &resume, b"resume", &ingress(), None, 1)
        .await
        .unwrap();
    let mut abandoned = event("abandoned", "");
    abandoned.kind = EventKind::Abandoned;
    abandoned.parent = Some(reference("resume"));
    c.ingest(&b, &abandoned, b"abandoned", &ingress(), None, 1)
        .await
        .unwrap();
    assert!(
        c.lineage(&b, &reference("abandoned"), store.sequence().unwrap(), 64)
            .unwrap()
            .events
            .iter()
            .all(|e| e.event.reference.event_id != "conflicting-final")
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn replay_preserves_first_observation_and_equal_text_events_remain_distinct() {
    let f = Fixture::new();
    let store = f.open().await;
    let c = Capture::new(&store);
    let b = binding("stream");
    let mut draft = event("a", "same words\n末尾 \0");
    let original = c
        .ingest(&b, &draft, b"\0raw\xff", &ingress(), None, 1)
        .await
        .unwrap();
    draft.observed_time_ms = 900;
    draft.source_version = "2.0".into();
    assert_eq!(
        c.ingest(&b, &draft, b"\0raw\xff", &ingress(), None, 2)
            .await
            .unwrap(),
        original
    );
    draft.event_id = "b".into();
    draft.kind = EventKind::Message {
        message_id: "b".into(),
        phase: MessagePhase::Final,
    };
    let second = c
        .ingest(&b, &draft, b"\0raw\xff", &ingress(), None, 2)
        .await
        .unwrap();
    assert_ne!(original.revision, second.revision);
    assert_eq!(c.raw(&original).unwrap(), b"\0raw\xff");
    assert_eq!(c.text(&original).unwrap(), "same words\n末尾 \0");
    draft.event_id = "a".into();
    draft.kind = EventKind::Message {
        message_id: "a".into(),
        phase: MessagePhase::Final,
    };
    assert_eq!(
        c.ingest(&b, &draft, b"changed", &ingress(), None, 2)
            .await
            .unwrap_err()
            .code,
        "CAPTURE_IDENTITY_CONFLICT"
    );
    store.close().await.unwrap();
    let store = f.open().await;
    let c = Capture::new(&store);
    assert_eq!(
        c.event(&b, &reference("a"), store.sequence().unwrap())
            .unwrap()
            .unwrap(),
        original
    );
    assert_eq!(c.raw(&original).unwrap(), b"\0raw\xff");
    store.close().await.unwrap();
}

#[tokio::test]
async fn late_deltas_never_replace_a_final_and_repeated_resume_keeps_the_branch_cutoff() {
    let f = Fixture::new();
    let store = f.open().await;
    let c = Capture::new(&store);
    let b = binding("stream");
    let a = event("a", "parent before fork");
    c.ingest(&b, &a, b"a", &ingress(), None, 1).await.unwrap();
    let mut child = event("child", "child only");
    child.branch = "fork".into();
    child.parent = Some(reference("a"));
    child.fork_cutoff = child.parent.clone();
    c.ingest(&b, &child, b"child", &ingress(), None, 1)
        .await
        .unwrap();
    let mut later = event("later", "parent after fork");
    later.parent = Some(reference("a"));
    c.ingest(&b, &later, b"later", &ingress(), None, 1)
        .await
        .unwrap();
    let mut delta = event("delta", "child ");
    delta.branch = "fork".into();
    delta.parent = child.parent.clone();
    delta.fork_cutoff = child.fork_cutoff.clone();
    delta.kind = EventKind::Message {
        message_id: "child".into(),
        phase: MessagePhase::Delta { index: 0 },
    };
    c.ingest(&b, &delta, b"delta", &ingress(), None, 1)
        .await
        .unwrap();
    c.ingest(&b, &delta, b"delta", &ingress(), None, 1)
        .await
        .unwrap();
    assert_eq!(
        c.message(&b, "child").unwrap().unwrap(),
        MessageView {
            final_event: Some(reference("child")),
            delta_count: 1
        }
    );
    let mut tip = reference("child");
    for i in 0..3 {
        let mut compact = event(&format!("compact-{i}"), "derived summary, not original");
        compact.branch = "fork".into();
        compact.parent = Some(tip.clone());
        compact.kind = EventKind::Compaction {
            first: reference("a").into(),
            last: tip.clone().into(),
            originals: vec![reference("a"), reference("child")],
        };
        c.ingest(&b, &compact, b"summary", &ingress(), None, 1)
            .await
            .unwrap();
        tip = reference(&compact.event_id);
    }
    let page = c.lineage(&b, &tip, store.sequence().unwrap(), 64).unwrap();
    assert!(page.next.is_none() && page.missing.is_none());
    assert_eq!(page.events.len(), 5);
    assert!(
        page.events
            .iter()
            .all(|v| v.event.reference.event_id != "later")
    );
    assert_eq!(
        c.text(page.events.last().unwrap()).unwrap(),
        "parent before fork"
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn real_file_reconcile_waits_for_a_partial_tail_and_resumes_after_reopen() {
    use std::io::Write;
    let f = Fixture::new();
    let store = f.open().await;
    let c = Capture::new(&store);
    let b = binding("log");
    let path = f.0.join("events.jsonl");
    let first = frame(event("a", "first"), b"a");
    let second = frame(event("b", "中文 tail"), b"b");
    let split = second.len() - 3;
    let mut bytes = first.clone();
    bytes.extend_from_slice(&second[..split]);
    std::fs::write(&path, &bytes).unwrap();
    let report = c.reconcile_jsonl(&b, &path, 64, 1).await.unwrap();
    assert_eq!(report.processed, 1);
    assert_eq!(
        report.checkpoint.checkpoint.pending,
        Some(PendingReason::PartialTail)
    );
    assert!(
        c.event(&b, &reference("b"), store.sequence().unwrap())
            .unwrap()
            .is_none()
    );
    store.close().await.unwrap();
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&second[split..])
        .unwrap();
    let store = f.open().await;
    let c = Capture::new(&store);
    let report = c.reconcile_jsonl(&b, &path, 64, 1).await.unwrap();
    assert_eq!(report.processed, 1);
    assert_eq!(report.checkpoint.checkpoint.pending, None);
    let seq = store.sequence().unwrap();
    let report = c.reconcile_jsonl(&b, &path, 64, 1).await.unwrap();
    assert_eq!(report.processed, 0);
    assert_eq!(store.sequence().unwrap(), seq);
    std::fs::rename(&path, f.0.join("rotated.jsonl")).unwrap();
    std::fs::write(&path, frame(event("c", "new file"), b"c")).unwrap();
    let report = c.reconcile_jsonl(&b, &path, 64, 1).await.unwrap();
    assert!(
        report
            .checkpoint
            .checkpoint
            .loss
            .contains(&LossReason::Rotation)
    );
    std::fs::write(&path, first).unwrap();
    let report = c.reconcile_jsonl(&b, &path, 64, 1).await.unwrap();
    assert!(
        report
            .checkpoint
            .checkpoint
            .loss
            .contains(&LossReason::Truncation)
    );
    assert_eq!(report.deduplicated, 1);
    assert_eq!(
        c.text(
            &c.event(&b, &reference("b"), store.sequence().unwrap())
                .unwrap()
                .unwrap()
        )
        .unwrap(),
        "中文 tail"
    );
    store.close().await.unwrap();
}
