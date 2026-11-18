use super::*;

fn settings(endpoint: &str, projects: Option<&[&str]>) -> String {
    let selection = projects.map_or_else(String::new, |projects| {
        let scopes = projects
            .iter()
            .map(|project| format!("{{user={:?},project={project:?}}}", scope(project).user))
            .collect::<Vec<_>>()
            .join(",");
        format!("embedding_scopes=[{scopes}]\n")
    });
    format!(
        "[retrieval]\nembedding='embed'\n{selection}[models.embed]\nkind='embedding'\nendpoint={endpoint:?}\nmodel='vectors-2'\ndimension=2\nlocal=true\ntimeout='2s'"
    )
}

#[test]
fn vector_scope_selection_preserves_lexical_coverage_and_skips_provider_calls() {
    let endpoint = super::super::model_tests::Endpoint::retrieval();
    let mut daemon = Daemon::new();
    seed(&daemon);
    daemon.configure(&settings(&endpoint.address, Some(&["unused", "alpha"])));
    daemon.start();
    daemon.cli(&[
        "index",
        "sync",
        "--idempotency-key",
        "selected-vector-build",
    ]);
    let built = finished(&daemon, "selected-vector-build");
    assert_eq!(built["state"], "succeeded", "{built}");
    assert_eq!(built["result"]["indexed_rows"], 3, "{built}");
    assert_eq!(built["result"]["vector"]["indexed_rows"], 2, "{built}");
    assert_eq!(
        built["result"]["vector"]["embedding_scopes"],
        json!([scope("unused"), scope("alpha")])
    );
    assert!(
        endpoint
            .requests
            .lock()
            .unwrap()
            .iter()
            .all(|(_, body)| { body["input"][0] != "离线私有检索 private search" }),
        "excluded memory reached the embedding provider"
    );
    let before = endpoint.requests.lock().unwrap().len();
    let scope_file = daemon.root.join("scope.json");
    std::fs::write(&scope_file, serde_json::to_vec(&scope("beta")).unwrap()).unwrap();
    daemon.cli(&[
        "search",
        "private",
        "--scope",
        scope_file.to_str().unwrap(),
        "--idempotency-key",
        "excluded-vector-search",
    ]);
    let job = finished(&daemon, "excluded-vector-search");
    assert_eq!(job["state"], "succeeded", "{job}");
    let output: Value = serde_json::from_str(job["result"]["output"].as_str().unwrap()).unwrap();
    assert_eq!(output["mode"], "keyword", "{output}");
    assert_eq!(
        output["items"][0]["memory"]["revision"]["key"]["id"],
        "private"
    );
    assert!(
        output["coverage"]["gaps"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gap| gap == "vector_scope_excluded"),
        "{output}"
    );
    assert_eq!(endpoint.requests.lock().unwrap().len(), before);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}

fn build(daemon: &Daemon, id: &str) -> Value {
    daemon.cli(&["index", "sync", "--idempotency-key", id]);
    let job = finished(daemon, id);
    assert_eq!(job["state"], "succeeded", "{job}");
    job["result"].clone()
}

fn search(daemon: &Daemon, project: &str, id: &str) -> Value {
    let file = daemon.root.join("scope.json");
    std::fs::write(&file, serde_json::to_vec(&scope(project)).unwrap()).unwrap();
    daemon.cli(&[
        "search",
        "offline",
        "--scope",
        file.to_str().unwrap(),
        "--idempotency-key",
        id,
    ]);
    let job = finished(daemon, id);
    assert_eq!(job["state"], "succeeded", "{job}");
    serde_json::from_str(job["result"]["output"].as_str().unwrap()).unwrap()
}

