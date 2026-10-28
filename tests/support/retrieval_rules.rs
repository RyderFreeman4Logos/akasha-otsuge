use super::*;

#[test]
fn applicable_rules_survive_query_mismatch_but_never_widen_a_read_grant() {
    let endpoint = super::super::model_tests::Endpoint::retrieval();
    let mut daemon = Daemon::new();
    seed(&daemon);
    daemon.configure(&format!("[retrieval]\nreranker='rank'\n[models.rank]\nkind='reranker'\nendpoint={:?}\nmodel='ranking'\nlocal=true\ntimeout='15s'", endpoint.address));
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
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let store = Storage::open(&config).await.unwrap();
            let domain = Domain::new(&store);
            for project in [None, Some("alpha"), Some("beta")] {
                let scope = Scope {
                    project: project.map(str::to_owned),
                    ..scope("alpha")
                };
                let id = match project {
                    None => "global-rule",
                    Some("alpha") => "local-rule",
                    _ => "sibling-rule",
                };
                let claim = if project == Some("alpha") {
                    "For this project use spaces for indentation."
                } else {
                    "Prefer tabs for indentation."
                };
                let ingress = Ingress::from_verified_human_channel(
                    "owner",
                    "fixture",
                    scope.clone(),
                    StatementIntent::ExplicitDecision,
                )
                .unwrap();
                let seq = domain
                    .record_source(
                        id,
                        &SourceDraft {
                            event_id: id.into(),
                            scope: scope.clone(),
                            text: claim.into(),
                            reported_role: None,
                            event_time_ms: None,
                            observed_time_ms: 1,
                        },
                        &ingress,
                        1,
                    )
                    .await
                    .unwrap()
                    .commit_seq;
                let span = SourceSpan {
                    event_id: id.into(),
                    revision: seq,
                    content_sha256: domain.source(id, seq).unwrap().content_sha256,
                    start: 0,
                    end: claim.len(),
                };
                domain
                    .adopt_statement(
                        &MemoryWrite {
                            request_id: format!("adopt-{id}"),
                            memory_id: id.into(),
                            expected_revision: None,
                            draft: MemoryDraft {
                                kind: MemoryKind::Preference,
                                scope: scope.clone(),
                                claim: claim.into(),
                                rationale: String::new(),
                                conditions: vec![],
                                assumptions: vec![],
                                source_spans: vec![span.clone()],
                                valid_from_ms: None,
                                valid_until_ms: None,
                            },
                        },
                        &ingress,
                        &Policy {
                            revision: "fixture".into(),
                            config_generation: 1,
                            mode: ReviewMode::Manual,
                            scope,
                            adopt_explicit_decisions: true,
                        },
                        &span,
                    )
                    .await
                    .unwrap();
            }
            store.close().await.unwrap();
        });
    let scope_file = daemon.root.join("scope.json");
    std::fs::write(&scope_file, serde_json::to_vec(&scope("alpha")).unwrap()).unwrap();
    daemon.start();
    daemon.cli(&["index", "sync", "--idempotency-key", "build-rules"]);
    assert_eq!(finished(&daemon, "build-rules")["state"], "succeeded");
    daemon.cli(&[
        "search",
        "离线",
        "--scope",
        scope_file.to_str().unwrap(),
        "--idempotency-key",
        "rules-full",
    ]);
    let job = finished(&daemon, "rules-full");
    assert_eq!(job["state"], "succeeded", "{job}");
    let output: Value = serde_json::from_str(job["result"]["output"].as_str().unwrap()).unwrap();
    let rules = output["rules"]
        .as_array()
        .expect("applicable rules must be separate from scored hits");
    assert_eq!(rules.len(), 2, "{output}");
    for id in ["global-rule", "local-rule"] {
        assert!(
            rules
                .iter()
                .any(|r| r["memory"]["revision"]["key"]["id"] == id)
        );
    }
    assert_eq!(output["items"].as_array().unwrap().len(), 1);
    let receipt = job["result"]["receipt_id"].as_str().unwrap();
    endpoint.pause();
    let sent = endpoint.requests.lock().unwrap().len();
    daemon.cli(&[
        "search",
        "离线",
        "--scope",
        scope_file.to_str().unwrap(),
        "--idempotency-key",
        "rules-race",
    ]);
    let until = Instant::now() + Duration::from_secs(5);
    while endpoint.requests.lock().unwrap().len() == sent {
        assert!(Instant::now() < until, "reranker was not reached");
        std::thread::sleep(Duration::from_millis(10));
    }
    daemon.cli(&["access", "grant", scope_file.to_str().unwrap()]);
    endpoint.resume();
    let raced = finished(&daemon, "rules-race");
    assert_eq!(raced["state"], "failed", "{raced}");
    assert_eq!(raced["error"], "READ_DENIED");
    assert!(raced["result"].is_null());
    let status = daemon.command().args(["index", "status"]).output().unwrap();
    assert!(
        !status.status.success(),
        "global index counts escaped a narrowed read grant"
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&status.stderr).unwrap()["error"]["code"],
        "READ_DENIED"
    );
    let replay = daemon
        .command()
        .args(["receipt", "replay", receipt])
        .output()
        .unwrap();
    assert!(
        !replay.status.success(),
        "receipt leaked a global rule after grant narrowed"
    );
    assert!(replay.stdout.is_empty());
    assert_eq!(
        serde_json::from_slice::<Value>(&replay.stderr).unwrap()["error"]["code"],
        "READ_DENIED"
    );
    daemon.cli(&[
        "search",
        "离线",
        "--scope",
        scope_file.to_str().unwrap(),
        "--idempotency-key",
        "rules-narrow",
    ]);
    let narrow = finished(&daemon, "rules-narrow");
    assert_eq!(narrow["state"], "succeeded", "{narrow}");
    let output: Value = serde_json::from_str(narrow["result"]["output"].as_str().unwrap()).unwrap();
    assert_eq!(output["rules"].as_array().unwrap().len(), 1, "{output}");
    assert_eq!(
        output["rules"][0]["memory"]["revision"]["key"]["id"],
        "local-rule"
    );
    assert!(
        output["index"].is_null(),
        "global index metadata escaped the scoped search: {output}"
    );
    assert_eq!(output["searched_content"], "active_memories");
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}
