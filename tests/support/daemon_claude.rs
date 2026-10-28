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

fn native_bridge(daemon: &Daemon, event: &Value) -> Value {
    let mut child = daemon
        .command()
        .args(["adapter", "claude-call"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(event.to_string().as_bytes())
        .unwrap();
    let output = child.wait_with_output().unwrap();
    let mut response: Value = serde_json::from_slice(if output.status.success() {
        &output.stdout
    } else {
        &output.stderr
    })
    .unwrap();
    response.as_object_mut().unwrap().remove("request_id");
    response
}

#[test]
fn native_sync_waits_for_the_pinned_event_after_async_transcript_flush() {
    use std::os::unix::fs::PermissionsExt;
    let mut daemon = Daemon::new();
    daemon.configure(&format!("[access]\nread_roots=[{:?}]\n", daemon.root));
    let binary = daemon.root.join("claude");
    std::fs::write(&binary, "#!/bin/sh\nprintf '2.1.311 (Claude Code)\\n'\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let path = daemon.root.join("not-flushed.jsonl");
    daemon.start();
    let registered = native_bridge(
        &daemon,
        &json!({"op":"register","binary":binary,
        "session_id":"11111111-1111-4111-8111-111111111111","transcript_path":path,"cwd":daemon.root}),
    );
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        std::fs::write(
            path,
            include_str!("../fixtures/claude/2.1.311/native-root-resume-compact.jsonl"),
        )
        .unwrap();
    });
    let sync = native_bridge(
        &daemon,
        &json!({"op":"sync","registration":registered["data"]["registration"],
        "tip":"a7c3b44a-26cd-4a68-9aff-3e98cce20493"}),
    );
    let completed = terminal(&daemon, sync["data"]["id"].as_str().unwrap());
    writer.join().unwrap();
    assert_eq!(completed["state"], "succeeded", "{completed}");
    assert!(
        completed["result"]["checkpoint"]["checkpoint"]["pending"].is_null(),
        "{completed}"
    );
    let missing = native_bridge(
        &daemon,
        &json!({"op":"sync","registration":registered["data"]["registration"],"tip":"not-emitted"}),
    );
    let id = missing["data"]["id"].as_str().unwrap();
    let end = Instant::now() + Duration::from_secs(5);
    let deadline = loop {
        let job = daemon.cli(&["job", "show", id])["data"].clone();
        if !job["result"].is_null() {
            assert_eq!(
                job["state"], "running",
                "missing tip must not report success: {job}"
            );
            break job["execution_deadline_ms"].clone();
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(10));
    };
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(
        daemon.cli(&["job", "show", id])["data"]["execution_deadline_ms"],
        deadline
    );
    daemon.cli(&["job", "cancel", id]);
    assert_eq!(terminal(&daemon, id)["state"], "cancelled");
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}

#[test]
fn native_child_registration_keeps_parent_identity_and_separate_history() {
    use std::os::unix::fs::PermissionsExt;
    let mut daemon = Daemon::new();
    daemon.configure(&format!("[access]\nread_roots=[{:?}]\n", daemon.root));
    let binary = daemon.root.join("claude");
    std::fs::write(&binary, "#!/bin/sh\nprintf '2.1.311 (Claude Code)\\n'\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let session = "11111111-1111-4111-8111-111111111111";
    let agent = "a2d42a7b5cfaf7eb6";
    let transcript = daemon.root.join(format!("{session}.jsonl"));
    std::fs::write(
        &transcript,
        include_str!("../fixtures/claude/2.1.311/native-root-resume-compact.jsonl"),
    )
    .unwrap();
    let children = daemon.root.join(session).join("subagents");
    std::fs::create_dir_all(&children).unwrap();
    let fixture = include_str!("../fixtures/claude/2.1.311/native-child.jsonl");
    std::fs::write(children.join(format!("agent-{agent}.jsonl")), fixture).unwrap();
    std::fs::write(children.join("agent-sibling.jsonl"), fixture).unwrap();
    std::fs::write(
        children.join("agent-grandchild.jsonl"),
        fixture.replace(agent, "grandchild"),
    )
    .unwrap();
    daemon.start();
    let root = native_bridge(
        &daemon,
        &json!({"op":"register","binary":binary,
        "session_id":session,"transcript_path":transcript,"cwd":daemon.root}),
    );
    let register = json!({"op":"register_child","parent_registration":root["data"]["registration"],"agent_id":agent});
    let child = native_bridge(&daemon, &register);
    assert!(
        child["data"]["registration"].is_string(),
        "native child registration required: {child}"
    );
    let call = json!({"op":"bind","registration":child["data"]["registration"],"session_id":session,
        "agent_id":agent,"turn_id":"child-turn","tool_call_id":"child-call","cursor":1,
        "tip":"6a101f82-6982-4ac8-b613-07085ed078ce"});
    let first = native_bridge(&daemon, &call);
    assert!(first["error"].is_null(), "{first}");
    let binding = &first["data"];
    assert_eq!(binding["caller"]["agent"], agent);
    assert_eq!(binding["caller"]["thread"], format!("{session}/{agent}"));
    assert_eq!(binding["proof"]["kind"], "registered_child");
    assert_eq!(binding["proof"]["parent"], session);
    assert_eq!(binding["capability"]["native_children"], true);
    for (key, value) in [
        ("agent_id", json!(null)),
        ("agent_id", json!("sibling")),
        ("session_id", json!("unrelated")),
        ("registration", root["data"]["registration"].clone()),
    ] {
        let mut wrong = call.clone();
        wrong[key] = value;
        assert_eq!(
            native_bridge(&daemon, &wrong)["error"]["code"],
            "CALLER_AMBIGUOUS"
        );
    }
    for invalid in ["main", "../escape", "a/b", ""] {
        let mut wrong = register.clone();
        wrong["agent_id"] = invalid.into();
        assert!(native_bridge(&daemon, &wrong)["error"].is_object());
    }
    let grandchild = native_bridge(
        &daemon,
        &json!({"op":"register_child",
        "parent_registration":child["data"]["registration"],"agent_id":"grandchild"}),
    );
    assert!(
        grandchild["data"]["registration"].is_string(),
        "{grandchild}"
    );
    let mut grandcall = call.clone();
    grandcall["registration"] = grandchild["data"]["registration"].clone();
    grandcall["agent_id"] = "grandchild".into();
    assert_eq!(
        native_bridge(&daemon, &grandcall)["data"]["proof"]["parent"],
        format!("{session}/{agent}")
    );
    let mut replacement = register.clone();
    replacement["parent_registration"] = grandchild["data"]["registration"].clone();
    assert_eq!(
        native_bridge(&daemon, &replacement)["error"]["code"],
        "BINDING_CONFLICT"
    );
    let refreshed = native_bridge(&daemon, &register);
    let mut replay = call.clone();
    replay["registration"] = refreshed["data"]["registration"].clone();
    assert_eq!(native_bridge(&daemon, &replay), first);
    let sibling = native_bridge(
        &daemon,
        &json!({"op":"register_child",
        "parent_registration":root["data"]["registration"],"agent_id":"sibling"}),
    );
    let mut wrong_source = call.clone();
    wrong_source["registration"] = sibling["data"]["registration"].clone();
    wrong_source["agent_id"] = "sibling".into();
    assert_eq!(
        native_bridge(&daemon, &wrong_source)["error"]["code"],
        "HISTORY_GAP"
    );
    let sync = native_bridge(
        &daemon,
        &json!({"op":"sync","registration":child["data"]["registration"],"tip":"d051798f-beae-456b-bf67-9cdd56ab8985"}),
    );
    let completed = terminal(&daemon, sync["data"]["id"].as_str().unwrap());
    assert_eq!(completed["state"], "succeeded", "{completed}");
    assert!(
        completed["result"]["checkpoint"]["checkpoint"]["pending"].is_null(),
        "{completed}"
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(native_bridge(&daemon, &replay), first);
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
            let source: SourceIdentity =
                serde_json::from_value(first["data"]["caller"].clone()).unwrap();
            let scope: Scope = serde_json::from_value(first["data"]["scope"].clone()).unwrap();
            let binding = CaptureBinding::from_source(
                source.clone(),
                scope,
                "native-file",
                CoverageLevel::VisibleText,
            )
            .unwrap();
            let page = Capture::new(&store)
                .lineage(
                    &binding,
                    &EventRef {
                        source: source.clone(),
                        event_id: "d051798f-beae-456b-bf67-9cdd56ab8985".into(),
                    },
                    store.sequence().unwrap(),
                    64,
                )
                .unwrap();
            assert!(page.missing.is_none() && page.next.is_none());
            assert_eq!(page.events.len(), 13);
            assert!(
                page.events
                    .iter()
                    .all(|e| e.event.reference.source == source)
            );
            assert!(page.events.last().unwrap().event.metadata.parent.is_none());
            store.close().await.unwrap();
        });
}

#[test]
fn native_caller_registration_pins_each_call_and_rejects_identity_replacement() {
    use std::os::unix::fs::PermissionsExt;
    let mut daemon = Daemon::new();
    daemon.configure(&format!("[access]\nread_roots=[{:?}]\n", daemon.root));
    let binary = daemon.root.join("claude");
    std::fs::write(&binary, "#!/bin/sh\nprintf '2.1.311 (Claude Code)\\n'\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let transcript = daemon.root.join("native.jsonl");
    std::fs::write(
        &transcript,
        include_str!("../fixtures/claude/2.1.311/native-root-resume-compact.jsonl"),
    )
    .unwrap();
    daemon.start();
    let registration = native_bridge(
        &daemon,
        &json!({"op":"register","binary":binary,
        "session_id":"11111111-1111-4111-8111-111111111111","transcript_path":transcript,"cwd":daemon.root}),
    );
    let token = registration["data"]["registration"]
        .as_str()
        .expect("native registration accepted");
    let call = json!({"op":"bind","registration":token,"session_id":"11111111-1111-4111-8111-111111111111",
        "agent_id":null,"turn_id":"turn-a","tool_call_id":"call-a","cursor":1,"tip":"a7c3b44a-26cd-4a68-9aff-3e98cce20493"});
    let first = native_bridge(&daemon, &call);
    assert!(first["error"].is_null(), "{first}");
    let binding = &first["data"];
    assert_eq!(binding["caller"]["agent"], "main");
    assert_eq!(binding["turn_id"], "turn-a");
    assert_eq!(binding["tool_call_id"], "call-a");
    assert_eq!(binding["requested_cursor"], 1);
    assert_eq!(binding["available_cursor"], 1);
    assert_eq!(binding["tip"]["event_id"], call["tip"]);
    assert_eq!(binding["capability"]["binding"], "exact_turn");
    assert!(!first.to_string().contains(token));
    assert_eq!(native_bridge(&daemon, &call), first);
    for (key, value) in [
        ("session_id", json!("other-session")),
        ("agent_id", json!("unknown-child")),
        ("turn_id", json!("other-turn")),
        ("tip", json!("other-tip")),
        ("cursor", json!(2)),
        ("registration", json!("forged")),
    ] {
        let mut changed = call.clone();
        changed[key] = value;
        let result = native_bridge(&daemon, &changed);
        assert!(
            result["error"]["code"].is_string(),
            "accepted replacement {key}: {result}"
        );
    }
    let mut next = call.clone();
    next["tool_call_id"] = "call-b".into();
    let second = native_bridge(&daemon, &next);
    assert!(second["error"].is_null(), "{second}");
    assert_ne!(second["data"]["invocation_id"], binding["invocation_id"]);
    let mut missing = next.clone();
    missing["tool_call_id"] = "call-missing".into();
    missing["tip"] = "not-yet-captured".into();
    assert_eq!(
        native_bridge(&daemon, &missing)["error"]["code"],
        "HISTORY_GAP"
    );
    let mut injected = call.clone();
    injected["scope"] = json!({"user":"other"});
    assert_eq!(
        native_bridge(&daemon, &injected)["error"]["code"],
        "INVALID_REQUEST"
    );
    daemon.configure("[access]\nread_roots=[]\n");
    daemon.cli(&["config", "reload"]);
    assert_eq!(
        native_bridge(&daemon, &call)["error"]["code"],
        "PATH_DENIED"
    );
    daemon.configure(&format!("[access]\nread_roots=[{:?}]\n", daemon.root));
    daemon.cli(&["config", "reload"]);
    assert_eq!(native_bridge(&daemon, &call), first);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(native_bridge(&daemon, &call), first);
    let refreshed = native_bridge(
        &daemon,
        &json!({"op":"register","binary":binary,
        "session_id":"11111111-1111-4111-8111-111111111111","transcript_path":transcript,"cwd":daemon.root}),
    );
    let mut replay = call.clone();
    replay["registration"] = refreshed["data"]["registration"].clone();
    assert_ne!(replay["registration"], call["registration"]);
    assert_eq!(native_bridge(&daemon, &replay), first);
    let synced = native_bridge(
        &daemon,
        &json!({"op":"sync","registration":replay["registration"],"tip":call["tip"]}),
    );
    let job_id = synced["data"]["id"]
        .as_str()
        .expect("durable native capture job accepted");
    let complete = terminal(&daemon, job_id);
    assert_eq!(complete["state"], "succeeded", "{complete}");
    assert!(complete["result"]["checkpoint"]["checkpoint"]["pending"].is_null());
    let lagged = daemon.root.join("lagged.jsonl");
    std::fs::write(&lagged, "").unwrap();
    let registered = native_bridge(
        &daemon,
        &json!({"op":"register","binary":binary,
        "session_id":"lagged-session","transcript_path":lagged,"cwd":daemon.root}),
    );
    let mut delayed_call = call.clone();
    delayed_call["registration"] = registered["data"]["registration"].clone();
    delayed_call["session_id"] = "lagged-session".into();
    let writer = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(500));
        std::fs::write(
            lagged,
            include_str!("../fixtures/claude/2.1.311/native-root-resume-compact.jsonl")
                .replace("11111111-1111-4111-8111-111111111111", "lagged-session"),
        )
        .unwrap();
    });
    let delayed = native_bridge(&daemon, &delayed_call);
    writer.join().unwrap();
    assert!(delayed["error"].is_null(), "{delayed}");
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}