#[test]
fn scope_changes_pin_the_old_selection_until_activation_and_restore_all_claims() {
    use sha2::Digest;

    let endpoint = super::super::model_tests::Endpoint::retrieval();
    let mut daemon = Daemon::new();
    seed(&daemon);
    daemon.configure(&settings(&endpoint.address, None));
    daemon.start();
    let all = build(&daemon, "all");
    assert_eq!(all["vector"]["indexed_rows"], 3);
    assert!(all["vector"].get("embedding_scopes").is_none());
    let legacy: akasha_otsuge::retrieval::VectorStatus =
        serde_json::from_value(all["vector"].clone()).unwrap();
    assert_eq!(serde_json::to_value(legacy).unwrap(), all["vector"]);
    let old_identity = sha2::Sha256::digest(
        serde_json::to_vec(&(
            "claim-4096-byte-mean-cosine-v1",
            &all["vector"]["model_identity"],
        ))
        .unwrap(),
    )
    .iter()
    .map(|byte| format!("{byte:02x}"))
    .collect::<String>();
    assert_eq!(all["vector"]["generation"], old_identity);

    daemon.configure(&settings(&endpoint.address, Some(&["alpha"])));
    daemon.cli(&["config", "reload"]);
    let alpha = build(&daemon, "alpha");
    assert_eq!(alpha["vector"]["indexed_rows"], 2);
    assert_ne!(alpha["vector"]["generation"], all["vector"]["generation"]);

    daemon.configure(&settings(&endpoint.address, Some(&["beta"])));
    daemon.cli(&["config", "reload"]);
    let during = search(&daemon, "alpha", "old-selected");
    assert_eq!(during["mode"], "hybrid", "{during}");
    assert_eq!(
        during["index"]["vector"]["generation"],
        alpha["vector"]["generation"]
    );
    assert!(
        during["coverage"]["gaps"]
            .as_array()
            .unwrap()
            .contains(&json!("generation_pending"))
    );
    assert!(
        !during["coverage"]["gaps"]
            .as_array()
            .unwrap()
            .contains(&json!("vector_scope_excluded"))
    );
    let before = endpoint.requests.lock().unwrap().len();
    let excluded = search(&daemon, "beta", "old-excluded");
    assert_eq!(excluded["mode"], "keyword", "{excluded}");
    assert!(
        excluded["coverage"]["gaps"]
            .as_array()
            .unwrap()
            .contains(&json!("vector_scope_excluded"))
    );
    assert_eq!(endpoint.requests.lock().unwrap().len(), before);

    let beta = build(&daemon, "beta");
    assert_eq!(beta["vector"]["indexed_rows"], 1);
    assert_ne!(beta["vector"]["generation"], alpha["vector"]["generation"]);
    assert_eq!(
        beta["vector"]["model_identity"],
        alpha["vector"]["model_identity"]
    );
    let excluded = search(&daemon, "alpha", "new-excluded");
    assert_eq!(excluded["mode"], "keyword", "{excluded}");
    assert_eq!(
        excluded["index"]["vector"]["generation"],
        beta["vector"]["generation"]
    );

    daemon.configure(&settings(&endpoint.address, None));
    daemon.cli(&["config", "reload"]);
    let restored = build(&daemon, "all-again");
    assert_eq!(
        restored["vector"]["generation"],
        all["vector"]["generation"]
    );
    assert_eq!(restored["vector"]["indexed_rows"], 3);
    assert!(restored["vector"].get("embedding_scopes").is_none());
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    let restarted = daemon.cli(&["index", "status"]);
    assert_eq!(
        restarted["data"]["vector"]["generation"],
        all["vector"]["generation"]
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}

#[test]
fn all_excluded_records_complete_the_prefix_without_embedding_calls() {
    let endpoint = super::super::model_tests::Endpoint::retrieval();
    let mut daemon = Daemon::new();
    let target = seed(&daemon);
    daemon.configure(&settings(&endpoint.address, Some(&["unused"])));
    daemon.start();
    let built = build(&daemon, "none-selected");
    assert_eq!(built["indexed_rows"], 3);
    assert_eq!(built["vector"]["indexed_rows"], 0);
    assert!(built["vector"]["watermark"].as_u64().unwrap() >= target);
    assert!(endpoint.requests.lock().unwrap().is_empty());
    let output = search(&daemon, "alpha", "text-only");
    assert_eq!(output["mode"], "keyword", "{output}");
    assert_eq!(
        output["items"][0]["memory"]["revision"]["key"]["id"],
        "offline"
    );
    assert!(endpoint.requests.lock().unwrap().is_empty());
    let empty = search(&daemon, "unused", "selected-but-empty");
    assert_eq!(empty["mode"], "hybrid", "{empty}");
    assert!(empty["items"].as_array().unwrap().is_empty());
    assert_eq!(endpoint.requests.lock().unwrap().len(), 1);
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
            let generation = built["vector"]["generation"].as_str().unwrap();
            let watermark = built["vector"]["watermark"].as_u64().unwrap();
            let mut excluded = 0;
            for seq in 1..=watermark {
                let completion = store
                    .current(&akasha_otsuge::storage::RecordKey {
                        kind: akasha_otsuge::storage::RecordKind::State,
                        id: format!("search/completion/{generation}/{seq:020}"),
                    })
                    .unwrap()
                    .expect("vector prefix has a completion gap");
                let value: Value = serde_json::from_str(&completion.content).unwrap();
                for record in value["records"].as_array().unwrap() {
                    assert_eq!(record["outcome"], "scope_excluded", "{value}");
                    excluded += 1;
                }
            }
            assert_eq!(excluded, 3);
            store.close().await.unwrap();
        });
}

