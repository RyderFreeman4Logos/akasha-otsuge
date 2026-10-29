#![cfg(target_os = "linux")]

use serde_json::{Value, json};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, MetadataExt};
use std::os::unix::net::UnixStream;
use std::path::PathBuf;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{Duration, Instant};

const BIN: &str = env!("CARGO_BIN_EXE_akasha-otsuge");

#[path = "support/daemon_models.rs"]
mod model_tests;

#[path = "support/daemon_retrieval.rs"]
mod retrieval_tests;

#[path = "support/daemon_codex.rs"]
mod codex_tests;

#[path = "support/daemon_hermes.rs"]
mod hermes_tests;
#[path = "support/daemon_pi.rs"]
mod pi_tests;

struct Daemon {
    root: PathBuf,
    socket_name: String,
    child: Option<Child>,
}
impl Daemon {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "akasha-daemon-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::DirBuilder::new()
            .mode(0o700)
            .create(&root)
            .unwrap();
        let daemon = Self {
            root,
            socket_name: "service.sock".into(),
            child: None,
        };
        daemon.configure("");
        daemon
    }
    fn configure(&self, extra: &str) {
        let input = format!(
            "[paths]\ndata_dir = {:?}\nsocket_path = {:?}\n{extra}",
            self.root.join("data"),
            self.socket()
        );
        std::fs::write(self.root.join("config.toml"), input).unwrap();
    }
    fn socket(&self) -> PathBuf {
        self.root.join("run").join(&self.socket_name)
    }
    fn command(&self) -> Command {
        let mut command = Command::new(
            std::env::var_os("AKASHA_ACCEPTANCE_BINARY").unwrap_or_else(|| BIN.into()),
        );
        command
            .env("HOME", &self.root)
            .env_remove("XDG_CONFIG_HOME")
            .env_remove("XDG_DATA_HOME")
            .env_remove("XDG_RUNTIME_DIR")
            .arg("--config")
            .arg(self.root.join("config.toml"))
            .arg("--socket")
            .arg(self.socket());
        command
    }
    fn start(&mut self) {
        self.start_with(&[]);
    }
    fn start_with(&mut self, options: &[&str]) {
        self.child = Some(
            self.command()
                .args(options)
                .args(["daemon", "run"])
                .stdin(Stdio::null())
                .stdout(Stdio::null())
                .stderr(Stdio::piped())
                .spawn()
                .unwrap(),
        );
        let end = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(exit) = self.child.as_mut().unwrap().try_wait().unwrap() {
                let mut stderr = String::new();
                self.child
                    .as_mut()
                    .unwrap()
                    .stderr
                    .take()
                    .unwrap()
                    .read_to_string(&mut stderr)
                    .unwrap();
                panic!("daemon exited before readiness: {exit}: {stderr}");
            }
            if UnixStream::connect(self.socket()).is_ok() {
                break;
            }
            assert!(Instant::now() < end, "daemon never became ready");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
    fn cli(&self, args: &[&str]) -> Value {
        let output = self.command().args(args).output().unwrap();
        assert!(
            output.status.success(),
            "{:?}: {}",
            args,
            String::from_utf8_lossy(&output.stderr)
        );
        serde_json::from_slice(&output.stdout).unwrap()
    }
    fn connect(&self) -> UnixStream {
        let stream = UnixStream::connect(self.socket()).unwrap();
        stream
            .set_read_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
            .set_write_timeout(Some(Duration::from_secs(5)))
            .unwrap();
        stream
    }
    fn wait(&mut self) {
        let end = Instant::now() + Duration::from_secs(15);
        loop {
            if let Some(exit) = self.child.as_mut().unwrap().try_wait().unwrap() {
                assert!(exit.success());
                self.child = None;
                return;
            }
            assert!(Instant::now() < end, "daemon did not drain");
            std::thread::sleep(Duration::from_millis(20));
        }
    }
}

#[test]
fn production_signals_drain_and_release_the_socket_for_restart() {
    let mut daemon = Daemon::new();
    for signal in ["-TERM", "-INT"] {
        daemon.start();
        assert_eq!(
            daemon.cli(&["daemon", "status"])["data"]["state"],
            "running"
        );
        assert!(
            Command::new("/bin/kill")
                .args([signal, &daemon.child.as_ref().unwrap().id().to_string()])
                .status()
                .unwrap()
                .success()
        );
        daemon.wait();
        assert!(!daemon.socket().exists());
    }
}

