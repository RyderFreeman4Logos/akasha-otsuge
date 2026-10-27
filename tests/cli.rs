use std::process::Command;

const BIN: &str = env!("CARGO_BIN_EXE_akasha-otsuge");

#[test]
fn help_and_version_are_successful_and_keep_stderr_empty() {
    for args in [vec![], vec!["-h"], vec!["--help"], vec!["help"]] {
        let output = Command::new(BIN).args(args).output().unwrap();
        assert!(output.status.success());
        assert!(String::from_utf8(output.stdout).unwrap().contains("Usage:"));
        assert!(output.stderr.is_empty());
    }
    for flag in ["-V", "--version", "version"] {
        let output = Command::new(BIN).arg(flag).output().unwrap();
        assert!(output.status.success());
        assert_eq!(
            output.stdout,
            concat!("akasha-otsuge ", env!("CARGO_PKG_VERSION"), "\n").as_bytes()
        );
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn invalid_commands_return_stable_errors_without_echoing_input() {
    for args in [
        vec!["secret-value"],
        vec!["--help", "secret-value"],
        vec!["--version", "extra"],
        vec!["consult"],
    ] {
        let output = Command::new(BIN).args(args).output().unwrap();
        assert_eq!(output.status.code(), Some(2));
        assert!(output.stdout.is_empty());
        assert_eq!(
            output.stderr,
            b"{\"schema_version\":1,\"error\":{\"code\":\"INVALID_ARGUMENT\",\"message\":\"Use --help for supported arguments\"}}\n"
        );
    }
}

#[cfg(unix)]
#[test]
fn non_utf8_arguments_are_rejected_without_a_panic() {
    use std::os::unix::ffi::OsStringExt;
    let output = Command::new(BIN)
        .arg(std::ffi::OsString::from_vec(vec![0xff]))
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("INVALID_ARGUMENT")
    );
}

#[cfg(target_os = "linux")]
#[test]
fn stdout_write_failure_is_not_reported_as_success() {
    let full = std::fs::OpenOptions::new()
        .write(true)
        .open("/dev/full")
        .unwrap();
    let output = Command::new(BIN)
        .arg("--help")
        .stdout(full)
        .output()
        .unwrap();
    assert_eq!(output.status.code(), Some(1));
    assert!(
        String::from_utf8(output.stderr)
            .unwrap()
            .contains("IO_ERROR")
    );
}

#[cfg(target_os = "linux")]
#[test]
fn daemon_unavailable_is_explicit_and_bounded() {
    let socket = std::env::temp_dir().join(format!("akasha-absent-{}.sock", std::process::id()));
    let start = std::time::Instant::now();
    let output = Command::new(BIN)
        .arg("--socket")
        .arg(socket)
        .args(["daemon", "status"])
        .output()
        .unwrap();
    assert!(start.elapsed() < std::time::Duration::from_secs(3));
    assert_eq!(output.status.code(), Some(3));
    assert!(output.stdout.is_empty());
    let error: serde_json::Value = serde_json::from_slice(&output.stderr).unwrap();
    assert_eq!(error["error"]["code"], "DAEMON_UNAVAILABLE");
}
