use super::extraction_tests::{finished, runtime, scope, seed_source, storage_config};
use super::*;
use akasha_otsuge::domain::{Authority, Domain, Origin};
use akasha_otsuge::storage::Storage;
use sha2::{Digest, Sha256};

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

#[test]
#[ignore = "requires explicitly supplied authenticated native profiles and receipt path"]
fn native_single_grill_actual() {
    native_grill_actual("single_pi");
}

#[test]
#[ignore = "requires explicitly supplied authenticated native profiles and receipt path"]
fn native_mixed_grill_actual() {
    native_grill_actual("pi_codex");
}

fn native_grill_actual(case: &str) {
    let pi = PathBuf::from(std::env::var_os("AKASHA_NATIVE_PI_BINARY").unwrap());
    let pi_home = PathBuf::from(std::env::var_os("AKASHA_NATIVE_PI_HOME").unwrap());
    let codex = PathBuf::from(std::env::var_os("AKASHA_NATIVE_CODEX_BINARY").unwrap());
    let codex_home = PathBuf::from(std::env::var_os("AKASHA_NATIVE_CODEX_HOME").unwrap());
    let output = PathBuf::from(std::env::var_os("AKASHA_NATIVE_GRILL_RECEIPT").unwrap());
    assert!(!output.exists());
    let originals = [
        (
            "offline",
            "This project must keep keyword search available when embeddings are offline.",
        ),
        (
            "private",
            "Do not send private queries to an external reranker.",
        ),
        (
            "measurement",
            "No latency measurements or rollout approval have been recorded for the proposed offline mode.",
        ),
    ];
    let mut receipt = json!({"schema_version":1,"status":"RUNNING","originals":originals,"cases":[],"extra_model_profiles":0});
    {
        let mut daemon = Daemon::new();
        let sources: Vec<_> = originals
            .iter()
            .map(|(id, text)| seed_source(&daemon, id, text))
            .collect();
        let mixed = if case == "pi_codex" {
            format!(
                "[executors.codex]\nkind='codex'\nbinary={codex:?}\nnative_home={codex_home:?}\nargs=['-c','model_reasoning_effort=\"low\"']\n"
            )
        } else {
            String::new()
        };
        let reviewer = if case == "pi_codex" { "codex" } else { "pi" };
        daemon.configure(&format!("[access]\nread_roots=[{pi_home:?},{codex_home:?}]\n[grill]\ninterviewer='pi'\nexpert='pi'\nreviewer='{reviewer}'\nmax_questions=2\nmax_rounds=2\nmax_runner_starts=5\n[executors.pi]\nkind='pi'\nbinary={pi:?}\nnative_home={pi_home:?}\n{mixed}"));
        daemon.cli(&[
            "adapter",
            "install",
            "pi",
            "--binary",
            pi.to_str().unwrap(),
            "--agent-dir",
            pi_home.to_str().unwrap(),
        ]);
        daemon.start();
        let plan = json!({"scope":scope(),"sources":sources,"question":"Propose a scoped offline search decision and a dependent rollout check. Preserve missing latency evidence and lack of human rollout approval. Choose exactly two questions, with the rollout question depending on the search question. Do not invent measurements."});
        let path = daemon.root.join("grill.json");
        std::fs::write(&path, serde_json::to_vec(&plan).unwrap()).unwrap();
        let admitted = daemon.cli(&[
            "grill",
            "run",
            path.to_str().unwrap(),
            "--idempotency-key",
            case,
        ]);
        assert_eq!(admitted["data"]["id"], case);
        let workflow_ms = admitted["data"]["execution_budget_ms"].as_u64().unwrap();
        let end = Instant::now() + Duration::from_millis(workflow_ms + 60_000);
        let mut previous_steps = None;
        let job = loop {
            let job = daemon.cli(&["job", "show", case])["data"].clone();
            if previous_steps != job["result"]["steps"].as_u64() {
                previous_steps = job["result"]["steps"].as_u64();
                receipt["progress"] = json!({"case":case,"completed_phases":previous_steps,"phase_budget":5,"job":job});
                std::fs::write(&output, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
                eprintln!("actual-grill case={case} completed_phases={previous_steps:?}");
            }
            if matches!(
                job["state"].as_str(),
                Some("succeeded" | "failed" | "cancelled")
            ) {
                break job;
            }
            assert!(
                Instant::now() < end,
                "native discussion exceeded its original workflow envelope"
            );
            std::thread::sleep(Duration::from_millis(500));
        };
        let mut row = json!({"case":case,"job":job,"plan":plan,"status":"FAILED","discussion":[]});
        receipt["current"] = row.clone();
        std::fs::write(&output, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
        if job["state"] == "succeeded" {
            assert_eq!(
                submit(&daemon, case, &plan)["data"]["result"],
                job["result"]
            );
        }
        let mut discussion = vec![];
        for step in job["result"]["discussion"].as_array().unwrap() {
            let mut texts = json!({"stage":step["stage"],"generator":step["generator"]});
            for kind in ["prompt", "output"] {
                let span = &step[kind];
                texts[kind] = daemon.cli(&[
                    "show",
                    span["event_id"].as_str().unwrap(),
                    "--revision",
                    &span["revision"].to_string(),
                    "--range",
                    &span["start"].to_string(),
                    &span["end"].to_string(),
                ])["data"]
                    .clone();
            }
            discussion.push(texts);
        }
        row["discussion"] = json!(discussion);
        daemon.cli(&["daemon", "stop"]);
        daemon.wait();
        let memories = runtime().block_on(async {
            let store = Storage::open(&storage_config(&daemon)).await.unwrap();
            let pointer = &job["result"]["checkpoint"];
            if let Some(parts) = pointer["parts"].as_array() {
                let mut text = String::new();
                for part in parts {
                    let part: akasha_otsuge::storage::RevisionRef =
                        serde_json::from_value(part.clone()).unwrap();
                    text.push_str(
                        &store
                            .revision(&part.key, part.commit_seq)
                            .unwrap()
                            .unwrap()
                            .content,
                    );
                }
                assert_eq!(text.len() as u64, pointer["bytes"].as_u64().unwrap());
                assert_eq!(
                    Sha256::digest(text.as_bytes())
                        .iter()
                        .map(|b| format!("{b:02x}"))
                        .collect::<String>(),
                    pointer["sha256"]
                );
                row["checkpoint"] = serde_json::from_str(&text).unwrap();
                receipt["current"] = row.clone();
                std::fs::write(&output, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
            }
            let domain = Domain::new(&store);
            let mut memories = vec![];
            for candidate in job["result"]["candidates"].as_array().into_iter().flatten() {
                let memory = domain
                    .memory(candidate["memory_id"].as_str().unwrap())
                    .unwrap()
                    .unwrap();
                assert_eq!(memory.record.authority, Authority::Candidate);
                assert_eq!(memory.record.provenance.origin, Origin::AutoGrill);
                assert!(scope().contains(&memory.record.draft.scope));
                for span in &memory.record.draft.source_spans {
                    let original = domain.source(&span.event_id, span.revision).unwrap();
                    assert_eq!(original.content_sha256, span.content_sha256);
                    assert!(original.payload.text.get(span.start..span.end).is_some());
                }
                memories.push(serde_json::to_value(memory).unwrap());
            }
            store.close().await.unwrap();
            memories
        });
        assert_eq!(job["state"], "succeeded", "{job}");
        let questions = row["checkpoint"]["questions"].as_array().unwrap();
        assert_eq!(questions.len(), 2);
        assert!(questions[0]["depends_on"].as_array().unwrap().is_empty());
        assert_eq!(questions[1]["depends_on"], json!([questions[0]["id"]]));
        let steps = row["checkpoint"]["steps"].as_array().unwrap();
        for (role, runner) in [
            ("interviewer", "pi"),
            ("expert", "pi"),
            ("reviewer", reviewer),
        ] {
            assert!(steps.iter().any(|step| {
                step["stage"]["role"] == role
                    && step["generator"]["runner"] == format!("executor/{runner}")
            }));
        }
        assert!(job["result"]["usage"]["starts"].as_u64().unwrap() <= 5);
        assert!(
            !memories.is_empty(),
            "native discussion produced no sample candidates"
        );
        assert!(memories.iter().any(|m| {
            !m["record"]["draft"]["assumptions"]
                .as_array()
                .unwrap()
                .is_empty()
        }));
        row["memories"] = json!(memories);
        row["status"] = json!("PASS_REAL_NATIVE_GRILL");
        receipt["cases"].as_array_mut().unwrap().push(row);
        receipt.as_object_mut().unwrap().remove("current");
        std::fs::write(&output, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    }
    receipt["status"] = json!("PASS_REAL_NATIVE_GRILL");
    std::fs::write(&output, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
}