#[test]
fn explicit_restart_activates_pending_socket_and_preserves_old_job_plan() {
    let mut daemon = Daemon::new();
    let access = format!("[access]\nread_roots=[{:?}]\n", daemon.root);
    daemon.configure(&access);
    daemon.start();
    for (step, runner) in [(1, 1800), (2, 2200)] {
        let prior_generation = daemon.cli(&["config", "status"])["data"]["active_generation"]
            .as_u64()
            .unwrap();
        if step == 2 {
            let binding = json!({"source":{"adapter":"fixture","profile":"test","session":"restart","thread":"thread","agent":"agent"},"scope":{"user":format!("uid:{}",rustix::process::geteuid().as_raw()),"project":null,"branch_or_version":null,"session":null},"stream":"stream","coverage":"available_payloads"});
            std::fs::write(daemon.root.join("binding.json"), binding.to_string()).unwrap();
            let mut file = std::fs::File::create(daemon.root.join("capture.jsonl")).unwrap();
            for n in 0..4096 {
                writeln!(file,"{}",json!({"schema_version":1,"event":{"event_id":format!("e-{n}"),"source_version":"1","runtime_instance":"test","branch":"main","parent":null,"fork_cutoff":null,"event_time_ms":null,"observed_time_ms":1,"visibility":"user","kind":{"kind":"resume"},"text":"original"},"raw_hex":"78"})).unwrap();
            }
            drop(file);
            let accepted = daemon.cli(&[
                "capture",
                "reconcile",
                daemon.root.join("binding.json").to_str().unwrap(),
                daemon.root.join("capture.jsonl").to_str().unwrap(),
                "--idempotency-key",
                "old-plan",
            ]);
            assert_eq!(accepted["data"]["config_generation"], prior_generation);
        }
        let old_socket = daemon.socket();
        let next_name = format!("service-{step}.sock");
        let next_socket = daemon.root.join("run").join(&next_name);
        let candidate = format!(
            "[paths]\ndata_dir={:?}\nsocket_path={:?}\n{access}[timeouts]\nrunner='{runner}s'\n",
            daemon.root.join("data"),
            next_socket
        );
        std::fs::write(daemon.root.join("config.toml"), candidate).unwrap();
        assert_eq!(
            daemon.cli(&["config", "reload"])["data"]["status"],
            "restart_required"
        );
        let previous_job =
            (step == 2).then(|| daemon.cli(&["job", "show", "old-plan"])["data"].clone());
        daemon.cli(&["daemon", "stop"]);
        daemon.wait();
        assert!(!old_socket.exists());
        daemon.socket_name = next_name;
        daemon.start();
        let status = daemon.cli(&["config", "status"])["data"].clone();
        assert_eq!(status["active_generation"], prior_generation + 1);
        assert_eq!(
            status["effective"]["config"]["paths"]["socket_path"],
            next_socket.to_str().unwrap()
        );
        assert_eq!(
            status["effective"]["effective_timeouts"]["runner"]["secs"],
            runner
        );
        assert!(status["pending"].is_null());
        if let Some(previous) = previous_job {
            let job = daemon.cli(&["job", "show", "old-plan"])["data"].clone();
            assert_eq!(job["config_generation"], prior_generation);
            assert_eq!(job["config_digest"], previous["config_digest"]);
            if !previous["started_at_ms"].is_null() {
                assert_eq!(job["started_at_ms"], previous["started_at_ms"]);
                assert_eq!(
                    job["execution_deadline_ms"],
                    previous["execution_deadline_ms"]
                );
            }
        }
    }
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}

#[test]
fn selected_profile_large_config_and_pending_paths_survive_restart() {
    let mut daemon = Daemon::new();
    let roots = serde_json::to_string(&vec!["/large/allowed"; 12000]).unwrap();
    let extra =
        format!("[access]\nread_roots={roots}\n[profiles.patient.timeouts]\nrunner='2100s'\n");
    daemon.configure(&extra);
    let original = std::fs::read(daemon.root.join("config.toml")).unwrap();
    assert!(original.len() > 128 * 1024);
    daemon.start_with(&["--profile", "patient"]);
    let status = daemon.cli(&["config", "status"])["data"].clone();
    assert_eq!(status["active_generation"], 1);
    assert_eq!(status["effective"]["selected_profile"], "patient");
    assert_eq!(
        status["effective"]["effective_timeouts"]["runner"]["secs"],
        2100
    );
    assert_eq!(
        status["effective"]["config"]["access"]["read_roots"]
            .as_array()
            .unwrap()
            .len(),
        12000
    );
    // Removing explicit paths must resolve the same startup defaults in validate and reload.
    std::fs::write(daemon.root.join("config.toml"), &extra).unwrap();
    let validated = daemon.cli(&["config", "validate"])["data"].clone();
    assert_eq!(
        validated["config"]["paths"]["data_dir"],
        daemon
            .root
            .join(".local/share/akasha-otsuge")
            .to_str()
            .unwrap()
    );
    assert_eq!(
        daemon.cli(&["config", "reload"])["data"]["status"],
        "restart_required"
    );
    let pending = daemon.cli(&["config", "status"])["data"].clone();
    assert_eq!(pending["pending"]["desired"], validated);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    std::fs::write(daemon.root.join("config.toml"), original).unwrap();
    daemon.start_with(&["--profile", "patient"]);
    let reopened = daemon.cli(&["config", "status"])["data"].clone();
    assert_eq!(reopened, pending);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}
