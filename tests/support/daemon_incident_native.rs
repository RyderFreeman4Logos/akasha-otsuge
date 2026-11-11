use super::*;

fn fixture(settings: &str) -> (Daemon, Value, Value) {
    super::super::codex_tests::consult_fixture_prepared(
        "editor_incident_actual",
        |_| settings.to_owned(),
        |daemon| prepare(daemon),
    )
}

fn prepare(daemon: &Daemon) {
    let workspace = daemon.root.join("workspace");
    for (id, version) in [("api-current", "v2"), ("api-v1", "v1")] {
        super::super::retrieval_tests::seed_records_with(
            daemon,
            &[(
                id,
                Some(workspace.to_str().unwrap()),
                "The fixture API v1 exports client::connect_legacy().",
            )],
            1,
            |draft| {
                draft.scope.branch_or_version = Some(version.into());
                draft.claim = format!(
                    "The fixture API {version} exports client::connect_legacy(); call client::connect_legacy() to connect."
                );
                draft.conditions = vec![format!("Only the controlled fixture API {version}.")];
            },
        );
    }
    std::fs::write(
        workspace.join("client.rs"),
        "pub fn connect() -> &'static str { \"v2\" }\n",
    )
    .unwrap();
    std::fs::write(
        workspace.join("fixed.rs"),
        "mod client; fn main() { println!(\"{}\", client::connect()); }\n",
    )
    .unwrap();
    std::fs::write(workspace.join("notes.rs"), "// Controlled API v2 fixture. client.rs defines the available API.\n// main.rs is deliberately constructed from the api-current memory's old function name.\n// fixed.rs is an independent compilation control; API v1 history must remain unchanged.\n").unwrap();
    std::fs::write(workspace.join(".gitignore"), "/app\n/fixed-app\n").unwrap();
}

fn executor_settings(binary: &std::path::Path, home: &std::path::Path) -> String {
    // The Caller fixture already opened [consult]; append its executor before other tables.
    format!(
        "executor='investigator'\nmax_runner_starts=1\n[review]\nmode='automatic'\n[incidents]\nexecutor='investigator'\ntriggers=['reproducible_failure']\nauto_repair=true\nmax_runner_starts=1\nmax_tokens=512000\n[executors.investigator]\nkind='codex'\nbinary={binary:?}\nnative_home={home:?}\n"
    )
}

#[test]
fn native_profile_configuration_precedes_binding_without_authentication() {
    let binary = std::path::Path::new("/bin/false");
    let (mut daemon, call, _) = fixture(&executor_settings(binary, &std::env::temp_dir()));
    let accepted = rpc(&daemon, json!({"op":"codex","event":call}));
    assert!(accepted.get("error").is_none(), "{accepted}");
    assert_eq!(
        accepted["data"]["consult_plan"]["executors"]["investigator"]["binary"],
        json!(binary)
    );
    assert_eq!(accepted["data"]["config_generation"], 1);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}

fn plan(daemon: &Daemon, bound: &Value, original: &Value) -> PathBuf {
    let remembered_call = original["record"]["draft"]["claim"]
        .as_str()
        .unwrap()
        .split_once("; call ")
        .unwrap()
        .1
        .strip_suffix(" to connect.")
        .unwrap();
    assert_eq!(remembered_call, "client::connect_legacy()");
    let workspace = daemon.root.join("workspace");
    std::fs::write(
        workspace.join("main.rs"),
        format!("mod client; fn main() {{ println!(\"{{}}\", {remembered_call}); }}\n"),
    )
    .unwrap();
    commit_fixture(&workspace);
    let file = daemon.root.join("old-api.json");
    std::fs::write(&file,serde_json::to_vec(&json!({
        "event_id":"old-api-failure","invocation_id":bound["data"]["invocation_id"],"artifacts":[],
        "incident":{"kind":"reproducible_failure","key":"fixture-api-v2","reason":"Investigate api-current against its originals, the recorded compiler error, client.rs, main.rs, fixed.rs and notes.rs. The fixture constructed main.rs from the old function name; a receipt alone is not proof of use. Correct only the listed v2 target if the evidence establishes an outdated fact. Preserve API v1 history and any uncertainty. This is controlled acceptance data, not human approval.",
        "targets":[{"memory_id":"api-current","expected_revision":original["revision"]["commit_seq"]}]}
    })).unwrap()).unwrap();
    file
}

