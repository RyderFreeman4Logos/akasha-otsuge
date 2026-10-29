use super::*;

fn native(daemon: &Daemon, event: Value) -> Value {
    let mut stream = daemon.connect();
    assert_eq!(receive(&mut stream)["protocol_version"], 1);
    send(
        &mut stream,
        &json!({"protocol_version":1,"request_id":"native-pi","command":{"op":"pi","event":event}}),
    );
    receive(&mut stream)
}

#[test]
fn pi_registration_binding_and_backfill_survive_restart_with_current_grants() {
    let mut daemon = Daemon::new();
    daemon.configure(&format!("[access]\nread_roots=[{:?}]\n", daemon.root));
    let binary = daemon.root.join("pi");
    assert!(Command::new("python3").args(["-c", "import pathlib,sys; p=pathlib.Path(sys.argv[1]); p.write_text(sys.argv[2]); p.chmod(0o700)"]).arg(&binary).arg("#!/bin/sh\nprintf '1.4.0\\n'\n").status().unwrap().success());
    let agent_dir = daemon.root.join("agent");
    std::fs::create_dir(&agent_dir).unwrap();
    let transcript = agent_dir.join("native.jsonl");
    let mut rows: Vec<Value> = include_str!("../fixtures/pi/1.4.0/root-before.jsonl")
        .split_terminator('\n')
        .map(|v| serde_json::from_str(v).unwrap())
        .collect();
    rows[0]["cwd"] = json!(daemon.root);
    let session = rows[0]["id"].clone();
    let manifest: Value =
        serde_json::from_str(include_str!("../fixtures/pi/1.4.0/manifest.json")).unwrap();
    let turn = manifest["ids"]["assistant"].clone();
    let row = rows.iter_mut().find(|v| v["id"] == turn).unwrap();
    row["message"]["content"][2]["name"] = json!("akasha_consult");
    row["message"]["content"][2]["arguments"] = json!({"hint":"review"});
    let original = rows.iter().map(|v| format!("{v}\n")).collect::<String>();
    std::fs::write(&transcript, &original).unwrap();
    daemon.start();
    let register = json!({"op":"register","binary":binary,"agent_dir":agent_dir,"session_id":session,"transcript_path":transcript,"cwd":daemon.root});
    let registered = native(&daemon, register.clone());
    let token = registered["data"]["registration"]
        .as_str()
        .unwrap_or_else(|| panic!("{registered}"));
    assert_eq!(
        native(&daemon, register.clone())["data"],
        registered["data"]
    );
    let bind = json!({"op":"bind","registration":token,"call":{"session_id":session,"leaf_id":manifest["ids"]["tool"],"turn_id":turn,"tool_call_id":"call_fixture","agent_dir":agent_dir,"transcript":transcript,"cwd":daemon.root}});
    let bound = native(&daemon, bind.clone());
    assert!(bound.get("error").is_none(), "{bound}");
    assert_eq!(bound["data"]["requested_cursor"], 5);
    assert_eq!(
        bound["data"]["tip"]["event_id"],
        format!("entry:{}", turn.as_str().unwrap())
    );
    assert!(!bound.to_string().contains(token));
    let mut bridge = daemon
        .command()
        .args(["adapter", "pi-call"])
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
    let changed_file = agent_dir.join("duplicate-session.jsonl");
    std::fs::write(&changed_file, &original).unwrap();
    let mut changed = register.clone();
    changed["transcript_path"] = json!(changed_file);
    assert_eq!(
        native(&daemon, changed)["error"]["code"],
        "BINDING_CONFLICT"
    );
    let pending = native(
        &daemon,
        json!({"op":"sync","registration":token,"until_entry":"recovered"}),
    );
    let job_id = pending["data"]["id"]
        .as_str()
        .unwrap_or_else(|| panic!("{pending}"));
    let wait = Instant::now() + Duration::from_secs(10);
    let deadline = loop {
        let job = daemon.cli(&["job", "show", job_id]);
        if job["data"]["execution_deadline_ms"].is_u64() {
            break job["data"]["execution_deadline_ms"].clone();
        }
        assert!(Instant::now() < wait, "{job}");
        std::thread::sleep(Duration::from_millis(20));
    };
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(native(&daemon, register)["data"], registered["data"]);
    assert_eq!(native(&daemon, bind.clone())["data"], bound["data"]);
    let extra = json!({"type":"message","id":"recovered","parentId":manifest["ids"]["tool"],"timestamp":"2026-10-29T00:00:00Z","message":{"role":"user","content":"Recovered after interruption","timestamp":1793232000000_i64}});
    std::fs::write(&transcript, format!("{original}{extra}\n")).unwrap();
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let job = daemon.cli(&["job", "show", job_id]);
        assert_eq!(job["data"]["execution_deadline_ms"], deadline);
        if job["data"]["state"] == "succeeded" {
            break;
        }
        assert!(
            Instant::now() < end
                && matches!(job["data"]["state"].as_str(), Some("queued" | "running")),
            "{job}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    let cancelled = native(
        &daemon,
        json!({"op":"sync","registration":token,"until_entry":"never-arrives"}),
    );
    let cancelled_id = cancelled["data"]["id"].as_str().unwrap();
    assert_eq!(
        daemon.cli(&["job", "cancel", cancelled_id])["data"]["cancel_requested"],
        true
    );
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let job = daemon.cli(&["job", "show", cancelled_id]);
        if job["data"]["state"] == "cancelled" {
            assert_eq!(job["data"]["error"], "CANCELLED");
            break;
        }
        assert!(Instant::now() < end, "cancel was not collected: {job}");
        std::thread::sleep(Duration::from_millis(20));
    }
    assert_eq!(
        native(
            &daemon,
            json!({"op":"sync","registration":token,"until_entry":"never-arrives"})
        )["data"]["state"],
        "cancelled"
    );
    daemon.configure("[access]\nread_roots=[]\n");
    daemon.cli(&["config", "reload"]);
    assert!(native(&daemon, bind.clone()).get("error").is_some());
    daemon.configure(&format!("[access]\nread_roots=[{:?}]\n", daemon.root));
    daemon.cli(&["config", "reload"]);
    daemon.cli(&["access", "revoke"]);
    assert_eq!(native(&daemon, bind)["error"]["code"], "READ_DENIED");
}
