use super::*;
use akasha_otsuge::{
    capture::{Capture, CaptureBinding, CoverageLevel, EventRef, SourceIdentity, Visibility},
    config::{Paths, resolve},
    domain::{Actor, Domain, Scope},
    storage::Storage,
};

fn terminal(daemon: &Daemon, id: &str) -> Value {
    let end = Instant::now() + Duration::from_secs(15);
    loop {
        let job = daemon.cli(&["job", "show", id])["data"].clone();
        if !matches!(job["state"].as_str(), Some("queued" | "running")) {
            return job;
        }
        assert!(Instant::now() < end, "Claude capture did not finish: {job}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn native_claude_backfill_preserves_visible_payloads_links_and_partial_tail() {
    let mut daemon = Daemon::new();
    daemon.configure(&format!("[access]\nread_roots=[{:?}]\n", daemon.root));
    let source = SourceIdentity {
        adapter: "claude_code".into(),
        profile: "test".into(),
        session: "session-a".into(),
        thread: "session-a".into(),
        agent: "main".into(),
    };
    let scope = Scope {
        user: format!("uid:{}", rustix::process::geteuid().as_raw()),
        project: None,
        branch_or_version: None,
        session: None,
    };
    let binding = daemon.root.join("binding.json");
    std::fs::write(
        &binding,
        json!({"source":source,"scope":scope,"stream":"native-file","coverage":"visible_text"})
            .to_string(),
    )
    .unwrap();
    let input = daemon.root.join("claude.jsonl");
    let mut bytes = Vec::new();
    for row in [
        json!({"type":"queue-operation","operation":"enqueue","timestamp":"2026-10-28T15:40:43.067Z","sessionId":"session-a"}),
        json!({"type":"user","uuid":"prompt-a","parentUuid":null,"sessionId":"session-a","version":"2.1.311","message":{"role":"user","content":"Keep the original 字 prompt"}}),
        json!({"type":"assistant","uuid":"response-a","parentUuid":"prompt-a","sessionId":"session-a","version":"2.1.311","message":{"role":"assistant","content":[{"type":"thinking","thinking":"PRIVATE_THINKING_SENTINEL","signature":"PRIVATE_SIGNATURE"},{"type":"text","text":"Visible answer"},{"type":"tool_use","id":"tool-a","name":"Read","input":{"file_path":"visible.txt"}}]}}),
        json!({"type":"user","uuid":"result-a","parentUuid":"response-a","sessionId":"session-a","version":"2.1.311","message":{"role":"user","content":[{"type":"tool_result","tool_use_id":"tool-a","content":"tool result 原文"}]}}),
    ] {
        serde_json::to_writer(&mut bytes, &row).unwrap();
        bytes.push(b'\n');
    }
    let final_row = json!({"type":"assistant","uuid":"final-a","parentUuid":"result-a","sessionId":"session-a","version":"2.1.311","message":{"role":"assistant","content":[{"type":"text","text":"Final answer"}]}}).to_string();
    bytes.extend_from_slice(&final_row.as_bytes()[..final_row.len() / 2]);
    std::fs::write(&input, &bytes).unwrap();
    daemon.start();
    let mut args = [
        "capture",
        "claude",
        binding.to_str().unwrap(),
        input.to_str().unwrap(),
        "--idempotency-key",
        "claude-partial",
    ];
    daemon.cli(&args);
    let first = terminal(&daemon, "claude-partial");
    assert_eq!(first["state"], "succeeded", "{first}");
    assert_eq!(
        first["result"]["checkpoint"]["checkpoint"]["pending"],
        "partial_tail"
    );
    let mut file = std::fs::OpenOptions::new()
        .append(true)
        .open(&input)
        .unwrap();
    file.write_all(&final_row.as_bytes()[final_row.len() / 2..])
        .unwrap();
    file.write_all(b"\n").unwrap();
    drop(file);
    args[5] = "claude-resume";
    daemon.cli(&args);
    let second = terminal(&daemon, "claude-resume");
    assert_eq!(second["state"], "succeeded", "{second}");
    assert!(second["result"]["checkpoint"]["checkpoint"]["pending"].is_null());
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let resolved = resolve(
                "",
                None,
                "",
                &Paths {
                    data_dir: daemon.root.join("data"),
                    socket_path: daemon.socket(),
                },
            )
            .unwrap();
            let store = Storage::open(&resolved).await.unwrap();
            let capture = Capture::new(&store);
            let bound = CaptureBinding::from_source(
                source.clone(),
                scope,
                "native-file",
                CoverageLevel::VisibleText,
            )
            .unwrap();
            for (id, text, visibility, parent) in [
                (
                    "prompt-a",
                    "Keep the original 字 prompt",
                    Visibility::User,
                    None,
                ),
                (
                    "response-a",
                    "Visible answer",
                    Visibility::Assistant,
                    Some("prompt-a"),
                ),
                (
                    "result-a",
                    "tool result 原文",
                    Visibility::Tool,
                    Some("response-a"),
                ),
                (
                    "final-a",
                    "Final answer",
                    Visibility::Assistant,
                    Some("result-a"),
                ),
            ] {
                let event = capture
                    .event(
                        &bound,
                        &EventRef {
                            source: source.clone(),
                            event_id: id.into(),
                        },
                        store.sequence().unwrap(),
                    )
                    .unwrap()
                    .unwrap();
                assert!(capture.text(&event).unwrap().contains(text));
                assert_eq!(event.event.metadata.visibility, visibility);
                assert_eq!(
                    event
                        .event
                        .metadata
                        .parent
                        .as_ref()
                        .map(|p| p.event_id.as_str()),
                    parent
                );
                let raw = String::from_utf8(capture.raw(&event).unwrap()).unwrap();
                assert!(!raw.contains("PRIVATE_THINKING"));
                assert!(!raw.contains("PRIVATE_SIGNATURE"));
                if id == "response-a" {
                    assert!(raw.contains("visible.txt"));
                }
                let normalized = &event.event.normalized[0];
                assert!(matches!(
                    Domain::new(&store)
                        .source(&normalized.event_id, normalized.revision)
                        .unwrap()
                        .provenance
                        .actor,
                    Actor::Unverified(_)
                ));
            }
            store.close().await.unwrap();
        });
}

