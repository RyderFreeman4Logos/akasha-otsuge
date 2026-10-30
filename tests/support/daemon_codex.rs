use super::*;

fn native(daemon: &Daemon, event: Value) -> Value {
    let mut stream = daemon.connect();
    assert_eq!(receive(&mut stream)["protocol_version"], 1);
    send(
        &mut stream,
        &json!({"protocol_version":1,"request_id":"native-codex","command":{"op":"codex","event":event}}),
    );
    receive(&mut stream)
}

fn consult_fixture(scenario: &str) -> (Daemon, Value, Value) {
    consult_fixture_with(scenario, |_| String::new())
}

fn consult_fixture_with(
    scenario: &str,
    settings: impl FnOnce(&std::path::Path) -> String,
) -> (Daemon, Value, Value) {
    let mut daemon = Daemon::new();
    let workspace = daemon.root.join("workspace");
    let home = daemon.root.join("native-home");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&home).unwrap();
    let concurrency = match scenario {
        "delegate_tree" => 4,
        "delegate_failure" | "delegate_parents" | "delegate_cancel" => 2,
        _ => 1,
    };
    let depth = if scenario == "delegate_tree" { 2 } else { 1 };
    daemon.configure(&format!("[access]\nread_roots=[{workspace:?},{home:?}]\n[runtime]\nmax_concurrent_jobs={concurrency}\n[consult]\nmax_delegation_depth={depth}\n{}", settings(&daemon.root)));
    let binary = daemon.root.join("codex");
    let host = include_str!("../fixtures/codex/0.164.0/runner_host.py")
        .replace("scenario = (root / 'scenario').read_text()", "scenario = (root / 'scenario').read_text()\nwith (root / 'starts').open('a') as f: f.write('start\\n')\n(root / 'runner.pid').write_text(str(os.getpid()))");
    crate::executable::write(&binary, &host);
    std::fs::write(daemon.root.join("scenario"), scenario).unwrap();
    std::fs::write(home.join("models_cache.json"), r#"{"models":[{"slug":"fixture","shell_type":"unified_exec","apply_patch_tool_type":"freeform","experimental_supported_tools":[],"tool_mode":"code_mode"}]}"#).unwrap();
    let transcript = workspace.join("native.jsonl");
    std::fs::write(
        &transcript,
        include_str!("../fixtures/codex/0.164.0/exec-root.jsonl")
            .replace("/fixture/workspace", workspace.to_str().unwrap()),
    )
    .unwrap();
    let hook: Value = serde_json::from_str(include_str!(
        "../fixtures/codex/0.164.0/exec-pre-tool-use.json"
    ))
    .unwrap();
    if scenario == "research" {
        super::retrieval_tests::seed_records_with(
            &daemon,
            &[
                (
                    "offline",
                    None,
                    "离线时仍支持关键词检索 offline keyword search",
                ),
                ("weather", None, "Tomorrow the weather is sunny"),
                ("private", Some("beta"), "离线私有检索 private search"),
            ],
            1,
            |draft| {
                draft.conditions = vec!["Only for the original local environment.".into()];
                draft.assumptions = vec!["The local index is available.".into()];
                draft.valid_from_ms = Some(1);
            },
        );
    }
    if matches!(scenario, "research" | "code_policy") {
        std::fs::write(workspace.join("notes.rs"), "memory needle\n").unwrap();
    }
    daemon.start();
    let registered = native(
        &daemon,
        json!({"op":"register","binary":binary,"session_id":hook["session_id"],"transcript_path":transcript,"cwd":workspace,"native_home":home}),
    );
    assert!(registered.get("error").is_none(), "{registered}");
    let token = registered["data"]["registration"].as_str().unwrap();
    sync_consult_fixture(&daemon, token);
    let call = json!({"session_id":hook["session_id"],"agent_id":null,"turn_id":hook["turn_id"],"tool_call_id":hook["tool_use_id"],"cwd":workspace});
    let bound = native(
        &daemon,
        json!({"op":"bind","registration":token,"call":call}),
    );
    assert!(bound.get("error").is_none(), "{bound}");
    let event = json!({"op":"consult","registration":token,"call":call,"args":{"hint":null}});
    (daemon, event, bound)
}

fn sync_consult_fixture(daemon: &Daemon, token: &str) {
    sync_fixture_until(daemon, token, 18);
}

