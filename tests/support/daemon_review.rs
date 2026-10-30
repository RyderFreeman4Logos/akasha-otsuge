use super::extraction_tests::{runtime, scope, storage_config};
use super::*;
use akasha_otsuge::config::ReviewMode;
use akasha_otsuge::domain::*;
use akasha_otsuge::storage::Storage;

fn seed(daemon: &Daemon, mode: ReviewMode, name: &str) -> u64 {
    runtime().block_on(async {
        let store = Storage::open(&storage_config(daemon)).await.unwrap();
        let domain = Domain::new(&store);
        let ingress = Ingress::tool("fixture", "build-receipt", scope()).unwrap();
        let text = "The fixture build passed on Linux.";
        let original = SourceDraft {
            event_id: format!("source-{name}"),
            scope: scope(),
            text: text.into(),
            reported_role: None,
            event_time_ms: None,
            observed_time_ms: 1,
        };
        let source = domain
            .record_source(&original.event_id, &original, &ingress, 1)
            .await
            .unwrap();
        let write = MemoryWrite {
            request_id: format!("proposal-{name}"),
            memory_id: name.into(),
            expected_revision: None,
            draft: MemoryDraft {
                kind: MemoryKind::Fact,
                scope: scope(),
                claim: text.into(),
                rationale: "native build receipt".into(),
                conditions: vec!["Linux".into()],
                assumptions: vec![],
                source_spans: vec![SourceSpan {
                    event_id: original.event_id.clone(),
                    revision: source.commit_seq,
                    content_sha256: domain
                        .source(&original.event_id, source.commit_seq)
                        .unwrap()
                        .content_sha256,
                    start: 0,
                    end: text.len(),
                }],
                valid_from_ms: None,
                valid_until_ms: None,
            },
        };
        let receipt = domain
            .propose(
                &write,
                &ingress,
                &Policy {
                    revision: "fixture-policy".into(),
                    config_generation: 1,
                    mode,
                    scope: scope(),
                    adopt_explicit_decisions: false,
                },
            )
            .await
            .unwrap();
        store.close().await.unwrap();
        receipt.commit_seq
    })
}

fn rpc(daemon: &Daemon, command: Value) -> Value {
    let mut stream = daemon.connect();
    receive(&mut stream);
    send(
        &mut stream,
        &json!({"protocol_version":1,"request_id":"review-test","command":command}),
    );
    receive(&mut stream)
}

