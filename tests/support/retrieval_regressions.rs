use super::*;

#[test]
fn legal_large_fact_does_not_block_scoped_search() {
    large_memory(false, false);
}

#[test]
fn legal_large_ancestor_fact_does_not_block_scoped_search() {
    large_memory(true, false);
}

#[test]
fn legal_large_rule_is_dropped_whole_with_a_budget_gap() {
    large_memory(false, true);
}

fn large_memory(ancestor: bool, rule: bool) {
    let mut daemon = Daemon::new();
    seed_records(&daemon, &[("large", Some("alpha"), "large fixture")], 1);
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
            let current = domain.memory("large").unwrap().unwrap();
            let mut record = current.record;
            record.previous_adoption = if rule {
                None
            } else {
                Some(current.revision.commit_seq)
            };
            if rule {
                record.draft.kind = MemoryKind::Procedure;
            }
            record.draft.conditions = vec!["x".repeat(4096); 31];
            record.draft.conditions.push("z".into());
            let limit = if rule { 130000 } else { 128 * 1024 - 16 };
            let remaining = limit - serde_json::to_vec(&record).unwrap().len();
            assert!(remaining < 4096);
            *record.draft.conditions.last_mut().unwrap() = "z".repeat(remaining + 1);
            let id = if rule { "large-rule" } else { "large" };
            let policy = Policy {
                revision: "fixture".into(),
                config_generation: 1,
                mode: ReviewMode::Automatic,
                scope: scope("alpha"),
                adopt_explicit_decisions: false,
            };
            domain
                .propose(
                    &MemoryWrite {
                        request_id: "grow-large".into(),
                        memory_id: id.into(),
                        expected_revision: if rule {
                            None
                        } else {
                            Some(current.revision.commit_seq)
                        },
                        draft: record.draft,
                    },
                    &Ingress::tool("fixture", "fixture", scope("alpha")).unwrap(),
                    &policy,
                )
                .await
                .unwrap();
            if rule {
                let proposal = domain.memory(id).unwrap().unwrap();
                let target = ReviewTarget {
                    memory_id: id.into(),
                    proposal_revision: proposal.revision.commit_seq,
                    semantic_sha256: proposal.record.semantic_sha256.clone(),
                    action: ReviewAction::Approve,
                };
                let ingress = Ingress::from_verified_review_channel(
                    "owner",
                    "approve-large",
                    scope("alpha"),
                    target.clone(),
                )
                .unwrap();
                let revision = domain
                    .record_source(
                        "approve-source",
                        &SourceDraft {
                            event_id: "approve-large".into(),
                            scope: scope("alpha"),
                            text: "Approved".into(),
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
                let source = SourceSpan {
                    event_id: "approve-large".into(),
                    revision,
                    content_sha256: domain
                        .source("approve-large", revision)
                        .unwrap()
                        .content_sha256,
                    start: 0,
                    end: 8,
                };
                domain
                    .review(
                        &ReviewRequest {
                            write: MemoryWrite {
                                request_id: "approve-large".into(),
                                memory_id: id.into(),
                                expected_revision: Some(proposal.revision.commit_seq),
                                draft: proposal.record.draft,
                            },
                            semantic_sha256: target.semantic_sha256,
                            source,
                            reason: "Synthetic explicit review".into(),
                            action: ReviewAction::Approve,
                        },
                        &ingress,
                        &policy,
                    )
                    .await
                    .unwrap();
            }
            let stored = domain.memory(id).unwrap().unwrap();
            let size = serde_json::to_vec(&stored.record).unwrap().len();
            assert!(size <= 128 * 1024);
            assert_eq!(stored.record.is_rule(), rule);
            if !rule {
                assert!(
                    size >= 128 * 1024 - 64,
                    "fixture was not near the legal limit: {size}"
                );
                assert!(serde_json::to_vec(&stored).unwrap().len() > 128 * 1024);
            }
            store.close().await.unwrap();
        });
    let mut query_scope = scope("alpha");
    if ancestor {
        query_scope.branch_or_version = Some("child".into());
    }
    let scope_file = daemon.root.join("scope.json");
    std::fs::write(&scope_file, serde_json::to_vec(&query_scope).unwrap()).unwrap();
    daemon.start();
    daemon.cli(&["index", "sync", "--idempotency-key", "large-build"]);
    assert_eq!(finished(&daemon, "large-build")["state"], "succeeded");
    daemon.cli(&[
        "search",
        "absentword",
        "--scope",
        scope_file.to_str().unwrap(),
        "--idempotency-key",
        "large-search",
    ]);
    let job = finished(&daemon, "large-search");
    assert_eq!(job["state"], "succeeded", "{job}");
    let raw = job["result"]["output"].as_str().unwrap();
    assert!(raw.len() <= 128 * 1024);
    let output: Value = serde_json::from_str(raw).unwrap();
    assert!(output["items"].as_array().unwrap().is_empty());
    assert!(output["rules"].as_array().unwrap().is_empty());
    if rule {
        assert_eq!(output["status"], "incomplete");
        assert!(
            output["coverage"]["gaps"]
                .as_array()
                .unwrap()
                .iter()
                .any(|g| g == "character_budget")
        );
    } else {
        assert_eq!(output["status"], "no_hit");
    }
    let archived = daemon.cli(&[
        "receipt",
        "show",
        job["result"]["receipt_id"].as_str().unwrap(),
    ]);
    assert_eq!(archived["data"]["data"]["output"], raw);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}

