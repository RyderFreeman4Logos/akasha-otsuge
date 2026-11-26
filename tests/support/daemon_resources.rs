use super::*;
use std::os::unix::fs::FileExt;

#[test]
fn native_startup_limits_preserve_pid_and_unrelated_environment() {
    for inherited in [None, Some(["8", "7", "64"]), Some(["2", "1", "32"])] {
        let mut daemon = Daemon::new();
        let mut command = daemon.command();
        let keys = [
            "MALLOC_ARENA_MAX",
            "LANCE_FTS_NUM_SHARDS",
            "LANCE_FTS_PARTITION_SIZE",
        ];
        for (index, key) in keys.iter().enumerate() {
            if let Some(values) = inherited {
                command.env(key, values[index]);
            } else {
                command.env_remove(key);
            }
        }
        command.env("GLIBC_TUNABLES", "glibc.malloc.trim_threshold=131072");
        daemon.start_command(command);
        let pid = daemon.child.as_ref().unwrap().id();
        assert_eq!(daemon.cli(&["daemon", "status"])["data"]["pid"], pid);
        let environment = std::fs::read(format!("/proc/{pid}/environ")).unwrap();
        for setting in [
            "MALLOC_ARENA_MAX=2",
            "LANCE_FTS_NUM_SHARDS=1",
            "LANCE_FTS_PARTITION_SIZE=32",
            "GLIBC_TUNABLES=glibc.malloc.trim_threshold=131072",
        ] {
            assert!(
                environment
                    .split(|byte| *byte == 0)
                    .any(|entry| entry == setting.as_bytes()),
                "missing native startup setting: {setting}"
            );
        }
        daemon.cli(&["daemon", "stop"]);
        daemon.wait();
    }
}

#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[test]
fn low_budget_startup_defaults_heap_trim_only_when_unset() {
    for (budget, inherited, expected) in [
        (320, None, Some("65536")),
        (320, Some("131072"), Some("131072")),
        (512, None, None),
    ] {
        let mut daemon = Daemon::new();
        daemon.configure(&format!("[runtime]\nmemory_budget_mib={budget}\n"));
        let mut command = daemon.command();
        // Exercise startup even when no other native setting requires re-exec.
        command
            .env("MALLOC_ARENA_MAX", "2")
            .env("LANCE_FTS_NUM_SHARDS", "1")
            .env("LANCE_FTS_PARTITION_SIZE", "32")
            .env_remove("MALLOC_TRIM_THRESHOLD_")
            .env_remove("GLIBC_TUNABLES");
        if let Some(value) = inherited {
            command.env("MALLOC_TRIM_THRESHOLD_", value);
        }
        daemon.start_command(command);
        let pid = daemon.child.as_ref().unwrap().id();
        let environment = std::fs::read(format!("/proc/{pid}/environ")).unwrap();
        let actual = environment
            .split(|byte| *byte == 0)
            .find_map(|entry| entry.strip_prefix(b"MALLOC_TRIM_THRESHOLD_="));
        assert_eq!(actual, expected.map(str::as_bytes));
        daemon.cli(&["daemon", "stop"]);
        daemon.wait();
    }
}

pub(super) fn delegated_command(daemon: &Daemon) -> Command {
    let mut command = daemon.command();
    command.env_remove("DBUS_SESSION_BUS_ADDRESS");
    for name in ["DBUS_SESSION_BUS_ADDRESS", "XDG_RUNTIME_DIR"] {
        if let Some(value) = std::env::var_os(name) {
            command.env(name, value);
        }
    }
    command
}

fn limit(path: &std::path::Path, name: &str) -> u64 {
    std::fs::read_to_string(path.join(name))
        .unwrap()
        .trim()
        .parse()
        .unwrap()
}