#[test]
fn recorded_native_resume_and_compact_keep_original_lineage_and_session_authority() {
    recorded_native_compaction(false);
}

#[test]
fn recorded_native_compaction_refuses_missing_original_ancestor() {
    recorded_native_compaction(true);
}

fn recorded_native_compaction(missing_ancestor: bool) {
    let mut daemon = Daemon::new();
    daemon.configure(&format!("[access]\nread_roots=[{:?}]\n", daemon.root));
    let source = SourceIdentity {
        adapter: "claude_code".into(),
        profile: "test".into(),
        session: "11111111-1111-4111-8111-111111111111".into(),
        thread: "11111111-1111-4111-8111-111111111111".into(),
        agent: "main".into(),
    };
    let scope = Scope {
        user: format!("uid:{}", rustix::process::geteuid().as_raw()),
        project: None,
        branch_or_version: None,
        session: None,
    };
    let binding = daemon.root.join("binding.json");
    std::fs::write(
        &binding,
        json!({"source":source,"scope":scope,"stream":"native-file","coverage":"visible_text"})
            .to_string(),
    )
    .unwrap();
    let input = daemon.root.join("native.jsonl");
    let fixture = include_str!("../fixtures/claude/2.1.311/native-root-resume-compact.jsonl");
    let mut rows: Vec<Value> = fixture
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    if missing_ancestor {
        rows.iter_mut()
            .find(|row| row["subtype"] == "compact_boundary")
            .unwrap()["logicalParentUuid"] = "unknown-original".into();
    }
    std::fs::write(
        &input,
        rows.iter()
            .map(|row| format!("{row}\n"))
            .collect::<String>(),
    )
    .unwrap();
    daemon.start();
    daemon.cli(&[
        "capture",
        "claude",
        binding.to_str().unwrap(),
        input.to_str().unwrap(),
        "--idempotency-key",
        "native-recording",
    ]);
    let job = terminal(&daemon, "native-recording");
    assert_eq!(job["state"], "succeeded", "{job}");
    if missing_ancestor {
        assert_eq!(
            job["result"]["checkpoint"]["checkpoint"]["pending"],
            "invalid_frame"
        );
    } else {
        assert!(
            job["result"]["checkpoint"]["checkpoint"]["pending"].is_null(),
            "{job}"
        );
    }
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    if missing_ancestor {
        return;
    }
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let store = Storage::open(
                &resolve(
                    "",
                    None,
                    "",
                    &Paths {
                        data_dir: daemon.root.join("data"),
                        socket_path: daemon.socket(),
                    },
                )
                .unwrap(),
            )
            .await
            .unwrap();
            let capture = Capture::new(&store);
            let bound = CaptureBinding::from_source(
                source.clone(),
                scope,
                "native-file",
                CoverageLevel::VisibleText,
            )
            .unwrap();
            let reference = |id: &str| EventRef {
                source: source.clone(),
                event_id: id.into(),
            };
            let compact = rows
                .iter()
                .find(|row| row["subtype"] == "compact_boundary")
                .unwrap();
            let event = capture
                .event(
                    &bound,
                    &reference(compact["uuid"].as_str().unwrap()),
                    store.sequence().unwrap(),
                )
                .unwrap()
                .unwrap();
            assert_eq!(
                event.event.metadata.parent,
                Some(reference(compact["logicalParentUuid"].as_str().unwrap()))
            );
            let akasha_otsuge::capture::EventKind::Compaction {
                first,
                last,
                originals,
            } = &event.event.metadata.kind
            else {
                panic!("compaction relation missing")
            };
            let first_prompt = rows
                .iter()
                .find(|row| row["message"]["content"] == "PRIVATE_CALLER_INPUT start")
                .unwrap();
            assert_eq!(
                first.as_ref(),
                &reference(first_prompt["uuid"].as_str().unwrap())
            );
            assert_eq!(last.as_ref(), event.event.metadata.parent.as_ref().unwrap());
            assert!(originals.contains(first) && originals.contains(last));
            let tip = rows
                .iter()
                .rev()
                .find_map(|row| row["uuid"].as_str())
                .unwrap();
            let page = capture
                .lineage(&bound, &reference(tip), store.sequence().unwrap(), 64)
                .unwrap();
            assert!(page.missing.is_none() && page.next.is_none());
            let texts: Vec<_> = page
                .events
                .iter()
                .map(|e| capture.text(e).unwrap())
                .collect();
            assert!(
                texts
                    .iter()
                    .any(|text| text == "PRIVATE_CALLER_INPUT start")
            );
            assert!(
                texts
                    .iter()
                    .any(|text| text == "PRIVATE_CALLER_INPUT resume")
            );
            let summary = page
                .events
                .iter()
                .find(|e| capture.text(e).unwrap() == "Synthetic caller summary retained.")
                .unwrap();
            assert_eq!(summary.event.metadata.visibility, Visibility::Session);
            let raw: Value = serde_json::from_slice(&capture.raw(summary).unwrap()).unwrap();
            assert_eq!(raw["isCompactSummary"], true);
            store.close().await.unwrap();
        });
}
