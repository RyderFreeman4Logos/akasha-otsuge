use super::*;

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
