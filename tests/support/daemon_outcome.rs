use super::*;

fn rpc(daemon: &Daemon, command: Value) -> Value {
    let mut stream = daemon.connect();
    receive(&mut stream);
    send(
        &mut stream,
        &json!({"protocol_version":1,"request_id":"outcome-test","command":command}),
    );
    receive(&mut stream)
}

fn finished(daemon: &Daemon, id: &str) -> Value {
    let mut wait = super::job_wait::JobWait::default();
    loop {
        let job = daemon.cli(&["job", "show", id])["data"].clone();
        if !matches!(job["state"].as_str(), Some("queued" | "running")) {
            assert_eq!(job["state"], "succeeded", "{job}");
            return job;
        }
        wait.check(&job);
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn outcomes_bind_only_attached_receipt_revisions_and_survive_restart() {
    let (mut daemon, call, binding) =
        super::codex_tests::consult_fixture_with("research", |_| String::new());
    daemon.cli(&["index", "sync", "--idempotency-key", "outcome-index"]);
    finished(&daemon, "outcome-index");
    let submitted = rpc(&daemon, json!({"op":"codex","event":call}));
    assert!(submitted.get("error").is_none(), "{submitted}");
    let job = finished(&daemon, submitted["data"]["id"].as_str().unwrap());
    let now = akasha_otsuge::ipc::now_ms().unwrap();
    let snapshot = json!({"base_commit":"a".repeat(40),"index_sha256":"b".repeat(64),
        "diff_sha256":"c".repeat(64),"tree_sha256":"d".repeat(64),"gaps":[]});
    let event = json!({
        "event_id":"build-one","invocation_id":binding["data"]["invocation_id"],
        "source":"test-framework","severity":"error","occurred_at_ms":now,
        "execution":{"argv":["cargo","test"],"started_at_ms":now-10,"finished_at_ms":now,
            "exit_code":101,"signal":null,"workspace":daemon.root.join("workspace"),
            "before":snapshot,"after":snapshot},
        "artifacts":[{"path":"target/app","bytes":42,"sha256":"e".repeat(64)}],
        "logs":[],"receipt_jobs":[job["id"]],"claims":[]
    });
    let command = json!({"op":"outcome_submit","event":event});
    let accepted = rpc(&daemon, command.clone());
    assert!(
        accepted.get("error").is_none(),
        "outcome entry unavailable: {accepted}"
    );
    assert!(accepted["data"]["commit_seq"].as_u64().is_some());
    let shown = daemon.cli(&["outcome", "show", "build-one"])["data"].clone();
    assert_eq!(shown["run_id"], binding["data"]["invocation_id"]);
    assert_eq!(shown["event"], event);
    let links = shown["links"].as_array().unwrap();
    assert!(!links.is_empty());
    assert!(links.iter().all(|link| link["relation"] == "attached"));
    assert!(
        links
            .iter()
            .all(|link| link["memory"]["key"]["id"] == "offline")
    );
    assert!(shown.get("incident").is_none());
    assert_eq!(
        shown["receipts"][0]["receipt_id"],
        job["result"]["receipt_id"]
    );
    for (field, value, code) in [
        (
            "receipt_jobs",
            json!(["outcome-index"]),
            "INVALID_OUTCOME_RECEIPT",
        ),
        (
            "receipt_jobs",
            json!([job["id"], job["id"]]),
            "INVALID_OUTCOME",
        ),
        (
            "claims",
            json!([{"job_id":job["id"],"memory":{"key":{"kind":"memory","id":"weather"},"commit_seq":1},"relation":"suspected","reason":"unexposed","evidence":[]}]),
            "INVALID_OUTCOME_CLAIM",
        ),
        (
            "claims",
            json!([{"job_id":job["id"],"memory":links[0]["memory"],"relation":"returned","reason":"forged delivery","evidence":[]}]),
            "INVALID_OUTCOME_CLAIM",
        ),
        (
            "claims",
            json!([{"job_id":job["id"],"memory":links[0]["memory"],"relation":"validated","reason":"no evidence","evidence":[]}]),
            "INVALID_OUTCOME_CLAIM",
        ),
    ] {
        let mut invalid = event.clone();
        invalid["event_id"] = json!("rejected");
        invalid[field] = value;
        assert_eq!(
            rpc(&daemon, json!({"op":"outcome_submit","event":invalid}))["error"]["code"],
            code
        );
    }
    let mut second = event.clone();
    second["event_id"] = json!("build-two");
    second["execution"]["exit_code"] = json!(0);
    second["artifacts"][0]["sha256"] = json!("f".repeat(64));
    second["claims"] = json!([{"job_id":job["id"],"memory":links[0]["memory"],"relation":"cited_or_claimed_used","reason":"explicit caller claim, not observed use","evidence":[]}]);
    let path = daemon.root.join("second-outcome.json");
    std::fs::write(&path, serde_json::to_vec(&second).unwrap()).unwrap();
    daemon.cli(&["outcome", "submit", path.to_str().unwrap()]);
    let second = daemon.cli(&["outcome", "show", "build-two"])["data"].clone();
    assert_eq!(second["run_id"], shown["run_id"]);
    assert_ne!(second["event"]["artifacts"], shown["event"]["artifacts"]);
    assert_eq!(second["links"].as_array().unwrap().len(), 2);
    assert_eq!(second["links"][1]["relation"], "cited_or_claimed_used");
    assert!(second.get("incident").is_none());
    assert_eq!(rpc(&daemon, command.clone())["data"], accepted["data"]);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(daemon.cli(&["outcome", "show", "build-one"])["data"], shown);
    assert_eq!(rpc(&daemon, command.clone())["data"], accepted["data"]);
    let mut conflict = command;
    conflict["event"]["execution"]["exit_code"] = json!(0);
    assert_eq!(
        rpc(&daemon, conflict)["error"]["code"],
        "IDEMPOTENCY_CONFLICT"
    );
    daemon.cli(&["access", "revoke"]);
    assert_eq!(
        rpc(&daemon, json!({"op":"outcome_show","id":"build-one"}))["error"]["code"],
        "READ_DENIED"
    );
    assert_eq!(
        rpc(&daemon, json!({"op":"outcome_submit","event":event}))["error"]["code"],
        "READ_DENIED"
    );
}

#[test]
fn outcome_wrapper_records_dirty_builds_and_retains_full_logs_without_claiming_causality() {
    use sha2::{Digest, Sha256};
    let sha = |bytes: &[u8]| {
        Sha256::digest(bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>()
    };
    let (daemon, call, binding) =
        super::codex_tests::consult_fixture_with("research", |_| String::new());
    daemon.cli(&["index", "sync", "--idempotency-key", "outcome-build-index"]);
    finished(&daemon, "outcome-build-index");
    let submitted = rpc(&daemon, json!({"op":"codex","event":call}));
    let job = finished(&daemon, submitted["data"]["id"].as_str().unwrap());
    let workspace = daemon.root.join("workspace");
    let git = |args: &[&str]| {
        let result = Command::new("git")
            .current_dir(&workspace)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .args(args)
            .output()
            .unwrap();
        assert!(
            result.status.success(),
            "{}",
            String::from_utf8_lossy(&result.stderr)
        );
        result.stdout
    };
    git(&["init", "--initial-branch=main"]);
    std::fs::write(workspace.join("main.rs"), "fn main() {}\n").unwrap();
    std::fs::write(workspace.join(".gitignore"), "/app\n/artifact.txt\n").unwrap();
    std::fs::write(workspace.join("build.sh"),
        "printf '%s' \"$1\" > artifact.txt\npython3 -c 'import sys; sys.stdout.write(\"payload-log-\"*20000)'\n\"$2\" main.rs -o app\n").unwrap();
    git(&["add", "."]);
    git(&[
        "-c",
        "user.name=Fixture",
        "-c",
        "user.email=fixture@example.invalid",
        "-c",
        "commit.gpgsign=false",
        "-c",
        "core.hooksPath=/dev/null",
        "commit",
        "-m",
        "fixture baseline",
    ]);
    let base = String::from_utf8(git(&["rev-parse", "HEAD"]))
        .unwrap()
        .trim()
        .to_owned();
    std::fs::write(
        workspace.join("main.rs"),
        "fn main() { does_not_exist(); }\n",
    )
    .unwrap();
    std::fs::write(
        workspace.join("untracked name.txt"),
        "first uncommitted input",
    )
    .unwrap();
    let diff = sha(&git(&[
        "diff",
        "--no-ext-diff",
        "--no-textconv",
        "--binary",
        "HEAD",
        "--",
        ".",
    ]));
    let sysroot = Command::new("rustc")
        .args(["--print", "sysroot"])
        .output()
        .unwrap();
    assert!(sysroot.status.success());
    let compiler =
        PathBuf::from(String::from_utf8(sysroot.stdout).unwrap().trim()).join("bin/rustc");
    assert!(compiler.is_file());
    let plan_path = daemon.root.join("run-plan.json");
    let mut plan = json!({"event_id":"real-build-one","invocation_id":binding["data"]["invocation_id"],"artifacts":["artifact.txt"]});
    std::fs::write(&plan_path, serde_json::to_vec(&plan).unwrap()).unwrap();
    let run = |artifact: &str| {
        daemon
            .command()
            .args([
                "outcome",
                "run",
                plan_path.to_str().unwrap(),
                "--",
                "sh",
                "build.sh",
                artifact,
                compiler.to_str().unwrap(),
            ])
            .output()
            .unwrap()
    };
    let first = run("first artifact");
    assert_eq!(
        first.status.code(),
        Some(1),
        "wrapper did not report the real compiler failure: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let first: Value = serde_json::from_slice(&first.stdout)
        .expect("wrapper must return the recorded outcome even when the command fails");
    let first = &first["data"];
    assert_eq!(first["event"]["execution"]["before"]["base_commit"], base);
    assert_eq!(first["event"]["execution"]["before"]["diff_sha256"], diff);
    assert_eq!(first["event"]["execution"]["before"]["gaps"], json!([]));
    assert_eq!(first["event"]["execution"]["exit_code"], 1);
    assert_eq!(first["event"]["severity"], "info");
    assert_eq!(first["event"]["receipt_jobs"], json!([job["id"]]));
    assert_eq!(
        first["event"]["artifacts"][0]["sha256"],
        sha(b"first artifact")
    );
    assert!(first.get("incident").is_none());
    assert!(
        first["links"]
            .as_array()
            .unwrap()
            .iter()
            .all(|v| v["relation"] == "attached")
    );
    assert!(first["logs"][0]["preview"].as_str().unwrap().len() <= 4096);
    assert_eq!(first["logs"][0]["preview_truncated"], true);
    assert!(
        first["logs"][1]["preview"]
            .as_str()
            .unwrap()
            .contains("does_not_exist")
    );
    let expected = "payload-log-".repeat(20_000).into_bytes();
    assert_eq!(first["logs"][0]["sha256"], sha(&expected));
    let mut bytes = Vec::new();
    let mut start = 0;
    loop {
        let part = daemon.cli(&[
            "outcome",
            "log",
            "real-build-one",
            "0",
            "--start",
            &start.to_string(),
        ]);
        let data = &part["data"];
        let raw = data["raw_hex"].as_str().unwrap();
        assert!(raw.len() <= 64 * 1024);
        bytes.extend(
            (0..raw.len())
                .step_by(2)
                .map(|i| u8::from_str_radix(&raw[i..i + 2], 16).unwrap()),
        );
        let Some(next) = data["next_start"].as_u64() else {
            break;
        };
        assert!(next > start);
        start = next;
    }
    assert_eq!(bytes, expected);
    let duplicate = run("must not execute");
    let denied: Value = serde_json::from_slice(&duplicate.stderr).unwrap();
    assert_eq!(denied["error"]["code"], "OUTCOME_ALREADY_STARTED");
    assert_eq!(
        std::fs::read(workspace.join("artifact.txt")).unwrap(),
        b"first artifact"
    );
    std::fs::write(
        workspace.join("untracked name.txt"),
        "second uncommitted input",
    )
    .unwrap();
    plan["event_id"] = json!("real-build-two");
    std::fs::write(&plan_path, serde_json::to_vec(&plan).unwrap()).unwrap();
    let second = run("second artifact");
    assert_eq!(second.status.code(), Some(1));
    let second: Value = serde_json::from_slice(&second.stdout).unwrap();
    let second = &second["data"];
    assert_eq!(second["run_id"], first["run_id"]);
    assert_eq!(second["event"]["execution"]["before"]["diff_sha256"], diff);
    assert_ne!(
        second["event"]["execution"]["before"]["tree_sha256"],
        first["event"]["execution"]["before"]["tree_sha256"]
    );
    assert_ne!(second["event"]["artifacts"], first["event"]["artifacts"]);
    assert_eq!(
        daemon.cli(&["outcome", "show", "real-build-one"])["data"],
        *first
    );
    plan["event_id"] = json!("environment-failure");
    plan["artifacts"] = json!([]);
    std::fs::write(&plan_path, serde_json::to_vec(&plan).unwrap()).unwrap();
    let missing = daemon
        .command()
        .args([
            "outcome",
            "run",
            plan_path.to_str().unwrap(),
            "--",
            workspace.join("missing-executable").to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert_eq!(missing.status.code(), Some(127));
    let missing: Value = serde_json::from_slice(&missing.stdout).unwrap();
    assert!(missing["data"]["event"]["execution"]["spawn_error"].is_string());
    assert!(missing["data"].get("incident").is_none());
    assert_eq!(missing["data"]["event"]["claims"], json!([]));
    daemon.cli(&["access", "revoke"]);
    plan["event_id"] = json!("revoked-run");
    std::fs::write(&plan_path, serde_json::to_vec(&plan).unwrap()).unwrap();
    let denied = run("denied artifact");
    assert!(!denied.status.success());
    assert_eq!(
        std::fs::read(workspace.join("artifact.txt")).unwrap(),
        b"second artifact"
    );
}
