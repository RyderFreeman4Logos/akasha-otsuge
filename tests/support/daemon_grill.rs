use super::extraction_tests::{finished, runtime, scope, seed_source, storage_config};
use super::*;
use akasha_otsuge::domain::{Authority, Domain, Origin};
use akasha_otsuge::storage::Storage;

fn fixture(extra: &str) -> (Daemon, Value) {
    let mut daemon = Daemon::new();
    let source = seed_source(
        &daemon,
        "grill-input",
        "Only if offline, use keyword search.",
    );
    let home = daemon.root.join("native-home");
    std::fs::create_dir(&home).unwrap();
    let binary = daemon.root.join("codex");
    executable::write(
        &binary,
        include_str!("../fixtures/codex/0.164.0/runner_host.py"),
    );
    std::fs::write(daemon.root.join("scenario"), "grill").unwrap();
    std::fs::write(home.join("models_cache.json"), r#"{"models":[{"slug":"fixture","shell_type":"unified_exec","apply_patch_tool_type":"freeform","experimental_supported_tools":[],"tool_mode":"code_mode"}]}"#).unwrap();
    daemon.configure(&format!("[grill]\ninterviewer='one'\nexpert='one'\nreviewer='one'\n{extra}\n[access]\nread_roots=[{home:?}]\n[executors.one]\nkind='codex'\nbinary={binary:?}\nnative_home={home:?}\n"));
    daemon.start();
    (
        daemon,
        json!({"scope":scope(),"sources":[source],"question":"Which search strategy is justified?"}),
    )
}

fn submit(daemon: &Daemon, id: &str, plan: &Value) -> Value {
    let mut stream = daemon.connect();
    receive(&mut stream);
    send(
        &mut stream,
        &json!({"protocol_version":1,"request_id":id,
        "command":{"op":"grill_run","idempotency_key":id,"plan":plan}}),
    );
    receive(&mut stream)
}

#[test]
fn auto_grill_isolates_initial_roles_and_preserves_disagreements_and_sources() {
    let (mut daemon, plan) = fixture("");
    let accepted = submit(&daemon, "discussion", &plan);
    assert!(accepted.get("error").is_none(), "{accepted}");
    let job = finished(&daemon, "discussion");
    assert_eq!(job["state"], "succeeded", "{job}");
    assert_eq!(job["result"]["origin"], "auto_grill");
    assert_eq!(job["result"]["usage"]["starts"], 5);
    assert_eq!(job["result"]["discussion"].as_array().unwrap().len(), 5);
    assert_eq!(job["result"]["stop_reason"], "no_new_evidence");
    let candidates = job["result"]["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 4);
    assert_eq!(
        submit(&daemon, "discussion", &plan)["data"]["result"],
        job["result"]
    );
    let runs: Vec<Value> = std::fs::read_to_string(daemon.root.join("grill-runs.jsonl"))
        .unwrap()
        .lines()
        .map(|l| serde_json::from_str(l).unwrap())
        .collect();
    assert_eq!(runs.len(), 5);
    for initial in &runs[1..3] {
        assert_eq!(initial["peers"], json!([]));
    }
    for cross in &runs[3..] {
        assert_eq!(cross["peers"].as_array().unwrap().len(), 2);
    }
    assert_ne!(runs[1]["pid"], runs[2]["pid"]);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    runtime().block_on(async {
        let store = Storage::open(&storage_config(&daemon)).await.unwrap();
        let domain = Domain::new(&store);
        for candidate in candidates {
            let memory = domain
                .memory(candidate["memory_id"].as_str().unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(memory.record.authority, Authority::Candidate);
            assert_eq!(memory.record.provenance.origin, Origin::AutoGrill);
            assert!(!memory.record.draft.assumptions.is_empty());
            assert!(memory.record.draft.rationale.contains("Opposition:"));
            assert!(memory.record.draft.source_spans.iter().any(|s| {
                domain
                    .source(&s.event_id, s.revision)
                    .unwrap()
                    .provenance
                    .origin
                    == Origin::AutoGrill
            }));
            for span in memory.record.draft.source_spans {
                let source = domain.source(&span.event_id, span.revision).unwrap();
                assert_eq!(span.content_sha256, source.content_sha256);
                assert!(source.payload.text.get(span.start..span.end).is_some());
            }
        }
        store.close().await.unwrap();
    });
}

#[test]
fn auto_grill_stops_at_start_budget_and_explicit_resume_retains_parent() {
    let (daemon, plan) = fixture("max_runner_starts=2");
    assert!(submit(&daemon, "limited", &plan).get("error").is_none());
    let original = finished(&daemon, "limited");
    assert_eq!(
        original["result"]["stop_reason"], "runner_budget",
        "{original}"
    );
    assert_eq!(original["result"]["usage"]["starts"], 2);
    assert_eq!(
        original["result"]["candidates"].as_array().unwrap().len(),
        2
    );
    let resumed = daemon.cli(&[
        "grill",
        "resume",
        "limited",
        "--idempotency-key",
        "continued",
    ]);
    assert_eq!(resumed["data"]["id"], "continued");
    let continued = finished(&daemon, "continued");
    assert_eq!(continued["state"], "succeeded", "{continued}");
    assert_eq!(continued["grill_plan"]["resume_from"], "limited");
    assert_eq!(continued["result"]["usage"]["starts"], 2);
    assert_eq!(daemon.cli(&["job", "show", "limited"])["data"], original);
}

#[test]
fn auto_grill_round_token_and_forbidden_tool_stops_are_bounded() {
    for (name, extra, scenario, state, reason, starts) in [
        (
            "round",
            "max_rounds=1",
            "grill",
            "succeeded",
            "round_limit",
            3,
        ),
        (
            "tokens",
            "max_tokens=10",
            "grill",
            "succeeded",
            "token_budget",
            1,
        ),
        ("tool", "", "grill_denied", "failed", "TOOL_DENIED", 1),
    ] {
        let (daemon, plan) = fixture(extra);
        std::fs::write(daemon.root.join("scenario"), scenario).unwrap();
        assert!(submit(&daemon, name, &plan).get("error").is_none());
        let job = finished(&daemon, name);
        assert_eq!(job["state"], state, "{job}");
        assert_eq!(job["result"]["stop_reason"], reason, "{job}");
        assert_eq!(job["result"]["usage"]["starts"], starts);
        if name == "tokens" {
            assert_eq!(job["result"]["usage"]["tokens"], 12);
        }
    }
}

#[test]
fn auto_grill_cancel_stops_native_process_and_keeps_completed_discussion_sources() {
    let (daemon, plan) = fixture("");
    std::fs::write(daemon.root.join("scenario"), "grill_wait").unwrap();
    assert!(
        submit(&daemon, "cancelled-grill", &plan)
            .get("error")
            .is_none()
    );
    let marker = daemon.root.join("grill-wait.pid");
    let deadline = Instant::now() + Duration::from_secs(15);
    while !marker.exists() {
        assert!(Instant::now() < deadline);
        std::thread::sleep(Duration::from_millis(20));
    }
    let pid = std::fs::read_to_string(marker).unwrap();
    daemon.cli(&["job", "cancel", "cancelled-grill"]);
    let job = finished(&daemon, "cancelled-grill");
    assert_eq!(job["state"], "cancelled", "{job}");
    assert_eq!(job["result"]["discussion"].as_array().unwrap().len(), 1);
    assert!(!PathBuf::from(format!("/proc/{pid}")).exists());
    let source = &job["result"]["discussion"][0]["output"];
    assert!(
        daemon.cli(&[
            "show",
            source["event_id"].as_str().unwrap(),
            "--revision",
            &source["revision"].to_string(),
            "--range",
            "0",
            &source["end"].to_string()
        ])["data"]["output"]
            .as_str()
            .unwrap()
            .contains("q1")
    );
}
