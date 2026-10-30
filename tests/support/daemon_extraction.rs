use super::*;
use akasha_otsuge::config::{Paths, resolve};
use akasha_otsuge::domain::*;
use akasha_otsuge::storage::Storage;

pub(super) fn scope() -> Scope {
    Scope {
        user: format!("uid:{}", rustix::process::geteuid().as_raw()),
        project: Some("alpha".into()),
        branch_or_version: None,
        session: None,
    }
}

pub(super) fn storage_config(daemon: &Daemon) -> akasha_otsuge::config::ResolvedConfig {
    resolve(
        "",
        None,
        "",
        &Paths {
            data_dir: daemon.root.join("data"),
            socket_path: daemon.socket(),
        },
    )
    .unwrap()
}

pub(super) fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

pub(super) fn seed_source(daemon: &Daemon, event_id: &str, text: &str) -> SourceSpan {
    runtime().block_on(async {
        let store = Storage::open(&storage_config(daemon)).await.unwrap();
        let domain = Domain::new(&store);
        let ingress = Ingress::from_verified_human_channel(
            &scope().user,
            "fixture-channel",
            scope(),
            StatementIntent::Tentative,
        )
        .unwrap();
        let receipt = domain
            .record_source(
                &format!("source-{event_id}"),
                &SourceDraft {
                    event_id: event_id.into(),
                    scope: scope(),
                    text: text.into(),
                    reported_role: Some("user".into()),
                    event_time_ms: None,
                    observed_time_ms: 1,
                },
                &ingress,
                1,
            )
            .await
            .unwrap();
        let span = SourceSpan {
            event_id: event_id.into(),
            revision: receipt.commit_seq,
            content_sha256: domain
                .source(event_id, receipt.commit_seq)
                .unwrap()
                .content_sha256,
            start: 0,
            end: text.len(),
        };
        store.close().await.unwrap();
        span
    })
}

fn submit(daemon: &Daemon, id: &str, plan: &Value) -> Value {
    let mut stream = daemon.connect();
    receive(&mut stream);
    send(
        &mut stream,
        &json!({"protocol_version":1,"request_id":id,
            "command":{"op":"extraction_run","idempotency_key":id,"plan":plan}}),
    );
    receive(&mut stream)
}

