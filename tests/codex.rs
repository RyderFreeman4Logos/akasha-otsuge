use akasha_otsuge::{
    adapter::codex::{TRANSCRIPT_STREAM, reconcile_transcript},
    capture::*,
    config::{Paths, resolve},
    domain::Scope,
    storage::Storage,
};
use std::{
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

#[path = "support/executable.rs"]
mod executable;

#[path = "support/codex_caller.rs"]
mod caller;
#[path = "support/codex_runner.rs"]
mod runner;

#[tokio::test]
async fn version_probe_reports_only_observed_capabilities_and_bounds_native_output() {
    use akasha_otsuge::adapter::{ErrorCode, SelfTest, codex::probe};
    let fixture = Fixture::new();
    let binary = fixture.0.join("codex");
    crate::executable::write(&binary, "#!/bin/sh\nprintf 'codex-cli 0.164.0\\n'\n");
    let report = probe(&binary).await.unwrap();
    assert_eq!(report.version.as_deref(), Some("0.164.0"));
    assert_eq!(report.executable, Some(binary.canonicalize().unwrap()));
    assert_eq!(report.self_test, SelfTest::NotRun);
    assert!(!report.native_children);
    assert!(report.capabilities.is_empty());
    let malformed = fixture.0.join("malformed");
    crate::executable::write(&malformed, "#!/bin/sh\nprintf 'fixture 0.164.0\\n'\n");
    assert_eq!(
        probe(&malformed).await.unwrap_err().code,
        ErrorCode::AdapterUnsupported
    );
    let oversized = fixture.0.join("oversized");
    crate::executable::write(&oversized, "#!/bin/sh\nhead -c 4097 /dev/zero\n");
    assert_eq!(
        probe(&oversized).await.unwrap_err().code,
        ErrorCode::OutputLimit
    );
    assert_eq!(
        probe(&fixture.0.join("absent")).await.unwrap_err().code,
        ErrorCode::ExecutorUnavailable
    );
}

const ROOT: &str = include_str!("fixtures/codex/0.164.0/root.jsonl");
const FORK: &str = include_str!("fixtures/codex/0.164.0/fork.jsonl");

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "akasha-codex-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&root).unwrap();
        Self(root)
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
fn source(fork: bool) -> SourceIdentity {
    let session = if fork {
        "01a184ae-8fce-7a31-852f-82207da91a95"
    } else {
        "01a184ae-8f51-7ec2-847b-8f9f907887e4"
    };
    SourceIdentity {
        adapter: "codex".into(),
        profile: "default".into(),
        session: session.into(),
        thread: session.into(),
        agent: "main".into(),
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
fn reference(fork: bool, ordinal: u64) -> EventRef {
    EventRef {
        source: source(fork),
        event_id: format!("row:{ordinal}"),
    }
}

#[tokio::test]
async fn native_history_replays_once_and_keeps_compaction_and_fork_ancestors() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let capture = Capture::new(&store);
    for (fork, text, count) in [(false, ROOT, 31), (true, FORK, 13)] {
        let path = fixture
            .0
            .join(if fork { "fork.jsonl" } else { "root.jsonl" });
        std::fs::write(&path, text).unwrap();
        let report = reconcile_transcript(&store, &source(fork), scope(), &path, 64, 1)
            .await
            .unwrap();
        assert_eq!(report.processed, count);
        assert_eq!(report.checkpoint.checkpoint.pending, None);
        assert_eq!(
            report.checkpoint.checkpoint.coverage,
            CoverageLevel::VisibleText
        );
        let replay = reconcile_transcript(&store, &source(fork), scope(), &path, 64, 1)
            .await
            .unwrap();
        assert_eq!(replay.processed, 0);
    }
    let page = capture
        .lineage(
            &binding(true),
            &reference(true, 43),
            store.sequence().unwrap(),
            64,
        )
        .unwrap();
    assert_eq!(page.events.len(), 44);
    assert_eq!(page.missing, None);
    let fork = page
        .events
        .iter()
        .find(|v| v.event.reference == reference(true, 31))
        .unwrap();
    assert_eq!(fork.event.metadata.kind, EventKind::Fork);
    assert_eq!(fork.event.metadata.parent, Some(reference(false, 30)));
    assert_eq!(fork.event.metadata.fork_cutoff, Some(reference(false, 30)));
    let compact = capture
        .event(
            &binding(false),
            &reference(false, 24),
            store.sequence().unwrap(),
        )
        .unwrap()
        .unwrap();
    match compact.event.metadata.kind {
        EventKind::Compaction { first, last, .. } => {
            assert_eq!(*first, reference(false, 0));
            assert_eq!(*last, reference(false, 23));
        }
        _ => panic!("native compaction must retain its original range"),
    }
    let texts = page
        .events
        .iter()
        .map(|event| capture.text(event).unwrap())
        .collect::<Vec<_>>();
    assert_eq!(
        texts
            .iter()
            .filter(|text| text.contains("describe the authorized context"))
            .count(),
        1
    );
    assert_eq!(
        texts
            .iter()
            .filter(|text| text.as_str() == "synthetic-context")
            .count(),
        1
    );
    assert!(texts.iter().any(|text| text.contains("context_describe")));
    assert!(
        !texts
            .iter()
            .any(|text| text.contains("native harness instructions"))
    );
    let original = capture
        .event(
            &binding(false),
            &reference(false, 7),
            store.sequence().unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(original.event.metadata.visibility, Visibility::User);
    assert!(
        String::from_utf8(capture.raw(&original).unwrap())
            .unwrap()
            .contains("msg_01a184ae-8f85-77a1-bd70-2b1bb9f77969")
    );
}

#[tokio::test]
async fn visible_user_attachments_and_public_summary_survive_without_private_payloads() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let mut rows: Vec<serde_json::Value> = ROOT
        .lines()
        .take(8)
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    rows[7]["payload"]["content"].as_array_mut().unwrap().push(serde_json::json!({"type":"input_image","image_url":"data:image/png;base64,PRIVATE_BINARY"}));
    rows[7]["payload"]["internal_chat_message_metadata_passthrough"]["content_item_kinds"] =
        serde_json::json!(["user.text", "user.image"]);
    rows.push(serde_json::json!({"ordinal":8,"type":"response_item","timestamp":"2026-10-28T22:43:15Z",
        "payload":{"type":"reasoning","id":"reasoning-1","summary":[{"type":"summary_text","text":"Public progress summary"}],"content":[{"type":"reasoning_text","text":"PRIVATE_REASONING"}],"encrypted_content":"PRIVATE_ENCRYPTED"}}));
    let path = fixture.0.join("visible.jsonl");
    let bytes = rows
        .iter()
        .map(|row| format!("{}\n", serde_json::to_string(row).unwrap()))
        .collect::<String>();
    std::fs::write(&path, bytes).unwrap();
    let report = reconcile_transcript(&store, &source(false), scope(), &path, 64, 1)
        .await
        .unwrap();
    assert_eq!(report.processed, 9);
    let capture = Capture::new(&store);
    let message = capture
        .event(
            &binding(false),
            &reference(false, 7),
            store.sequence().unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(message.event.metadata.visibility, Visibility::User);
    let text = capture.text(&message).unwrap();
    assert!(text.contains("describe the authorized context"));
    assert!(text.contains("[binary attachment unavailable]"));
    let summary = capture
        .event(
            &binding(false),
            &reference(false, 8),
            store.sequence().unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(capture.text(&summary).unwrap(), "Public progress summary");
    for event in [message, summary] {
        let raw = String::from_utf8(capture.raw(&event).unwrap()).unwrap();
        assert!(!raw.contains("PRIVATE_"));
    }
}

#[tokio::test]
async fn partial_backfill_reopens_and_rotation_replays_the_same_native_events() {
    let fixture = Fixture::new();
    let path = fixture.0.join("native.jsonl");
    let split = ROOT.rfind('\n').unwrap() - 5;
    std::fs::write(&path, &ROOT[..split]).unwrap();
    let store = fixture.open().await;
    let report = reconcile_transcript(&store, &source(false), scope(), &path, 64, 1)
        .await
        .unwrap();
    assert_eq!(report.processed, 30);
    assert_eq!(
        report.checkpoint.checkpoint.pending,
        Some(PendingReason::PartialTail)
    );
    store.close().await.unwrap();
    drop(store);
    use std::io::Write;
    std::fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .unwrap()
        .write_all(&ROOT.as_bytes()[split..])
        .unwrap();
    let store = fixture.open().await;
    let report = reconcile_transcript(&store, &source(false), scope(), &path, 64, 1)
        .await
        .unwrap();
    assert_eq!(report.processed, 1);
    assert_eq!(report.checkpoint.checkpoint.pending, None);
    std::fs::write(fixture.0.join("replacement"), ROOT).unwrap();
    std::fs::rename(fixture.0.join("replacement"), &path).unwrap();
    let replay = reconcile_transcript(&store, &source(false), scope(), &path, 64, 1)
        .await
        .unwrap();
    assert_eq!(replay.processed, 31);
    assert_eq!(replay.deduplicated, 31);
    assert_eq!(replay.checkpoint.checkpoint.loss, [LossReason::Rotation]);
}

#[tokio::test]
async fn unsupported_versions_wrong_sessions_missing_ordinals_and_unknown_rows_fail_closed() {
    for (label, mutation, expected) in [
        ("version", "version", Some("ADAPTER_UNSUPPORTED")),
        ("session", "session", Some("BINDING_CONFLICT")),
        ("gap", "ordinal", None),
        ("unknown", "type", None),
    ] {
        let fixture = Fixture::new();
        let store = fixture.open().await;
        let mut rows: Vec<serde_json::Value> = ROOT
            .lines()
            .take(2)
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        match mutation {
            "version" => rows[0]["payload"]["cli_version"] = "0.999.0".into(),
            "session" => rows[0]["payload"]["id"] = "other-thread".into(),
            "ordinal" => rows[1]["ordinal"] = 3.into(),
            _ => rows[1]["type"] = "future_native_record".into(),
        }
        let path = fixture.0.join(label);
        std::fs::write(
            &path,
            rows.iter()
                .map(|row| format!("{row}\n"))
                .collect::<String>(),
        )
        .unwrap();
        let report = reconcile_transcript(&store, &source(false), scope(), &path, 64, 1).await;
        if let Some(code) = expected {
            assert_eq!(report.unwrap_err().code, code);
        } else {
            let report = report.unwrap();
            assert_eq!(report.processed, 1);
            assert_eq!(
                report.checkpoint.checkpoint.pending,
                Some(PendingReason::InvalidFrame)
            );
        }
    }
}

#[tokio::test]
async fn native_process_resume_appends_without_expanding_an_existing_fork_cutoff() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let mut sources = Vec::new();
    for (name, text, count) in [
        (
            "root",
            include_str!("fixtures/codex/0.164.0/restart-root.jsonl"),
            41,
        ),
        (
            "fork",
            include_str!("fixtures/codex/0.164.0/restart-fork.jsonl"),
            13,
        ),
    ] {
        let meta: serde_json::Value = serde_json::from_str(text.lines().next().unwrap()).unwrap();
        let mut identity = source(false);
        identity.session = meta["payload"]["session_id"].as_str().unwrap().into();
        identity.thread = identity.session.clone();
        let path = fixture.0.join(name);
        std::fs::write(&path, text).unwrap();
        let report = reconcile_transcript(&store, &identity, scope(), &path, 64, 1)
            .await
            .unwrap();
        assert_eq!(report.processed, count);
        assert_eq!(report.checkpoint.checkpoint.pending, None);
        sources.push(identity);
    }
    let capture = Capture::new(&store);
    let root_binding = CaptureBinding::from_source(
        sources[0].clone(),
        scope(),
        TRANSCRIPT_STREAM,
        CoverageLevel::VisibleText,
    )
    .unwrap();
    let fork_binding = CaptureBinding::from_source(
        sources[1].clone(),
        scope(),
        TRANSCRIPT_STREAM,
        CoverageLevel::VisibleText,
    )
    .unwrap();
    let resumed = capture
        .event(
            &root_binding,
            &EventRef {
                source: sources[0].clone(),
                event_id: "row:34".into(),
            },
            store.sequence().unwrap(),
        )
        .unwrap()
        .unwrap();
    assert_eq!(
        capture.text(&resumed).unwrap(),
        "synthetic after process restart"
    );
    let fork = capture
        .lineage(
            &fork_binding,
            &EventRef {
                source: sources[1].clone(),
                event_id: "row:43".into(),
            },
            store.sequence().unwrap(),
            64,
        )
        .unwrap();
    assert_eq!(fork.events.len(), 44);
    assert_eq!(fork.missing, None);
    assert!(
        fork.events
            .iter()
            .all(|event| event.event.reference != resumed.event.reference)
    );
    assert!(!fork.events.iter().any(|event| {
        capture
            .text(event)
            .unwrap()
            .contains("synthetic after process restart")
    }));
    store.close().await.unwrap();
}
