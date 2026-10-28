use serde_json::Value;
use std::{
    fs,
    os::unix::fs::{PermissionsExt, symlink},
    path::PathBuf,
    process::Command,
};

const BIN: &str = env!("CARGO_BIN_EXE_akasha-otsuge");

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

#[test]
fn claude_plugin_install_is_reversible_and_preserves_unrelated_configuration() {
    let root =
        Fixture(std::env::temp_dir().join(format!("akasha-claude-install-{}", std::process::id())));
    fs::create_dir(&root.0).unwrap();
    let native = root.0.join("claude");
    fs::write(&native, "#!/bin/sh\nprintf '2.1.311 (Claude Code)\\n'\n").unwrap();
    fs::set_permissions(&native, fs::Permissions::from_mode(0o700)).unwrap();
    let config = root.0.join("user-settings.json");
    let original = br#"{"hooks":{"SessionStart":[{"hooks":[{"type":"command","command":"keep-me"}]}]},"custom":"untouched"}"#;
    fs::write(&config, original).unwrap();
    let directory = root.0.join("plugin with ' quotes");
    let app_config = root.0.join("akasha config.toml");
    let run = |command: &[&str]| {
        Command::new(BIN)
            .arg("--config")
            .arg(&app_config)
            .env("HOME", &root.0)
            .env("CLAUDE_CONFIG_DIR", &root.0)
            .args(command)
            .output()
            .unwrap()
    };
    let install = [
        "adapter",
        "install",
        "claude",
        "--directory",
        directory.to_str().unwrap(),
        "--binary",
        native.to_str().unwrap(),
    ];
    let first = run(&install);
    assert!(
        first.status.success(),
        "install failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    let receipt: Value = serde_json::from_slice(&first.stdout).unwrap();
    assert_eq!(receipt["data"]["plugin_dir"], directory.to_str().unwrap());
    let doctor = [
        "doctor",
        "--host",
        "claude",
        "--binary",
        native.to_str().unwrap(),
        "--directory",
        directory.to_str().unwrap(),
    ];
    let status = run(&doctor);
    assert!(
        status.status.success(),
        "doctor failed: {}",
        String::from_utf8_lossy(&status.stderr)
    );
    let status: Value = serde_json::from_slice(&status.stdout).unwrap();
    assert_eq!(status["data"]["version"], "2.1.311");
    assert_eq!(status["data"]["self_test"]["status"], "not_run");
    assert_eq!(status["installation"]["state"], "verified");
    assert_eq!(status["coverage"]["version_supported"], true);
    assert_eq!(status["coverage"]["fork_parent"], "unavailable");
    assert_eq!(status["coverage"]["model_auth"], "not_checked");
    assert_eq!(fs::read(&config).unwrap(), original);
    assert_eq!(
        fs::metadata(&directory).unwrap().permissions().mode() & 0o777,
        0o700
    );
    let js = fs::read_to_string(directory.join("hooks/config.js")).unwrap();
    assert!(js.contains(&serde_json::to_string(app_config.to_str().unwrap()).unwrap()));
    assert_eq!(
        fs::read_to_string(directory.join("hooks/caller.js")).unwrap(),
        include_str!("../src/adapter/claude/caller.js")
    );
    assert!(
        run(&install).status.success(),
        "identical reinstall must be idempotent"
    );
    let remove = [
        "adapter",
        "uninstall",
        "claude",
        "--directory",
        directory.to_str().unwrap(),
    ];
    fs::write(directory.join("user-file"), "preserve").unwrap();
    assert!(!run(&remove).status.success());
    assert_eq!(
        fs::read_to_string(directory.join("user-file")).unwrap(),
        "preserve"
    );
    fs::remove_file(directory.join("user-file")).unwrap();
    let caller = directory.join("hooks/caller.js");
    fs::write(&caller, "user edit").unwrap();
    assert!(!run(&doctor).status.success());
    assert!(!run(&install).status.success());
    assert!(!run(&remove).status.success());
    assert_eq!(fs::read_to_string(&caller).unwrap(), "user edit");
    fs::write(&caller, include_str!("../src/adapter/claude/caller.js")).unwrap();
    assert!(run(&remove).status.success());
    assert!(!directory.exists());
    assert_eq!(fs::read(&config).unwrap(), original);
    assert!(run(&install).status.success());
    let types = directory.join(".claude-plugin/types");
    fs::create_dir_all(types.join("claude-code")).unwrap();
    let declaration = types.join("claude-code/index.d.ts");
    fs::write(&declaration, "native generated types with user additions").unwrap();
    let tsconfig = directory.join("tsconfig.json");
    fs::write(&tsconfig, "user customized tsconfig").unwrap();
    fs::remove_file(&tsconfig).unwrap();
    symlink(&config, &tsconfig).unwrap();
    assert!(!run(&doctor).status.success());
    assert!(!run(&remove).status.success());
    assert!(caller.exists());
    assert_eq!(fs::read(&config).unwrap(), original);
    fs::remove_file(&tsconfig).unwrap();
    fs::write(&tsconfig, "user customized tsconfig").unwrap();
    assert!(
        run(&doctor).status.success(),
        "native type generation must not invalidate the plugin"
    );
    assert!(run(&install).status.success());
    let removed = run(&remove);
    assert!(removed.status.success());
    let removed: Value = serde_json::from_slice(&removed.stdout).unwrap();
    assert_eq!(
        removed["data"]["preserved_paths"],
        serde_json::json!([".claude-plugin/types", "tsconfig.json"])
    );
    assert_eq!(
        fs::read_to_string(&declaration).unwrap(),
        "native generated types with user additions"
    );
    assert_eq!(
        fs::read_to_string(&tsconfig).unwrap(),
        "user customized tsconfig"
    );
    assert!(!directory.join("hooks").exists());
    assert!(!directory.join(".claude-plugin/plugin.json").exists());
    assert!(!directory.join("akasha-install.json").exists());
    fs::remove_dir_all(&directory).unwrap();
    assert!(run(&install).status.success());
    fs::remove_file(&caller).unwrap();
    symlink(&config, &caller).unwrap();
    assert!(!run(&remove).status.success());
    assert_eq!(fs::read(&config).unwrap(), original);
}
