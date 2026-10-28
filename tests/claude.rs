#![cfg(unix)]

use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

const BIN: &str = env!("CARGO_BIN_EXE_akasha-otsuge");

struct HostFixture(PathBuf);
impl HostFixture {
    fn new(script: &str) -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let root = std::env::temp_dir().join(format!(
            "akasha-claude-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        fs::create_dir(root.join("bin")).unwrap();
        let executable = root.join("native-claude");
        fs::write(&executable, script).unwrap();
        fs::set_permissions(&executable, fs::Permissions::from_mode(0o700)).unwrap();
        symlink(&executable, root.join("bin/claude")).unwrap();
        Self(root)
    }
    fn command(&self) -> Command {
        let mut command = Command::new(BIN);
        command
            .env_clear()
            .env("HOME", &self.0)
            .env("PATH", self.0.join("bin"))
            .env("WITNESS", self.0.join("argv"))
            .args(["doctor", "--host", "claude"]);
        command
    }
}
impl Drop for HostFixture {
    fn drop(&mut self) {
        fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn doctor_probes_actual_claude_without_daemon_or_other_hosts() {
    let host = HostFixture::new(
        "#!/bin/sh\n[ \"$#\" = 1 ] && [ \"$1\" = --version ] || exit 22\nprintf '%s\\n' \"$1\" > \"$WITNESS\"\nprintf '2.1.311 (Claude Code)\\n'\n",
    );
    let output = host.command().output().unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stderr.is_empty());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["schema_version"], 1);
    assert_eq!(report["data"]["host"], "claude_code");
    assert_eq!(report["data"]["version"], "2.1.311");
    assert_eq!(
        report["data"]["executable"],
        fs::canonicalize(host.0.join("native-claude"))
            .unwrap()
            .to_str()
            .unwrap()
    );
    assert_eq!(report["data"]["self_test"]["status"], "not_run");
    assert_eq!(report["data"]["binding"], "unavailable");
    assert_eq!(report["data"]["history"], "unavailable");
    assert_eq!(fs::read(host.0.join("argv")).unwrap(), b"--version\n");
}

#[test]
fn doctor_uses_literal_binary_override_and_preserves_cli_validation() {
    let host = HostFixture::new("#!/bin/sh\nprintf '2.1.311 (Claude Code)\\n'\n");
    let literal = host.0.join("Claude ' $(touch unwanted)");
    fs::rename(host.0.join("native-claude"), &literal).unwrap();
    let output = host
        .command()
        .arg("--binary")
        .arg(&literal)
        .output()
        .unwrap();
    assert!(output.status.success());
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(
        report["data"]["executable"],
        literal.canonicalize().unwrap().to_str().unwrap()
    );
    assert!(!host.0.join("unwanted").exists());
    for args in [
        vec!["--binary"],
        vec!["--secret", "private"],
        vec!["--binary", "a", "--binary", "b"],
    ] {
        let output = host.command().args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(error["error"]["code"], "INVALID_ARGUMENT");
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private"));
    }
}

#[test]
fn doctor_refuses_missing_failed_invalid_or_oversized_hosts_without_echoing_output() {
    for (script, error) in [
        ("#!/bin/sh\nprintf 'private' >&2\nexit 7\n", "RUNNER_FAILED"),
        ("#!/bin/sh\nprintf 'private'\n", "ADAPTER_UNSUPPORTED"),
        (
            "#!/bin/sh\ni=0; while [ \"$i\" -lt 1024 ]; do printf 'private'; i=$((i+1)); done\n",
            "OUTPUT_LIMIT",
        ),
        (
            "#!/bin/sh\ni=0; while [ \"$i\" -lt 1024 ]; do printf 'private' >&2; i=$((i+1)); done\n",
            "OUTPUT_LIMIT",
        ),
    ] {
        let host = HostFixture::new(script);
        let output = host.command().output().unwrap();
        assert_eq!(output.status.code(), Some(1));
        assert!(output.stdout.is_empty());
        let value: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
        assert_eq!(value["error"]["code"], error);
        assert!(!String::from_utf8_lossy(&output.stderr).contains("private"));
    }
    let host = HostFixture::new("#!/bin/sh\nexit 0\n");
    fs::remove_file(host.0.join("native-claude")).unwrap();
    let output = host.command().output().unwrap();
    let value: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["error"]["code"], "EXECUTOR_UNAVAILABLE");
}

#[cfg(target_os = "linux")]
#[test]
fn doctor_deadline_terminates_the_owned_process_group() {
    let host =
        HostFixture::new("#!/bin/sh\n/bin/sleep 60 &\nprintf '%s' \"$!\" > \"$WITNESS\"\nwait\n");
    let start = std::time::Instant::now();
    let output = host.command().output().unwrap();
    assert!(start.elapsed() < std::time::Duration::from_secs(12));
    let value: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(value["error"]["code"], "RUNNER_TIMEOUT");
    let pid = fs::read_to_string(host.0.join("argv"))
        .unwrap()
        .parse::<u32>()
        .unwrap();
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        let stat = fs::read_to_string(format!("/proc/{pid}/stat"));
        if stat
            .as_ref()
            .is_err_and(|e| e.kind() == std::io::ErrorKind::NotFound)
            || stat
                .as_ref()
                .is_ok_and(|s| s.rsplit_once(") ").unwrap().1.starts_with('Z'))
        {
            break;
        }
        assert!(
            std::time::Instant::now() < deadline,
            "owned child remains live"
        );
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
}
