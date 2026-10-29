use std::{
    path::{Path, PathBuf},
    process::{Command, Output},
    sync::atomic::{AtomicU64, Ordering},
};

#[path = "support/pi_capture.rs"]
mod capture;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "akasha-pi-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
    async fn open(&self) -> akasha_otsuge::storage::Storage {
        use akasha_otsuge::config::{Paths, resolve};
        akasha_otsuge::storage::Storage::open(
            &resolve(
                "",
                None,
                "",
                &Paths {
                    data_dir: self.0.join("data"),
                    socket_path: self.0.join("service.sock"),
                },
            )
            .unwrap(),
        )
        .await
        .unwrap()
    }
    fn executable(&self, name: &str, script: &str) -> PathBuf {
        let path = self.0.join(name);
        // Close the writer process before another thread can spawn this file.
        assert!(Command::new("python3")
            .args(["-c", "import pathlib,sys; p=pathlib.Path(sys.argv[1]); p.write_text(sys.argv[2]); p.chmod(0o700)"])
            .arg(&path)
            .arg(script)
            .status()
            .unwrap()
            .success());
        path
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

fn doctor(binary: &Path) -> Output {
    Command::new(env!("CARGO_BIN_EXE_akasha-otsuge"))
        .args(["doctor", "--host", "pi", "--binary"])
        .arg(binary)
        .output()
        .unwrap()
}

#[test]
fn doctor_observes_pi_version_without_model_or_daemon_claims() {
    let fixture = Fixture::new();
    for (version, supported) in [("1.4.0", true), ("0.78.0", false), ("1.4.1", false)] {
        let binary = fixture.executable(
            version,
            &format!("#!/bin/sh\n[ \"$#\" = 1 ] && [ \"$1\" = --version ] || exit 78\nprintf '{version}\\n'\n"),
        );
        let output = doctor(&binary);
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        assert_eq!(report["data"]["host"], "pi");
        assert_eq!(report["data"]["version"], version);
        assert_eq!(report["data"]["executable"], binary.to_str().unwrap());
        assert_eq!(report["data"]["self_test"]["status"], "not_run");
        assert_eq!(report["data"]["binding"], "unavailable");
        assert!(report["data"]["machine_protocol"].is_null());
        assert_eq!(report["coverage"]["version_supported"], supported);
        assert_eq!(report["coverage"]["model_auth"], "not_checked");
        assert_eq!(report["coverage"]["hook_trust"], "not_checked");
        assert!(output.stderr.is_empty());
    }
}

#[test]
fn doctor_rejects_malformed_failed_or_unbounded_pi_probes() {
    let fixture = Fixture::new();
    for (name, script, code) in [
        (
            "malformed",
            "printf 'pi version unknown\\n'",
            "ADAPTER_UNSUPPORTED",
        ),
        (
            "two-lines",
            "printf '1.4.0\\n1.4.0\\n'",
            "ADAPTER_UNSUPPORTED",
        ),
        ("failed", "printf '1.4.0\\n'; exit 7", "RUNNER_FAILED"),
        ("oversized", "head -c 4097 /dev/zero", "OUTPUT_LIMIT"),
    ] {
        let binary = fixture.executable(name, &format!("#!/bin/sh\n{script}\n"));
        let output = doctor(&binary);
        assert!(!output.status.success());
        assert!(
            String::from_utf8_lossy(&output.stderr).contains(code),
            "{}: {}",
            name,
            String::from_utf8_lossy(&output.stderr)
        );
    }
    let output = doctor(&fixture.0.join("absent"));
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("EXECUTOR_UNAVAILABLE"));
}