pub(super) fn finished(daemon: &Daemon, id: &str) -> Value {
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
fn text_extraction_is_durable_source_backed_candidate_only_and_idempotent() {
    let endpoint = super::model_tests::Endpoint::extraction();
    let mut daemon = Daemon::new();
    let source = seed_source(
        &daemon,
        "conditional",
        "Only if offline, use keyword search.",
    );
    daemon.configure(&format!(
        "[retrieval]\nextraction='extractor'\n[review]\nmode='automatic'\n[models.extractor]\nkind='text'\nendpoint={:?}\nmodel='controlled-extractor'\nlocal=true\n",
        endpoint.address
    ));
    daemon.start();
    let plan = json!({"scope":scope(),"sources":[source]});
    let accepted = submit(&daemon, "extract-first", &plan);
    assert!(
        accepted.get("error").is_none(),
        "extraction was not admitted: {accepted}"
    );
    let job = finished(&daemon, "extract-first");
    assert_eq!(job["state"], "succeeded", "{job}");
    let candidates = job["result"]["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 1);
    let memory_id = candidates[0]["memory_id"].as_str().unwrap().to_owned();
    assert_eq!(
        submit(&daemon, "extract-first", &plan)["data"]["result"],
        job["result"]
    );
    assert_eq!(endpoint.requests.lock().unwrap().len(), 1);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    runtime().block_on(async {
        let store = Storage::open(&storage_config(&daemon)).await.unwrap();
        let domain = Domain::new(&store);
        let memory = domain.memory(&memory_id).unwrap().unwrap();
        assert_eq!(memory.record.authority, Authority::Candidate);
        assert_eq!(memory.record.lifecycle, Lifecycle::Candidate);
        assert_eq!(memory.record.provenance.origin, Origin::Extraction);
        assert_eq!(memory.record.draft.conditions, ["offline"]);
        assert_eq!(memory.record.draft.source_spans, [source]);
        assert_eq!(
            memory.record.provenance.generator.unwrap().prompt_version,
            "extraction-v1"
        );
        assert_eq!(
            domain
                .source(
                    "conditional",
                    plan["sources"][0]["revision"].as_u64().unwrap()
                )
                .unwrap()
                .payload
                .text,
            "Only if offline, use keyword search."
        );
        store.close().await.unwrap();
    });
}

fn native_fixture(scenario: &str) -> (Daemon, Value) {
    let mut daemon = Daemon::new();
    let source = seed_source(
        &daemon,
        "native-conditional",
        "Only if offline, use keyword search.",
    );
    let home = daemon.root.join("native-home");
    std::fs::create_dir(&home).unwrap();
    let binary = daemon.root.join("codex");
    executable::write(
        &binary,
        include_str!("../fixtures/codex/0.164.0/runner_host.py"),
    );
    std::fs::write(daemon.root.join("scenario"), scenario).unwrap();
    std::fs::write(home.join("models_cache.json"), r#"{"models":[{"slug":"fixture","shell_type":"unified_exec","apply_patch_tool_type":"freeform","experimental_supported_tools":[],"tool_mode":"code_mode"}]}"#).unwrap();
    daemon.configure(&format!("[access]\nread_roots=[{home:?}]\n[retrieval]\nextraction='executor:existing'\n[executors.existing]\nkind='codex'\nbinary={binary:?}\nnative_home={home:?}\n"));
    daemon.start();
    let plan = json!({"scope":scope(),"sources":[source]});
    (daemon, plan)
}

#[test]
fn native_extraction_uses_an_existing_executor_without_extra_model_profiles() {
    let (daemon, plan) = native_fixture("extraction");
    let path = daemon.root.join("extraction.json");
    std::fs::write(&path, serde_json::to_vec(&plan).unwrap()).unwrap();
    let admitted = daemon.cli(&[
        "extract",
        "run",
        path.to_str().unwrap(),
        "--idempotency-key",
        "native-extraction",
    ]);
    assert_eq!(admitted["data"]["id"], "native-extraction");
    let job = finished(&daemon, "native-extraction");
    assert_eq!(job["state"], "succeeded", "{job}");
    assert_eq!(job["result"]["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(job["result"]["generator"]["runner"], "executor/existing");
    assert_eq!(
        job["result"]["generator"]["prompt_version"],
        "extraction-v1"
    );
    assert_eq!(job["result"]["trace"]["usage"]["starts"], 1);
}

#[test]
fn native_extraction_cannot_expand_its_evidence_with_research_tools() {
    let (daemon, plan) = native_fixture("extraction_denied");
    assert!(
        submit(&daemon, "denied-extraction", &plan)
            .get("error")
            .is_none()
    );
    let job = finished(&daemon, "denied-extraction");
    assert_eq!(job["state"], "failed", "{job}");
    assert_eq!(job["error"], "TOOL_DENIED", "{job}");
    assert!(job["result"]["candidates"].is_null());
}

fn review_extracted(daemon: &Daemon, id: &str, action: ReviewAction) {
    runtime().block_on(async {
        let store = Storage::open(&storage_config(daemon)).await.unwrap();
        let domain = Domain::new(&store);
        let current = domain.memory(id).unwrap().unwrap();
        let target = ReviewTarget {
            memory_id: id.into(),
            proposal_revision: current.revision.commit_seq,
            semantic_sha256: current.record.semantic_sha256.clone(),
            action,
        };
        let ingress = Ingress::from_verified_review_channel(
            &scope().user,
            "fixture-review",
            scope(),
            target.clone(),
        )
        .unwrap();
        let event_id = format!("review-{}", current.revision.commit_seq);
        let text = "Review this exact candidate revision.";
        let receipt = domain
            .record_source(
                &event_id,
                &SourceDraft {
                    event_id: event_id.clone(),
                    scope: scope(),
                    text: text.into(),
                    reported_role: None,
                    event_time_ms: None,
                    observed_time_ms: 1,
                },
                &ingress,
                1,
            )
            .await
            .unwrap();
        let source = SourceSpan {
            event_id: event_id.clone(),
            revision: receipt.commit_seq,
            content_sha256: domain
                .source(&event_id, receipt.commit_seq)
                .unwrap()
                .content_sha256,
            start: 0,
            end: text.len(),
        };
        domain
            .review(
                &ReviewRequest {
                    write: MemoryWrite {
                        request_id: format!("apply-{event_id}"),
                        memory_id: id.into(),
                        expected_revision: Some(current.revision.commit_seq),
                        draft: current.record.draft,
                    },
                    semantic_sha256: target.semantic_sha256,
                    source,
                    reason: "Explicit fixture review".into(),
                    action,
                },
                &ingress,
                &Policy {
                    revision: "fixture".into(),
                    config_generation: 1,
                    mode: akasha_otsuge::config::ReviewMode::Manual,
                    scope: scope(),
                    adopt_explicit_decisions: false,
                },
            )
            .await
            .unwrap();
        store.close().await.unwrap();
    });
}

#[test]
fn extracted_records_survive_index_pause_resume_rebuild_and_retraction() {
    let text = super::model_tests::Endpoint::extraction();
    let vectors = super::model_tests::Endpoint::retrieval();
    let mut daemon = Daemon::new();
    let source = seed_source(
        &daemon,
        "lifecycle-source",
        "Only if offline, use keyword search.",
    );
    let settings = format!(
        "[runtime]\nmax_concurrent_jobs=2\npaused=false\n[retrieval]\nextraction='extractor'\nembedding='embed'\n[models.extractor]\nkind='text'\nendpoint={:?}\nmodel='controlled'\nlocal=true\n[models.embed]\nkind='embedding'\nendpoint={:?}\nmodel='vectors'\ndimension=2\nlocal=true\n",
        text.address, vectors.address
    );
    daemon.configure(&settings);
    daemon.start();
    submit(
        &daemon,
        "lifecycle-extract",
        &json!({"scope":scope(),"sources":[source]}),
    );
    let job = finished(&daemon, "lifecycle-extract");
    assert_eq!(job["state"], "succeeded", "{job}");
    let id = job["result"]["candidates"][0]["memory_id"]
        .as_str()
        .unwrap()
        .to_owned();
    daemon.cli(&["index", "sync", "--idempotency-key", "candidate-index"]);
    let candidate = finished(&daemon, "candidate-index");
    assert_eq!(candidate["state"], "succeeded", "{candidate}");
    assert_eq!(candidate["result"]["indexed_rows"], 0);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    review_extracted(&daemon, &id, ReviewAction::Approve);
    daemon.start();
    vectors.pause();
    daemon.cli(&["index", "sync", "--idempotency-key", "pause-index"]);
    let until = Instant::now() + Duration::from_secs(10);
    while vectors.requests.lock().unwrap().is_empty() {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(20));
    }
    daemon.cli(&["job", "cancel", "pause-index"]);
    vectors.resume();
    assert_eq!(finished(&daemon, "pause-index")["state"], "cancelled");
    daemon.configure(&settings.replace("paused=false", "paused=true"));
    daemon.cli(&["config", "reload"]);
    let paused = daemon.cli(&["index", "status"])["data"].clone();
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(daemon.cli(&["index", "status"])["data"], paused);
    daemon.configure(&settings);
    daemon.cli(&["config", "reload"]);
    daemon.cli(&["index", "sync", "--idempotency-key", "resume-index"]);
    let resumed = finished(&daemon, "resume-index");
    assert_eq!(resumed["state"], "succeeded", "{resumed}");
    assert_eq!(resumed["result"]["indexed_rows"], 1);
    assert_eq!(resumed["result"]["vector"]["indexed_rows"], 1);
    let scope_file = daemon.root.join("scope.json");
    std::fs::write(&scope_file, serde_json::to_vec(&scope()).unwrap()).unwrap();
    let calls = vectors.requests.lock().unwrap().len();
    vectors.pause();
    daemon.cli(&[
        "search",
        "offline",
        "--scope",
        scope_file.to_str().unwrap(),
        "--idempotency-key",
        "held-search",
    ]);
    let until = Instant::now() + Duration::from_secs(10);
    while vectors.requests.lock().unwrap().len() == calls {
        assert!(Instant::now() < until);
        std::thread::sleep(Duration::from_millis(20));
    }
    daemon.cli(&["index", "rebuild", "--idempotency-key", "busy-rebuild"]);
    let busy = finished(&daemon, "busy-rebuild");
    assert_eq!(busy["state"], "failed", "{busy}");
    assert_eq!(busy["error"], "RESOURCE_BUSY");
    assert_eq!(
        daemon.cli(&["index", "status"])["data"]["generation"],
        resumed["result"]["generation"]
    );
    vectors.resume();
    assert_eq!(finished(&daemon, "held-search")["state"], "succeeded");
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    std::fs::remove_dir_all(daemon.root.join("data/search")).unwrap();
    daemon.start();
    daemon.cli(&["index", "rebuild", "--idempotency-key", "rebuild-index"]);
    let rebuilt = finished(&daemon, "rebuild-index");
    assert_eq!(rebuilt["state"], "succeeded", "{rebuilt}");
    assert_ne!(
        rebuilt["result"]["generation"],
        resumed["result"]["generation"]
    );
    assert_ne!(
        rebuilt["result"]["vector"]["generation"],
        resumed["result"]["vector"]["generation"]
    );
    assert_eq!(rebuilt["result"]["indexed_rows"], 1);
    assert_eq!(rebuilt["result"]["vector"]["indexed_rows"], 1);
    assert_eq!(
        daemon.cli(&["index", "rebuild", "--idempotency-key", "rebuild-index"])["data"]["result"],
        rebuilt["result"]
    );
    daemon.cli(&["index", "rebuild", "--idempotency-key", "rebuild-existing"]);
    let replaced = finished(&daemon, "rebuild-existing");
    assert_eq!(replaced["state"], "succeeded", "{replaced}");
    assert_eq!(replaced["result"]["vector"]["indexed_rows"], 1);
    let tables = std::fs::read_dir(daemon.root.join("data/search"))
        .unwrap()
        .filter_map(|entry| {
            let name = entry.unwrap().file_name().to_string_lossy().into_owned();
            name.ends_with(".lance").then_some(name)
        })
        .collect::<Vec<_>>();
    assert_eq!(
        tables.len(),
        2,
        "obsolete lexical/vector tables were retained: {tables:?}"
    );
    daemon.cli(&[
        "search",
        "offline",
        "--scope",
        scope_file.to_str().unwrap(),
        "--idempotency-key",
        "before-retraction",
    ]);
    let before = finished(&daemon, "before-retraction");
    assert_eq!(before["state"], "succeeded", "{before}");
    assert!(before["result"]["output"].as_str().unwrap().contains(&id));
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    review_extracted(&daemon, &id, ReviewAction::Reject);
    daemon.start();
    for (search_id, sync) in [
        ("stale-after-retraction", false),
        ("synced-after-retraction", true),
    ] {
        if sync {
            daemon.cli(&["index", "sync", "--idempotency-key", "retraction-index"]);
            assert_eq!(finished(&daemon, "retraction-index")["state"], "succeeded");
        }
        daemon.cli(&[
            "search",
            "offline",
            "--scope",
            scope_file.to_str().unwrap(),
            "--idempotency-key",
            search_id,
        ]);
        let result = finished(&daemon, search_id);
        assert_eq!(result["state"], "succeeded", "{result}");
        assert!(!result["result"]["output"].as_str().unwrap().contains(&id));
    }
    assert_eq!(text.requests.lock().unwrap().len(), 1);
}

#[test]
#[ignore = "requires an explicitly supplied authenticated Pi profile and receipt path"]
fn native_single_cli_extraction_actual() {
    let binary = PathBuf::from(std::env::var_os("AKASHA_NATIVE_PI_BINARY").unwrap());
    let home = PathBuf::from(std::env::var_os("AKASHA_NATIVE_PI_HOME").unwrap());
    let output = PathBuf::from(std::env::var_os("AKASHA_NATIVE_EXTRACTION_RECEIPT").unwrap());
    assert!(!output.exists());
    let mut daemon = Daemon::new();
    let originals = [
        (
            "conditional",
            "This project must use keyword search when embeddings are offline.",
        ),
        (
            "negative",
            "Do not send private queries to an external reranker.",
        ),
        (
            "quoted",
            "Alice said she prefers tabs; this is Alice's view, not my preference.",
        ),
        (
            "temporary",
            "For this one debugging session only, use verbose logs; it is not a permanent policy.",
        ),
        (
            "automated",
            "Two automated agents suggested enabling auto-publish. I have not approved their suggestion.",
        ),
    ];
    let sources: Vec<_> = originals
        .iter()
        .map(|(id, text)| seed_source(&daemon, id, text))
        .collect();
    daemon.configure(&format!("[access]\nread_roots=[{home:?}]\n[retrieval]\nextraction='executor:existing'\n[executors.existing]\nkind='pi'\nbinary={binary:?}\nnative_home={home:?}\n"));
    daemon.cli(&[
        "adapter",
        "install",
        "pi",
        "--binary",
        binary.to_str().unwrap(),
        "--agent-dir",
        home.to_str().unwrap(),
    ]);
    daemon.start();
    let plan = json!({"scope":scope(),"sources":sources});
    let path = daemon.root.join("extraction.json");
    std::fs::write(&path, serde_json::to_vec(&plan).unwrap()).unwrap();
    daemon.cli(&[
        "extract",
        "run",
        path.to_str().unwrap(),
        "--idempotency-key",
        "actual-native-extraction",
    ]);
    let end = Instant::now() + Duration::from_secs(2200);
    let job = loop {
        let job = daemon.cli(&["job", "show", "actual-native-extraction"])["data"].clone();
        if matches!(
            job["state"].as_str(),
            Some("succeeded" | "failed" | "cancelled")
        ) {
            break job;
        }
        assert!(
            Instant::now() < end,
            "native extraction exceeded the complete workflow envelope"
        );
        std::thread::sleep(Duration::from_millis(500));
    };
    let mut receipt = json!({"schema_version":1,"originals":originals,"plan":plan,"job":job,"native_home":home,"daemon_fixture":daemon.root,"extra_model_profiles":0});
    std::fs::write(&output, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    assert_eq!(job["state"], "succeeded", "{job}");
    assert_eq!(job["result"]["origin"], "extraction");
    assert_eq!(job["result"]["trace"]["usage"]["starts"], 1);
    assert_eq!(
        submit(&daemon, "actual-native-extraction", &plan)["data"]["result"],
        job["result"]
    );
    for span in &sources {
        let shown = daemon.cli(&[
            "show",
            &span.event_id,
            "--revision",
            &span.revision.to_string(),
            "--range",
            &span.start.to_string(),
            &span.end.to_string(),
        ]);
        assert_eq!(
            shown["data"]["output"],
            originals
                .iter()
                .find(|(id, _)| *id == span.event_id)
                .unwrap()
                .1
        );
    }
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    let records = runtime().block_on(async {
        let store = Storage::open(&storage_config(&daemon)).await.unwrap();
        let domain = Domain::new(&store);
        let mut records = vec![];
        for candidate in job["result"]["candidates"].as_array().unwrap() {
            let memory = domain
                .memory(candidate["memory_id"].as_str().unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(memory.record.authority, Authority::Candidate);
            assert_eq!(memory.record.lifecycle, Lifecycle::Candidate);
            assert_eq!(memory.record.provenance.origin, Origin::Extraction);
            assert!(scope().contains(&memory.record.draft.scope));
            for span in &memory.record.draft.source_spans {
                let source = domain.source(&span.event_id, span.revision).unwrap();
                assert_eq!(source.content_sha256, span.content_sha256);
                assert!(source.payload.text.get(span.start..span.end).is_some());
            }
            records.push(serde_json::to_value(memory).unwrap());
        }
        store.close().await.unwrap();
        records
    });
    assert!(
        !records.is_empty(),
        "actual extractor returned no sample candidates"
    );
    receipt["memories"] = json!(records);
    receipt["status"] = json!("PASS_REAL_SINGLE_CLI_EXTRACTION");
    std::fs::write(&output, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
}