#[test]
fn invalid_vector_zero_retains_diagnostic() {
    invalid_vector("invalid-zero");
}

#[test]
fn invalid_vector_overflow_retains_diagnostic() {
    invalid_vector("invalid-overflow");
}

#[test]
fn invalid_vector_cancelled_mean_retains_diagnostic() {
    invalid_vector("invalid-mean");
}

fn invalid_vector(model: &str) {
    let endpoint = super::super::model_tests::Endpoint::retrieval();
    let mut daemon = Daemon::new();
    let claim = format!("{}y", "x".repeat(4096));
    seed_records(&daemon, &[("invalid", Some("alpha"), &claim)], 1);
    daemon.configure(&format!("[retrieval]\nembedding='embed'\n[models.embed]\nkind='embedding'\nendpoint={:?}\nmodel={model:?}\ndimension=2\nlocal=true\ntimeout='2s'",endpoint.address));
    daemon.start();
    daemon.cli(&["index", "sync", "--idempotency-key", "invalid-build"]);
    let job = finished(&daemon, "invalid-build");
    assert_eq!(job["state"], "failed", "{job}");
    assert_eq!(job["error"], "PROVIDER_OUTPUT_INVALID", "{job}");
    let status = daemon.cli(&["index", "status"])["data"]["vector"].clone();
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    let reopened = daemon.cli(&["index", "status"])["data"]["vector"].clone();
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    assert_eq!(reopened, status);
    assert_eq!(status["watermark"], 0);
    assert_eq!(
        status["last_call"]["error"], "PROVIDER_OUTPUT_INVALID",
        "{status}"
    );
    assert_eq!(status["last_call"]["identity"], status["model_identity"]);
    assert_eq!(status["last_call"]["profile"], "embed");
    assert_eq!(status["last_call"]["trace"]["automatic_retries"], 0);
    assert_eq!(
        status["last_call"]["trace"]["provider"]["configured_ms"],
        2000
    );
}

