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
