use super::*;
use serde_json::Value;

#[test]
fn plugin_install_is_opt_in_idempotent_and_undo_preserves_edits() {
    let fixture = Fixture::new();
    let home = fixture.0.join("home");
    std::fs::create_dir(&home).unwrap();
    let config = home.join("config.yaml");
    std::fs::write(
        &config,
        "# preserve my settings\nplugins:\n  isolation: host\n",
    )
    .unwrap();
    let binary = fixture.0.join("hermes");
    crate::executable::write(
        &binary,
        "#!/bin/sh\n[ \"$1\" = --version ] || exit 78\nprintf 'Hermes Agent v0.22.5+1914.gfdb04a0\\n'\n",
    );
    let install = || {
        Command::new(env!("CARGO_BIN_EXE_akasha-otsuge"))
            .arg("--socket")
            .arg(fixture.0.join("daemon.sock"))
            .args(["adapter", "install", "hermes", "--home"])
            .arg(&home)
            .arg("--binary")
            .arg(&binary)
            .output()
            .unwrap()
    };
    let undo = || {
        Command::new(env!("CARGO_BIN_EXE_akasha-otsuge"))
            .args(["adapter", "uninstall", "hermes", "--home"])
            .arg(&home)
            .output()
            .unwrap()
    };
    let installed = install();
    assert!(
        installed.status.success(),
        "{}",
        String::from_utf8_lossy(&installed.stderr)
    );
    let report: Value = serde_json::from_slice(&installed.stdout).unwrap();
    assert_eq!(report["data"]["settings_modified"], false);
    assert_eq!(report["data"]["state"], "installed");
    let plugin = home.join("plugins/akasha-otsuge");
    let source = plugin.join("__init__.py");
    let bytes = std::fs::read(&source).unwrap();
    assert!(
        String::from_utf8_lossy(&bytes).contains(fixture.0.join("daemon.sock").to_str().unwrap())
    );
    assert!(plugin.join("plugin.yaml").is_file());
    assert!(install().status.success());
    std::fs::write(&source, b"# user edit\n").unwrap();
    for refused in [install(), undo()] {
        assert!(!refused.status.success());
        let error: Value = serde_json::from_slice(&refused.stderr).unwrap();
        assert_eq!(error["error"]["code"], "ADAPTER_INSTALL_CONFLICT");
    }
    assert_eq!(std::fs::read(&source).unwrap(), b"# user edit\n");
    std::fs::write(&source, &bytes).unwrap();
    std::fs::write(plugin.join("user-note"), b"keep").unwrap();
    assert!(undo().status.success());
    assert!(!source.exists());
    assert!(!plugin.join("plugin.yaml").exists());
    assert_eq!(std::fs::read(plugin.join("user-note")).unwrap(), b"keep");
    assert_eq!(
        std::fs::read_to_string(config).unwrap(),
        "# preserve my settings\nplugins:\n  isolation: host\n"
    );
}
