use super::*;
use akasha_otsuge::config::{Paths, ReviewMode, resolve};
use akasha_otsuge::domain::*;
use akasha_otsuge::storage::Storage;

#[path = "retrieval_regressions.rs"]
mod regressions;

fn scope(project: &str) -> Scope {
    Scope {
        user: format!("uid:{}", rustix::process::geteuid().as_raw()),
        project: Some(project.into()),
        branch_or_version: None,
        session: None,
    }
}

fn seed(daemon: &Daemon) -> u64 {
    seed_records(
        daemon,
        &[
            (
                "offline",
                "alpha",
                "离线时仍支持关键词检索 offline keyword search",
            ),
            ("private", "beta", "离线私有检索 private search"),
            ("weather", "alpha", "Tomorrow the weather is sunny"),
        ],
        1,
    )
}
fn seed_records(daemon: &Daemon, records: &[(&str, &str, &str)], span_parts: usize) -> u64 {
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
            for &(id, project, text) in records {
                let scope = scope(project);
                let ingress = Ingress::tool("fixture", "fixture", scope.clone()).unwrap();
                let source = SourceDraft {
                    event_id: id.into(),
                    scope: scope.clone(),
                    text: text.into(),
                    reported_role: None,
                    event_time_ms: None,
                    observed_time_ms: 1,
                };
                let receipt = domain
                    .record_source(&format!("source-{id}"), &source, &ingress, 1)
                    .await
                    .unwrap();
                let span = SourceSpan {
                    event_id: id.into(),
                    revision: receipt.commit_seq,
                    content_sha256: domain
                        .source(id, receipt.commit_seq)
                        .unwrap()
                        .content_sha256,
                    start: 0,
                    end: text.len(),
                };
                let draft = MemoryDraft {
                    kind: MemoryKind::Fact,
                    scope: scope.clone(),
                    claim: text.into(),
                    rationale: "Synthetic retrieval fixture".into(),
                    conditions: vec![],
                    assumptions: vec![],
                    source_spans: if span_parts == 1 {
                        vec![span]
                    } else {
                        (0..span_parts)
                            .map(|i| SourceSpan {
                                start: i,
                                end: i + 1,
                                ..span.clone()
                            })
                            .collect()
                    },
                    valid_from_ms: None,
                    valid_until_ms: None,
                };
                let policy = Policy {
                    revision: "fixture".into(),
                    config_generation: 1,
                    mode: ReviewMode::Automatic,
                    scope,
                    adopt_explicit_decisions: false,
                };
                domain
                    .propose(
                        &MemoryWrite {
                            request_id: format!("write-{id}"),
                            memory_id: id.into(),
                            expected_revision: None,
                            draft,
                        },
                        &ingress,
                        &policy,
                    )
                    .await
                    .unwrap();
            }
            let seq = store.sequence().unwrap();
            store.close().await.unwrap();
            seq
        })
}

