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
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        let job = daemon.cli(&["job", "show", id])["data"].clone();
        if !matches!(job["state"].as_str(), Some("queued" | "running")) {
            assert_eq!(job["state"], "succeeded", "{job}");
            return job;
        }
        assert!(Instant::now() < end, "{job}");
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
