use super::*;

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
    seed_records(&daemon, &[("invalid", "alpha", &claim)], 1);
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
            ("first", "alpha", "offline first"),
            ("forbidden", "beta", "private forbidden"),
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