fn finished(daemon: &Daemon, id: &str) -> Value {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let job = daemon.cli(&["job", "show", id])["data"].clone();
        if matches!(
            job["state"].as_str(),
            Some("succeeded" | "failed" | "cancelled")
        ) {
            return job;
        }
        assert!(Instant::now() < end, "{job}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn empty_library_returns_no_hit_with_an_explicit_memory_only_boundary() {
    let mut daemon = Daemon::new();
    let scope_file = daemon.root.join("scope.json");
    std::fs::write(&scope_file, serde_json::to_vec(&scope("alpha")).unwrap()).unwrap();
    daemon.start();
    daemon.cli(&["index", "sync", "--idempotency-key", "empty-build"]);
    let build = finished(&daemon, "empty-build");
    assert_eq!(build["state"], "succeeded", "{build}");
    daemon.cli(&[
        "search",
        "missing",
        "--scope",
        scope_file.to_str().unwrap(),
        "--idempotency-key",
        "empty-search",
    ]);
    let job = finished(&daemon, "empty-search");
    assert_eq!(job["state"], "succeeded", "{job}");
    let output: Value = serde_json::from_str(job["result"]["output"].as_str().unwrap()).unwrap();
    assert_eq!(output["status"], "no_hit");
    assert_eq!(output["searched_content"], "active_memories");
    assert_eq!(output["index"]["indexed_rows"], 0);
    assert!(output["items"].as_array().unwrap().is_empty());
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}

#[test]
fn keyword_search_is_scoped_persistent_and_archives_exact_output() {
    let mut daemon = Daemon::new();
    let seed_seq = seed(&daemon);
    let scope_file = daemon.root.join("scope.json");
    std::fs::write(&scope_file, serde_json::to_vec(&scope("alpha")).unwrap()).unwrap();
    daemon.start();
    let hello = receive(&mut daemon.connect());
    for capability in ["retrieval_jobs", "index_status", "original_range"] {
        assert!(
            hello["capabilities"]
                .as_array()
                .unwrap()
                .iter()
                .any(|c| c == capability),
            "missing {capability}"
        );
    }
    daemon.cli(&["index", "sync", "--idempotency-key", "build"]);
    let built = finished(&daemon, "build");
    assert_eq!(built["state"], "succeeded", "{built}");
    for id in ["search-before", "search-after"] {
        daemon.cli(&[
            "search",
            "离线",
            "--scope",
            scope_file.to_str().unwrap(),
            "--idempotency-key",
            id,
        ]);
        let job = finished(&daemon, id);
        assert_eq!(job["state"], "succeeded", "{job}");
        let receipt = job["result"]["receipt_id"].as_str().unwrap();
        let replay = daemon
            .command()
            .args(["receipt", "replay", receipt])
            .output()
            .unwrap();
        assert!(
            replay.status.success(),
            "{}",
            String::from_utf8_lossy(&replay.stderr)
        );
        assert_eq!(
            replay.stdout,
            job["result"]["output"].as_str().unwrap().as_bytes()
        );
        let result: Value = serde_json::from_slice(&replay.stdout).unwrap();
        assert_eq!(result["items"].as_array().unwrap().len(), 1, "{result}");
        assert_eq!(
            result["items"][0]["memory"]["revision"]["key"]["id"],
            "offline"
        );
        assert!(result["index"]["lexical_watermark"].as_u64().unwrap() >= seed_seq);
        assert_eq!(result["mode"], "keyword");
        let archived = daemon.cli(&["receipt", "show", receipt]);
        assert_eq!(archived["data"]["data"]["output"], job["result"]["output"]);
        assert_eq!(
            archived["data"]["data"]["record"]["delivery"]["phase"],
            "delivered"
        );
        daemon.cli(&["daemon", "stop"]);
        daemon.wait();
        if id == "search-before" {
            daemon.start();
        }
    }
}

#[test]
fn original_ranges_preserve_utf8_budget_receipt_and_live_permission() {
    let mut daemon = Daemon::new();
    daemon.configure("[retrieval]\nmax_original_chars=1");
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
    let text = format!("A{}", "中😀".repeat(8000));
    let revision = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let store = Storage::open(&config).await.unwrap();
            let domain = Domain::new(&store);
            let source = SourceDraft {
                event_id: "long-original".into(),
                scope: scope("alpha"),
                text: text.clone(),
                reported_role: None,
                event_time_ms: None,
                observed_time_ms: 1,
            };
            let ingress = Ingress::tool("fixture", "fixture", scope("alpha")).unwrap();
            let seq = domain
                .record_source("long-source", &source, &ingress, 1)
                .await
                .unwrap()
                .commit_seq;
            store.close().await.unwrap();
            seq
        });
    daemon.start();
    let show = daemon.cli(&[
        "show",
        "long-original",
        "--revision",
        &revision.to_string(),
        "--range",
        "1",
        "8",
    ])["data"]
        .clone();
    assert_eq!(show["output"], "中");
    assert_eq!(show["start"], 1);
    assert_eq!(show["end"], 4);
    assert_eq!(show["requested_end"], 8);
    assert_eq!(show["total_bytes"], text.len());
    assert_eq!(show["truncated"], true);
    let receipt = show["receipt_id"].as_str().unwrap();
    let archived = daemon.cli(&["receipt", "show", receipt])["data"]["data"].clone();
    assert_eq!(archived["output"], show["output"]);
    assert_eq!(archived["record"]["sources"][0]["start"], 1);
    assert_eq!(archived["record"]["sources"][0]["end"], 4);
    assert_eq!(archived["record"]["truncation"]["original_bytes"], 7);
    assert_eq!(archived["record"]["status"], "incomplete");
    let replay = daemon
        .command()
        .args(["receipt", "replay", receipt])
        .output()
        .unwrap();
    assert!(replay.status.success());
    assert_eq!(replay.stdout, "中".as_bytes());
    for (start, end) in [("2", "8"), ("1", "3"), ("8", "1"), ("1", "99999999")] {
        let output = daemon
            .command()
            .args([
                "show",
                "long-original",
                "--revision",
                &revision.to_string(),
                "--range",
                start,
                end,
            ])
            .output()
            .unwrap();
        assert!(!output.status.success());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "INVALID_RANGE");
    }
    let narrower = daemon.root.join("other-scope.json");
    std::fs::write(&narrower, serde_json::to_vec(&scope("beta")).unwrap()).unwrap();
    daemon.cli(&["access", "grant", narrower.to_str().unwrap()]);
    for args in [
        vec![
            "show",
            "long-original",
            "--revision",
            "1",
            "--range",
            "1",
            "8",
        ],
        vec!["receipt", "replay", receipt],
    ] {
        let output = daemon.command().args(args).output().unwrap();
        assert!(!output.status.success());
        assert!(output.stdout.is_empty());
        let error: Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "READ_DENIED");
    }
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}

