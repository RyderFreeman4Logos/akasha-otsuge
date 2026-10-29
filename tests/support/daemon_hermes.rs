use super::*;

fn bridge(daemon: &Daemon, phase: &str, event: Value) -> Value {
    let started = Instant::now();
    let mut child = daemon
        .command()
        .args(["adapter", "hermes-call"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(event.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{phase} after {:?}: {}",
        started.elapsed(),
        String::from_utf8_lossy(&out.stderr)
    );
    serde_json::from_slice(&out.stdout).unwrap()
}

#[test]
fn hermes_bridge_exports_the_requested_native_home_and_persists_background_history() {
    let mut daemon = Daemon::new();
    daemon.configure(&format!("[access]\nread_roots=[{:?}]\n", daemon.root));
    let home = daemon.root.join("hermes-home");
    std::fs::create_dir(&home).unwrap();
    let mut export: Value =
        serde_json::from_str(include_str!("../fixtures/hermes/0.22.5/root-before.jsonl")).unwrap();
    export["cwd"] = json!(daemon.root);
    export["messages"][2]["tool_calls"][0]["function"]["name"] = "akasha_consult".into();
    export["messages"][2]["tool_calls"][0]["function"]["arguments"] = "{}".into();
    std::fs::write(
        home.join("native.jsonl"),
        serde_json::to_vec(&export).unwrap(),
    )
    .unwrap();
    let binary = daemon.root.join("hermes");
    crate::executable::write(
        &binary,
        "#!/bin/sh\nif [ \"$#\" = 1 ] && [ \"$1\" = --version ]; then printf 'Hermes Agent v0.22.5+1914.gfdb04a0\\n'; exit 0; fi\n[ \"$#\" = 7 ] && [ \"$1\" = sessions ] && [ \"$2\" = export ] && [ \"$3\" = - ] && [ \"$4\" = --session-id ] && [ \"$6\" = --format ] && [ \"$7\" = jsonl ] || exit 78\n[ \"$HERMES_DISABLE_LAZY_INSTALLS\" = 1 ] || exit 79\nexec /bin/cat \"$HERMES_HOME/native.jsonl\"\n",
    );
    daemon.start();
    let event = json!({"op":"bind","binary":binary,"home":home,"call":{
        "session_id":export["id"],"task_id":"native-task","turn_id":"native-turn","tool_call_id":"call_alpha",
        "profile_home":home,"cwd":daemon.root}});
    let bound = bridge(&daemon, "first-bind", event.clone());
    assert_eq!(bound["data"]["requested_cursor"], 4);
    assert_eq!(bound["data"]["caller"]["adapter"], "hermes");
    assert_eq!(
        bridge(&daemon, "retry-bind", event)["data"]["invocation_id"],
        bound["data"]["invocation_id"]
    );
    let synced = bridge(
        &daemon,
        "background-sync",
        json!({"op":"sync","binary":binary,"home":home,"session_id":export["id"]}),
    );
    assert!(synced["data"]["id"].is_string(), "{synced}");
    let id = synced["data"]["id"].as_str().unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let state = daemon.cli(&["job", "show", id]);
        if state["data"]["state"] == "succeeded" {
            break;
        }
        assert_ne!(state["data"]["state"], "failed", "{state}");
        assert!(Instant::now() < end, "{state}");
        std::thread::sleep(Duration::from_millis(20));
    }
    let spool = home.join("plugin-data/akasha-otsuge/capture");
    let files = std::fs::read_dir(&spool)
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect::<Vec<_>>();
    assert_eq!(files.len(), 1);
    let text = std::fs::read_to_string(&files[0]).unwrap();
    assert!(!text.contains("PRIVATE_REASONING_SENTINEL"));
    assert!(!text.contains("PRIVATE_CONFIG_SENTINEL"));
    assert_eq!(std::fs::metadata(&files[0]).unwrap().mode() & 0o777, 0o600);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}
