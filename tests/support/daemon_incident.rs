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

fn investigated(daemon: &Daemon, id: &str) -> Value {
    let mut wait = super::job_wait::JobWait::default();
    loop {
        let item = daemon.cli(&["incident", "show", id])["data"].clone();
        if item["record"]["investigation"] == "finished" {
            return item;
        }
        if let Some(job_id) = item["record"]["job_id"].as_str() {
            let job = daemon.cli(&["job", "show", job_id])["data"].clone();
            if matches!(job["state"].as_str(), Some("queued" | "running")) {
                wait.check(&job);
            }
        }
        let now = akasha_otsuge::ipc::now_ms().unwrap();
        let admission_end = item["record"]["first_report_at_ms"].as_u64().unwrap() + 30_000;
        assert!(
            item["record"]["job_id"].is_string() || now < admission_end,
            "automatic investigation was not admitted within the configured queue budget: {item}"
        );
        assert!(
            now < item["record"]["expires_at_ms"].as_u64().unwrap(),
            "{item}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn configured_incident_investigation_runs_once_and_keeps_environment_reports_separate() {
    let (daemon, _, bound) = super::codex_tests::consult_fixture_with(
        "editor_environment",
        |root| {
            format!(
                "[review]\nmode='automatic'\n[timeouts]\nqueue='30s'\n[incidents]\nexecutor='editor'\ntriggers=['reproducible_failure']\nauto_repair=true\nmax_runner_starts=1\nmax_tokens=64000\n[executors.editor]\nkind='codex'\nbinary={:?}\nnative_home={:?}\n",
                root.join("codex"),
                root.join("native-home")
            )
        },
    );
    let original = daemon.cli(&["memory", "export", "editable"])["data"].clone();
    let mut event = json!({"event_id":"network-report","invocation_id":bound["data"]["invocation_id"],
        "source":"network-test","severity":"error","occurred_at_ms":akasha_otsuge::ipc::now_ms().unwrap(),
        "execution":null,"artifacts":[],"logs":[],"receipt_jobs":[],"claims":[],
        "incident":{"kind":"reproducible_failure","key":"network-fixture","reason":"Compare the reported failure with the memory, originals and notes.rs; attachment is not causal proof.",
            "targets":[{"memory_id":"editable","expected_revision":original["revision"]["commit_seq"]}]}});
    let submitted = rpc(&daemon, json!({"op":"outcome_submit","event":event}));
    assert!(submitted.get("error").is_none(), "{submitted}");
    let id = daemon.cli(&["outcome", "show", "network-report"])["data"]["incident"]
        .as_str()
        .unwrap()
        .to_owned();
    let result = investigated(&daemon, &id);
    assert_eq!(
        result["record"]["classification"], "environment",
        "{result}"
    );
    // The source report has no execution evidence; investigation must keep that uncertainty.
    assert_eq!(result["record"]["state"], "open");
    assert!(result["record"]["error"].is_null(), "{result}");
    let job_id = result["record"]["job_id"].as_str().unwrap();
    let job = daemon.cli(&["job", "show", job_id])["data"].clone();
    assert_eq!(job["state"], "succeeded", "{job}");
    assert_eq!(job["result"]["editor"]["classification"], "environment");
    assert!(job["result"]["patch_receipt"].is_null());
    assert_eq!(job["result"]["usage"]["starts"], 1);
    assert_eq!(
        std::fs::read_to_string(daemon.root.join("starts"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    event["event_id"] = json!("network-repeat");
    assert!(
        rpc(&daemon, json!({"op":"outcome_submit","event":event}))
            .get("error")
            .is_none()
    );
    let duplicate = investigated(&daemon, &id);
    assert_eq!(duplicate["record"]["job_id"], result["record"]["job_id"]);
    assert_eq!(duplicate["record"]["occurrences"], 2);
    assert_eq!(
        std::fs::read_to_string(daemon.root.join("starts"))
            .unwrap()
            .lines()
            .count(),
        1
    );
    assert_eq!(
        daemon.cli(&["memory", "export", "editable"])["data"],
        original
    );
}

#[test]
fn incident_context_reads_outcome_and_full_logs_before_policy_controlled_repair() {
    for (auto_repair, completeness, authority) in [
        (true, "complete", "delegated_approved"),
        (false, "complete", "candidate"),
        (true, "partial", "candidate"),
    ] {
        let (daemon, _, bound) = super::codex_tests::consult_fixture_with(
            "editor_incident",
            |root| {
                format!(
                    "[review]\nmode='automatic'\n[timeouts]\nqueue='30s'\n[incidents]\nexecutor='editor'\ntriggers=['reproducible_failure']\nauto_repair={auto_repair}\nmax_runner_starts=1\nmax_tokens=64000\n[executors.editor]\nkind='codex'\nbinary={:?}\nnative_home={:?}\n",
                    root.join("codex"),
                    root.join("native-home")
                )
            },
        );
        let original = daemon.cli(&["memory", "export", "editable"])["data"].clone();
        let now = akasha_otsuge::ipc::now_ms().unwrap();
        let raw = "controlled fixture failure: only Linux was verified\n".repeat(1000);
        let saved = rpc(
            &daemon,
            json!({"op":"outcome_log_write","chunk":{
            "invocation_id":bound["data"]["invocation_id"],"event_id":"platform-failure","stream":"stderr","part":0,
            "observed_at_ms":now,"raw_hex":akasha_otsuge::capture::encode_hex(raw.as_bytes())}}),
        );
        assert!(saved.get("error").is_none(), "{saved}");
        let snapshot = json!({"base_commit":"a".repeat(40),"index_sha256":"b".repeat(64),"diff_sha256":"c".repeat(64),"tree_sha256":"d".repeat(64),"gaps":[]});
        let event = json!({"event_id":"platform-failure","invocation_id":bound["data"]["invocation_id"],
            "source":"controlled-platform-test","severity":"error","occurred_at_ms":now,
            "execution":{"argv":["controlled-compiler"],"started_at_ms":now,"finished_at_ms":now,"exit_code":1,"signal":null,
                "workspace":daemon.root.join("workspace"),"before":snapshot,"after":snapshot},
            "artifacts":[],"logs":[{"stream":"stderr","reference":saved["data"],"completeness":completeness}],"receipt_jobs":[],"claims":[],
            "incident":{"kind":"reproducible_failure","key":"platform-check","reason":"Check the platform claim against its originals, notes.rs and the recorded outcome/logs.",
                "targets":[{"memory_id":"editable","expected_revision":original["revision"]["commit_seq"]}]}});
        let submitted = rpc(&daemon, json!({"op":"outcome_submit","event":event}));
        assert!(submitted.get("error").is_none(), "{submitted}");
        let id = daemon.cli(&["outcome", "show", "platform-failure"])["data"]["incident"]
            .as_str()
            .unwrap()
            .to_owned();
        let result = investigated(&daemon, &id);
        let description: Value = serde_json::from_slice(
            &std::fs::read(daemon.root.join("incident-context.json")).unwrap(),
        )
        .unwrap();
        assert!(
            description["investigation"].is_object(),
            "the native context has no admitted outcome handles: {description}"
        );
        assert_eq!(
            result["record"]["classification"], "memory_error",
            "{result}"
        );
        let reads: Value = serde_json::from_slice(
            &std::fs::read(daemon.root.join("incident-evidence.json")).unwrap(),
        )
        .unwrap();
        let handle = description["investigation"]["logs"][0]["handle"]
            .as_str()
            .unwrap();
        use sha2::Digest;
        assert_eq!(
            reads[handle]["sha256"],
            akasha_otsuge::capture::encode_hex(&sha2::Sha256::digest(raw.as_bytes()))
        );
        assert_eq!(reads[handle]["bytes"], raw.len());
        assert!(reads[handle]["pages"].as_u64().unwrap() > 1);
        let changed = daemon.cli(&["memory", "export", "editable"])["data"].clone();
        assert_eq!(changed["record"]["authority"], authority, "{result}");
        assert_eq!(
            changed["record"]["draft"]["scope"],
            original["record"]["draft"]["scope"]
        );
        assert_eq!(changed["previous"], original["revision"]["commit_seq"]);
        let job =
            daemon.cli(&["job", "show", result["record"]["job_id"].as_str().unwrap()])["data"]
                .clone();
        if completeness == "partial" {
            assert!(
                job["result"]["gaps"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|g| g == "outcome_log_incomplete"),
                "{job}"
            );
        } else {
            assert_eq!(job["result"]["gaps"], json!([]), "{job}");
        }
        assert_eq!(
            result["record"]["state"],
            if authority == "candidate" {
                "open"
            } else {
                "resolved"
            }
        );
    }
}

#[test]
fn incident_classifications_keep_nonmemory_failures_and_missing_recall_out_of_patches() {
    for (classification, expected_state) in [
        ("code_bug", "dismissed"),
        ("environment", "dismissed"),
        ("execution_error", "dismissed"),
        ("requirements_changed", "dismissed"),
        ("retrieval_gap", "open"),
        ("insufficient_evidence", "open"),
        ("stale_conditions", "resolved"),
    ] {
        let (daemon, _, bound) = super::codex_tests::consult_fixture_with(
            &format!("editor_incident_{classification}"),
            |root| {
                format!(
                    "[review]\nmode='automatic'\n[timeouts]\nqueue='30s'\n[incidents]\nexecutor='editor'\ntriggers=['user_correction']\nauto_repair=true\nmax_runner_starts=1\n[executors.editor]\nkind='codex'\nbinary={:?}\nnative_home={:?}\n",
                    root.join("codex"),
                    root.join("native-home")
                )
            },
        );
        let original = daemon.cli(&["memory", "export", "editable"])["data"].clone();
        let targets = if classification == "retrieval_gap" {
            json!([])
        } else {
            json!([{"memory_id":"editable","expected_revision":original["revision"]["commit_seq"]}])
        };
        let event = json!({"event_id":"classified","invocation_id":bound["data"]["invocation_id"],
            "source":"controlled-classification","severity":"warning","occurred_at_ms":akasha_otsuge::ipc::now_ms().unwrap(),
            "execution":null,"artifacts":[],"logs":[],"receipt_jobs":[],"claims":[],
            "incident":{"kind":"user_correction","key":"reported-problem","reason":"Investigate the reported problem without assuming memory is responsible.","targets":targets}});
        let response = rpc(&daemon, json!({"op":"outcome_submit","event":event}));
        assert!(response.get("error").is_none(), "{response}");
        let outcome = daemon.cli(&["outcome", "show", "classified"]);
        let result = investigated(&daemon, outcome["data"]["incident"].as_str().unwrap());
        assert_eq!(
            result["record"]["classification"], classification,
            "{result}"
        );
        assert_eq!(result["record"]["state"], expected_state, "{result}");
        assert_eq!(result["record"]["priority"], "urgent");
        let after = daemon.cli(&["memory", "export", "editable"])["data"].clone();
        if classification == "stale_conditions" {
            assert_eq!(after["previous"], original["revision"]["commit_seq"]);
            assert_eq!(
                after["record"]["draft"]["scope"],
                original["record"]["draft"]["scope"]
            );
        } else {
            assert_eq!(after, original, "{classification}");
        }
    }
}

#[test]
fn incident_grouping_and_priority_follow_policy_without_starting_unconfigured_runners() {
    for deduplicate in [true, false] {
        let (daemon, _, bound) = super::codex_tests::consult_fixture_with("editor_ledger", |_| {
            format!(
                "[incidents]\ntriggers=['expired','dependency_changed','user_correction']\ndeduplicate={deduplicate}\nescalate_after=2\n[incidents.priorities]\nexpired='urgent'\n"
            )
        });
        let mut ids = Vec::new();
        for n in 0..3 {
            let event = json!({"event_id":format!("expiry-{n}"),"invocation_id":bound["data"]["invocation_id"],
                "source":"maintenance","severity":"info","occurred_at_ms":akasha_otsuge::ipc::now_ms().unwrap(),
                "execution":null,"artifacts":[],"logs":[],"receipt_jobs":[],"claims":[],
                "incident":{"kind":"expired","key":"expiry-group","reason":"The documented validity ended."}});
            let reply = rpc(&daemon, json!({"op":"outcome_submit","event":event}));
            assert!(reply.get("error").is_none(), "{reply}");
            let id = daemon.cli(&["outcome", "show", &format!("expiry-{n}")])["data"]["incident"]
                .as_str()
                .unwrap()
                .to_owned();
            let incident = daemon.cli(&["incident", "show", &id])["data"].clone();
            assert_eq!(incident["record"]["priority"], "urgent");
            assert!(
                incident["record"]["priority_reasons"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|v| v == "configured_trigger")
            );
            assert_eq!(
                incident["record"]["occurrences"],
                if deduplicate { n + 1 } else { 1 }
            );
            ids.push(id);
        }
        assert_eq!(
            ids.iter().collect::<std::collections::BTreeSet<_>>().len(),
            if deduplicate { 1 } else { 3 }
        );
        for (n, kind, severity, expected) in [
            (0, "dependency_changed", "info", "normal"),
            (
                1,
                "dependency_changed",
                "info",
                if deduplicate { "high" } else { "normal" },
            ),
            (2, "dependency_changed", "critical", "urgent"),
            (3, "user_correction", "info", "urgent"),
        ] {
            let event_id = format!("maintenance-{n}");
            let event = json!({"event_id":event_id,"invocation_id":bound["data"]["invocation_id"],
                "source":"maintenance","severity":severity,"occurred_at_ms":akasha_otsuge::ipc::now_ms().unwrap(),
                "execution":null,"artifacts":[],"logs":[],"receipt_jobs":[],"claims":[],
                "incident":{"kind":kind,"key":"maintenance-group","reason":"Explicit maintenance signal."}});
            let reply = rpc(&daemon, json!({"op":"outcome_submit","event":event}));
            assert!(reply.get("error").is_none(), "{reply}");
            let outcome = daemon.cli(&["outcome", "show", &event_id]);
            let incident = daemon.cli(&[
                "incident",
                "show",
                outcome["data"]["incident"].as_str().unwrap(),
            ]);
            assert_eq!(incident["data"]["record"]["priority"], expected);
            assert_eq!(incident["data"]["record"]["state"], "open");
        }
        assert!(!daemon.root.join("starts").exists());
    }
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
