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
                Some("alpha"),
                "离线时仍支持关键词检索 offline keyword search",
            ),
            ("private", Some("beta"), "离线私有检索 private search"),
            ("weather", Some("alpha"), "Tomorrow the weather is sunny"),
        ],
        1,
    )
}
pub(super) fn seed_records(
    daemon: &Daemon,
    records: &[(&str, Option<&str>, &str)],
    span_parts: usize,
) -> u64 {
    seed_records_with(daemon, records, span_parts, |_| {})
}

pub(super) fn seed_records_with(
    daemon: &Daemon,
    records: &[(&str, Option<&str>, &str)],
    span_parts: usize,
    customize: impl Fn(&mut MemoryDraft),
) -> u64 {
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
                let scope = Scope {
                    project: project.map(Into::into),
                    ..scope("")
                };
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
                let mut draft = MemoryDraft {
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
                customize(&mut draft);
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

#[path = "retrieval_rules.rs"]
mod rules;

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

#[path = "retrieval_lifecycle.rs"]
mod lifecycle;