#[test]
fn applicable_rules_survive_query_mismatch_but_never_widen_a_read_grant() {
    let endpoint = super::model_tests::Endpoint::retrieval();
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

#[test]
fn real_vector_and_rerank_paths_are_scoped_versioned_and_degrade_when_offline() {
    let endpoint = super::model_tests::Endpoint::retrieval();
    let mut daemon = Daemon::new();
    seed(&daemon);
    let settings = |dimension| {
        format!(
            "[retrieval]\nembedding='embed'\nreranker='rank'\n[models.embed]\nkind='embedding'\nendpoint={:?}\nmodel='vectors-{dimension}'\ndimension={dimension}\nlocal=true\ntimeout='2s'\n[models.rank]\nkind='reranker'\nendpoint={:?}\nmodel='ranking'\nlocal=true\ntimeout='2s'",
            endpoint.address, endpoint.address
        )
    };
    daemon.configure(&settings(2));
    let scope_file = daemon.root.join("scope.json");
    std::fs::write(&scope_file, serde_json::to_vec(&scope("alpha")).unwrap()).unwrap();
    daemon.start();
    daemon.cli(&["index", "sync", "--idempotency-key", "vector-build"]);
    let built = finished(&daemon, "vector-build");
    assert_eq!(built["state"], "succeeded", "{built}");
    assert!(
        built["result"]["vector"]["watermark"]
            .as_u64()
            .is_some_and(|n| n > 0),
        "{built}"
    );
    let generation = built["result"]["vector"]["generation"].clone();
    daemon.cli(&[
        "search",
        "without network",
        "--scope",
        scope_file.to_str().unwrap(),
        "--idempotency-key",
        "semantic",
    ]);
    let job = finished(&daemon, "semantic");
    assert_eq!(job["state"], "succeeded", "{job}");
    let output: Value = serde_json::from_str(job["result"]["output"].as_str().unwrap()).unwrap();
    assert_eq!(output["mode"], "hybrid_reranked", "{output}");
    assert_eq!(
        output["items"][0]["memory"]["revision"]["key"]["id"],
        "offline"
    );
    assert!(
        !output["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|i| i["memory"]["revision"]["key"]["id"] == "private")
    );
    assert_eq!(output["index"]["vector"]["generation"], generation);
    let receipt = daemon.cli(&[
        "receipt",
        "show",
        job["result"]["receipt_id"].as_str().unwrap(),
    ]);
    assert!(
        receipt["data"]["data"]["record"]["index_generation"]
            .as_str()
            .unwrap()
            .contains(generation.as_str().unwrap()),
        "receipt omitted the vector generation"
    );
    assert!(
        endpoint
            .requests
            .lock()
            .unwrap()
            .iter()
            .any(|(path, body)| path == "/v1/rerank"
                && !body["documents"].as_array().unwrap().is_empty())
    );
    daemon.configure(&settings(3));
    daemon.cli(&["config", "reload"]);
    daemon.cli(&["index", "sync", "--idempotency-key", "vector-three"]);
    let rebuilt = finished(&daemon, "vector-three");
    assert_eq!(rebuilt["state"], "succeeded", "{rebuilt}");
    assert_ne!(rebuilt["result"]["vector"]["generation"], generation);
    assert_eq!(rebuilt["result"]["vector"]["dimension"], 3);
    drop(endpoint);
    daemon.cli(&[
        "search",
        "离线",
        "--scope",
        scope_file.to_str().unwrap(),
        "--idempotency-key",
        "offline-search",
    ]);
    let job = finished(&daemon, "offline-search");
    assert_eq!(job["state"], "succeeded", "{job}");
    let output: Value = serde_json::from_str(job["result"]["output"].as_str().unwrap()).unwrap();
    assert_eq!(output["mode"], "keyword");
    assert_eq!(output["status"], "incomplete");
    assert_eq!(output["coverage"]["complete"], false);
    assert_eq!(
        output["items"][0]["memory"]["revision"]["key"]["id"],
        "offline"
    );
    assert!(
        output["model_calls"]
            .as_array()
            .unwrap()
            .iter()
            .all(|c| c["error"].is_string())
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}

#[test]
fn search_character_budget_returns_a_truthful_partial_receipt() {
    let mut daemon = Daemon::new();
    seed(&daemon);
    daemon.configure("[retrieval]\nmax_original_chars=1");
    let scope_file = daemon.root.join("scope.json");
    std::fs::write(&scope_file, serde_json::to_vec(&scope("alpha")).unwrap()).unwrap();
    daemon.start();
    daemon.cli(&["index", "sync", "--idempotency-key", "budget-build"]);
    assert_eq!(finished(&daemon, "budget-build")["state"], "succeeded");
    daemon.cli(&[
        "search",
        "离线",
        "--scope",
        scope_file.to_str().unwrap(),
        "--idempotency-key",
        "bounded-search",
    ]);
    let job = finished(&daemon, "bounded-search");
    assert_eq!(job["state"], "succeeded", "{job}");
    let output: Value = serde_json::from_str(job["result"]["output"].as_str().unwrap()).unwrap();
    assert!(output["items"].as_array().unwrap().is_empty(), "{output}");
    assert_eq!(output["status"], "incomplete");
    assert!(
        output["coverage"]["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g == "character_budget")
    );
    let receipt = job["result"]["receipt_id"].as_str().unwrap();
    let saved = daemon.cli(&["receipt", "show", receipt]);
    assert_eq!(saved["data"]["data"]["output"], job["result"]["output"]);
    assert_eq!(saved["data"]["data"]["record"]["status"], "incomplete");
    assert!(
        saved["data"]["data"]["record"]["truncation"]["original_bytes"]
            .as_u64()
            .unwrap()
            > job["result"]["output"].as_str().unwrap().len() as u64
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}

#[test]
fn old_generation_serves_queries_while_new_embedding_is_held() {
    generation_switch(2);
}

#[test]
fn queued_search_keeps_its_admitted_model_after_generation_switch() {
    generation_switch(1);
}

fn generation_switch(jobs: usize) {
    let old = super::model_tests::Endpoint::retrieval();
    let next = super::model_tests::Endpoint::retrieval();
    let mut daemon = Daemon::new();
    seed(&daemon);
    let settings = |endpoint: &str, dimension| {
        format!(
            "[runtime]\nmax_concurrent_jobs={jobs}\nmemory_budget_mib=512\n[retrieval]\nembedding='embed'\n[models.embed]\nkind='embedding'\nendpoint={endpoint:?}\nmodel='vectors-{dimension}'\ndimension={dimension}\nlocal=true\ntimeout={:?}",
            if dimension == 2 { "1700s" } else { "15s" }
        )
    };
    daemon.configure(&settings(&old.address, 2));
    let scope_file = daemon.root.join("scope.json");
    std::fs::write(&scope_file, serde_json::to_vec(&scope("alpha")).unwrap()).unwrap();
    daemon.start();
    daemon.cli(&["index", "sync", "--idempotency-key", "old-generation"]);
    let original = finished(&daemon, "old-generation");
    assert_eq!(original["state"], "succeeded", "{original}");
    next.pause();
    daemon.configure(&settings(&next.address, 3));
    daemon.cli(&["config", "reload"]);
    daemon.cli(&["index", "sync", "--idempotency-key", "new-generation"]);
    let until = Instant::now() + Duration::from_secs(5);
    while next.requests.lock().unwrap().is_empty() {
        assert!(Instant::now() < until, "builder never sent its embedding");
        std::thread::sleep(Duration::from_millis(10));
    }
    let during = daemon.cli(&["index", "status"])["data"].clone();
    daemon.cli(&[
        "search",
        "without network",
        "--scope",
        scope_file.to_str().unwrap(),
        "--idempotency-key",
        "during-build",
    ]);
    let until = Instant::now() + Duration::from_secs(3);
    let completed_while_held = loop {
        let job = daemon.cli(&["job", "show", "during-build"])["data"].clone();
        if jobs == 1 {
            assert_eq!(job["state"], "queued", "{job}");
            break false;
        }
        if job["state"] == "succeeded" {
            break true;
        }
        if job["state"] == "failed" || Instant::now() >= until {
            break false;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if jobs == 2 {
        daemon.configure(
            &settings(&next.address, 3).replace("memory_budget_mib=512", "memory_budget_mib=1024"),
        );
        daemon.cli(&["config", "reload"]);
        daemon.cli(&[
            "search",
            "offline",
            "--scope",
            scope_file.to_str().unwrap(),
            "--idempotency-key",
            "cache-busy",
        ]);
        let busy = finished(&daemon, "cache-busy");
        assert_eq!(busy["state"], "failed", "{busy}");
        assert_eq!(busy["error"], "RESOURCE_BUSY");
    }
    next.resume();
    let built = finished(&daemon, "new-generation");
    let job = finished(&daemon, "during-build");
    if jobs == 2 {
        daemon.cli(&[
            "search",
            "sunny",
            "--scope",
            scope_file.to_str().unwrap(),
            "--idempotency-key",
            "cache-drained",
        ]);
        let drained = finished(&daemon, "cache-drained");
        assert_eq!(drained["state"], "succeeded", "{drained}");
    }
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    assert_eq!(
        completed_while_held,
        jobs == 2,
        "search blocked behind a provider call in the builder: {job}"
    );
    assert_eq!(built["state"], "succeeded", "{built}");
    assert_eq!(during["building"], true, "{during}");
    assert_eq!(during["searched_content"], "active_memories");
    assert_eq!(
        during["active_vector"]["generation"],
        original["result"]["vector"]["generation"]
    );
    assert!(
        job["execution_budget_ms"].as_u64().unwrap() >= 2_120_000,
        "old model's full provider/queue clocks were clipped: {job}"
    );
    let output: Value = serde_json::from_str(job["result"]["output"].as_str().unwrap()).unwrap();
    assert_eq!(
        output["coverage"]["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g == "index_building"),
        jobs == 2,
        "{output}"
    );
    assert_eq!(output["mode"], "hybrid", "{output}");
    assert_eq!(
        output["index"]["vector"]["generation"],
        original["result"]["vector"]["generation"]
    );
    assert_eq!(
        output["items"][0]["memory"]["revision"]["key"]["id"],
        "offline"
    );
    assert!(
        output["coverage"]["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g == "generation_pending")
    );
    assert_eq!(
        next.requests
            .lock()
            .unwrap()
            .iter()
            .filter(|(_, body)| body["input"][0] == "without network")
            .count(),
        0
    );
}

#[test]
fn reranking_long_claims_respects_wire_budget_and_reports_truncation() {
    let endpoint = super::model_tests::Endpoint::retrieval();
    let mut daemon = Daemon::new();
    let long = format!("offline {}", "long claim text ".repeat(1400));
    seed_records(
        &daemon,
        &[("long-one", "alpha", &long), ("long-two", "alpha", &long)],
        1,
    );
    daemon.configure(&format!("[retrieval]\nreranker='rank'\nmax_original_chars=100000\n[models.rank]\nkind='reranker'\nendpoint={:?}\nmodel='ranking'\nlocal=true\ntimeout='2s'",endpoint.address));
    let scope_file = daemon.root.join("scope.json");
    std::fs::write(&scope_file, serde_json::to_vec(&scope("alpha")).unwrap()).unwrap();
    daemon.start();
    daemon.cli(&["index", "sync", "--idempotency-key", "long-build"]);
    assert_eq!(finished(&daemon, "long-build")["state"], "succeeded");
    daemon.cli(&[
        "search",
        "offline",
        "--scope",
        scope_file.to_str().unwrap(),
        "--idempotency-key",
        "long-rank",
    ]);
    let job = finished(&daemon, "long-rank");
    assert_eq!(job["state"], "succeeded", "{job}");
    let output: Value = serde_json::from_str(job["result"]["output"].as_str().unwrap()).unwrap();
    assert_eq!(output["mode"], "keyword_reranked", "{output}");
    assert_eq!(output["items"].as_array().unwrap().len(), 2);
    assert!(
        output["coverage"]["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "reranker_input_truncated")
    );
    let calls = endpoint.requests.lock().unwrap();
    assert!(!calls.is_empty());
    for (_, body) in calls.iter() {
        assert!(serde_json::to_vec(body).unwrap().len() <= 32768);
        assert!(
            body["documents"]
                .as_array()
                .unwrap()
                .iter()
                .all(|d| d.as_str().unwrap().len() <= 4096)
        );
    }
    drop(calls);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}

#[test]
fn source_union_budget_returns_partial_output_instead_of_failing_the_job() {
    let mut daemon = Daemon::new();
    let ids: Vec<_> = (0..20).map(|n| format!("span-{n:02}")).collect();
    let records: Vec<_> = ids
        .iter()
        .map(|id| (id.as_str(), "alpha", "offline source evidence"))
        .collect();
    seed_records(&daemon, &records, 4);
    let scope_file = daemon.root.join("scope.json");
    std::fs::write(&scope_file, serde_json::to_vec(&scope("alpha")).unwrap()).unwrap();
    daemon.start();
    daemon.cli(&["index", "sync", "--idempotency-key", "spans-build"]);
    assert_eq!(finished(&daemon, "spans-build")["state"], "succeeded");
    daemon.cli(&[
        "search",
        "offline",
        "--scope",
        scope_file.to_str().unwrap(),
        "--idempotency-key",
        "spans-search",
    ]);
    let job = finished(&daemon, "spans-search");
    assert_eq!(job["state"], "succeeded", "{job}");
    let output: Value = serde_json::from_str(job["result"]["output"].as_str().unwrap()).unwrap();
    assert_eq!(output["status"], "incomplete");
    assert_eq!(output["items"].as_array().unwrap().len(), 16);
    assert!(
        output["coverage"]["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|g| g == "source_budget")
    );
    let receipt = daemon.cli(&[
        "receipt",
        "show",
        job["result"]["receipt_id"].as_str().unwrap(),
    ]);
    assert_eq!(
        receipt["data"]["data"]["record"]["sources"]
            .as_array()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(receipt["data"]["data"]["output"], job["result"]["output"]);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}

#[test]
fn failed_vector_build_retains_trace_without_advancing_its_prefix() {
    let endpoint = super::model_tests::Endpoint::retrieval();
    let address = endpoint.address.clone();
    drop(endpoint);
    let mut daemon = Daemon::new();
    seed(&daemon);
    daemon.configure(&format!("[retrieval]\nembedding='embed'\n[models.embed]\nkind='embedding'\nendpoint={address:?}\nmodel='offline'\ndimension=2\nlocal=true\ntimeout='1s'"));
    daemon.start();
    daemon.cli(&["index", "sync", "--idempotency-key", "failed-build"]);
    let job = finished(&daemon, "failed-build");
    assert_eq!(job["state"], "failed");
    let status = daemon.cli(&["index", "status"])["data"].clone();
    assert_eq!(status["vector"]["watermark"], 0);
    assert!(
        status["vector"]["last_call"]["error"].is_string(),
        "missing failed call diagnostics: {status}"
    );
    assert_eq!(
        status["vector"]["last_call"]["trace"]["automatic_retries"],
        0
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}

#[test]
fn retired_and_corrected_memories_are_filtered_before_index_catchup() {
    let mut daemon = Daemon::new();
    seed(&daemon);
    let scope_file = daemon.root.join("scope.json");
    std::fs::write(&scope_file, serde_json::to_vec(&scope("alpha")).unwrap()).unwrap();
    daemon.start();
    daemon.cli(&["index", "sync", "--idempotency-key", "before-change"]);
    assert_eq!(finished(&daemon, "before-change")["state"], "succeeded");
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
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
            let current = domain.memory("offline").unwrap().unwrap();
            let target = ReviewTarget {
                memory_id: "offline".into(),
                proposal_revision: current.revision.commit_seq,
                semantic_sha256: current.record.semantic_sha256.clone(),
                action: ReviewAction::Reject,
            };
            let ingress = Ingress::from_verified_review_channel(
                "owner",
                "retire",
                scope("alpha"),
                target.clone(),
            )
            .unwrap();
            let source = domain
                .record_source(
                    "retire-source",
                    &SourceDraft {
                        event_id: "retire-source".into(),
                        scope: scope("alpha"),
                        text: "Retire this exact revision".into(),
                        reported_role: None,
                        event_time_ms: None,
                        observed_time_ms: 1,
                    },
                    &ingress,
                    1,
                )
                .await
                .unwrap();
            let policy = Policy {
                revision: "fixture".into(),
                config_generation: 1,
                mode: ReviewMode::Manual,
                scope: scope("alpha"),
                adopt_explicit_decisions: false,
            };
            domain
                .review(
                    &ReviewRequest {
                        write: MemoryWrite {
                            request_id: "retire".into(),
                            memory_id: "offline".into(),
                            expected_revision: Some(current.revision.commit_seq),
                            draft: current.record.draft,
                        },
                        semantic_sha256: target.semantic_sha256,
                        source: SourceSpan {
                            event_id: "retire-source".into(),
                            revision: source.commit_seq,
                            content_sha256: domain
                                .source("retire-source", source.commit_seq)
                                .unwrap()
                                .content_sha256,
                            start: 0,
                            end: 25,
                        },
                        reason: "fixture withdrawal".into(),
                        action: ReviewAction::Reject,
                    },
                    &ingress,
                    &policy,
                )
                .await
                .unwrap();
            let current = domain.memory("weather").unwrap().unwrap();
            let mut draft = current.record.draft;
            draft.rationale = "Corrected explanation".into();
            domain
                .propose(
                    &MemoryWrite {
                        request_id: "correct-weather".into(),
                        memory_id: "weather".into(),
                        expected_revision: Some(current.revision.commit_seq),
                        draft,
                    },
                    &Ingress::tool("fixture", "fixture", scope("alpha")).unwrap(),
                    &Policy {
                        mode: ReviewMode::Automatic,
                        ..policy
                    },
                )
                .await
                .unwrap();
            let index = akasha_otsuge::retrieval::Index::open(&config, &store)
                .await
                .unwrap();
            assert_eq!(
                index
                    .keyword("offline", &scope("alpha"), 20)
                    .await
                    .unwrap()
                    .len(),
                1,
                "withdrawn row must still physically exist for this control"
            );
            drop(index);
            store.close().await.unwrap();
        });
    daemon.start();
    for query in ["offline", "weather"] {
        daemon.cli(&[
            "search",
            query,
            "--scope",
            scope_file.to_str().unwrap(),
            "--idempotency-key",
            query,
        ]);
        let job = finished(&daemon, query);
        assert_eq!(job["state"], "succeeded", "{job}");
        let output: Value =
            serde_json::from_str(job["result"]["output"].as_str().unwrap()).unwrap();
        assert!(output["items"].as_array().unwrap().is_empty(), "{output}");
        assert_eq!(output["status"], "incomplete");
        assert!(
            output["coverage"]["gaps"]
                .as_array()
                .unwrap()
                .iter()
                .any(|g| g == "index_lag")
        );
    }
    daemon.cli(&["index", "sync", "--idempotency-key", "catchup"]);
    assert_eq!(finished(&daemon, "catchup")["state"], "succeeded");
    for (query, id) in [("offline", "retired-final"), ("weather", "corrected-final")] {
        daemon.cli(&[
            "search",
            query,
            "--scope",
            scope_file.to_str().unwrap(),
            "--idempotency-key",
            id,
        ]);
        let job = finished(&daemon, id);
        let output: Value =
            serde_json::from_str(job["result"]["output"].as_str().unwrap()).unwrap();
        assert_eq!(output["coverage"]["complete"], true, "{output}");
        if query == "offline" {
            assert_eq!(output["status"], "no_hit");
        } else {
            assert_eq!(
                output["items"][0]["memory"]["record"]["draft"]["rationale"],
                "Corrected explanation"
            );
        }
    }
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}

#[test]
fn capture_original_larger_than_a_source_record_is_read_progressively() {
    use akasha_otsuge::capture::*;
    use sha2::{Digest, Sha256};
    let mut daemon = Daemon::new();
    daemon.configure("[retrieval]\nmax_original_chars=20000");
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
    let text = "原文😀\n".repeat(9000);
    assert!(text.len() > 64 * 1024);
    let event = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let store = Storage::open(&config).await.unwrap();
            let binding = CaptureBinding::from_source(
                SourceIdentity {
                    adapter: "fixture".into(),
                    profile: "p".into(),
                    session: "s".into(),
                    thread: "t".into(),
                    agent: "a".into(),
                },
                scope("alpha"),
                "fixture",
                CoverageLevel::VisibleText,
            )
            .unwrap();
            let event = Capture::new(&store)
                .ingest(
                    &binding,
                    &EventDraft {
                        event_id: "long-capture".into(),
                        source_version: "1".into(),
                        runtime_instance: "fixture".into(),
                        branch: "main".into(),
                        parent: None,
                        fork_cutoff: None,
                        event_time_ms: None,
                        observed_time_ms: 1,
                        visibility: Visibility::User,
                        kind: EventKind::Message {
                            message_id: "long-message".into(),
                            phase: MessagePhase::Final,
                        },
                        text: text.clone(),
                    },
                    text.as_bytes(),
                    &Ingress::unverified("fixture", "fixture", scope("alpha")).unwrap(),
                    None,
                    1,
                )
                .await
                .unwrap();
            store.close().await.unwrap();
            event
        });
    assert!(event.event.normalized.len() > 1);
    daemon.start();
    let mut original = String::new();
    for span in event.event.normalized {
        let shown = daemon.cli(&[
            "show",
            &span.event_id,
            "--revision",
            &span.revision.to_string(),
            "--range",
            &span.start.to_string(),
            &span.end.to_string(),
        ])["data"]
            .clone();
        assert_eq!(shown["truncated"], false);
        let archived =
            daemon.cli(&["receipt", "show", shown["receipt_id"].as_str().unwrap()])["data"]["data"]
                .clone();
        assert_eq!(
            archived["record"]["sources"][0],
            serde_json::to_value(&span).unwrap()
        );
        assert_eq!(archived["output"], shown["output"]);
        original.push_str(shown["output"].as_str().unwrap());
    }
    assert_eq!(original, text);
    assert_eq!(
        Sha256::digest(original.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect::<String>(),
        event.event.text_sha256
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}
