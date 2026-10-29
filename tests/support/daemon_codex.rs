use super::*;

fn native(daemon: &Daemon, event: Value) -> Value {
    let mut stream = daemon.connect();
    assert_eq!(receive(&mut stream)["protocol_version"], 1);
    send(
        &mut stream,
        &json!({"protocol_version":1,"request_id":"native-codex","command":{"op":"codex","event":event}}),
    );
    receive(&mut stream)
}

fn consult_fixture(scenario: &str) -> (Daemon, Value, Value) {
    let mut daemon = Daemon::new();
    let workspace = daemon.root.join("workspace");
    let home = daemon.root.join("native-home");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::create_dir(&home).unwrap();
    daemon.configure(&format!("[access]\nread_roots=[{workspace:?},{home:?}]\n"));
    let binary = daemon.root.join("codex");
    let host = include_str!("../fixtures/codex/0.164.0/runner_host.py")
        .replace("scenario = (root / 'scenario').read_text()", "scenario = (root / 'scenario').read_text()\nwith (root / 'starts').open('a') as f: f.write('start\\n')\n(root / 'runner.pid').write_text(str(os.getpid()))");
    crate::executable::write(&binary, &host);
    std::fs::write(daemon.root.join("scenario"), scenario).unwrap();
    std::fs::write(home.join("models_cache.json"), r#"{"models":[{"slug":"fixture","shell_type":"unified_exec","apply_patch_tool_type":"freeform","experimental_supported_tools":[],"tool_mode":"code_mode"}]}"#).unwrap();
    let transcript = workspace.join("native.jsonl");
    std::fs::write(
        &transcript,
        include_str!("../fixtures/codex/0.164.0/exec-root.jsonl")
            .replace("/fixture/workspace", workspace.to_str().unwrap()),
    )
    .unwrap();
    let hook: Value = serde_json::from_str(include_str!(
        "../fixtures/codex/0.164.0/exec-pre-tool-use.json"
    ))
    .unwrap();
    daemon.start();
    let registered = native(
        &daemon,
        json!({"op":"register","binary":binary,"session_id":hook["session_id"],"transcript_path":transcript,"cwd":workspace,"native_home":home}),
    );
    assert!(registered.get("error").is_none(), "{registered}");
    let token = registered["data"]["registration"].as_str().unwrap();
    let call = json!({"session_id":hook["session_id"],"agent_id":null,"turn_id":hook["turn_id"],"tool_call_id":hook["tool_use_id"],"cwd":workspace});
    let bound = native(
        &daemon,
        json!({"op":"bind","registration":token,"call":call}),
    );
    assert!(bound.get("error").is_none(), "{bound}");
    let event = json!({"op":"consult","registration":token,"call":call,"args":{"hint":null}});
    (daemon, event, bound)
}

#[test]
fn consult_reused_profile_obeys_current_read_roots() {
    consult_home_policy(false, false);
}

#[test]
fn consult_queued_profile_obeys_current_read_roots() {
    consult_home_policy(true, false);
}

#[test]
fn consult_reused_profile_rejects_path_redirection() {
    consult_home_policy(false, true);
}

#[test]
fn consult_queued_profile_rejects_path_redirection() {
    consult_home_policy(true, true);
}

fn consult_home_policy(queued: bool, redirect: bool) {
    let (daemon, event, _) = consult_fixture("success");
    let workspace = daemon.root.join("workspace");
    let home = daemon.root.join("native-home");
    let endpoint = super::model_tests::Endpoint::retrieval();
    endpoint.pause();
    let model = format!(
        "[models.hold]\nkind='embedding'\nendpoint={:?}\nmodel='vectors-2'\ndimension=2\nlocal=true\n",
        endpoint.address
    );
    let mut job_id = None;
    if queued {
        daemon.configure(&format!(
            "[access]\nread_roots=[{workspace:?},{home:?}]\n{model}"
        ));
        daemon.cli(&["config", "reload"]);
        let plan = daemon.root.join("hold.json");
        std::fs::write(
            &plan,
            json!({"calls":[{"profile":"hold","input":{"kind":"embedding","texts":["barrier"]}}]})
                .to_string(),
        )
        .unwrap();
        daemon.cli(&[
            "model",
            "run",
            plan.to_str().unwrap(),
            "--idempotency-key",
            "hold",
        ]);
        let end = Instant::now() + Duration::from_secs(10);
        while endpoint.requests.lock().unwrap().is_empty() {
            assert!(Instant::now() < end, "model did not reach the barrier");
            std::thread::sleep(Duration::from_millis(10));
        }
        let submitted = native(&daemon, event.clone());
        assert_eq!(submitted["data"]["state"], "queued", "{submitted}");
        assert_eq!(submitted["data"]["config_generation"], 1);
        job_id = Some(submitted["data"]["id"].as_str().unwrap().to_owned());
    }
    if redirect {
        let replacement = daemon.root.join("redirected-home");
        std::fs::rename(&home, &replacement).unwrap();
        std::os::unix::fs::symlink(replacement, &home).unwrap();
    } else {
        daemon.configure(&format!("[access]\nread_roots=[{workspace:?}]\n{model}"));
        daemon.cli(&["config", "reload"]);
    }
    let expected = if redirect {
        "READ_DENIED"
    } else {
        "PATH_DENIED"
    };
    if let Some(id) = job_id {
        endpoint.resume();
        let end = Instant::now() + Duration::from_secs(10);
        let job = loop {
            let job = daemon.cli(&["job", "show", &id])["data"].clone();
            if !matches!(job["state"].as_str(), Some("queued" | "running")) {
                break job;
            }
            assert!(Instant::now() < end, "queued consult did not finish: {job}");
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(job["state"], "failed", "{job}");
        assert_eq!(job["error"], expected);
    } else {
        assert_eq!(native(&daemon, event)["error"]["code"], expected);
    }
    assert!(
        !daemon.root.join("starts").exists(),
        "denied profile launched a native worker"
    );
}

#[test]
fn consult_job_is_distinct_durable_and_deduplicates_native_execution() {
    let (mut daemon, event, bound) = consult_fixture("success");
    let submitted = native(&daemon, event.clone());
    assert!(submitted.get("error").is_none(), "{submitted}");
    let id = submitted["data"]["id"].as_str().unwrap();
    assert_ne!(id, bound["data"]["invocation_id"].as_str().unwrap());
    assert_eq!(native(&daemon, event.clone())["data"]["id"], id);
    let mut conflicting = event.clone();
    conflicting["args"]["hint"] = "A different request".into();
    assert_eq!(
        native(&daemon, conflicting)["error"]["code"],
        "IDEMPOTENCY_CONFLICT"
    );
    let end = Instant::now() + Duration::from_secs(20);
    let complete = loop {
        let job = daemon.cli(&["job", "show", id]);
        match job["data"]["state"].as_str() {
            Some("succeeded") => break job["data"].clone(),
            Some("queued" | "running") if Instant::now() < end => {
                assert!(job["data"]["result"].is_null());
                std::thread::sleep(Duration::from_millis(20));
            }
            _ => panic!("consult failed: {job}"),
        }
    };
    assert!(complete["result"]["receipt_id"].is_string());
    assert!(
        complete["result"]["output"]
            .as_str()
            .unwrap()
            .contains("bounded-context")
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(native(&daemon, event)["data"]["id"], id);
    assert_eq!(
        daemon.cli(&["job", "show", id])["data"]["result"],
        complete["result"]
    );
    assert_eq!(
        std::fs::read_to_string(daemon.root.join("starts")).unwrap(),
        "start\n"
    );
    daemon.cli(&["access", "revoke"]);
    let out = daemon.command().args(["job", "show", id]).output().unwrap();
    assert!(!out.status.success());
}

#[test]
fn consult_cancel_shutdown_and_crash_end_native_processes_without_replaying() {
    consult_lifecycle("cancel");
}

#[test]
fn consult_detached_descendants_settle_before_cancellation_releases_ownership() {
    consult_lifecycle("cancel_detached");
}

fn consult_lifecycle(scenario: &str) {
    use rustix::process::{Pid, PidfdFlags, Signal, pidfd_open, pidfd_send_signal};
    struct Cleanup(Vec<rustix::fd::OwnedFd>);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            for fd in &self.0 {
                let _ = pidfd_send_signal(fd, Signal::KILL);
            }
        }
    }
    for mode in ["cancel", "shutdown", "crash"] {
        let (mut daemon, event, _) = consult_fixture(scenario);
        let submitted = native(&daemon, event.clone());
        assert!(submitted.get("error").is_none(), "{submitted}");
        let id = submitted["data"]["id"].as_str().unwrap();
        let end = Instant::now() + Duration::from_secs(10);
        while !daemon.root.join("child.pid").exists() {
            assert!(Instant::now() < end, "native fixture did not start");
            std::thread::sleep(Duration::from_millis(10));
        }
        let mut cleanup = Cleanup(Vec::new());
        let mut processes = Vec::new();
        for file in ["runner.pid", "child.pid"] {
            let pid: i32 = std::fs::read_to_string(daemon.root.join(file))
                .unwrap()
                .parse()
                .unwrap();
            cleanup
                .0
                .push(pidfd_open(Pid::from_raw(pid).unwrap(), PidfdFlags::empty()).unwrap());
            processes.push(pid);
        }
        if mode == "cancel" {
            daemon.cli(&["job", "cancel", id]);
        } else if mode == "shutdown" {
            daemon.cli(&["daemon", "stop"]);
            daemon.wait();
        } else {
            let mut child = daemon.child.take().unwrap();
            child.kill().unwrap();
            child.wait().unwrap();
        }
        let end = Instant::now() + Duration::from_secs(3);
        loop {
            let live = processes
                .iter()
                .any(|pid| std::path::Path::new(&format!("/proc/{pid}")).exists());
            if !live {
                break;
            }
            assert!(
                Instant::now() < end,
                "daemon crash left a live native process"
            );
            std::thread::sleep(Duration::from_millis(10));
        }
        if mode != "cancel" {
            daemon.start();
        }
        let end = Instant::now() + Duration::from_secs(10);
        let job = loop {
            let job = daemon.cli(&["job", "show", id]);
            if matches!(job["data"]["state"].as_str(), Some("failed" | "cancelled")) {
                break job;
            }
            assert!(
                Instant::now() < end,
                "terminal state was not persisted: {job}"
            );
            std::thread::sleep(Duration::from_millis(10));
        };
        assert_eq!(
            job["data"]["state"],
            if mode == "cancel" {
                "cancelled"
            } else {
                "failed"
            }
        );
        assert_eq!(
            job["data"]["error"],
            if mode == "cancel" {
                "CANCELLED"
            } else {
                "RUNNER_INTERRUPTED"
            }
        );
        assert_eq!(
            std::fs::read_dir(daemon.root.join("data/runner-work"))
                .unwrap()
                .count(),
            0,
            "{mode}"
        );
        assert_eq!(native(&daemon, event)["data"]["id"], id);
        assert_eq!(
            std::fs::read_to_string(daemon.root.join("starts")).unwrap(),
            "start\n"
        );
    }
}

#[test]
fn native_hook_uses_private_call_files_without_exposing_registration_credentials() {
    let mut daemon = Daemon::new();
    daemon.configure(&format!("[access]\nread_roots=[{:?}]\n", daemon.root));
    let binary = daemon.root.join("codex");
    crate::executable::write(&binary, "#!/bin/sh\nprintf 'codex-cli 0.164.0\\n'\n");
    let transcript = daemon.root.join("native.jsonl");
    let raw = include_str!("../fixtures/codex/0.164.0/exec-root.jsonl")
        .replace("/fixture/workspace", daemon.root.to_str().unwrap());
    std::fs::write(&transcript, raw).unwrap();
    let mut hook: Value = serde_json::from_str(include_str!(
        "../fixtures/codex/0.164.0/exec-pre-tool-use.json"
    ))
    .unwrap();
    hook["cwd"] = json!(daemon.root);
    hook["transcript_path"] = json!(transcript);
    hook["tool_input"]["command"] = "akasha-otsuge consult \"find prior decision\"".into();
    let state = daemon.root.join("hook-state");
    daemon.start();
    let mut child = daemon
        .command()
        .args([
            "adapter",
            "codex-hook",
            "--binary",
            binary.to_str().unwrap(),
            "--state-dir",
            state.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(hook.to_string().as_bytes())
        .unwrap();
    let out = child.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let result: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(result["hookSpecificOutput"]["permissionDecision"], "allow");
    let command = result["hookSpecificOutput"]["updatedInput"]["command"]
        .as_str()
        .unwrap();
    let mut call_files = Vec::new();
    for entry in std::fs::read_dir(&state).unwrap() {
        let path = entry.unwrap().path();
        if path
            .file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("call-")
        {
            call_files.push(path);
        }
    }
    assert_eq!(call_files.len(), 1);
    let private: Value = serde_json::from_slice(&std::fs::read(&call_files[0]).unwrap()).unwrap();
    assert_eq!(
        std::fs::metadata(&call_files[0]).unwrap().mode() & 0o777,
        0o600
    );
    assert!(
        private.get("registration").is_none(),
        "sandbox-readable result must not contain a registration capability"
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    let bound = Command::new("/bin/sh")
        .args(["-c", command])
        .output()
        .unwrap();
    assert!(
        bound.status.success(),
        "{}",
        String::from_utf8_lossy(&bound.stderr)
    );
    assert!(
        serde_json::from_slice::<Value>(&bound.stdout)
            .unwrap()
            .get("registration")
            .is_none()
    );
    assert_eq!(
        serde_json::from_slice::<Value>(&bound.stdout).unwrap()["data"]["requested_cursor"],
        10
    );
    assert!(
        call_files[0].exists(),
        "the sandboxed reader must not need write access"
    );
    hook["hook_event_name"] = "PostToolUse".into();
    let mut child = daemon
        .command()
        .args([
            "adapter",
            "codex-hook",
            "--binary",
            binary.to_str().unwrap(),
            "--state-dir",
            state.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    child
        .stdin
        .take()
        .unwrap()
        .write_all(hook.to_string().as_bytes())
        .unwrap();
    assert!(child.wait_with_output().unwrap().status.success());
    assert!(
        !call_files[0].exists(),
        "host callback must remove the consumed call file"
    );
    hook["hook_event_name"] = "PreToolUse".into();
    let mut failed = daemon
        .command()
        .args([
            "adapter",
            "codex-hook",
            "--binary",
            binary.to_str().unwrap(),
            "--state-dir",
            state.to_str().unwrap(),
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    failed
        .stdin
        .take()
        .unwrap()
        .write_all(hook.to_string().as_bytes())
        .unwrap();
    let out = failed.wait_with_output().unwrap();
    assert!(
        out.status.success(),
        "recognized invocation errors must deny through the native protocol"
    );
    let denied: Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(denied["hookSpecificOutput"]["permissionDecision"], "deny");
    assert_eq!(
        denied["hookSpecificOutput"]["permissionDecisionReason"],
        "DAEMON_UNAVAILABLE"
    );
    assert!(denied["hookSpecificOutput"].get("updatedInput").is_none());
}

#[test]
fn concurrent_native_hooks_share_state_without_cross_binding_or_global_lock_contention() {
    let mut daemon = Daemon::new();
    daemon.configure(&format!("[access]\nread_roots=[{:?}]\n", daemon.root));
    let binary = daemon.root.join("codex");
    crate::executable::write(
        &binary,
        "#!/bin/sh\nsleep 0.2\nprintf 'codex-cli 0.164.0\\n'\n",
    );
    let state = daemon.root.join("hook-state");
    daemon.start();
    let original: Value = serde_json::from_str(include_str!(
        "../fixtures/codex/0.164.0/exec-pre-tool-use.json"
    ))
    .unwrap();
    let mut children = Vec::new();
    for n in 0..2 {
        let session = format!("concurrent-session-{n}");
        let path = daemon.root.join(format!("native-{n}.jsonl"));
        let raw = include_str!("../fixtures/codex/0.164.0/exec-root.jsonl")
            .replace(original["session_id"].as_str().unwrap(), &session)
            .replace("/fixture/workspace", daemon.root.to_str().unwrap());
        std::fs::write(&path, raw).unwrap();
        let mut hook = original.clone();
        hook["session_id"] = session.clone().into();
        hook["cwd"] = json!(daemon.root);
        hook["transcript_path"] = json!(path);
        hook["tool_input"]["command"] = "akasha-otsuge consult".into();
        let mut child = daemon
            .command()
            .args([
                "adapter",
                "codex-hook",
                "--binary",
                binary.to_str().unwrap(),
                "--state-dir",
                state.to_str().unwrap(),
            ])
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .unwrap();
        child
            .stdin
            .take()
            .unwrap()
            .write_all(hook.to_string().as_bytes())
            .unwrap();
        children.push((session, child));
    }
    for (session, child) in children {
        let out = child.wait_with_output().unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let hook: Value = serde_json::from_slice(&out.stdout).unwrap();
        let command = hook["hookSpecificOutput"]["updatedInput"]["command"]
            .as_str()
            .unwrap();
        let out = Command::new("/bin/sh")
            .args(["-c", command])
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "{}",
            String::from_utf8_lossy(&out.stderr)
        );
        let bound: Value = serde_json::from_slice(&out.stdout).unwrap();
        assert_eq!(bound["data"]["caller"]["session"], session);
    }
}

#[test]
fn native_registration_binding_and_backfill_survive_daemon_restart() {
    let mut daemon = Daemon::new();
    daemon.configure(&format!("[access]\nread_roots=[{:?}]\n", daemon.root));
    let binary = daemon.root.join("codex");
    crate::executable::write(&binary, "#!/bin/sh\nprintf 'codex-cli 0.164.0\\n'\n");
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

#[test]
fn codex_launcher_install_is_opt_in_atomic_and_preserves_edited_files() {
    let daemon = Daemon::new();
    let binary = daemon.root.join("codex ' native");
    crate::executable::write(&binary, "#!/bin/sh\nprintf 'codex-cli 0.164.0\\n'\n");
    let launcher = daemon.root.join("akasha codex");
    let state = daemon.root.join("call state");
    let install = || {
        daemon
            .command()
            .args([
                "adapter",
                "install",
                "codex",
                "--binary",
                binary.to_str().unwrap(),
                "--launcher",
                launcher.to_str().unwrap(),
                "--state-dir",
                state.to_str().unwrap(),
            ])
            .output()
            .unwrap()
    };
    let first = install();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let original = std::fs::read(&launcher).unwrap();
    assert_eq!(std::fs::metadata(&launcher).unwrap().mode() & 0o777, 0o700);
    assert!(
        Command::new(&launcher)
            .arg("--version")
            .output()
            .unwrap()
            .status
            .success()
    );
    assert!(install().status.success());
    assert_eq!(std::fs::read(&launcher).unwrap(), original);
    let mut edited = original.clone();
    edited.extend_from_slice(b"# user change\n");
    std::fs::write(&launcher, &edited).unwrap();
    assert!(!install().status.success());
    let undo = || {
        daemon
            .command()
            .args([
                "adapter",
                "uninstall",
                "codex",
                "--launcher",
                launcher.to_str().unwrap(),
            ])
            .output()
            .unwrap()
    };
    assert!(!undo().status.success());
    assert_eq!(std::fs::read(&launcher).unwrap(), edited);
    std::fs::write(&launcher, original).unwrap();
    assert!(undo().status.success());
    assert!(!launcher.exists());
    std::os::unix::fs::symlink(&binary, &launcher).unwrap();
    assert!(!install().status.success());
    assert!(!undo().status.success());
    assert!(binary.exists());
}