#[test]
fn candidate_review_queue_is_durable_and_separate_from_active_memory() {
    let mut daemon = Daemon::new();
    let manual = seed(&daemon, ReviewMode::Manual, "manual");
    let automatic = seed(&daemon, ReviewMode::Automatic, "automatic");
    seed(&daemon, ReviewMode::Policy, "policy");
    daemon.start();
    let listed = rpc(
        &daemon,
        json!({"op":"review_list","scope":scope(),"after":null}),
    );
    assert!(
        listed.get("error").is_none(),
        "review inbox unavailable: {listed}"
    );
    let items = listed["data"]["items"].as_array().unwrap();
    assert_eq!(items.len(), 1, "{listed}");
    let item = &items[0];
    assert_eq!(item["memory_id"], "manual");
    assert_eq!(item["proposal_revision"], manual);
    assert_eq!(item["state"], "open");
    let changed = rpc(
        &daemon,
        json!({"op":"review_state", "idempotency_key":"investigate",
        "id":item["id"],"expected_revision":item["revision"],"state":"investigating","reason":"Check applicability"}),
    );
    assert!(changed.get("error").is_none(), "{changed}");
    let shown = rpc(
        &daemon,
        json!({"op":"review_show","id":item["id"],"at":null}),
    );
    assert_eq!(shown["data"]["item"]["state"], "investigating");
    assert_eq!(
        daemon.cli(&["memory", "show", "automatic"])["data"]["data"]["state"],
        "active"
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    let reopened = rpc(
        &daemon,
        json!({"op":"review_show","id":item["id"],"at":null}),
    );
    assert_eq!(reopened, shown);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    runtime().block_on(async {
        let store = Storage::open(&storage_config(&daemon)).await.unwrap();
        let domain = Domain::new(&store);
        assert_eq!(
            domain
                .active("automatic")
                .unwrap()
                .unwrap()
                .revision
                .commit_seq,
            automatic
        );
        assert!(domain.active("manual").unwrap().is_none());
        assert_eq!(
            domain
                .memory("manual")
                .unwrap()
                .unwrap()
                .revision
                .commit_seq,
            manual
        );
        store.close().await.unwrap();
    });
}

fn operator_cli(daemon: &Daemon, plan: &std::path::Path, id: &str, answer: &str) -> Value {
    let command = daemon.command();
    let args: Vec<_> = std::iter::once(command.get_program())
        .chain(command.get_args())
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    let output = Command::new("python3")
        .arg("-c")
        .arg(include_str!("../fixtures/review_operator.py"))
        .arg(serde_json::to_string(&args).unwrap())
        .arg(plan)
        .arg(id)
        .arg(answer)
        .env("HOME", &daemon.root)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "operator harness: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    serde_json::from_slice(&output.stdout).unwrap()
}

#[test]
fn review_cli_exact_edit_requires_operator_and_preserves_original() {
    let mut daemon = Daemon::new();
    let original_revision = seed(&daemon, ReviewMode::Manual, "edited");
    daemon.start();
    let scope_file = daemon.root.join("scope.json");
    std::fs::write(&scope_file, serde_json::to_vec(&scope()).unwrap()).unwrap();
    let list = daemon.cli(&["review", "list", "--scope", scope_file.to_str().unwrap()]);
    let item = &list["data"]["items"][0];
    let show = daemon.cli(&["review", "show", item["id"].as_str().unwrap()]);
    let original = &show["data"]["proposal"];
    let mut draft = original["record"]["draft"].clone();
    draft["claim"] =
        json!("This build receipt applies only to the fixture, not every Linux build.");
    let decision = json!({"id":item["id"],"expected_revision":item["revision"],
        "action":"approve","draft":draft,"reason":"Limit the claim to the observed build."});
    let plan = daemon.root.join("decision.json");
    std::fs::write(&plan, serde_json::to_vec(&decision).unwrap()).unwrap();
    let preview = daemon.cli(&[
        "review",
        "preview",
        plan.to_str().unwrap(),
        "--idempotency-key",
        "edit-one",
    ]);
    assert_eq!(preview["data"]["before"], *original);
    assert_eq!(preview["data"]["after"], draft);
    let raw = rpc(
        &daemon,
        json!({"op":"review_apply","decision":decision,"idempotency_key":"edit-one",
        "preview_sha256":preview["data"]["preview_sha256"]}),
    );
    assert_eq!(raw["error"]["code"], "REVIEW_OPERATOR_REQUIRED");
    let noninteractive = daemon
        .command()
        .args([
            "review",
            "apply",
            plan.to_str().unwrap(),
            "--idempotency-key",
            "edit-one",
        ])
        .output()
        .unwrap();
    assert!(!noninteractive.status.success());
    assert!(String::from_utf8_lossy(&noninteractive.stderr).contains("REVIEW_OPERATOR_REQUIRED"));
    let declined = operator_cli(&daemon, &plan, "edit-one", "decline");
    assert_ne!(declined["exit_code"], 0);
    assert_eq!(
        daemon.cli(&["review", "show", item["id"].as_str().unwrap()])["data"],
        show["data"]
    );
    let approved = operator_cli(&daemon, &plan, "edit-one", "confirm");
    assert_eq!(approved["exit_code"], 0, "{approved}");
    let response = &approved["response"];
    let seq = response["data"]["commit_seq"].as_u64().unwrap();
    let resolved = daemon.cli(&["review", "show", item["id"].as_str().unwrap()]);
    assert_eq!(resolved["data"]["item"]["state"], "resolved");
    assert_eq!(resolved["data"]["proposal"], *original);
    assert_eq!(resolved["data"]["result"]["record"]["draft"], draft);
    let replay = operator_cli(&daemon, &plan, "edit-one", "confirm");
    assert_eq!(replay["response"]["data"]["commit_seq"], seq);
    let stale = operator_cli(&daemon, &plan, "different-id", "confirm");
    assert_ne!(stale["exit_code"], 0);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(
        daemon.cli(&["review", "show", item["id"].as_str().unwrap()])["data"],
        resolved["data"]
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    runtime().block_on(async {
        let store = Storage::open(&storage_config(&daemon)).await.unwrap();
        let domain = Domain::new(&store);
        assert_eq!(
            domain
                .revision("edited", original_revision)
                .unwrap()
                .unwrap()
                .record
                .draft
                .claim,
            "The fixture build passed on Linux."
        );
        let memory = domain.active("edited").unwrap().unwrap();
        assert_eq!(memory.revision.commit_seq, seq);
        assert_eq!(memory.record.authority, Authority::HumanConfirmed);
        assert_eq!(
            memory.record.provenance.actor,
            Actor::Tool("fixture".into())
        );
        let approval = memory.record.approval.unwrap();
        assert_eq!(approval.proposal_revision, Some(original_revision));
        let source = domain
            .source(&approval.source.event_id, approval.source.revision)
            .unwrap();
        assert!(
            source
                .payload
                .text
                .contains("Limit the claim to the observed build.")
        );
        assert_eq!(approval.source.revision, seq);
        assert!(store.outbox(seq).unwrap().unwrap().revisions.len() >= 5);
        store.close().await.unwrap();
    });
}

#[test]
fn review_state_retries_keep_their_policy_and_permissions_after_reload() {
    let mut daemon = Daemon::new();
    seed(&daemon, ReviewMode::Manual, "state-retry");
    daemon.start();
    let page = rpc(
        &daemon,
        json!({"op":"review_list","scope":scope(),"after":null}),
    );
    let item = &page["data"]["items"][0];
    let update = json!({"op":"review_state","idempotency_key":"retry-state","id":item["id"],
        "expected_revision":item["revision"],"state":"dismissed","reason":"Not applicable"});
    let first = rpc(&daemon, update.clone());
    assert!(first.get("error").is_none(), "{first}");
    daemon.configure("[review]\nmode='automatic'\n");
    daemon.cli(&["config", "reload"]);
    let replay = rpc(&daemon, update.clone());
    assert_eq!(
        replay, first,
        "a retry must use the accepted policy and receipt"
    );
    daemon.cli(&["access", "revoke"]);
    for request in [
        update,
        json!({"op":"review_list","scope":scope(),"after":null}),
        json!({"op":"review_show","id":item["id"],"at":null}),
    ] {
        assert_eq!(rpc(&daemon, request)["error"]["code"], "READ_DENIED");
    }
}

#[test]
fn opening_a_review_does_not_suspend_active_memory_and_explicit_rejection_retires_it() {
    let mut daemon = Daemon::new();
    let active = seed(&daemon, ReviewMode::Automatic, "active-fact");
    daemon.start();
    let open = rpc(
        &daemon,
        json!({"op":"review_open","memory_id":"active-fact","expected_revision":active,
        "reason":"Review this active fact","idempotency_key":"open-active"}),
    );
    assert!(open.get("error").is_none(), "{open}");
    assert_eq!(
        daemon.cli(&["memory", "show", "active-fact"])["data"]["data"]["state"],
        "active"
    );
    let list = rpc(
        &daemon,
        json!({"op":"review_list","scope":scope(),"after":null}),
    );
    let item = &list["data"]["items"][0];
    let original = rpc(
        &daemon,
        json!({"op":"review_show","id":item["id"],"at":null}),
    );
    assert_eq!(
        original["data"]["proposal"]["record"]["lifecycle"],
        "active"
    );
    let plan = daemon.root.join("retire.json");
    std::fs::write(
        &plan,
        serde_json::to_vec(
            &json!({"id":item["id"],"expected_revision":item["revision"],
        "action":"reject","draft":null,"reason":"The observation is no longer applicable"}),
        )
        .unwrap(),
    )
    .unwrap();
    let retired = operator_cli(&daemon, &plan, "retire-active", "confirm");
    assert_eq!(retired["exit_code"], 0, "{retired}");
    assert_eq!(
        daemon.cli(&["memory", "show", "active-fact"])["data"]["data"]["state"],
        "retired"
    );
    let historical = daemon.cli(&["memory", "show", "active-fact", "--at", &active.to_string()]);
    assert_eq!(historical["data"]["data"]["state"], "active");
}