impl Drop for Daemon {
    fn drop(&mut self) {
        if let Some(child) = self.child.as_mut() {
            let _ = child.kill();
            let _ = child.wait();
        }
        std::fs::remove_dir_all(&self.root).unwrap();
    }
}
fn receive(stream: &mut UnixStream) -> Value {
    let mut prefix = [0; 4];
    stream.read_exact(&mut prefix).unwrap();
    let n = u32::from_be_bytes(prefix) as usize;
    assert!(n <= 2 * 1024 * 1024);
    let mut bytes = vec![0; n];
    stream.read_exact(&mut bytes).unwrap();
    serde_json::from_slice(&bytes).unwrap()
}
fn send(stream: &mut UnixStream, value: &Value) {
    let bytes = serde_json::to_vec(value).unwrap();
    stream
        .write_all(&(bytes.len() as u32).to_be_bytes())
        .unwrap();
    stream.write_all(&bytes).unwrap();
}

#[test]
fn private_daemon_lifecycle_and_bounded_versioned_protocol() {
    let mut daemon = Daemon::new();
    daemon.start();
    assert_eq!(
        std::fs::metadata(daemon.socket().parent().unwrap())
            .unwrap()
            .mode()
            & 0o777,
        0o700
    );
    assert_eq!(
        std::fs::metadata(daemon.socket()).unwrap().mode() & 0o777,
        0o600
    );
    let status = daemon.cli(&["daemon", "status"]);
    assert_eq!(status["data"]["state"], "running");
    assert_eq!(status["data"]["pid"], daemon.child.as_ref().unwrap().id());
    assert_eq!(
        daemon.cli(&["doctor", "--json"])["data"]["transport"],
        "unix"
    );
    let mut stream = daemon.connect();
    let hello = receive(&mut stream);
    assert_eq!(hello["protocol_version"], 1);
    assert!(
        hello["capabilities"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v == "daemon")
    );
    let max = hello["max_message_bytes"].as_u64().unwrap();
    send(
        &mut stream,
        &json!({"protocol_version":999,"request_id":"bad-version","command":{"op":"status"}}),
    );
    assert_eq!(receive(&mut stream)["error"]["code"], "PROTOCOL_VERSION");
    let mut stream = daemon.connect();
    receive(&mut stream);
    stream.write_all(&((max + 1) as u32).to_be_bytes()).unwrap();
    assert_eq!(receive(&mut stream)["error"]["code"], "MESSAGE_TOO_LARGE");
    let mut held = Vec::new();
    for _ in 0..16 {
        let mut stream = daemon.connect();
        receive(&mut stream);
        held.push(stream);
    }
    let busy = daemon
        .command()
        .args(["daemon", "status"])
        .output()
        .unwrap();
    assert_eq!(
        busy.status.code(),
        Some(4),
        "{}",
        String::from_utf8_lossy(&busy.stderr)
    );
    assert!(String::from_utf8_lossy(&busy.stderr).contains("RESOURCE_BUSY"));
    drop(held);
    let second = daemon.command().args(["daemon", "run"]).output().unwrap();
    assert!(!second.status.success());
    assert!(String::from_utf8_lossy(&second.stderr).contains("DAEMON_ALREADY_RUNNING"));
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    assert!(!daemon.socket().exists());
    daemon.start();
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}

