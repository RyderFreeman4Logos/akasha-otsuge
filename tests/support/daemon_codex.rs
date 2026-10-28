use super::*;
use std::os::unix::fs::PermissionsExt;

fn native(daemon: &Daemon, event: Value) -> Value {
    let mut stream = daemon.connect();
    assert_eq!(receive(&mut stream)["protocol_version"], 1);
    send(
        &mut stream,
        &json!({"protocol_version":1,"request_id":"native-codex","command":{"op":"codex","event":event}}),
    );
    receive(&mut stream)
}

#[test]
fn native_registration_binding_and_backfill_survive_daemon_restart() {
    let mut daemon = Daemon::new();
    daemon.configure(&format!("[access]\nread_roots=[{:?}]\n", daemon.root));
    let binary = daemon.root.join("codex");
    std::fs::write(&binary, "#!/bin/sh\nprintf 'codex-cli 0.164.0\\n'\n").unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let raw = include_str!("../fixtures/codex/0.164.0/exec-root.jsonl")
        .replace("/fixture/workspace", daemon.root.to_str().unwrap());
    let transcript = daemon.root.join("native.jsonl");
    std::fs::write(&transcript, raw).unwrap();
    let hook: Value = serde_json::from_str(include_str!(
        "../fixtures/codex/0.164.0/exec-pre-tool-use.json"
    ))
    .unwrap();
    daemon.start();
    let doctor = daemon.cli(&[
        "doctor",
        "--host",
        "codex",
        "--binary",
        binary.to_str().unwrap(),
    ]);
    assert_eq!(doctor["data"]["version"], "0.164.0");
    assert_eq!(doctor["coverage"]["model_auth"], "not_checked");
    let registered = native(
        &daemon,
        json!({"op":"register","binary":binary,"session_id":hook["session_id"],"transcript_path":transcript,"cwd":daemon.root}),
    );
    let token = registered["data"]["registration"]
        .as_str()
        .expect("native registration must succeed");
    let bind = json!({"op":"bind","registration":token,"call":{"session_id":hook["session_id"],"agent_id":null,"turn_id":hook["turn_id"],"tool_call_id":hook["tool_use_id"],"cwd":daemon.root}});
    let bound = native(&daemon, bind.clone());
    assert!(bound.get("error").is_none(), "{bound}");
    assert_eq!(bound["data"]["requested_cursor"], 10);
    assert_eq!(bound["data"]["tip"]["event_id"], "row:9");
    assert!(!bound.to_string().contains(token));
    let mut bridge = daemon
        .command()
        .args(["adapter", "codex-call"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    bridge
        .stdin
        .take()
        .unwrap()
        .write_all(bind.to_string().as_bytes())
        .unwrap();
    let output = bridge.wait_with_output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&output.stdout).unwrap()["data"],
        bound["data"]
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(native(&daemon, bind.clone())["data"], bound["data"]);
    let sync = native(
        &daemon,
        json!({"op":"sync","registration":token,"until_ordinal":18}),
    );
    assert!(sync.get("error").is_none(), "{sync}");
    let id = sync["data"]["id"].as_str().unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let job = daemon.cli(&["job", "show", id]);
        match job["data"]["state"].as_str() {
            Some("succeeded") => break,
            Some("queued" | "running") if Instant::now() < end => {
                std::thread::sleep(Duration::from_millis(20))
            }
            _ => panic!("native capture did not finish: {job}"),
        }
    }
    let mut wrong = bind.clone();
    wrong["call"]["turn_id"] = "other-turn".into();
    assert_eq!(native(&daemon, wrong)["error"]["code"], "CALLER_AMBIGUOUS");
    daemon.cli(&["access", "revoke"]);
    assert_eq!(native(&daemon, bind)["error"]["code"], "READ_DENIED");
}
