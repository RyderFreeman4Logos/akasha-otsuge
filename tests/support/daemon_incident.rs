use super::*;

fn rpc(daemon: &Daemon, command: Value) -> Value {
    let mut stream = daemon.connect();
    receive(&mut stream);
    send(
        &mut stream,
        &json!({"protocol_version":1,"request_id":"incident-test","command":command}),
    );
    receive(&mut stream)
}

#[test]
fn incident_signals_deduplicate_durably_without_inventing_memory_causality() {
    let (mut daemon, _, bound) = super::codex_tests::consult_fixture_with("editor_ledger", |_| {
        "[incidents]\ntriggers=['user_correction','dependency_changed','expired','contradiction','reproducible_failure']\n".into()
    });
    let original = daemon.cli(&["memory", "export", "editable"])["data"].clone();
    let target =
        json!({"memory_id":"editable","expected_revision":original["revision"]["commit_seq"]});
    let plain = json!({"event_id":"ordinary-failure","invocation_id":bound["data"]["invocation_id"],
        "source":"compiler","severity":"error","occurred_at_ms":akasha_otsuge::ipc::now_ms().unwrap(),
        "execution":null,"artifacts":[],"logs":[],"receipt_jobs":[],"claims":[]});
    assert!(
        rpc(&daemon, json!({"op":"outcome_submit","event":plain}))
            .get("error")
            .is_none()
    );
    assert!(
        daemon.cli(&["outcome", "show", "ordinary-failure"])["data"]
            .get("incident")
            .is_none()
    );
    let mut event = plain.clone();
    event["event_id"] = json!("reported-0");
    event["incident"] = json!({"kind":"user_correction","key":"platform-claim","reason":"The platform claim needs investigation, not assumed blame.","targets":[target]});
    let accepted = rpc(&daemon, json!({"op":"outcome_submit","event":event}));
    assert!(
        accepted.get("error").is_none(),
        "configured incident signal rejected: {accepted}"
    );
    let id = daemon.cli(&["outcome", "show", "reported-0"])["data"]["incident"]
        .as_str()
        .unwrap()
        .to_owned();
    assert_eq!(
        rpc(&daemon, json!({"op":"outcome_submit","event":event}))["data"],
        accepted["data"]
    );
    for number in 1..100 {
        event["event_id"] = json!(format!("reported-{number}"));
        assert!(
            rpc(&daemon, json!({"op":"outcome_submit","event":event}))
                .get("error")
                .is_none()
        );
    }
    let first = daemon.cli(&["incident", "show", &id])["data"].clone();
    assert_eq!(first["record"]["occurrences"], 100);
    assert_eq!(first["record"]["state"], "open");
    assert_eq!(first["record"]["priority"], "urgent");
    assert_eq!(first["record"]["error"], "EXECUTOR_UNAVAILABLE");
    assert!(first["record"]["job_id"].is_null());
    let mut events = std::collections::BTreeSet::new();
    let mut page = first.clone();
    loop {
        for item in page["events"].as_array().unwrap() {
            assert!(events.insert(item["event_id"].as_str().unwrap().to_owned()));
        }
        let Some(next) = page["next"].as_str() else {
            break;
        };
        page = daemon.cli(&["incident", "show", &id, "--after", next])["data"].clone();
    }
    assert_eq!(events.len(), 100);
    assert!(!daemon.root.join("starts").exists());
    assert_eq!(
        daemon.cli(&["memory", "export", "editable"])["data"],
        original
    );
    let scope_path = daemon.root.join("incident-scope.json");
    std::fs::write(
        &scope_path,
        serde_json::to_vec(&original["record"]["draft"]["scope"]).unwrap(),
    )
    .unwrap();
    let project_only =
        daemon.cli(&["incident", "list", "--scope", scope_path.to_str().unwrap()])["data"].clone();
    assert!(project_only["items"].as_array().unwrap().is_empty());
    // This controlled registered root is owner-scoped, broader than its target memory.
    std::fs::write(
        &scope_path,
        serde_json::to_vec(&first["record"]["scope"]).unwrap(),
    )
    .unwrap();
    let list =
        daemon.cli(&["incident", "list", "--scope", scope_path.to_str().unwrap()])["data"].clone();
    assert_eq!(list["items"].as_array().unwrap().len(), 1);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(daemon.cli(&["incident", "show", &id])["data"], first);
    assert!(
        rpc(&daemon, json!({"op":"outcome_submit","event":event}))["data"]["commit_seq"]
            .is_number()
    );
    assert_eq!(
        daemon.cli(&["incident", "show", &id])["data"]["record"]["occurrences"],
        100
    );
    let update = json!({"id":id,"expected_revision":first["revision"],"state":"dismissed","reason":"Operator excluded memory causality."});
    let path = daemon.root.join("incident-decision.json");
    std::fs::write(&path, serde_json::to_vec(&update).unwrap()).unwrap();
    let updated = daemon.cli(&[
        "incident",
        "update",
        path.to_str().unwrap(),
        "--idempotency-key",
        "dismiss-platform",
    ]);
    assert_eq!(
        daemon.cli(&[
            "incident",
            "update",
            path.to_str().unwrap(),
            "--idempotency-key",
            "dismiss-platform"
        ])["data"],
        updated["data"]
    );
    assert_eq!(
        rpc(
            &daemon,
            json!({"op":"incident_update","update":update,"idempotency_key":"stale-decision"})
        )["error"]["code"],
        "REVISION_CONFLICT"
    );
    assert_eq!(
        daemon.cli(&["incident", "show", &id])["data"]["record"]["state"],
        "dismissed"
    );
    assert_eq!(
        daemon.cli(&["memory", "export", "editable"])["data"],
        original
    );
}