#[test]
fn configuration_reload_and_watcher_share_durable_generations() {
    let mut daemon = Daemon::new();
    daemon.start();
    let first = daemon.cli(&["config", "status", "--json"]);
    assert_eq!(first["data"]["active_generation"], 1);
    assert_eq!(
        daemon.cli(&["config", "show", "--effective"])["data"],
        first["data"]["effective"]
    );
    daemon.configure("[timeouts]\nrunner = '2000s'\n");
    let validated = daemon.cli(&["config", "validate"]);
    assert_eq!(
        validated["data"]["effective_timeouts"]["runner"]["secs"],
        2000
    );
    assert_eq!(
        daemon.cli(&["config", "reload"])["data"]["status"],
        "applied"
    );
    let changed = daemon.cli(&["config", "status", "--json"]);
    assert_eq!(changed["data"]["active_generation"], 2);
    assert_eq!(
        changed["data"]["effective"]["effective_timeouts"]["runner"]["secs"],
        2000
    );
    std::fs::write(daemon.root.join("config.toml"), "[broken").unwrap();
    assert_eq!(
        daemon.cli(&["config", "reload"])["data"]["status"],
        "rejected"
    );
    assert_eq!(
        daemon.cli(&["config", "reload"])["data"]["status"],
        "rejected"
    );
    assert_eq!(
        daemon.cli(&["config", "status", "--json"])["data"]["active_generation"],
        2
    );
    daemon.configure("[timeouts]\nrunner = '2000s'\n");
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(
        daemon.cli(&["config", "status", "--json"])["data"]["active_generation"],
        2
    );
    // Atomic rename takes the identical reload path, with no explicit reload request.
    let candidate = format!(
        "[paths]\ndata_dir={:?}\nsocket_path={:?}\n[timeouts]\nrunner='2100s'\n",
        daemon.root.join("data"),
        daemon.socket()
    );
    std::fs::write(daemon.root.join("next.toml"), candidate).unwrap();
    std::fs::rename(
        daemon.root.join("next.toml"),
        daemon.root.join("config.toml"),
    )
    .unwrap();
    let end = Instant::now() + Duration::from_secs(8);
    loop {
        let status = daemon.cli(&["config", "status", "--json"]);
        if status["data"]["active_generation"] == 3 {
            assert_eq!(
                status["data"]["effective"]["effective_timeouts"]["runner"]["secs"],
                2100
            );
            break;
        }
        assert!(Instant::now() < end, "watcher did not publish candidate");
        std::thread::sleep(Duration::from_millis(50));
    }
    let pending = format!(
        "[paths]\ndata_dir={:?}\nsocket_path={:?}\n[timeouts]\nrunner='2200s'\n",
        daemon.root.join("other-data"),
        daemon.socket()
    );
    std::fs::write(daemon.root.join("config.toml"), pending).unwrap();
    assert_eq!(
        daemon.cli(&["config", "reload"])["data"]["status"],
        "restart_required"
    );
    assert_eq!(
        daemon.cli(&["config", "reload"])["data"]["status"],
        "restart_required"
    );
    let status = daemon.cli(&["config", "status", "--json"]);
    assert_eq!(status["data"]["active_generation"], 3);
    assert_eq!(
        status["data"]["pending"]["desired"]["effective_timeouts"]["runner"]["secs"],
        2200
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}

#[test]
fn configuration_fifo_is_rejected_before_any_blocking_read() {
    let daemon = Daemon::new();
    std::fs::remove_file(daemon.root.join("config.toml")).unwrap();
    assert!(
        Command::new("mkfifo")
            .arg(daemon.root.join("config.toml"))
            .status()
            .unwrap()
            .success()
    );
    let mut child = daemon
        .command()
        .args(["daemon", "run"])
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let end = Instant::now() + Duration::from_secs(2);
    let status = loop {
        if let Some(status) = child.try_wait().unwrap() {
            break Some(status);
        }
        if Instant::now() >= end {
            break None;
        }
        std::thread::sleep(Duration::from_millis(10));
    };
    if status.is_none() {
        child.kill().unwrap();
        child.wait().unwrap();
    }
    assert!(status.is_some(), "configuration FIFO blocked the CLI");
    assert!(!status.unwrap().success());
}

#[test]
fn queue_expiry_and_running_cancellation_do_not_wait_for_a_free_slot() {
    let mut daemon = Daemon::new();
    let access = format!("[access]\nread_roots=[{:?}]\n", daemon.root);
    daemon.configure(&access);
    let scope = json!({"user":format!("uid:{}",rustix::process::geteuid().as_raw()),"project":null,"branch_or_version":null,"session":null});
    std::fs::write(daemon.root.join("binding.json"),serde_json::to_vec(&json!({"source":{"adapter":"fixture","profile":"test","session":"queue","thread":"thread","agent":"agent"},"scope":scope,"stream":"stream","coverage":"available_payloads"})).unwrap()).unwrap();
    let mut file = std::fs::File::create(daemon.root.join("capture.jsonl")).unwrap();
    for n in 0..4096 {
        writeln!(file,"{}",json!({"schema_version":1,"event":{"event_id":format!("e-{n}"),"source_version":"1","runtime_instance":"test","branch":"main","parent":null,"fork_cutoff":null,"event_time_ms":null,"observed_time_ms":1,"visibility":"user","kind":{"kind":"resume"},"text":"original"},"raw_hex":"78"})).unwrap();
    }
    drop(file);
    daemon.start();
    let binding = daemon.root.join("binding.json");
    let input = daemon.root.join("capture.jsonl");
    let mut args = [
        "capture",
        "reconcile",
        binding.to_str().unwrap(),
        input.to_str().unwrap(),
        "--idempotency-key",
        "running",
    ];
    daemon.cli(&args);
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let status = daemon.cli(&["job", "show", "running"]);
        if status["data"]["state"] == "running" {
            break;
        }
        assert!(
            Instant::now() < end,
            "execution slot was never obtained: {status}"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    let original = daemon.cli(&["job", "show", "running"])["data"].clone();
    daemon.configure(&format!("{access}[runtime]\nmax_queued_jobs=1\n"));
    daemon.cli(&["config", "reload"]);
    args[5] = "holds-queue";
    let queued = daemon.cli(&args)["data"].clone();
    assert_eq!(queued["state"], "queued");
    assert_eq!(daemon.cli(&args)["data"], queued);
    args[5] = "queue-full";
    let busy = daemon.command().args(args).output().unwrap();
    assert_eq!(busy.status.code(), Some(4));
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    let recovered = daemon.cli(&["job", "show", "running"])["data"].clone();
    assert_eq!(recovered["started_at_ms"], original["started_at_ms"]);
    assert_eq!(
        recovered["execution_deadline_ms"],
        original["execution_deadline_ms"]
    );
    assert_eq!(
        recovered["config_generation"],
        original["config_generation"]
    );
    assert_eq!(
        daemon.cli(&["job", "cancel", "holds-queue"])["data"]["state"],
        "cancelled"
    );
    daemon.configure(&format!("{access}[timeouts]\nqueue='1ms'\n"));
    daemon.cli(&["config", "reload"]);
    args[5] = "expires";
    daemon.cli(&args);
    let end = Instant::now() + Duration::from_secs(4);
    loop {
        let status = daemon.cli(&["job", "show", "expires"]);
        if status["data"]["state"] == "failed" {
            assert_eq!(status["data"]["error"], "QUEUE_TIMEOUT");
            break;
        }
        assert!(
            Instant::now() < end,
            "expired queue retained behind running work: {status}"
        );
        std::thread::sleep(Duration::from_millis(50));
    }
    let cancelled = daemon.cli(&["job", "cancel", "running"]);
    assert_eq!(cancelled["data"]["cancel_requested"], true);
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let status = daemon.cli(&["job", "show", "running"]);
        if status["data"]["state"] == "cancelled" {
            assert_eq!(status["data"]["error"], "CANCELLED");
            break;
        }
        assert!(
            Instant::now() < end,
            "running cancel was not collected: {status}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    daemon.configure(&format!("{access}[runtime]\npaused=true\n"));
    daemon.cli(&["config", "reload"]);
    args[5] = "paused";
    let busy = daemon.command().args(args).output().unwrap();
    assert_eq!(busy.status.code(), Some(4));
    daemon.configure(&format!("{access}[timeouts]\ntools='1ms'\n"));
    daemon.cli(&["config", "reload"]);
    args[5] = "short-budget";
    daemon.cli(&args);
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let status = daemon.cli(&["job", "show", "short-budget"]);
        if status["data"]["state"] == "failed" {
            assert_eq!(status["data"]["error"], "JOB_TIMEOUT");
            assert_eq!(status["data"]["execution_budget_ms"], 1);
            break;
        }
        assert!(
            Instant::now() < end,
            "job budget was not enforced: {status}"
        );
        std::thread::sleep(Duration::from_millis(20));
    }
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}

#[test]
fn accepted_reconciliation_survives_process_death_and_pins_configuration() {
    let mut daemon = Daemon::new();
    daemon.configure(&format!("[access]\nread_roots=[{:?}]\n", daemon.root));
    let scope = json!({"user":format!("uid:{}",rustix::process::geteuid().as_raw()),"project":"fixture","branch_or_version":null,"session":null});
    let source = json!({"adapter":"fixture","profile":"test","session":"session","thread":"thread","agent":"agent"});
    std::fs::write(daemon.root.join("binding.json"), serde_json::to_vec(&json!({"source":source,"scope":scope,"stream":"stream","coverage":"available_payloads"})).unwrap()).unwrap();
    let mut file = std::fs::File::create(daemon.root.join("capture.jsonl")).unwrap();
    for n in 0..256 {
        let frame = json!({"schema_version":1,"event":{"event_id":format!("event-{n}"),"source_version":"1","runtime_instance":"test","branch":"main","parent":null,"fork_cutoff":null,"event_time_ms":null,"observed_time_ms":1,"visibility":"user","kind":{"kind":"message","message_id":format!("message-{n}"),"phase":{"phase":"final"}},"text":"captured original"},"raw_hex":"726177"});
        let _: akasha_otsuge::capture::CaptureFrame =
            serde_json::from_value(frame.clone()).unwrap();
        writeln!(file, "{frame}").unwrap();
    }
    drop(file);
    daemon.start();
    let binding = daemon.root.join("binding.json");
    let input = daemon.root.join("capture.jsonl");
    let args = [
        "capture",
        "reconcile",
        binding.to_str().unwrap(),
        input.to_str().unwrap(),
        "--idempotency-key",
        "durable-capture",
    ];
    let accepted = daemon.cli(&args);
    assert_eq!(accepted["data"]["config_generation"], 1);
    assert!(matches!(
        accepted["data"]["state"].as_str(),
        Some("queued" | "running")
    ));
    let mut queued_args = args;
    queued_args[5] = "queued-cancel";
    daemon.cli(&queued_args);
    assert_eq!(
        daemon.cli(&["job", "cancel", "queued-cancel"])["data"]["state"],
        "cancelled"
    );
    daemon.child.as_mut().unwrap().kill().unwrap();
    daemon.child.as_mut().unwrap().wait().unwrap();
    daemon.child = None;
    daemon.configure(&format!(
        "[access]\nread_roots=[{:?}]\n[timeouts]\ntools='130s'\n",
        daemon.root
    ));
    daemon.start();
    let end = Instant::now() + Duration::from_secs(30);
    let completed = loop {
        let status = daemon.cli(&["job", "show", "durable-capture"]);
        if status["data"]["state"] == "succeeded" {
            break status;
        }
        assert!(
            !matches!(
                status["data"]["state"].as_str(),
                Some("failed" | "cancelled")
            ),
            "{status}"
        );
        assert!(Instant::now() < end, "recovery did not complete: {status}");
        std::thread::sleep(Duration::from_millis(50));
    };
    assert_eq!(completed["data"]["config_generation"], 1);
    assert_eq!(completed["data"]["execution_budget_ms"], 120000);
    assert!(completed["data"]["result"]["checkpoint"]["checkpoint"]["pending"].is_null());
    assert!(
        completed["data"]["result"]["checkpoint"]["checkpoint"]["loss"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert_eq!(daemon.cli(&args)["data"], completed["data"]);
    assert_eq!(
        daemon.cli(&["config", "status"])["data"]["active_generation"],
        2
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        use akasha_otsuge::{
            capture::*,
            config::{Paths, resolve},
            storage::Storage,
        };
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
        let store = Storage::open(&config).await.unwrap();
        let capture = Capture::new(&store);
        let source: SourceIdentity = serde_json::from_value(source).unwrap();
        let binding = CaptureBinding::from_source(
            source.clone(),
            serde_json::from_value(scope).unwrap(),
            "stream",
            CoverageLevel::AvailablePayloads,
        )
        .unwrap();
        let before = store.sequence().unwrap();
        for n in 0..256 {
            let event = capture
                .event(
                    &binding,
                    &EventRef {
                        source: source.clone(),
                        event_id: format!("event-{n}"),
                    },
                    before,
                )
                .unwrap()
                .unwrap();
            assert_eq!(capture.raw(&event).unwrap(), b"raw");
            assert_eq!(capture.text(&event).unwrap(), "captured original");
            assert!(
                store
                    .receipt(&event.event.publication_request_id)
                    .unwrap()
                    .is_some()
            );
        }
        let replay = capture
            .reconcile_jsonl(&binding, &input, 64, 1)
            .await
            .unwrap();
        assert_eq!(replay.processed, 0);
        assert_eq!(store.sequence().unwrap(), before);
        store.close().await.unwrap();
    });
}

#[test]
fn production_history_diff_and_receipt_reads_use_the_domain_and_live_grant() {
    use akasha_otsuge::config::{Paths, ReviewMode, resolve};
    use akasha_otsuge::domain::*;
    use akasha_otsuge::storage::Storage;
    let mut daemon = Daemon::new();
    let scope = Scope {
        user: format!("uid:{}", rustix::process::geteuid().as_raw()),
        project: Some("fixture".into()),
        branch_or_version: None,
        session: None,
    };
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
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    let (a, b, expected_history, expected_receipt, pages) = runtime.block_on(async {
        let store = Storage::open(&config).await.unwrap();
        let domain = Domain::new(&store);
        let reader =
            ReadContext::from_verified_binding(&scope.user, "local-management", "local-management")
                .unwrap();
        domain
            .set_read_grant("fixture-grant", &scope.user, Some(&scope), None, 1)
            .await
            .unwrap();
        let ingress = Ingress::tool("fixture-tool", "fixture-run", scope.clone()).unwrap();
        let source = SourceDraft {
            event_id: "source".into(),
            scope: scope.clone(),
            text: "Observed evidence".into(),
            reported_role: None,
            event_time_ms: None,
            observed_time_ms: 1,
        };
        let seq = domain
            .record_source("fixture-source", &source, &ingress, 1)
            .await
            .unwrap()
            .commit_seq;
        let span = SourceSpan {
            event_id: "source".into(),
            revision: seq,
            content_sha256: domain.source("source", seq).unwrap().content_sha256,
            start: 0,
            end: source.text.len(),
        };
        let policy = Policy {
            revision: "fixture-policy".into(),
            config_generation: 1,
            mode: ReviewMode::Automatic,
            scope: scope.clone(),
            adopt_explicit_decisions: false,
        };
        let mut write = MemoryWrite {
            request_id: "write-a".into(),
            memory_id: "choice".into(),
            expected_revision: None,
            draft: MemoryDraft {
                kind: MemoryKind::Fact,
                scope: scope.clone(),
                claim: "A".into(),
                rationale: "Measured".into(),
                conditions: vec![],
                assumptions: vec![],
                source_spans: vec![span],
                valid_from_ms: None,
                valid_until_ms: None,
            },
        };
        let a = domain
            .propose(&write, &ingress, &policy)
            .await
            .unwrap()
            .commit_seq;
        write.request_id = "write-other".into();
        write.memory_id = "other".into();
        write.draft.claim = "C".into();
        domain.propose(&write, &ingress, &policy).await.unwrap();
        write.request_id = "write-b".into();
        write.memory_id = "choice".into();
        write.expected_revision = Some(a);
        write.draft.claim = "B".into();
        let b = domain
            .propose(&write, &ingress, &policy)
            .await
            .unwrap()
            .commit_seq;
        domain
            .prepare_recall(
                &reader,
                &RecallDraft {
                    request_id: "fixture-receipt".into(),
                    output: "Actual archived output\n".into(),
                    scope: scope.clone(),
                    read_seq: b,
                    retrieval_mode: RetrievalMode::HistoricalState,
                    selected: vec![domain.active("choice").unwrap().unwrap().revision],
                    sources: vec![],
                    status: RecallStatus::Found,
                    coverage: Coverage {
                        complete: true,
                        gaps: vec![],
                    },
                    truncation: None,
                    config_generation: 1,
                    index_generation: None,
                },
            )
            .await
            .unwrap();
        let history = serde_json::to_value(
            domain
                .history(
                    &reader,
                    "choice",
                    1..=b,
                    None,
                    PageBudget {
                        limit: 32,
                        max_bytes: 1024 * 1024,
                    },
                )
                .await
                .unwrap()
                .data,
        )
        .unwrap();
        let receipt = serde_json::to_value(
            domain
                .recall_receipt(&reader, "fixture-receipt")
                .await
                .unwrap()
                .data,
        )
        .unwrap();
        let one = PageBudget { limit: 1, max_bytes: 1024 * 1024 };
        let full = PageBudget { limit: 32, max_bytes: 1024 * 1024 };
        let h = domain.history(&reader, "choice", 1..=b, None, one).await.unwrap().data.next.unwrap();
        let s = domain.state(&reader, &scope, b, None, one).await.unwrap().data.next.unwrap();
        let d = domain.diff(&reader, &scope, a, b, None, one).await.unwrap().data.next.unwrap();
        let pages = json!({
            "history": {"cursor": h, "tail": domain.history(&reader,"choice",1..=b,Some(&h),full).await.unwrap().data},
            "state": {"cursor": s, "tail": domain.state(&reader,&scope,b,Some(&s),full).await.unwrap().data},
            "diff": {"cursor": d, "tail": domain.diff(&reader,&scope,a,b,Some(&d),full).await.unwrap().data}
        });
        store.close().await.unwrap();
        (a, b, history, receipt, pages)
    });
    std::fs::write(
        daemon.root.join("scope.json"),
        serde_json::to_vec(&scope).unwrap(),
    )
    .unwrap();
    daemon.start();
    let memory = daemon.cli(&["memory", "show", "choice", "--at", &a.to_string()]);
    assert_eq!(
        memory["data"]["data"]["revision"]["memory"]["record"]["draft"]["claim"],
        "A"
    );
    assert_eq!(
        daemon.cli(&["memory", "history", "choice", "--to", &b.to_string()])["data"]["data"],
        expected_history
    );
    let scope_path = daemon.root.join("scope.json");
    let state = daemon.cli(&[
        "state",
        "show",
        "--scope",
        scope_path.to_str().unwrap(),
        "--at",
        &b.to_string(),
    ]);
    assert_eq!(state["data"]["data"]["items"].as_array().unwrap().len(), 2);
    assert!(
        state["data"]["data"]["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|v| v["memory"]["record"]["draft"]["claim"] == "B")
    );
    let diff = daemon.cli(&[
        "state",
        "diff",
        &a.to_string(),
        &b.to_string(),
        "--scope",
        scope_path.to_str().unwrap(),
    ]);
    let differences = diff["data"]["data"]["items"].as_array().unwrap();
    assert_eq!(differences.len(), 2);
    let changed = differences
        .iter()
        .find(|v| v["after"]["memory"]["record"]["draft"]["claim"] == "B")
        .unwrap();
    assert_eq!(changed["before"]["memory"]["record"]["draft"]["claim"], "A");
    assert_eq!(changed["after"]["memory"]["record"]["draft"]["claim"], "B");
    let cursor = daemon.root.join("cursor.json");
    for kind in ["history", "state", "diff"] {
        std::fs::write(&cursor, serde_json::to_vec(&pages[kind]["cursor"]).unwrap()).unwrap();
        let (a, b) = (a.to_string(), b.to_string());
        let args = match kind {
            "history" => vec![
                "memory",
                "history",
                "choice",
                "--from",
                "1",
                "--to",
                &b,
                "--cursor",
                cursor.to_str().unwrap(),
            ],
            "state" => vec![
                "state",
                "show",
                "--at",
                &b,
                "--scope",
                scope_path.to_str().unwrap(),
                "--cursor",
                cursor.to_str().unwrap(),
            ],
            _ => vec![
                "state",
                "diff",
                &a,
                &b,
                "--scope",
                scope_path.to_str().unwrap(),
                "--cursor",
                cursor.to_str().unwrap(),
            ],
        };
        assert_eq!(daemon.cli(&args)["data"]["data"], pages[kind]["tail"]);
    }
    assert_eq!(
        daemon.cli(&["receipt", "show", "fixture-receipt"])["data"]["data"],
        expected_receipt
    );
    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .unwrap();
    let failed = daemon
        .command()
        .args(["receipt", "replay", "fixture-receipt"])
        .stdout(full)
        .output()
        .unwrap();
    assert!(!failed.status.success());
    assert!(String::from_utf8_lossy(&failed.stderr).contains("IO_ERROR"));
    assert_eq!(
        daemon.cli(&["receipt", "show", "fixture-receipt"])["data"]["data"]["record"]["delivery"]["phase"],
        "prepared"
    );
    for _ in 0..2 {
        let replay = daemon
            .command()
            .args(["receipt", "replay", "fixture-receipt"])
            .output()
            .unwrap();
        assert!(
            replay.status.success(),
            "{}",
            String::from_utf8_lossy(&replay.stderr)
        );
        assert_eq!(replay.stdout, b"Actual archived output\n");
        assert!(replay.stderr.is_empty());
    }
    assert_eq!(
        daemon.cli(&["receipt", "show", "fixture-receipt"])["data"]["data"]["record"]["delivery"]["phase"],
        "delivered"
    );
    daemon.cli(&["access", "revoke"]);
    let refused = daemon
        .command()
        .args(["memory", "show", "choice", "--at", &a.to_string()])
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("READ_DENIED"));
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    let refused = daemon
        .command()
        .args(["receipt", "show", "fixture-receipt"])
        .output()
        .unwrap();
    assert!(!refused.status.success());
    assert!(String::from_utf8_lossy(&refused.stderr).contains("READ_DENIED"));
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
}
