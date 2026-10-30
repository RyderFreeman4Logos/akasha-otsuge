use super::extraction_tests::{runtime, scope, storage_config};
use super::*;
use akasha_otsuge::config::ReviewMode;
use akasha_otsuge::domain::*;
use akasha_otsuge::storage::Storage;

fn seed(daemon: &Daemon, mode: ReviewMode, name: &str) -> u64 {
    seed_memory(
        daemon,
        mode,
        name,
        MemoryKind::Fact,
        scope(),
        "The fixture build passed on Linux.",
    )
}

fn seed_memory(
    daemon: &Daemon,
    mode: ReviewMode,
    name: &str,
    kind: MemoryKind,
    applicability: Scope,
    text: &str,
) -> u64 {
    runtime().block_on(async {
        let store = Storage::open(&storage_config(daemon)).await.unwrap();
        let domain = Domain::new(&store);
        let ingress = Ingress::tool("fixture", "build-receipt", applicability.clone()).unwrap();
        let original = SourceDraft {
            event_id: format!("source-{name}"),
            scope: applicability.clone(),
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
                kind,
                scope: applicability.clone(),
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
                    scope: applicability.clone(),
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
    evidence(
        "exact-edit",
        &daemon,
        &json!({"synthetic_operator":true,"original":show,"decision":decision,"preview":preview,"raw_ipc_denial":raw,"declined":declined,"approved":approved,"resolved":resolved,"replay":replay,"stale":stale}),
    );
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

#[test]
fn operator_confirmation_rechecks_permissions_and_config_before_committing() {
    let mut daemon = Daemon::new();
    seed(&daemon, ReviewMode::Manual, "recheck");
    daemon.start();
    let page = rpc(
        &daemon,
        json!({"op":"review_list","scope":scope(),"after":null}),
    );
    let item = &page["data"]["items"][0];
    let plan = daemon.root.join("approve.json");
    std::fs::write(
        &plan,
        serde_json::to_vec(
            &json!({"id":item["id"],"expected_revision":item["revision"],
        "action":"approve","draft":null,"reason":"Use this exact observation"}),
        )
        .unwrap(),
    )
    .unwrap();
    let revoked = operator_cli(&daemon, &plan, "revoked", "confirm-revoke");
    assert_eq!(
        revoked["response"]["error"]["code"], "READ_DENIED",
        "{revoked}"
    );
    let scope_file = daemon.root.join("scope.json");
    std::fs::write(&scope_file, serde_json::to_vec(&scope()).unwrap()).unwrap();
    daemon.cli(&["access", "grant", scope_file.to_str().unwrap()]);
    let reloaded = operator_cli(&daemon, &plan, "reloaded", "confirm-reload");
    assert_eq!(
        reloaded["response"]["error"]["code"], "REVIEW_PREVIEW_CHANGED",
        "{reloaded}"
    );
    let unchanged = daemon.cli(&["review", "show", item["id"].as_str().unwrap()]);
    assert_eq!(unchanged["data"]["item"], *item);
    let accepted = operator_cli(&daemon, &plan, "new-operation", "confirm");
    assert_eq!(accepted["exit_code"], 0, "{accepted}");
    let applied = daemon.cli(&["review", "show", item["id"].as_str().unwrap()]);
    assert_eq!(
        applied["data"]["result"]["record"]["authorization"]["mode"],
        "automatic"
    );
    assert_eq!(applied["data"]["result"]["record"]["config_generation"], 2);
    daemon.configure("[review]\nmode='manual'\n");
    daemon.cli(&["config", "reload"]);
    let replay = operator_cli(&daemon, &plan, "new-operation", "confirm");
    assert_eq!(replay["response"]["data"], accepted["response"]["data"]);
    evidence(
        "confirmation-rechecks",
        &daemon,
        &json!({"revoked":revoked,"reloaded":reloaded,"accepted":accepted,"replay":replay}),
    );
}

fn tool_original(daemon: &Daemon) -> SourceSpan {
    runtime().block_on(async {
        let store = Storage::open(&storage_config(daemon)).await.unwrap();
        let domain = Domain::new(&store);
        let text = "Only if offline, use keyword search.";
        let source = SourceDraft {
            event_id: "policy-original".into(),
            scope: scope(),
            text: text.into(),
            reported_role: None,
            event_time_ms: None,
            observed_time_ms: 1,
        };
        let ingress = Ingress::tool("controlled-observation", "policy-matrix", scope()).unwrap();
        let receipt = domain
            .record_source("policy-original", &source, &ingress, 1)
            .await
            .unwrap();
        let span = SourceSpan {
            event_id: source.event_id.clone(),
            revision: receipt.commit_seq,
            content_sha256: domain
                .source(&source.event_id, receipt.commit_seq)
                .unwrap()
                .content_sha256,
            start: 0,
            end: text.len(),
        };
        store.close().await.unwrap();
        span
    })
}

fn extractor_config(endpoint: &str, mode: &str) -> String {
    format!(
        "[runtime]\nmax_concurrent_jobs=1\n[retrieval]\nextraction='extractor'\n[review]\nmode='{mode}'\n[models.extractor]\nkind='text'\nendpoint={endpoint:?}\nmodel='controlled-extractor'\nlocal=true\n"
    )
}

#[test]
fn extraction_cli_obeys_all_three_policies_for_the_same_candidate() {
    for mode in ["manual", "policy", "automatic"] {
        let endpoint = super::model_tests::Endpoint::extraction();
        let mut daemon = Daemon::new();
        let source = tool_original(&daemon);
        daemon.configure(&extractor_config(&endpoint.address, mode));
        daemon.start();
        let file = daemon.root.join("extract.json");
        std::fs::write(
            &file,
            serde_json::to_vec(&json!({"scope":scope(),"sources":[source]})).unwrap(),
        )
        .unwrap();
        daemon.cli(&[
            "extract",
            "run",
            file.to_str().unwrap(),
            "--idempotency-key",
            "extract-policy",
        ]);
        let job = super::extraction_tests::finished(&daemon, "extract-policy");
        assert_eq!(job["state"], "succeeded", "{job}");
        let memory_id = job["result"]["candidates"][0]["memory_id"]
            .as_str()
            .unwrap();
        let list = rpc(
            &daemon,
            json!({"op":"review_list","scope":scope(),"after":null}),
        );
        let item = &list["data"]["items"][0];
        let show = daemon.cli(&["review", "show", item["id"].as_str().unwrap()]);
        assert_eq!(
            show["data"]["proposal"]["record"]["authorization"]["mode"],
            mode
        );
        assert_eq!(
            show["data"]["proposal"]["record"]["authority"],
            if mode == "automatic" {
                "delegated_approved"
            } else {
                "candidate"
            }
        );
        assert_eq!(
            daemon.cli(&["memory", "show", memory_id])["data"]["data"]["state"],
            if mode == "automatic" {
                "active"
            } else {
                "absent"
            }
        );
        if mode != "automatic" {
            let plan = daemon.root.join("approve-policy.json");
            std::fs::write(
                &plan,
                serde_json::to_vec(
                    &json!({"id":item["id"],"expected_revision":item["revision"],
                "action":"approve","draft":null,"reason":"Approve this scoped conditional result"}),
                )
                .unwrap(),
            )
            .unwrap();
            let approved = operator_cli(&daemon, &plan, "approve-policy", "confirm");
            assert_eq!(approved["exit_code"], 0, "{approved}");
            assert_eq!(
                daemon.cli(&["memory", "show", memory_id])["data"]["data"]["state"],
                "active"
            );
        }
        evidence(
            &format!("policy-{mode}"),
            &daemon,
            &json!({"controlled_model":true,"synthetic_operator":true,"mode":mode,"job":job,"original_proposal":show,"final_memory":daemon.cli(&["memory","show",memory_id])}),
        );
    }
}

#[test]
fn extraction_policy_reload_pins_running_jobs_and_reevaluates_new_operations() {
    let endpoint = super::model_tests::Endpoint::extraction();
    endpoint.pause();
    let mut daemon = Daemon::new();
    let source = tool_original(&daemon);
    daemon.configure(&extractor_config(&endpoint.address, "manual"));
    daemon.start();
    let file = daemon.root.join("extract.json");
    std::fs::write(
        &file,
        serde_json::to_vec(&json!({"scope":scope(),"sources":[source]})).unwrap(),
    )
    .unwrap();
    daemon.cli(&[
        "extract",
        "run",
        file.to_str().unwrap(),
        "--idempotency-key",
        "before-reload",
    ]);
    let end = Instant::now() + Duration::from_secs(20);
    while endpoint.requests.lock().unwrap().is_empty() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(10));
    }
    daemon.configure(&extractor_config(&endpoint.address, "automatic"));
    daemon.cli(&["config", "reload"]);
    endpoint.resume();
    let old = super::extraction_tests::finished(&daemon, "before-reload");
    assert_eq!(old["state"], "succeeded", "{old}");
    let memory_id = old["result"]["candidates"][0]["memory_id"]
        .as_str()
        .unwrap();
    assert_eq!(
        daemon.cli(&["memory", "show", memory_id])["data"]["data"]["state"],
        "absent"
    );
    daemon.cli(&[
        "extract",
        "run",
        file.to_str().unwrap(),
        "--idempotency-key",
        "after-reload",
    ]);
    let new = super::extraction_tests::finished(&daemon, "after-reload");
    assert_eq!(new["state"], "succeeded", "{new}");
    assert_eq!(new["result"]["candidates"][0]["memory_id"], memory_id);
    let active = daemon.cli(&["memory", "show", memory_id]);
    assert_eq!(
        active["data"]["data"]["state"], "active",
        "new operation did not apply the new policy: {active}"
    );
    assert_eq!(
        active["data"]["data"]["revision"]["memory"]["record"]["config_generation"],
        2
    );
    assert_eq!(old["config_generation"], 1);
    assert_eq!(new["config_generation"], 2);
    evidence(
        "running-policy-pinning",
        &daemon,
        &json!({"controlled_model":true,"before_reload":old,"after_reload":new,"active":active}),
    );
}

fn evidence(name: &str, daemon: &Daemon, value: &Value) {
    use sha2::{Digest, Sha256};
    if let Some(root) = std::env::var_os("AKASHA_REVIEW_EVIDENCE") {
        let path = PathBuf::from(root).join(format!("{name}.json"));
        let binary = std::env::var_os("AKASHA_ACCEPTANCE_BINARY")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(BIN));
        let digest: String = Sha256::digest(std::fs::read(&binary).unwrap())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let receipt = json!({"schema_version":1,"binary":binary,"binary_sha256":digest,"fixture_root":daemon.root,"data":value});
        std::fs::write(path, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    }
}

fn approve_item(daemon: &Daemon, item: &Value, id: &str) -> Value {
    let plan = daemon.root.join(format!("{id}.json"));
    std::fs::write(&plan,serde_json::to_vec(&json!({"id":item["id"],"expected_revision":item["revision"],
        "action":"approve","draft":null,"reason":"Synthetic operator confirms this exact scoped rule"})).unwrap()).unwrap();
    let result = operator_cli(daemon, &plan, id, "confirm");
    assert_eq!(result["exit_code"], 0, "{result}");
    result
}

#[test]
fn scoped_operator_taste_and_explicit_successors_keep_unrelated_rules() {
    let mut daemon = Daemon::new();
    let global = Scope {
        project: None,
        ..scope()
    };
    let local = Scope {
        branch_or_version: Some("v2".into()),
        ..scope()
    };
    seed_memory(
        &daemon,
        ReviewMode::Manual,
        "global-taste",
        MemoryKind::Preference,
        global.clone(),
        "Prefer offline tools",
    );
    seed_memory(
        &daemon,
        ReviewMode::Manual,
        "local-exception",
        MemoryKind::Preference,
        local.clone(),
        "Use the online tool for alpha v2",
    );
    daemon.start();
    let page = rpc(
        &daemon,
        json!({"op":"review_list","scope":global,"after":null}),
    );
    let mut approvals = Vec::new();
    for item in page["data"]["items"].as_array().unwrap() {
        approvals.push(approve_item(
            &daemon,
            item,
            &format!("approve-{}", item["memory_id"].as_str().unwrap()),
        ));
    }
    let global_before = daemon.cli(&["memory", "show", "global-taste"]);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    runtime().block_on(async {
        let store = Storage::open(&storage_config(&daemon)).await.unwrap();
        let domain = Domain::new(&store);
        let old = domain.memory("global-taste").unwrap().unwrap();
        let mut draft = old.record.draft.clone();
        draft.claim = "Prefer offline tools and retain audit receipts".into();
        domain
            .propose(
                &MemoryWrite {
                    request_id: "successor-proposal".into(),
                    memory_id: "global-taste".into(),
                    expected_revision: Some(old.revision.commit_seq),
                    draft,
                },
                &Ingress::tool("later-generator", "later-proposal", global.clone()).unwrap(),
                &Policy {
                    revision: "automatic-proposal".into(),
                    config_generation: 1,
                    mode: ReviewMode::Automatic,
                    scope: global.clone(),
                    adopt_explicit_decisions: false,
                },
            )
            .await
            .unwrap();
        let candidate = domain.memory("global-taste").unwrap().unwrap();
        assert_eq!(candidate.record.authority, Authority::Candidate);
        assert!(candidate.record.approval.is_none());
        assert_eq!(domain.active("global-taste").unwrap().unwrap(), old);
        store.close().await.unwrap();
    });
    daemon.start();
    assert_eq!(
        daemon.cli(&["memory", "show", "global-taste"])["data"]["data"],
        global_before["data"]["data"]
    );
    let queue = rpc(
        &daemon,
        json!({"op":"review_list","scope":global,"after":null}),
    );
    let next = queue["data"]["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|i| i["memory_id"] == "global-taste" && i["state"] == "open")
        .unwrap();
    approvals.push(approve_item(&daemon, next, "explicit-successor"));
    daemon.cli(&["index", "sync", "--idempotency-key", "taste-index"]);
    assert_eq!(
        super::extraction_tests::finished(&daemon, "taste-index")["state"],
        "succeeded"
    );
    let mut searches = Vec::new();
    for (id, query_scope, expected) in [
        ("local", local, vec!["local-exception", "global-taste"]),
        (
            "other",
            Scope {
                project: Some("beta".into()),
                branch_or_version: Some("v2".into()),
                ..global
            },
            vec!["global-taste"],
        ),
    ] {
        let file = daemon.root.join(format!("{id}-scope.json"));
        std::fs::write(&file, serde_json::to_vec(&query_scope).unwrap()).unwrap();
        daemon.cli(&[
            "search",
            "unrelated query",
            "--scope",
            file.to_str().unwrap(),
            "--idempotency-key",
            id,
        ]);
        let job = super::extraction_tests::finished(&daemon, id);
        assert_eq!(job["state"], "succeeded", "{job}");
        let result: Value =
            serde_json::from_str(job["result"]["output"].as_str().unwrap()).unwrap();
        let actual: Vec<_> = result["rules"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["memory"]["revision"]["key"]["id"].as_str().unwrap())
            .collect();
        assert_eq!(actual, expected, "{result}");
        assert!(
            result["rules"]
                .as_array()
                .unwrap()
                .iter()
                .any(|r| r["memory"]["record"]["draft"]["claim"]
                    == "Prefer offline tools and retain audit receipts")
        );
        searches.push(result);
    }
    evidence(
        "scoped-taste-and-successor",
        &daemon,
        &json!({"synthetic_operator":true,"approvals":approvals,"searches":searches,"original_global":global_before}),
    );
}

#[test]
fn inbox_pages_and_review_state_history_are_bounded_and_cannot_approve() {
    let mut daemon = Daemon::new();
    let active = seed(&daemon, ReviewMode::Automatic, "paged");
    daemon.start();
    for index in 0..35 {
        let result = rpc(
            &daemon,
            json!({"op":"review_open","memory_id":"paged","expected_revision":active,
            "reason":format!("Investigation {index}"),"idempotency_key":format!("page-{index}")}),
        );
        assert!(result.get("error").is_none(), "{result}");
    }
    let first = rpc(
        &daemon,
        json!({"op":"review_list","scope":scope(),"after":null}),
    );
    assert_eq!(first["data"]["items"].as_array().unwrap().len(), 32);
    let second = rpc(
        &daemon,
        json!({"op":"review_list","scope":scope(),"after":first["data"]["next"]}),
    );
    assert_eq!(second["data"]["items"].as_array().unwrap().len(), 3);
    assert!(second["data"]["next"].is_null());
    let item = &first["data"]["items"][0];
    let mut revision = item["revision"].clone();
    for state in ["investigating", "resolved", "open", "dismissed"] {
        let changed = rpc(
            &daemon,
            json!({"op":"review_state","id":item["id"],"expected_revision":revision,
            "state":state,"reason":"State alone does not authorize a memory change","idempotency_key":state}),
        );
        assert!(changed.get("error").is_none(), "{changed}");
        revision = changed["data"]["commit_seq"].clone();
    }
    let original = rpc(
        &daemon,
        json!({"op":"review_show","id":item["id"],"at":item["revision"]}),
    );
    assert_eq!(original["data"]["item"], *item);
    let invalid = rpc(
        &daemon,
        json!({"op":"review_state","id":item["id"],"expected_revision":revision,
        "state":"ack","reason":"Notification acknowledged","idempotency_key":"ack"}),
    );
    assert_eq!(invalid["error"]["code"], "INVALID_REQUEST");
    let current = daemon.cli(&["memory", "show", "paged"]);
    assert_eq!(
        current["data"]["data"]["revision"]["memory"]["revision"]["commit_seq"],
        active
    );
    assert_eq!(
        current["data"]["data"]["revision"]["memory"]["record"]["authority"],
        "delegated_approved"
    );
}
