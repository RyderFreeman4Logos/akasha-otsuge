use std::{
    os::unix::fs::PermissionsExt,
    path::PathBuf,
    process::Command,
    sync::atomic::{AtomicU64, Ordering},
};

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "akasha-hermes-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&path).unwrap();
        Self(path)
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}

#[test]
fn doctor_observes_hermes_without_model_calls_or_a_daemon() {
    let fixture = Fixture::new();
    let binary = fixture.0.join("hermes");
    std::fs::write(
        &binary,
        "#!/bin/sh\n[ \"$#\" = 1 ] && [ \"$1\" = --version ] || exit 78\nprintf 'Hermes Agent v0.22.5+1914.gfdb04a0 (2026.9.24) · stable · local fdb04a00c162\\n'\n",
    )
    .unwrap();
    std::fs::set_permissions(&binary, std::fs::Permissions::from_mode(0o700)).unwrap();
    let output = Command::new(env!("CARGO_BIN_EXE_akasha-otsuge"))
        .args(["doctor", "--host", "hermes", "--binary"])
        .arg(&binary)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
    let report: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
    assert_eq!(report["data"]["host"], "hermes");
    assert_eq!(report["data"]["version"], "0.22.5+1914.gfdb04a0");
    assert_eq!(report["data"]["self_test"]["status"], "not_run");
    assert_eq!(report["coverage"]["model_auth"], "not_checked");
    assert_eq!(report["coverage"]["version_supported"], true);
    assert!(output.stderr.is_empty());
}