fn assert_groups(status: &Value, target: u64, peak: u64, total: u64) -> PathBuf {
    let resources = &status["resources"];
    assert_eq!(
        resources["enforcement"]["managed_cgroup_limits"], true,
        "{resources}"
    );
    let cgroup = &resources["cgroup"];
    assert_eq!(cgroup["complete"], true, "{cgroup}");
    let root = PathBuf::from(cgroup["path"].as_str().unwrap());
    assert_eq!(root.file_name().unwrap(), cgroup["unit"].as_str().unwrap());
    assert!(
        root.file_name()
            .unwrap()
            .to_str()
            .unwrap()
            .starts_with("akasha-otsuge-")
    );
    assert_eq!(
        root.metadata().unwrap().uid(),
        rustix::process::geteuid().as_raw()
    );
    for (name, path, high, maximum) in [
        ("daemon_group", root.join("daemon"), target, peak),
        (
            "runner_group",
            root.join("runner"),
            total - peak,
            total - peak,
        ),
        ("service_total", root.clone(), total, total),
    ] {
        let group = &cgroup[name];
        assert_eq!(group["path"], path.to_str().unwrap());
        assert_eq!(group["high_bytes"], high * 1024 * 1024);
        assert_eq!(group["max_bytes"], maximum * 1024 * 1024);
        assert_eq!(limit(&path, "memory.high"), high * 1024 * 1024);
        assert_eq!(limit(&path, "memory.max"), maximum * 1024 * 1024);
        assert_eq!(group["oom_kills"], 0);
        assert!(group["peak_bytes"].as_u64().unwrap() >= group["current_bytes"].as_u64().unwrap());
    }
    let pid = status["pid"].as_u64().unwrap();
    assert_eq!(
        std::fs::read_to_string(root.join("daemon/cgroup.procs"))
            .unwrap()
            .trim(),
        pid.to_string()
    );
    assert!(
        std::fs::read_to_string(root.join("runner/cgroup.procs"))
            .unwrap()
            .trim()
            .is_empty()
    );
    assert!(cgroup["daemon_group"]["peak_bytes"].as_u64().unwrap() > 0);
    root
}

#[test]
#[ignore = "requires systemd user memory delegation"]
fn owned_cgroup_limits_survive_reopen_and_collect_after_normal_stop() {
    let mut daemon = Daemon::new();
    let command = delegated_command(&daemon);
    daemon.start_command(command);
    let first = daemon.cli(&["daemon", "status"])["data"].clone();
    let root = assert_groups(&first, 512, 640, 2048);
    let events = std::fs::File::open(root.join("cgroup.events")).unwrap();
    daemon.configure("[runtime]\nmemory_budget_mib=1024\n");
    daemon.cli(&["config", "reload"]);
    let raised = wait_for_storage_budget(&daemon, 1024);
    assert_eq!(assert_groups(&raised, 1024, 1280, 2048), root);
    daemon.configure("[runtime]\nmemory_budget_mib=512\n");
    daemon.cli(&["config", "reload"]);
    let lowered = wait_for_storage_budget(&daemon, 512);
    assert_eq!(assert_groups(&lowered, 512, 640, 2048), root);
    assert_eq!(lowered["pid"], first["pid"]);
    for name in ["daemon_group", "runner_group", "service_total"] {
        assert!(
            lowered["resources"]["cgroup"][name]["peak_bytes"]
                .as_u64()
                .unwrap()
                >= first["resources"]["cgroup"][name]["peak_bytes"]
                    .as_u64()
                    .unwrap()
        );
    }
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        let mut bytes = [0; 4096];
        match events.read_at(&mut bytes, 0) {
            Ok(n) if String::from_utf8_lossy(&bytes[..n]).contains("populated 0") => break,
            Err(error) if error.raw_os_error() == Some(19) => break,
            Err(error) => panic!("owned scope read failed: {error}"),
            Ok(_) => assert!(Instant::now() < end, "owned scope still populated"),
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    while root.exists() {
        assert!(Instant::now() < end, "empty owned scope was not collected");
        std::thread::sleep(Duration::from_millis(10));
    }
}
