use super::*;
use serde_json::Value;

#[test]
fn extension_install_is_opt_in_integrity_checked_and_uninstall_recovers() {
    let fixture = Fixture::new();
    let home = fixture.0.join("agent");
    std::fs::create_dir(&home).unwrap();
    let settings = home.join("settings.json");
    std::fs::write(&settings, b"{\"theme\":\"mine\"}\n").unwrap();
    let binary = fixture.executable("pi", "#!/bin/sh\nprintf '1.4.0\\n'\n");
    let install = || {
        Command::new(env!("CARGO_BIN_EXE_akasha-otsuge"))
            .arg("--socket")
            .arg(fixture.0.join("daemon.sock"))
            .args(["adapter", "install", "pi", "--agent-dir"])
            .arg(&home)
            .arg("--binary")
            .arg(&binary)
            .output()
            .unwrap()
    };
    let uninstall = || {
        Command::new(env!("CARGO_BIN_EXE_akasha-otsuge"))
            .args(["adapter", "uninstall", "pi", "--agent-dir"])
            .arg(&home)
            .output()
            .unwrap()
    };
    let first = install();
    assert!(
        first.status.success(),
        "{}",
        String::from_utf8_lossy(&first.stderr)
    );
    let result: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(result["data"]["settings_modified"], false);
    let extension = home.join("extensions/akasha-otsuge");
    let source = extension.join("index.ts");
    let bridge = extension.join("bridge.cjs");
    let receipt = extension.join("install.json");
    let original = std::fs::read(&source).unwrap();
    assert!(String::from_utf8_lossy(&original).contains(env!("CARGO_BIN_EXE_akasha-otsuge")));
    assert!(install().status.success());
    std::fs::write(&bridge, b"// my edit\n").unwrap();
    for refused in [install(), uninstall()] {
        assert!(!refused.status.success());
        assert!(String::from_utf8_lossy(&refused.stderr).contains("ADAPTER_INSTALL_CONFLICT"));
    }
    assert_eq!(std::fs::read(&bridge).unwrap(), b"// my edit\n");
    std::fs::write(&bridge, include_bytes!("../../plugins/pi/bridge.cjs")).unwrap();
    std::fs::write(extension.join("user-note"), b"keep").unwrap();
    // Simulate a crash after durable removal intent and the first owned-file unlink.
    let mut state: Value = serde_json::from_slice(&std::fs::read(&receipt).unwrap()).unwrap();
    state["phase"] = serde_json::json!("removing");
    std::fs::write(&receipt, serde_json::to_vec(&state).unwrap()).unwrap();
    std::fs::remove_file(&source).unwrap();
    assert!(!install().status.success());
    let removed = uninstall();
    assert!(
        removed.status.success(),
        "{}",
        String::from_utf8_lossy(&removed.stderr)
    );
    assert!(!source.exists() && !bridge.exists() && !receipt.exists());
    assert_eq!(std::fs::read(extension.join("user-note")).unwrap(), b"keep");
    assert!(uninstall().status.success());
    assert_eq!(std::fs::read(settings).unwrap(), b"{\"theme\":\"mine\"}\n");
}