#[test]
fn failed_explicit_rebuild_keeps_published_search_and_resumes_after_restart() {
    let endpoint = super::super::model_tests::Endpoint::retrieval();
    let mut daemon = Daemon::new();
    seed(&daemon);
    let retrieval =
        settings(&endpoint.address, Some(&["beta"])).replace("timeout='2s'", "timeout='30s'");
    daemon.configure(&format!("[runtime]\nmax_concurrent_jobs=2\n{retrieval}"));
    daemon.start();
    let old = build(&daemon, "published");
    let count = endpoint.requests.lock().unwrap().len();
    endpoint.pause();
    daemon.cli(&["index", "rebuild", "--idempotency-key", "replacement"]);
    let until = Instant::now() + Duration::from_secs(10);
    while endpoint.requests.lock().unwrap().len() == count {
        assert!(Instant::now() < until, "candidate did not reach embedding");
        std::thread::sleep(Duration::from_millis(10));
    }
    let building = daemon.cli(&["index", "status"])["data"].clone();
    assert_eq!(building["generation"], old["generation"]);
    assert_eq!(building["rebuild"]["phase"], "building");
    assert_ne!(building["rebuild"]["generation"], old["generation"]);
    assert_eq!(
        building["vector"]["generation"],
        old["vector"]["generation"]
    );
    let during = search(&daemon, "alpha", "during");
    assert_eq!(
        during["items"][0]["memory"]["revision"]["key"]["id"],
        "offline"
    );
    daemon.cli(&["job", "cancel", "replacement"]);
    endpoint.resume();
    let cancelled = finished(&daemon, "replacement");
    assert_ne!(cancelled["state"], "succeeded", "{cancelled}");
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    let original = search(&daemon, "alpha", "after-restart");
    assert_eq!(original["items"], during["items"]);
    let calls = endpoint.requests.lock().unwrap().len();
    endpoint.pause();
    daemon.cli(&[
        "index",
        "rebuild",
        "--idempotency-key",
        "replacement-second-key",
    ]);
    let until = Instant::now() + Duration::from_secs(10);
    while endpoint.requests.lock().unwrap().len() == calls {
        assert!(
            Instant::now() < until,
            "replacement did not resume embedding"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let retried = daemon.cli(&["index", "status"])["data"].clone();
    assert_eq!(
        retried["rebuild"]["generation"],
        building["rebuild"]["generation"]
    );
    assert_eq!(
        retried["rebuild"]["vector"]["generation"],
        building["rebuild"]["vector"]["generation"]
    );
    let table_count = std::fs::read_dir(daemon.root.join("data/search"))
        .unwrap()
        .filter_map(|p| p.unwrap().path().extension().map(|e| e == "lance"))
        .filter(|is_table| *is_table)
        .count();
    assert_eq!(
        table_count, 4,
        "only the published and candidate lexical/vector pairs are live"
    );
    daemon.cli(&["job", "cancel", "replacement-second-key"]);
    endpoint.resume();
    assert_eq!(
        finished(&daemon, "replacement-second-key")["state"],
        "cancelled"
    );
    let replacement = build(&daemon, "resume");
    assert_eq!(replacement["generation"], building["rebuild"]["generation"]);
    assert_ne!(replacement["generation"], old["generation"]);
    assert_ne!(
        replacement["vector"]["generation"],
        old["vector"]["generation"]
    );
    assert_eq!(replacement["vector"]["indexed_rows"], 1);
    let active = daemon.cli(&["index", "status"])["data"].clone();
    assert_eq!(
        active["active_vector"]["generation"],
        replacement["vector"]["generation"]
    );
    assert!(active.get("rebuild").is_none());
    let final_result = search(&daemon, "alpha", "after-publication");
    assert_eq!(final_result["items"], during["items"]);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}