fn sync_fixture_until(daemon: &Daemon, token: &str, ordinal: u64) {
    let sync = native(
        daemon,
        json!({"op":"sync","registration":token,"until_ordinal":ordinal}),
    );
    assert!(sync.get("error").is_none(), "{sync}");
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        let job =
            daemon.cli(&["job", "show", sync["data"]["id"].as_str().unwrap()])["data"].clone();
        if !matches!(job["state"].as_str(), Some("queued" | "running")) {
            assert_eq!(job["state"], "succeeded", "{job}");
            break;
        }
        assert!(
            Instant::now() < end,
            "fixture capture did not finish: {job}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

fn named_routes(root: &std::path::Path, fallback: bool, failing: bool) -> String {
    let binary = root.join(if failing { "failed" } else { "codex" });
    let second = root.join("codex");
    format!(
        "executor='primary'\n[executors.primary]\nkind='codex'\nbinary={binary:?}\nfallback={}\n[executors.second]\nkind='codex'\nbinary={second:?}\n",
        if fallback { "['second']" } else { "[]" }
    )
}

fn named_terminal(daemon: &Daemon, event: Value) -> Value {
    let submitted = native(daemon, event);
    let id = submitted["data"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{submitted}"));
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        let job = daemon.cli(&["job", "show", id])["data"].clone();
        if !matches!(job["state"].as_str(), Some("queued" | "running")) {
            return job;
        }
        assert!(Instant::now() < end, "named executor did not finish: {job}");
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn consult_named_executor_runs_the_pinned_profile() {
    let (daemon, event, bound) =
        consult_fixture_with("success", |root| named_routes(root, false, false));
    let job = named_terminal(&daemon, event);
    assert_eq!(job["state"], "succeeded", "{job}");
    assert_eq!(job["run_trace"]["runs"][0]["executor_profile"], "primary");
    assert_eq!(
        job["run_trace"]["runs"][0]["origin"],
        bound["data"]["invocation_id"]
    );
    assert_eq!(job["run_trace"]["usage"]["starts"], 1);
}

#[test]
fn consult_named_fallback_is_explicit_and_shares_origin_budget() {
    for fallback in [false, true] {
        let (daemon, event, bound) =
            consult_fixture_with("success", |root| named_routes(root, fallback, true));
        let host = std::fs::read_to_string(daemon.root.join("codex"))
            .unwrap()
            .replace(
                "scenario = (root / 'scenario').read_text()",
                "scenario = 'auth'",
            );
        super::executable::write(&daemon.root.join("failed"), &host);
        let job = named_terminal(&daemon, event);
        assert_eq!(
            job["state"],
            if fallback { "succeeded" } else { "failed" },
            "{job}"
        );
        let runs = job["run_trace"]["runs"].as_array().unwrap();
        assert_eq!(runs.len(), if fallback { 2 } else { 1 });
        assert_eq!(runs[0]["error"], "AUTH_REQUIRED");
        assert!(
            runs.iter()
                .all(|r| r["origin"] == bound["data"]["invocation_id"]
                    && r["parent"].is_null()
                    && r["depth"] == 0)
        );
        assert_eq!(job["run_trace"]["usage"]["starts"], runs.len());
        assert_eq!(
            std::fs::read_to_string(daemon.root.join("starts"))
                .unwrap()
                .lines()
                .count(),
            runs.len()
        );
        if fallback {
            assert_eq!(runs[1]["executor_profile"], "second");
        }
    }
}

#[test]
fn consult_named_profile_keeps_generation_but_obeys_live_disable() {
    for disable in [false, true] {
        let (daemon, event, _) =
            consult_fixture_with("success", |root| named_routes(root, false, false));
        let root = &daemon.root;
        daemon.configure(&format!("[access]\nread_roots=[{:?},{:?}]\n[executors.primary]\nkind='codex'\nbinary={:?}\nenabled={}\n", root.join("workspace"), root.join("native-home"), root.join("absent"), !disable));
        daemon.cli(&["config", "reload"]);
        let job = named_terminal(&daemon, event);
        assert_eq!(job["config_generation"], 1);
        assert_eq!(
            job["state"],
            if disable { "failed" } else { "succeeded" },
            "{job}"
        );
        if disable {
            assert_eq!(job["error"], "EXECUTOR_UNAVAILABLE");
            assert!(!root.join("starts").exists());
        }
    }
}

#[test]
fn consult_named_fallback_stops_at_shared_start_limit() {
    let (daemon, event, _) = consult_fixture_with("success", |root| {
        format!(
            "max_runner_starts=2\nexecutor='primary'\n[executors.primary]\nkind='codex'\nbinary={0:?}\nfallback=['second','third']\n[executors.second]\nkind='codex'\nbinary={0:?}\nfallback=['third']\n[executors.third]\nkind='codex'\nbinary={1:?}\n",
            root.join("failed"),
            root.join("codex")
        )
    });
    let host = std::fs::read_to_string(daemon.root.join("codex"))
        .unwrap()
        .replace(
            "scenario = (root / 'scenario').read_text()",
            "scenario = 'auth'",
        );
    super::executable::write(&daemon.root.join("failed"), &host);
    let job = named_terminal(&daemon, event);
    assert_eq!(job["state"], "failed", "{job}");
    assert_eq!(job["error"], "BUDGET_EXHAUSTED", "{job}");
    assert_eq!(job["run_trace"]["usage"]["starts"], 2);
    assert_eq!(job["run_trace"]["runs"].as_array().unwrap().len(), 2);
    assert_eq!(
        std::fs::read_to_string(daemon.root.join("starts"))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[test]
fn consult_named_fallback_never_bypasses_source_authority() {
    let (daemon, event, _) =
        consult_fixture_with("conclusion_forged", |root| named_routes(root, true, false));
    let job = named_terminal(&daemon, event);
    assert_eq!(job["error"], "READ_DENIED", "{job}");
    assert_eq!(job["run_trace"]["usage"]["starts"], 1);
    assert_eq!(job["run_trace"]["runs"].as_array().unwrap().len(), 1);
}

#[test]
fn consult_named_delegates_inherit_the_active_executor() {
    let (daemon, event, _) =
        consult_fixture_with("delegate_failure", |root| named_routes(root, false, false));
    let job = named_terminal(&daemon, event);
    assert_eq!(job["state"], "succeeded", "{job}");
    let runs = job["run_trace"]["runs"].as_array().unwrap();
    assert_eq!(runs.len(), 3);
    assert!(runs.iter().all(|r| r["executor_profile"] == "primary"));
    assert!(
        runs[1..]
            .iter()
            .all(|r| r["parent"] == runs[0]["id"] && r["depth"] == 1)
    );
    assert_eq!(job["run_trace"]["usage"]["starts"], 3);
}

#[test]
fn consult_named_home_is_independent_of_the_caller_profile() {
    let (daemon, event, _) = consult_fixture_with("success", |root| {
        format!(
            "executor='primary'\n[executors.primary]\nkind='codex'\nbinary={:?}\nnative_home={:?}\n",
            root.join("codex"),
            root.join("native-home/worker")
        )
    });
    let caller = daemon.root.join("native-home");
    let worker = caller.join("worker");
    std::fs::create_dir(&worker).unwrap();
    std::fs::rename(
        caller.join("models_cache.json"),
        worker.join("models_cache.json"),
    )
    .unwrap();
    let job = named_terminal(&daemon, event);
    assert_eq!(job["state"], "succeeded", "{job}");
    assert_eq!(
        job["consult_plan"]["executors"]["primary"]["home"],
        worker.to_str().unwrap()
    );
}

#[test]
fn consult_named_model_absent_from_native_catalog_never_starts_a_worker() {
    let (daemon, event, _) = consult_fixture_with("success", |root| {
        format!(
            "executor='primary'\n[executors.primary]\nkind='codex'\nbinary={:?}\nmodel='chosen'\n[models.chosen]\nkind='text'\nendpoint='http://127.0.0.1:9/v1'\nmodel='unknown-model'\nlocal=true\n",
            root.join("codex")
        )
    });
    let job = named_terminal(&daemon, event);
    assert_eq!(job["error"], "ADAPTER_UNSUPPORTED", "{job}");
    assert!(!daemon.root.join("starts").exists());
}

#[test]
fn consult_named_model_and_safe_options_reach_the_native_process() {
    let (daemon, event, _) = consult_fixture_with("success", |root| {
        format!(
            "executor='primary'\n[executors.primary]\nkind='codex'\nbinary={:?}\nmodel='chosen'\nargs=['-c', 'model_reasoning_effort=\"low\"']\n[models.chosen]\nkind='text'\nendpoint='http://127.0.0.1:9/v1'\nmodel='fixture'\nlocal=true\n",
            root.join("codex")
        )
    });
    let binary = daemon.root.join("codex");
    let host = std::fs::read_to_string(&binary).unwrap().replace("root = pathlib.Path(__file__).parent", "root = pathlib.Path(__file__).parent\n(root / 'native-options.json').write_text(json.dumps({'args':sys.argv[1:]}))");
    super::executable::write(&binary, &host);
    let job = named_terminal(&daemon, event);
    assert_eq!(job["state"], "succeeded", "{job}");
    let options: Value =
        serde_json::from_slice(&std::fs::read(daemon.root.join("native-options.json")).unwrap())
            .unwrap();
    let args = options["args"].as_array().unwrap();
    for expected in [
        "model_reasoning_effort=\"low\"",
        "model=\"fixture\"",
        "model_provider=\"akasha_executor\"",
        "model_providers.akasha_executor.base_url=\"http://127.0.0.1:9/v1\"",
        "model_providers.akasha_executor.wire_api=\"responses\"",
        "model_providers.akasha_executor.requires_openai_auth=false",
    ] {
        assert!(
            args.iter().any(|arg| arg == expected),
            "missing {expected}: {options}"
        );
    }
    assert_eq!(job["run_trace"]["runs"][0]["executor_profile"], "primary");
}

#[test]
fn consult_delegate_two_parents_refuse_without_deadlock() {
    let (daemon, event, _) = consult_fixture("delegate_parents");
    let workspace = daemon.root.join("workspace");
    let second_session = "01a184c7-1cbf-7093-af67-23202f7dceec";
    let transcript = workspace.join("second.jsonl");
    let text = std::fs::read_to_string(workspace.join("native.jsonl"))
        .unwrap()
        .replace(
            event["call"]["session_id"].as_str().unwrap(),
            second_session,
        );
    std::fs::write(&transcript, text).unwrap();
    let registered = native(
        &daemon,
        json!({"op":"register","binary":daemon.root.join("codex"),"session_id":second_session,"transcript_path":transcript,"cwd":workspace,"native_home":daemon.root.join("native-home")}),
    );
    assert!(registered.get("error").is_none(), "{registered}");
    let token = registered["data"]["registration"].as_str().unwrap();
    sync_consult_fixture(&daemon, token);
    let mut second = event.clone();
    second["registration"] = json!(token);
    second["call"]["session_id"] = json!(second_session);
    let first = native(&daemon, event);
    let second = native(&daemon, second);
    for submitted in [&first, &second] {
        assert!(submitted.get("error").is_none(), "{submitted}");
        let end = Instant::now() + Duration::from_secs(15);
        loop {
            let job =
                daemon.cli(&["job", "show", submitted["data"]["id"].as_str().unwrap()])["data"]
                    .clone();
            if !matches!(job["state"].as_str(), Some("queued" | "running")) {
                assert_eq!(job["state"], "succeeded", "{job}");
                assert_eq!(job["run_trace"]["usage"]["starts"], 1);
                break;
            }
            assert!(Instant::now() < end, "parent delegate deadlocked: {job}");
            std::thread::sleep(Duration::from_millis(10));
        }
    }
    assert_ne!(first["data"]["id"], second["data"]["id"]);
    assert_eq!(
        std::fs::read_to_string(daemon.root.join("starts"))
            .unwrap()
            .lines()
            .count(),
        2
    );
}

#[test]
fn consult_delegate_default_capacity_refuses_without_deadlock() {
    consult_delegation("delegate_denied", 1, "DELEGATION_UNAVAILABLE");
}

#[test]
fn consult_delegate_descendants_share_budget_and_origin() {
    consult_delegation("delegate_tree", 3, "BUDGET_EXHAUSTED");
}

#[test]
fn consult_delegate_failure_keeps_other_branch_evidence() {
    consult_delegation("delegate_failure", 3, "AUTH_REQUIRED");
}

fn consult_delegation(scenario: &str, starts: usize, gap: &str) {
    let (daemon, event, bound) = consult_fixture(scenario);
    let submitted = native(&daemon, event);
    assert!(submitted.get("error").is_none(), "{submitted}");
    let end = Instant::now() + Duration::from_secs(20);
    let job = loop {
        let job =
            daemon.cli(&["job", "show", submitted["data"]["id"].as_str().unwrap()])["data"].clone();
        if !matches!(job["state"].as_str(), Some("queued" | "running")) {
            break job;
        }
        assert!(Instant::now() < end, "delegate did not finish: {job}");
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(job["state"], "succeeded", "{job}");
    let output: Value = serde_json::from_str(job["result"]["output"].as_str().unwrap()).unwrap();
    assert_eq!(output["answer"], "bounded-context");
    assert!(
        output["gaps"].as_array().unwrap().iter().any(|v| v == gap),
        "{output}"
    );
    assert!(!output["evidence"].as_array().unwrap().is_empty());
    assert_eq!(
        std::fs::read_to_string(daemon.root.join("starts"))
            .unwrap()
            .lines()
            .count(),
        starts
    );
    let trace = &job["run_trace"];
    assert_eq!(trace["usage"]["starts"], starts);
    let runs = trace["runs"].as_array().unwrap();
    assert_eq!(runs.len(), starts);
    assert!(
        runs.iter()
            .all(|r| r["origin"] == bound["data"]["invocation_id"])
    );
    assert!(
        runs.iter()
            .all(|r| r["state"] == "succeeded" || r["state"] == "failed")
    );
    assert!(runs[0]["parent"].is_null());
    for run in &runs[1..] {
        assert!(runs.iter().any(|parent| parent["id"] == run["parent"]
            && parent["depth"].as_u64().unwrap() + 1 == run["depth"].as_u64().unwrap()));
    }
    if scenario == "delegate_tree" {
        assert_eq!(trace["usage"]["tokens"], 72);
    }
    if scenario == "delegate_failure" {
        assert_eq!(runs[1]["error"], "AUTH_REQUIRED");
        assert_eq!(runs[2]["state"], "succeeded");
    }
}

#[test]
fn consult_progressive_research_keeps_source_versions_and_workspace_changes() {
    let (mut daemon, event, bound) = consult_fixture("research");
    daemon.cli(&["index", "sync", "--idempotency-key", "research-index"]);
    let finished = |id: &str| {
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            let job = daemon.cli(&["job", "show", id])["data"].clone();
            if !matches!(job["state"].as_str(), Some("queued" | "running")) {
                assert_eq!(
                    job["state"],
                    "succeeded",
                    "{job}; checkpoint={:?}",
                    std::fs::read_to_string(daemon.root.join("research-checkpoint.json"))
                );
                break job;
            }
            assert!(Instant::now() < end, "research did not finish: {job}");
            std::thread::sleep(Duration::from_millis(10));
        }
    };
    finished("research-index");
    let submitted = native(&daemon, event);
    assert!(submitted.get("error").is_none(), "{submitted}");
    let job = finished(submitted["data"]["id"].as_str().unwrap());
    let output = job["result"]["output"].as_str().unwrap();
    assert!(output.chars().count() <= 4000);
    let output: Value = serde_json::from_str(output).unwrap();
    assert_eq!(output["status"], "incomplete");
    let memory = output["memories"]
        .as_array()
        .unwrap()
        .iter()
        .find(|m| m["reference"]["key"]["id"] == "offline")
        .unwrap();
    assert_eq!(
        memory["conditions"][0],
        "Only for the original local environment."
    );
    assert_eq!(memory["assumptions"][0], "The local index is available.");
    assert_eq!(memory["valid_from_ms"], 1);
    assert!(memory["valid_until_ms"].is_null());
    assert!(
        output["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "workspace_changed")
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    let receipt = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            use akasha_otsuge::{
                adapter::Invocation,
                config::{Paths, resolve},
                domain::{Domain, ReadContext},
                storage::Storage,
            };
            let config = resolve(
                "",
                None,
                "",
                &Paths {
                    data_dir: daemon.root.join("data"),
                    socket_path: daemon.socket(),
                },
            )
            .unwrap();
            let store = Storage::open(&config).await.unwrap();
            let principal = format!("uid:{}", rustix::process::geteuid().as_raw());
            let invocation = Invocation::load(
                &store,
                bound["data"]["invocation_id"].as_str().unwrap(),
                &principal,
            )
            .await
            .unwrap();
            let context = ReadContext::from_verified_binding(
                &principal,
                invocation.id(),
                job["id"].as_str().unwrap(),
            )
            .unwrap();
            let receipt = Domain::new(&store)
                .recall_receipt(&context, job["result"]["receipt_id"].as_str().unwrap())
                .await
                .unwrap()
                .data;
            assert_eq!(receipt.output, job["result"]["output"].as_str().unwrap());
            let domain = Domain::new(&store);
            let previous = domain.active("offline").unwrap().unwrap();
            let mut draft = previous.record.draft.clone();
            draft.claim.push_str(" updated after completion");
            let scope = draft.scope.clone();
            domain
                .propose(
                    &akasha_otsuge::domain::MemoryWrite {
                        request_id: "changed-after-consult".into(),
                        memory_id: "offline".into(),
                        expected_revision: Some(previous.revision.commit_seq),
                        draft,
                    },
                    &akasha_otsuge::domain::Ingress::tool("fixture", "fixture", scope.clone())
                        .unwrap(),
                    &akasha_otsuge::domain::Policy {
                        revision: "fixture".into(),
                        config_generation: 1,
                        mode: akasha_otsuge::config::ReviewMode::Automatic,
                        scope,
                        adopt_explicit_decisions: false,
                    },
                )
                .await
                .unwrap();
            store.close().await.unwrap();
            serde_json::to_value(receipt).unwrap()
        });
    daemon.start();
    let current = daemon
        .command()
        .args(["consult", "show", job["id"].as_str().unwrap()])
        .output()
        .unwrap();
    assert!(
        !current.status.success(),
        "superseded memory was returned as current"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&current.stderr).unwrap()["error"]["code"],
        "STALE_RECALL"
    );
    let historical = daemon.cli(&["consult", "receipt", job["id"].as_str().unwrap()]);
    assert_eq!(
        historical["data"]["data"]["output"],
        job["result"]["output"]
    );
    assert_eq!(historical["data"]["retrieval_mode"], "receipt_replay");
    let record = &receipt["record"];
    assert!(
        record["selected"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["key"]["id"] == "offline")
    );
    assert!(
        record["sources"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["event_id"].as_str().unwrap().starts_with("consult-code/"))
    );
    assert!(record["read_seq"].as_u64().unwrap() <= record["validated_at_seq"].as_u64().unwrap());
    for observation in output["code"].as_array().unwrap() {
        assert!(
            record["sources"]
                .as_array()
                .unwrap()
                .contains(&observation["source"])
        );
    }
}

#[test]
fn consult_conclusions_validate_evidence_no_hit_conditions_and_conflicts() {
    for scenario in [
        "conclusion_found",
        "conclusion_no_hit",
        "conclusion_forged",
        "conclusion_conflict",
    ] {
        let (daemon, event, _) = consult_fixture(scenario);
        let submitted = native(&daemon, event);
        let id = submitted["data"]["id"].as_str().unwrap();
        let end = Instant::now() + Duration::from_secs(15);
        let result = loop {
            let value = daemon.cli(&["consult", "show", id])["data"].clone();
            if value["status"] != "pending" {
                break value;
            }
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(10));
        };
        match scenario {
            "conclusion_found" => {
                assert_eq!(result["status"], "found", "{result}");
                assert_eq!(result["evidence"].as_array().unwrap().len(), 1);
            }
            "conclusion_no_hit" => {
                assert_eq!(result["status"], "no_hit", "{result}");
                assert!(result["evidence"].as_array().unwrap().is_empty());
                assert!(result["memories"].as_array().unwrap().is_empty());
            }
            "conclusion_forged" => assert_eq!(result["error"]["code"], "READ_DENIED", "{result}"),
            _ => {
                assert_eq!(result["status"], "incomplete", "{result}");
                assert_eq!(
                    result["conditions"][0],
                    "Only for the original local environment."
                );
                assert_eq!(result["conflicts"][0], "The two observations disagree.");
            }
        }
    }
}

#[test]
fn consult_public_result_expands_receipt_and_originals_without_private_trace() {
    consult_public_result(false);
}

#[test]
fn consult_public_large_result_preserves_complete_answer_and_conditions() {
    consult_public_result(true);
}

fn consult_public_result(large: bool) {
    let (mut daemon, mut event, _) = consult_fixture("code_policy");
    if large {
        event["args"]["hint"] = json!("long-output");
    }
    let submitted = native(&daemon, event);
    let id = submitted["data"]["id"].as_str().unwrap();
    let end = Instant::now() + Duration::from_secs(15);
    while !daemon.root.join("code-read-ready").exists() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(10));
    }
    let pending = daemon.cli(&["consult", "show", id])["data"].clone();
    assert_eq!(
        pending,
        json!({"status":"pending","job_id":id,"receipt_id":null})
    );
    std::fs::write(daemon.root.join("code-read-release"), "").unwrap();
    let result = loop {
        let result = daemon.cli(&["consult", "show", id])["data"].clone();
        if result["status"] != "pending" {
            break result;
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(
        result["status"],
        if large { "incomplete" } else { "found" },
        "{result}"
    );
    assert!(result.to_string().chars().count() <= 4000);
    for private in ["run_trace", "consult_plan", "usage", "native", "workspace"] {
        assert!(result.get(private).is_none());
    }
    assert_eq!(result["job_id"], id);
    let receipt = daemon.cli(&["consult", "receipt", id])["data"].clone();
    assert_eq!(
        serde_json::from_str::<Value>(receipt["data"]["output"].as_str().unwrap()).unwrap(),
        result
    );
    let full = daemon.cli(&["consult", "receipt", id, "--full"])["data"].clone();
    let expanded: Value = serde_json::from_str(full["data"]["output"].as_str().unwrap()).unwrap();
    if large {
        assert_eq!(
            expanded["answer"],
            "说明".repeat(2500) + "；仅在全部测试通过时适用。"
        );
        assert_eq!(
            result["detail_receipt_id"],
            full["data"]["record"]["receipt_id"]
        );
        assert_eq!(
            receipt["data"]["record"]["truncation"]["reason"],
            "result_budget"
        );
        assert!(result.get("answer").is_none());
    } else {
        assert_eq!(expanded, result);
    }
    let index = receipt["data"]["record"]["sources"]
        .as_array()
        .unwrap()
        .iter()
        .position(|s| s["event_id"].as_str().unwrap().starts_with("consult-code/"))
        .unwrap()
        .to_string();
    let original = daemon.cli(&["consult", "original", id, &index])["data"].clone();
    assert_eq!(original["output"], "memory needle\n");
    let rest = daemon.cli(&["consult", "original", id, &index, "--start", "7"])["data"].clone();
    assert_eq!(rest["output"], "needle\n");
    let invalid = daemon
        .command()
        .args(["consult", "original", id, "999"])
        .output()
        .unwrap();
    assert!(!invalid.status.success());
    assert_eq!(
        serde_json::from_slice::<Value>(&invalid.stderr).unwrap()["error"]["code"],
        "INVALID_RANGE"
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(daemon.cli(&["consult", "show", id])["data"], result);
    let home = daemon.root.join("native-home");
    daemon.configure(&format!("[access]\nread_roots=[{home:?}]\n"));
    daemon.cli(&["config", "reload"]);
    for args in [
        vec!["consult", "show", id],
        vec!["consult", "receipt", id],
        vec!["consult", "receipt", id, "--full"],
        vec!["consult", "original", id, &index],
    ] {
        let output = daemon.command().args(args).output().unwrap();
        assert!(!output.status.success());
        assert_eq!(
            serde_json::from_slice::<Value>(&output.stderr).unwrap()["error"]["code"],
            "READ_DENIED"
        );
    }
}

#[test]
fn consult_public_failure_reports_error_without_a_receipt() {
    let (daemon, event, _) = consult_fixture("auth");
    let submitted = native(&daemon, event);
    let id = submitted["data"]["id"].as_str().unwrap();
    let end = Instant::now() + Duration::from_secs(15);
    let result = loop {
        let result = daemon.cli(&["consult", "show", id])["data"].clone();
        if result["status"] != "pending" {
            break result;
        }
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(10));
    };
    assert_eq!(
        result,
        json!({"status":"error","job_id":id,"receipt_id":null,"error":{"code":"AUTH_REQUIRED"}})
    );
}

#[test]
fn consult_code_rechecks_read_policy_before_return() {
    consult_code_policy(false, false);
}

#[test]
fn consult_code_rechecks_workspace_identity_before_return() {
    consult_code_policy(false, true);
}

#[test]
fn consult_code_rechecks_read_policy_before_replay() {
    consult_code_policy(true, false);
}

#[test]
fn consult_code_rechecks_workspace_identity_before_replay() {
    consult_code_policy(true, true);
}

fn consult_code_policy(delivered: bool, redirect: bool) {
    let (mut daemon, event, _) = consult_fixture("code_policy");
    let submitted = native(&daemon, event);
    assert!(submitted.get("error").is_none(), "{submitted}");
    let id = submitted["data"]["id"].as_str().unwrap();
    let end = Instant::now() + Duration::from_secs(15);
    while !daemon.root.join("code-read-ready").exists() {
        assert!(Instant::now() < end, "code read did not reach barrier");
        std::thread::sleep(Duration::from_millis(10));
    }
    let release =
        |daemon: &Daemon| std::fs::write(daemon.root.join("code-read-release"), "").unwrap();
    let terminal = |daemon: &Daemon| loop {
        let job = daemon.cli(&["job", "show", id])["data"].clone();
        if !matches!(job["state"].as_str(), Some("queued" | "running")) {
            break job;
        }
        assert!(Instant::now() < end, "consult did not finish: {job}");
        std::thread::sleep(Duration::from_millis(10));
    };
    if delivered {
        release(&daemon);
        let job = terminal(&daemon);
        assert_eq!(job["state"], "succeeded", "{job}");
        assert!(job["result"]["output"].as_str().is_some());
        daemon.cli(&["daemon", "stop"]);
        daemon.wait();
        daemon.start();
        assert_eq!(
            daemon.cli(&["job", "show", id])["data"]["result"],
            job["result"]
        );
    }
    let workspace = daemon.root.join("workspace");
    if redirect {
        std::fs::rename(&workspace, daemon.root.join("original-workspace")).unwrap();
        std::fs::create_dir(&workspace).unwrap();
    } else {
        let home = daemon.root.join("native-home");
        daemon.configure(&format!("[access]\nread_roots=[{home:?}]\n"));
        daemon.cli(&["config", "reload"]);
    }
    if delivered {
        let output = daemon.command().args(["job", "show", id]).output().unwrap();
        assert!(!output.status.success());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "READ_DENIED");
    } else {
        release(&daemon);
        let job = terminal(&daemon);
        assert_eq!(job["state"], "failed", "{job}");
        assert_eq!(job["error"], "READ_DENIED");
        assert!(job["result"].is_null());
    }
}

#[test]
fn consult_reused_profile_obeys_current_read_roots() {
    consult_home_policy(false, false);
}

#[test]
fn consult_queued_profile_obeys_current_read_roots() {
    consult_home_policy(true, false);
}

#[test]
fn consult_reused_profile_rejects_path_redirection() {
    consult_home_policy(false, true);
}

#[test]
fn consult_queued_profile_rejects_path_redirection() {
    consult_home_policy(true, true);
}

fn consult_home_policy(queued: bool, redirect: bool) {
    let (daemon, event, _) = consult_fixture("success");
    let workspace = daemon.root.join("workspace");
    let home = daemon.root.join("native-home");
    let endpoint = super::model_tests::Endpoint::retrieval();
    endpoint.pause();
    let model = format!(
        "[models.hold]\nkind='embedding'\nendpoint={:?}\nmodel='vectors-2'\ndimension=2\nlocal=true\n",
        endpoint.address
    );
    let mut job_id = None;
    if queued {
        daemon.configure(&format!(
            "[access]\nread_roots=[{workspace:?},{home:?}]\n{model}"
        ));
        daemon.cli(&["config", "reload"]);
        let plan = daemon.root.join("hold.json");
        std::fs::write(
            &plan,
            json!({"calls":[{"profile":"hold","input":{"kind":"embedding","texts":["barrier"]}}]})
                .to_string(),
        )
        .unwrap();
        daemon.cli(&[
            "model",
            "run",
            plan.to_str().unwrap(),
            "--idempotency-key",
            "hold",
        ]);
        let end = Instant::now() + Duration::from_secs(10);
        while endpoint.requests.lock().unwrap().is_empty() {
            assert!(Instant::now() < end, "model did not reach the barrier");
            std::thread::sleep(Duration::from_millis(10));
        }
        let submitted = native(&daemon, event.clone());
        assert_eq!(submitted["data"]["state"], "queued", "{submitted}");
        assert_eq!(submitted["data"]["config_generation"], 1);
        job_id = Some(submitted["data"]["id"].as_str().unwrap().to_owned());
    }
    if redirect {
        let replacement = daemon.root.join("redirected-home");
        std::fs::rename(&home, &replacement).unwrap();
        std::os::unix::fs::symlink(replacement, &home).unwrap();
    } else {
        daemon.configure(&format!("[access]\nread_roots=[{workspace:?}]\n{model}"));
        daemon.cli(&["config", "reload"]);
    }
    let expected = if redirect {
        "READ_DENIED"
    } else {
        "PATH_DENIED"
    };
    if let Some(id) = job_id {
        endpoint.resume();
        let end = Instant::now() + Duration::from_secs(10);
        let job = loop {
            let job = daemon.cli(&["job", "show", &id])["data"].clone();
            if !matches!(job["state"].as_str(), Some("queued" | "running")) {
                break job;
            }
            assert!(Instant::now() < end, "queued consult did not finish: {job}");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(job["state"], "failed", "{job}");
        assert_eq!(job["error"], expected);
    } else {
        assert_eq!(native(&daemon, event)["error"]["code"], expected);
    }
    assert!(
        !daemon.root.join("starts").exists(),
        "denied profile launched a native worker"
    );
}

#[test]
fn consult_job_is_distinct_durable_and_deduplicates_native_execution() {
    let (mut daemon, event, bound) = consult_fixture("success");
    let submitted = native(&daemon, event.clone());
    assert!(submitted.get("error").is_none(), "{submitted}");
    let id = submitted["data"]["id"].as_str().unwrap();
    assert_ne!(id, bound["data"]["invocation_id"].as_str().unwrap());
    assert_eq!(native(&daemon, event.clone())["data"]["id"], id);
    let mut conflicting = event.clone();
    conflicting["args"]["hint"] = "A different request".into();
    assert_eq!(
        native(&daemon, conflicting)["error"]["code"],
        "IDEMPOTENCY_CONFLICT"
    );
    let end = Instant::now() + Duration::from_secs(20);
    let complete = loop {
        let job = daemon.cli(&["job", "show", id]);
        match job["data"]["state"].as_str() {
            Some("succeeded") => break job["data"].clone(),
            Some("queued" | "running") if Instant::now() < end => {
                assert!(job["data"]["result"].is_null());
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => panic!("consult failed: {job}"),
        }
    };
    assert!(complete["result"]["receipt_id"].is_string());
    assert!(
        complete["result"]["output"]
            .as_str()
            .unwrap()
            .contains("bounded-context")
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(native(&daemon, event)["data"]["id"], id);
    assert_eq!(
        daemon.cli(&["job", "show", id])["data"]["result"],
        complete["result"]
    );
    assert_eq!(
        std::fs::read_to_string(daemon.root.join("starts")).unwrap(),
        "start\n"
    );
    daemon.cli(&["access", "revoke"]);
    let out = daemon.command().args(["job", "show", id]).output().unwrap();
    assert!(!out.status.success());
}

#[test]
fn consult_cancel_shutdown_and_crash_end_native_processes_without_replaying() {
    consult_lifecycle("cancel");
}

#[test]
fn consult_detached_descendants_settle_before_cancellation_releases_ownership() {
    consult_lifecycle("cancel_detached");
}

#[test]
fn consult_delegate_cancellation_and_crash_reap_the_whole_run_tree() {
    consult_lifecycle("delegate_cancel");
}

fn consult_lifecycle(scenario: &str) {
    use rustix::process::{Pid, PidfdFlags, Signal, pidfd_open, pidfd_send_signal};
    struct Cleanup(Vec<rustix::fd::OwnedFd>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for fd in &self.0 {
                let _ = pidfd_send_signal(fd, Signal::KILL);
            }
        }
    }
    for mode in ["cancel", "shutdown", "crash"] {
        let (mut daemon, event, _) = consult_fixture(scenario);
        let submitted = native(&daemon, event.clone());
        assert!(submitted.get("error").is_none(), "{submitted}");
        let id = submitted["data"]["id"].as_str().unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        while !daemon.root.join("child.pid").exists() {
            assert!(Instant::now() < end, "native fixture did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut cleanup = Cleanup(Vec::new());
        let mut processes = Vec::new();
        let files = if scenario == "delegate_cancel" {
            vec!["parent.pid", "runner.pid", "child.pid"]
        } else {
            vec!["runner.pid", "child.pid"]
        };
        for file in files {
            let pid: i32 = std::fs::read_to_string(daemon.root.join(file))
                .unwrap()
                .parse()
                .unwrap();
            cleanup
                .0
                .push(pidfd_open(Pid::from_raw(pid).unwrap(), PidfdFlags::empty()).unwrap());
            processes.push(pid);
        }
        if mode == "cancel" {
            daemon.cli(&["job", "cancel", id]);
        } else if mode == "shutdown" {
            daemon.cli(&["daemon", "stop"]);
            daemon.wait();
        } else {
            let mut child = daemon.child.take().unwrap();
            child.kill().unwrap();
            child.wait().unwrap();
        }
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            let live = processes
                .iter()
                .any(|pid| std::path::Path::new(&format!("/proc/{pid}")).exists());
            if !live {
                break;
            }
            assert!(
                Instant::now() < end,
                "daemon crash left a live native process"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        if mode != "cancel" {
            daemon.start();
        }
        let end = Instant::now() + Duration::from_secs(10);
        let job = loop {
            let job = daemon.cli(&["job", "show", id]);
            if matches!(job["data"]["state"].as_str(), Some("failed" | "cancelled")) {
                break job;
            }
            assert!(
                Instant::now() < end,
                "terminal state was not persisted: {job}"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(
            job["data"]["state"],
            if mode == "cancel" {
                "cancelled"
            } else {
                "failed"
            }
        );
        assert_eq!(
            job["data"]["error"],
            if mode == "cancel" {
                "CANCELLED"
            } else {
                "RUNNER_INTERRUPTED"
            }
        );
        let runs = job["data"]["run_trace"]["runs"].as_array().unwrap();
        assert_eq!(
            runs.len(),
            if scenario == "delegate_cancel" { 2 } else { 1 }
        );
        for run in runs {
            assert_eq!(run["state"], "failed");
            assert_eq!(run["error"], job["data"]["error"]);
        }
        assert_eq!(
            std::fs::read_dir(daemon.root.join("data/runner-work"))
                .unwrap()
                .count(),
            0,
            "{mode}"
        );
        assert_eq!(native(&daemon, event)["data"]["id"], id);
        assert_eq!(
            std::fs::read_to_string(daemon.root.join("starts")).unwrap(),
            if scenario == "delegate_cancel" {
                "start\nstart\n"
            } else {
                "start\n"
            }
        );
    }
}

fn hook(daemon: &Daemon, payload: &Value) -> Value {
    let mut child = daemon
        .command()
        .args([
            "adapter",
            "codex-hook",
            "--binary",
            daemon.root.join("codex").to_str().unwrap(),
            "--state-dir",
            daemon.root.join("hook-state").to_str().unwrap(),
        ])
        .env("CODEX_HOME", daemon.root.join("native-home"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(payload.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn consult_hook_submits_and_expands_durable_results_inside_the_read_only_sandbox() {
    let (mut daemon, _, _) = consult_fixture("conclusion_found");
    let mut payload: Value = serde_json::from_str(include_str!(
        "../fixtures/codex/0.164.0/exec-pre-tool-use.json"
    ))
    .unwrap();
    payload["cwd"] = json!(daemon.root.join("workspace"));
    payload["transcript_path"] = json!(daemon.root.join("workspace/native.jsonl"));
    payload["tool_input"]["command"] = json!("akasha-otsuge consult \"long-output\"");
    let invoke = |daemon: &Daemon, payload: &Value| {
        let response = hook(daemon, payload);
        assert_eq!(
            response["hookSpecificOutput"]["permissionDecision"], "allow",
            "command={}; response={response}",
            payload["tool_input"]["command"]
        );
        let command = response["hookSpecificOutput"]["updatedInput"]["command"]
            .as_str()
            .unwrap();
        let result = Command::new("/bin/sh")
            .args(["-c", command])
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        serde_json::from_slice::<Value>(&result.stdout).unwrap()["data"].clone()
    };
    let submitted = invoke(&daemon, &payload);
    let id = submitted["job_id"]
        .as_str()
        .unwrap_or_else(|| panic!("{submitted}"));
    assert!(submitted.get("consult_plan").is_none());
    assert_eq!(invoke(&daemon, &payload)["job_id"], id);
    let end = Instant::now() + Duration::from_secs(15);
    while daemon.cli(&["consult", "show", id])["data"]["status"] == "pending" {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(10));
    }
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    for (i, command) in [
        format!("akasha-otsuge consult show {id}"),
        format!("akasha-otsuge consult receipt {id}"),
        format!("akasha-otsuge consult receipt {id} --full"),
        format!("akasha-otsuge consult original {id} 0"),
        format!("akasha-otsuge job cancel {id}"),
    ]
    .into_iter()
    .enumerate()
    {
        payload["tool_use_id"] = json!(format!("followup-{i}"));
        payload["tool_input"]["command"] = json!(command);
        let value = invoke(&daemon, &payload);
        assert!(value.get("run_trace").is_none());
        if i == 2 {
            let full: Value =
                serde_json::from_str(value["data"]["output"].as_str().unwrap()).unwrap();
            assert!(full["answer"].as_str().unwrap().chars().count() > 5000);
        }
        payload["hook_event_name"] = json!("PostToolUse");
        hook(&daemon, &payload);
        payload["hook_event_name"] = json!("PreToolUse");
    }
    daemon.cli(&["access", "revoke"]);
    payload["tool_use_id"] = json!("revoked-query");
    payload["tool_input"]["command"] = json!(format!("akasha-otsuge consult show {id}"));
    assert_eq!(
        hook(&daemon, &payload)["hookSpecificOutput"]["permissionDecisionReason"],
        "READ_DENIED"
    );
    for command in [
        format!("akasha-otsuge consult show {id}; echo unwanted"),
        "akasha-otsuge job cancel foreign-job".into(),
        format!("akasha-otsuge consult original {id} 0 --start -1"),
    ] {
        payload["tool_input"]["command"] = json!(command);
        assert_eq!(
            hook(&daemon, &payload)["hookSpecificOutput"]["permissionDecisionReason"],
            "INVALID_ARGUMENT"
        );
    }
}

#[test]
fn native_hook_uses_private_call_files_without_exposing_registration_credentials() {
    let mut daemon = Daemon::new();
    daemon.configure(&format!("[access]\nread_roots=[{:?}]\n", daemon.root));
    let binary = daemon.root.join("codex");
    crate::executable::write(&binary, "#!/bin/sh\nprintf 'codex-cli 0.164.0\\n'\n");
    let transcript = daemon.root.join("native.jsonl");
    let raw = include_str!("../fixtures/codex/0.164.0/exec-root.jsonl")
        .replace("/fixture/workspace", daemon.root.to_str().unwrap());
    std::fs::write(&transcript, raw).unwrap();
    let mut hook: Value = serde_json::from_str(include_str!(
        "../fixtures/codex/0.164.0/exec-pre-tool-use.json"
    ))
    .unwrap();
    hook["cwd"] = json!(daemon.root);
    hook["transcript_path"] = json!(transcript);
    hook["tool_input"]["command"] = "akasha-otsuge consult \"find prior decision\"".into();
    let state = daemon.root.join("hook-state");
    std::fs::create_dir(daemon.root.join(".codex")).unwrap();
    daemon.start();
    // This privacy test does not require cold catch-up to fit the two-second bind budget.
    let registered = native(
        &daemon,
        json!({"op":"register","binary":binary,"session_id":hook["session_id"],"transcript_path":transcript,"cwd":daemon.root}),
    );
    sync_fixture_until(
        &daemon,
        registered["data"]["registration"].as_str().unwrap(),
        9,
    );
    let mut child = daemon
        .command()
        .args([
            "adapter",
            "codex-hook",
            "--binary",
            binary.to_str().unwrap(),
            "--state-dir",
            state.to_str().unwrap(),
        ])
        .env("CODEX_HOME", daemon.root.join(".codex"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(hook.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["hookSpecificOutput"]["permissionDecision"], "allow");
    let command = result["hookSpecificOutput"]["updatedInput"]["command"]
        .as_str()
        .unwrap();
    let mut call_files = Vec::new();
    for entry in std::fs::read_dir(&state).unwrap() {
        let path = entry.unwrap().path();
        if path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("call-")
        {
            call_files.push(path);
        }
    }
    assert_eq!(call_files.len(), 1);
    let private: Value = serde_json::from_slice(&std::fs::read(&call_files[0]).unwrap()).unwrap();
    assert_eq!(
        std::fs::metadata(&call_files[0]).unwrap().mode() & 0o777,
        0o600
    );
    assert!(
        private.get("registration").is_none(),
        "sandbox-readable result must not contain a registration capability"
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    let bound = Command::new("/bin/sh")
        .args(["-c", command])
        .output()
        .unwrap();
    assert!(
        bound.status.success(),
        "{}",
        String::from_utf8_lossy(&bound.stderr)
    );
    assert!(
        serde_json::from_slice::<Value>(&bound.stdout)
            .unwrap()
            .get("registration")
            .is_none()
    );
    let result: Value = serde_json::from_slice(&bound.stdout).unwrap();
    assert!(
        result["data"]["job_id"]
            .as_str()
            .unwrap()
            .starts_with("consult/")
    );
    assert!(result["data"].get("consult_plan").is_none());
    assert!(result["data"].get("run_trace").is_none());
    assert!(
        call_files[0].exists(),
        "the sandboxed reader must not need write access"
    );
    hook["hook_event_name"] = "PostToolUse".into();
    let mut child = daemon
        .command()
        .args([
            "adapter",
            "codex-hook",
            "--binary",
            binary.to_str().unwrap(),
            "--state-dir",
            state.to_str().unwrap(),
        ])
        .env("CODEX_HOME", daemon.root.join(".codex"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(hook.to_string().as_bytes())
        .unwrap();
    assert!(child.wait_with_output().unwrap().status.success());
    assert!(
        !call_files[0].exists(),
        "host callback must remove the consumed call file"
    );
    hook["hook_event_name"] = "PreToolUse".into();
    let mut failed = daemon
        .command()
        .args([
            "adapter",
            "codex-hook",
            "--binary",
            binary.to_str().unwrap(),
            "--state-dir",
            state.to_str().unwrap(),
        ])
        .env("CODEX_HOME", daemon.root.join(".codex"))
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    failed
        .stdin
        .take()
        .unwrap()
        .write_all(hook.to_string().as_bytes())
        .unwrap();
    let out = failed.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "recognized invocation errors must deny through the native protocol"
    );
    let denied: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(denied["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_eq!(
        denied["hookSpecificOutput"]["permissionDecisionReason"],
        "DAEMON_UNAVAILABLE"
    );
    assert!(denied["hookSpecificOutput"].get("updatedInput").is_none());
}

#[test]
fn concurrent_native_hooks_share_state_without_cross_binding_or_global_lock_contention() {
    let mut daemon = Daemon::new();
    daemon.configure(&format!("[access]\nread_roots=[{:?}]\n", daemon.root));
    let binary = daemon.root.join("codex");
    crate::executable::write(
        &binary,
        "#!/bin/sh\nsleep 0.2\nprintf 'codex-cli 0.164.0\\n'\n",
    );
    let state = daemon.root.join("hook-state");
    std::fs::create_dir(daemon.root.join(".codex")).unwrap();
    daemon.start();
    let original: Value = serde_json::from_str(include_str!(
        "../fixtures/codex/0.164.0/exec-pre-tool-use.json"
    ))
    .unwrap();
    let mut hooks = Vec::new();
    for n in 0..2 {
        let session = format!("concurrent-session-{n}");
        let path = daemon.root.join(format!("native-{n}.jsonl"));
        let raw = include_str!("../fixtures/codex/0.164.0/exec-root.jsonl")
            .replace(original["session_id"].as_str().unwrap(), &session)
            .replace("/fixture/workspace", daemon.root.to_str().unwrap());
        std::fs::write(&path, raw).unwrap();
        let mut hook = original.clone();
        hook["session_id"] = session.clone().into();
        hook["cwd"] = json!(daemon.root);
        hook["transcript_path"] = json!(path);
        hook["tool_input"]["command"] = "akasha-otsuge consult".into();
        // Isolate concurrent hook ownership from cold capture's separate two-second budget.
        let registered = native(
            &daemon,
            json!({"op":"register","binary":binary,"session_id":session,"transcript_path":path,"cwd":daemon.root}),
        );
        assert!(registered.get("error").is_none(), "{registered}");
        sync_fixture_until(
            &daemon,
            registered["data"]["registration"].as_str().unwrap(),
            9,
        );
        hooks.push((session, hook));
    }
    let mut children = Vec::new();
    for (session, hook) in hooks {
        let mut child = daemon
            .command()
            .args([
                "adapter",
                "codex-hook",
                "--binary",
                binary.to_str().unwrap(),
                "--state-dir",
                state.to_str().unwrap(),
            ])
            .env("CODEX_HOME", daemon.root.join(".codex"))
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(hook.to_string().as_bytes())
            .unwrap();
        children.push((session, child));
    }
    for (session, child) in children {
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let hook: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert!(
            hook["hookSpecificOutput"]["updatedInput"]["command"].is_string(),
            "concurrent hook for {session} did not admit the prepared native call: {hook}"
        );
        let command = hook["hookSpecificOutput"]["updatedInput"]["command"]
            .as_str()
            .unwrap();
        let out = Command::new("/bin/sh")
            .args(["-c", command])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let bound: Value = serde_json::from_slice(&out.stdout).unwrap();
        let id = bound["data"]["job_id"].as_str().unwrap();
        let matching = std::fs::read_dir(&state).unwrap().any(|entry| {
            let value: Value =
                serde_json::from_slice(&std::fs::read(entry.unwrap().path()).unwrap()).unwrap();
            value["call"]["session_id"] == session && value["response"]["job_id"] == id
        });
        assert!(matching, "the public job must belong to this native call");
    }
}

#[test]
fn native_registration_binding_and_backfill_survive_daemon_restart() {
    let mut daemon = Daemon::new();
    daemon.configure(&format!("[access]\nread_roots=[{:?}]\n", daemon.root));
    let binary = daemon.root.join("codex");
    crate::executable::write(&binary, "#!/bin/sh\nprintf 'codex-cli 0.164.0\\n'\n");
    let raw = include_str!("../fixtures/codex/0.164.0/exec-root.jsonl")
        .replace("/fixture/workspace", daemon.root.to_str().unwrap());
    let transcript = daemon.root.join("native.jsonl");
    std::fs::write(&transcript, raw).unwrap();
    let hook: Value = serde_json::from_str(include_str!(
        "../fixtures/codex/0.164.0/exec-pre-tool-use.json"
    ))
    .unwrap();
    daemon.start();
    let doctor = daemon.cli(&[
        "doctor",
        "--host",
        "codex",
        "--binary",
        binary.to_str().unwrap(),
    ]);
    assert_eq!(doctor["data"]["version"], "0.164.0");
    assert_eq!(doctor["coverage"]["model_auth"], "not_checked");
    let registered = native(
        &daemon,
        json!({"op":"register","binary":binary,"session_id":hook["session_id"],"transcript_path":transcript,"cwd":daemon.root}),
    );
    let token = registered["data"]["registration"]
        .as_str()
        .expect("native registration must succeed");
    sync_fixture_until(&daemon, token, 9);
    let bind = json!({"op":"bind","registration":token,"call":{"session_id":hook["session_id"],"agent_id":null,"turn_id":hook["turn_id"],"tool_call_id":hook["tool_use_id"],"cwd":daemon.root}});
    let bound = native(&daemon, bind.clone());
    assert!(bound.get("error").is_none(), "{bound}");
    assert_eq!(bound["data"]["requested_cursor"], 10);
    assert_eq!(bound["data"]["tip"]["event_id"], "row:9");
    assert!(!bound.to_string().contains(token));
    let mut bridge = daemon
        .command()
        .args(["adapter", "codex-call"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    bridge
        .stdin
        .take()
        .unwrap()
        .write_all(bind.to_string().as_bytes())
        .unwrap();
    let output = bridge.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"],
        bound["data"]
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(native(&daemon, bind.clone())["data"], bound["data"]);
    let sync = native(
        &daemon,
        json!({"op":"sync","registration":token,"until_ordinal":18}),
    );
    assert!(sync.get("error").is_none(), "{sync}");
    let id = sync["data"]["id"].as_str().unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let job = daemon.cli(&["job", "show", id]);
        match job["data"]["state"].as_str() {
            Some("succeeded") => break,
            Some("queued" | "running") if Instant::now() < end => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => panic!("native capture did not finish: {job}"),
        }
    }
    let mut wrong = bind.clone();
    wrong["call"]["turn_id"] = "other-turn".into();
    assert_eq!(native(&daemon, wrong)["error"]["code"], "CALLER_AMBIGUOUS");
    daemon.cli(&["access", "revoke"]);
    assert_eq!(native(&daemon, bind)["error"]["code"], "READ_DENIED");
}

#[test]
fn codex_launcher_install_is_opt_in_atomic_and_preserves_edited_files() {
    let daemon = Daemon::new();
    let binary = daemon.root.join("codex ' native");
    crate::executable::write(&binary, "#!/bin/sh\nprintf 'codex-cli 0.164.0\\n'\n");
    let launcher = daemon.root.join("akasha codex");
    let state = daemon.root.join("call state");
    let install = || {
        daemon
            .command()
            .args([
                "adapter",
                "install",
                "codex",
                "--binary",
                binary.to_str().unwrap(),
                "--launcher",
                launcher.to_str().unwrap(),
                "--state-dir",
                state.to_str().unwrap(),
            ])
            .output()
            .unwrap()
    };
    let first = install();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let original = std::fs::read(&launcher).unwrap();
    assert_eq!(std::fs::metadata(&launcher).unwrap().mode() & 0o777, 0o700);
    assert!(
        Command::new(&launcher)
            .arg("--version")
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(install().status.success());
    assert_eq!(std::fs::read(&launcher).unwrap(), original);
    let mut edited = original.clone();
    edited.extend_from_slice(b"# user change\n");
    std::fs::write(&launcher, &edited).unwrap();
    assert!(!install().status.success());
    let undo = || {
        daemon
            .command()
            .args([
                "adapter",
                "uninstall",
                "codex",
                "--launcher",
                launcher.to_str().unwrap(),
            ])
            .output()
            .unwrap()
    };
    assert!(!undo().status.success());
    assert_eq!(std::fs::read(&launcher).unwrap(), edited);
    std::fs::write(&launcher, original).unwrap();
    assert!(undo().status.success());
    assert!(!launcher.exists());
    std::os::unix::fs::symlink(&binary, &launcher).unwrap();
    assert!(!install().status.success());
    assert!(!undo().status.success());
    assert!(binary.exists());
}