fn compile_failure(daemon: &Daemon, file: &std::path::Path) -> Value {
    let compiler = compiler();
    let output = daemon
        .command()
        .args([
            "outcome",
            "run",
            file.to_str().unwrap(),
            "--",
            compiler.to_str().unwrap(),
            "main.rs",
            "-o",
            "app",
        ])
        .output()
        .unwrap();
    assert_eq!(
        output.status.code(),
        Some(1),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let record = serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"].clone();
    assert_eq!(record["event"]["execution"]["before"]["gaps"], json!([]));
    assert!(
        record["logs"][1]["preview"]
            .as_str()
            .unwrap()
            .contains("connect_legacy")
    );
    record
}

#[test]
fn old_api_fixture_has_independent_compile_controls_and_separate_version_history() {
    let (daemon, _, bound) = fixture("");
    let original = daemon.cli(&["memory", "export", "api-current"])["data"].clone();
    assert!(
        bound["data"]["capture_seq"].as_u64().unwrap()
            >= original["revision"]["commit_seq"].as_u64().unwrap(),
        "the recall fence predates its intended evidence"
    );
    assert_eq!(
        original["record"]["draft"]["scope"]["branch_or_version"],
        "v2"
    );
    scoped_search_preflight(&daemon, &bound, &original);
    let record = compile_failure(&daemon, &plan(&daemon, &bound, &original));
    assert!(record.get("incident").is_none());
    let workspace = daemon.root.join("workspace");
    let success = Command::new(compiler())
        .current_dir(&workspace)
        .args(["fixed.rs", "-o", "fixed-app"])
        .output()
        .unwrap();
    assert!(
        success.status.success(),
        "{}",
        String::from_utf8_lossy(&success.stderr)
    );
    assert_eq!(
        Command::new(workspace.join("fixed-app"))
            .output()
            .unwrap()
            .stdout,
        b"v2\n"
    );
    assert_eq!(
        daemon.cli(&["memory", "export", "api-v1"])["data"]["record"]["draft"]["scope"]["branch_or_version"],
        "v1"
    );
    assert!(!daemon.root.join("starts").exists());
}

fn save(path: &std::path::Path, receipt: &Value) {
    std::fs::write(path, serde_json::to_vec_pretty(receipt).unwrap()).unwrap();
}

fn job(daemon: &Daemon, id: &str, output: &std::path::Path, receipt: &mut Value) -> Value {
    let mut wait = super::super::job_wait::JobWait::default();
    let mut previous = Value::Null;
    loop {
        let job = daemon.cli(&["job", "show", id])["data"].clone();
        let progress = json!({"id":id,"state":job["state"],"trace":job["run_trace"]});
        if progress != previous {
            receipt["progress"] = progress.clone();
            previous = progress;
            save(output, receipt);
        }
        if !matches!(job["state"].as_str(), Some("queued" | "running")) {
            return job;
        }
        wait.check(&job);
        std::thread::sleep(Duration::from_millis(500));
    }
}

fn scoped_search_preflight(daemon: &Daemon, bound: &Value, original: &Value) -> Value {
    let output = daemon.root.join("scope-preflight.json");
    let mut receipt = json!({});
    daemon.cli(&["index", "sync", "--idempotency-key", "scope-index"]);
    assert_eq!(
        job(daemon, "scope-index", &output, &mut receipt)["state"],
        "succeeded"
    );
    let historical = daemon.cli(&["memory", "export", "api-v1"])["data"].clone();
    for (id, scope, expected) in [
        ("scope-owner", &bound["data"]["scope"], None),
        (
            "scope-v2",
            &original["record"]["draft"]["scope"],
            Some("api-current"),
        ),
        (
            "scope-v1",
            &historical["record"]["draft"]["scope"],
            Some("api-v1"),
        ),
    ] {
        let file = daemon.root.join(format!("{id}.json"));
        std::fs::write(&file, serde_json::to_vec(scope).unwrap()).unwrap();
        daemon.cli(&[
            "search",
            "connect_legacy",
            "--scope",
            file.to_str().unwrap(),
            "--idempotency-key",
            id,
        ]);
        let finished = job(daemon, id, &output, &mut receipt);
        assert_eq!(finished["state"], "succeeded", "{finished}");
        let result: Value =
            serde_json::from_str(finished["result"]["output"].as_str().unwrap()).unwrap();
        let ids: Vec<_> = result["items"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item["memory"]["revision"]["key"]["id"].as_str().unwrap())
            .collect();
        assert_eq!(ids, expected.into_iter().collect::<Vec<_>>(), "{result}");
        receipt[id] = result;
    }
    receipt
}

#[test]
#[ignore = "requires an explicitly supplied authenticated Codex profile and evidence path"]
fn native_incident_actual() {
    let binary = PathBuf::from(std::env::var_os("AKASHA_NATIVE_INCIDENT_BINARY").unwrap());
    let home = PathBuf::from(std::env::var_os("AKASHA_NATIVE_INCIDENT_HOME").unwrap());
    let output = PathBuf::from(std::env::var_os("AKASHA_NATIVE_INCIDENT_RECEIPT").unwrap());
    assert!(!output.exists());
    let settings = executor_settings(&binary, &home);
    let (mut daemon, _, bound) = fixture(&settings);
    let original = daemon.cli(&["memory", "export", "api-current"])["data"].clone();
    let historical = daemon.cli(&["memory", "export", "api-v1"])["data"].clone();
    let unrelated = daemon.cli(&["memory", "export", "editable"])["data"].clone();
    let mut receipt = json!({"schema_version":1,"status":"RUNNING","source":"controlled Caller; program built from exported v2 memory; actual authenticated Codex investigator, actual rustc failure and successful independent control; no native recall claim","scratch":daemon.root,"before":original,"historical_before":historical,"unrelated_before":unrelated});
    save(&output, &receipt);
    daemon.configure(&format!(
        "[access]\nread_roots=[{:?},{home:?}]\n[consult]\nmax_delegation_depth=0\n{settings}",
        daemon.root
    ));
    daemon.cli(&["config", "reload"]);
    receipt["scope_preflight"] = scoped_search_preflight(&daemon, &bound, &original);
    receipt["status"] = json!("FAILED");
    save(&output, &receipt);
    let file = plan(&daemon, &bound, &original);
    let outcome = compile_failure(&daemon, &file);
    receipt["outcome"] = outcome.clone();
    save(&output, &receipt);
    assert!(outcome["links"].as_array().unwrap().is_empty());
    let incident_id = outcome["incident"].as_str().unwrap();
    let end = Instant::now() + Duration::from_secs(300);
    let admitted = loop {
        let item = daemon.cli(&["incident", "show", incident_id])["data"].clone();
        if item["record"]["job_id"].is_string() {
            break item;
        }
        assert!(
            item["record"]["investigation"] != "finished" && Instant::now() < end,
            "{item}"
        );
        std::thread::sleep(Duration::from_millis(50));
    };
    let finished = job(
        &daemon,
        admitted["record"]["job_id"].as_str().unwrap(),
        &output,
        &mut receipt,
    );
    receipt["investigation_job"] = finished.clone();
    save(&output, &receipt);
    assert_eq!(finished["state"], "succeeded", "{finished}");
    let incident = investigated(&daemon, incident_id);
    let after = daemon.cli(&["memory", "export", "api-current"])["data"].clone();
    receipt["incident"] = incident.clone();
    receipt["after"] = after.clone();
    save(&output, &receipt);
    assert!(matches!(
        incident["record"]["classification"].as_str(),
        Some("memory_error" | "stale_conditions")
    ));
    assert_eq!(after["previous"], original["revision"]["commit_seq"]);
    assert_eq!(
        after["record"]["draft"]["scope"],
        original["record"]["draft"]["scope"]
    );
    match after["record"]["authority"].as_str() {
        Some("delegated_approved") => {
            assert_eq!(incident["record"]["state"], "resolved", "{incident}");
            assert_eq!(finished["result"]["gaps"], json!([]));
            assert_eq!(finished["result"]["conflicts"], json!([]));
            receipt["handling"] = json!("automatic_adoption");
        }
        Some("candidate") => {
            assert_eq!(incident["record"]["state"], "open", "{incident}");
            assert!(!finished["result"]["gaps"].as_array().unwrap().is_empty());
            assert_eq!(
                after["record"]["previous_adoption"],
                original["revision"]["commit_seq"]
            );
            assert_eq!(incident["record"]["priority"], "high");
            receipt["handling"] = json!("evidence_limited_candidate_for_review");
        }
        _ => panic!("unexpected repair authority: {after}"),
    }
    assert_ne!(
        after["record"]["draft"]["claim"],
        original["record"]["draft"]["claim"]
    );
    assert_eq!(
        daemon.cli(&["memory", "export", "api-v1"])["data"],
        historical
    );
    assert_eq!(
        daemon.cli(&["memory", "export", "editable"])["data"],
        unrelated
    );
    let workspace = daemon.root.join("workspace");
    let compiler = compiler();
    let success = Command::new(&compiler)
        .current_dir(&workspace)
        .args(["fixed.rs", "-o", "fixed-app"])
        .output()
        .unwrap();
    assert!(
        success.status.success(),
        "{}",
        String::from_utf8_lossy(&success.stderr)
    );
    assert_eq!(
        Command::new(workspace.join("fixed-app"))
            .output()
            .unwrap()
            .stdout,
        b"v2\n"
    );
    receipt["compiler"] = json!(
        String::from_utf8(Command::new(&compiler).arg("-vV").output().unwrap().stdout).unwrap()
    );
    let replay = rpc(
        &daemon,
        json!({"op":"outcome_submit","event":outcome["event"]}),
    );
    assert!(replay.get("error").is_none(), "{replay}");
    assert_eq!(
        daemon.cli(&["memory", "export", "api-current"])["data"],
        after
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    receipt["status"] = json!("PASS_REAL_NATIVE_INCIDENT");
    save(&output, &receipt);
}