#[test]
fn sync_stops_after_a_live_grant_shrinks() {
    let endpoint = super::super::model_tests::Endpoint::retrieval();
    let mut daemon = Daemon::new();
    seed_records(
        &daemon,
        &[
            ("first", Some("alpha"), "offline first"),
            ("forbidden", Some("beta"), "private forbidden"),
        ],
        1,
    );
    daemon.configure(&format!("[runtime]\nmax_concurrent_jobs=1\n[retrieval]\nembedding='embed'\n[models.embed]\nkind='embedding'\nendpoint={:?}\nmodel='vectors-2'\ndimension=2\nlocal=true\ntimeout='10s'",endpoint.address));
    let scope_file = daemon.root.join("scope.json");
    std::fs::write(&scope_file, serde_json::to_vec(&scope("alpha")).unwrap()).unwrap();
    let owner_file = daemon.root.join("owner.json");
    let mut owner = scope("alpha");
    owner.project = None;
    std::fs::write(&owner_file, serde_json::to_vec(&owner).unwrap()).unwrap();
    daemon.start();
    endpoint.pause();
    daemon.cli(&["index", "sync", "--idempotency-key", "shrinking-build"]);
    let until = Instant::now() + Duration::from_secs(5);
    while endpoint.requests.lock().unwrap().is_empty() {
        assert!(Instant::now() < until, "first embedding never arrived");
        std::thread::sleep(Duration::from_millis(10));
    }
    daemon.cli(&["access", "grant", scope_file.to_str().unwrap()]);
    // With one worker, this job cannot finish before the sync releases its slot.
    daemon.cli(&[
        "search",
        "sentinel",
        "--scope",
        scope_file.to_str().unwrap(),
        "--idempotency-key",
        "after-shrink",
    ]);
    endpoint.resume();
    let sentinel = finished(&daemon, "after-shrink");
    assert_eq!(sentinel["state"], "succeeded", "{sentinel}");
    daemon.cli(&["access", "grant", owner_file.to_str().unwrap()]);
    let job = finished(&daemon, "shrinking-build");
    let status = daemon.cli(&["index", "status"])["data"].clone();
    let leaked = endpoint
        .requests
        .lock()
        .unwrap()
        .iter()
        .any(|(_, body)| body.to_string().contains("private forbidden"));
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    assert!(
        !leaked,
        "a forbidden Memory reached the provider after grant shrink"
    );
    assert_eq!(job["state"], "failed", "{job}");
    assert_eq!(job["error"], "READ_DENIED", "{job}");
    assert_eq!(status["vector"]["watermark"], 0, "{status}");
}

#[test]
fn archived_global_metadata_keeps_its_required_grant() {
    let mut daemon = Daemon::new();
    seed(&daemon);
    let scope_file = daemon.root.join("scope.json");
    std::fs::write(&scope_file, serde_json::to_vec(&scope("alpha")).unwrap()).unwrap();
    daemon.start();
    daemon.cli(&["index", "sync", "--idempotency-key", "metadata-build"]);
    assert_eq!(finished(&daemon, "metadata-build")["state"], "succeeded");
    let search = |id| {
        daemon.cli(&[
            "search",
            "absentword",
            "--scope",
            scope_file.to_str().unwrap(),
            "--idempotency-key",
            id,
        ]);
        let job = finished(&daemon, id);
        assert_eq!(job["state"], "succeeded", "{job}");
        job
    };
    let wide = search("wide-metadata");
    let output: Value = serde_json::from_str(wide["result"]["output"].as_str().unwrap()).unwrap();
    assert_eq!(output["index"]["indexed_rows"], 3);
    assert!(output["items"].as_array().unwrap().is_empty());
    assert!(output["rules"].as_array().unwrap().is_empty());
    daemon.cli(&["access", "grant", scope_file.to_str().unwrap()]);
    let receipt = wide["result"]["receipt_id"].as_str().unwrap();
    let mut escaped = Vec::new();
    for args in [
        vec!["receipt", "show", receipt],
        vec!["receipt", "replay", receipt],
        vec!["job", "show", "wide-metadata"],
    ] {
        let result = daemon.command().args(&args).output().unwrap();
        if result.status.success() {
            escaped.push(args);
        } else {
            assert!(result.stdout.is_empty());
            let error: Value = serde_json::from_slice(&result.stderr).unwrap();
            assert_eq!(error["error"]["code"], "READ_DENIED");
        }
    }
    let narrow = search("narrow-metadata");
    let output: Value = serde_json::from_str(narrow["result"]["output"].as_str().unwrap()).unwrap();
    assert!(output["index"].is_null());
    let receipt = narrow["result"]["receipt_id"].as_str().unwrap();
    assert_eq!(
        daemon.cli(&["receipt", "show", receipt])["data"]["data"]["output"],
        narrow["result"]["output"]
    );
    let replay = daemon
        .command()
        .args(["receipt", "replay", receipt])
        .output()
        .unwrap();
    assert!(replay.status.success());
    assert_eq!(
        replay.stdout,
        narrow["result"]["output"].as_str().unwrap().as_bytes()
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    assert!(
        escaped.is_empty(),
        "archived global metadata escaped through {escaped:?}"
    );
}
