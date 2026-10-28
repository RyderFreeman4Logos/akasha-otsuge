use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Barrier};
use std::time::Duration;

use akasha_otsuge::config::{
    ConfigManager, MAX_CONFIG_BYTES, Paths, ReloadStatus, ResolvedConfig,
    deadline::{Deadline, Layer},
    read_file, resolve,
};

fn paths() -> Paths {
    Paths {
        data_dir: "/tmp/akasha-config-test/data".into(),
        socket_path: "/tmp/akasha-config-test/service.sock".into(),
    }
}

fn config(input: &str) -> ResolvedConfig {
    resolve(input, None, "", &paths()).unwrap()
}

fn manager(input: &str) -> ConfigManager<()> {
    ConfigManager::new(1, config(input), |_| Ok(())).unwrap()
}

const MODELS: &str = r#"
[models.text]
kind = "text"
endpoint = "http://remote-model.example/v1"
model = "chat"
local = true
[models.embed]
kind = "embedding"
endpoint = "http://remote-model.example/v1"
model = "embed"
dimension = 128
local = true
[models.rank]
kind = "reranker"
endpoint = "http://remote-model.example/v1"
model = "rank"
local = true
"#;

#[test]
fn defaults_cover_all_local_model_roles_without_an_endpoint_requirement() {
    let empty = config("");
    assert!(empty.config().models.is_empty());
    assert!(empty.config().executors.is_empty());
    assert!(empty.config().retrieval.embedding.is_none());
    assert_eq!(empty.config().runtime.memory_budget_mib, 512);
    assert_eq!(empty.config().runtime.max_concurrent_jobs, 1);
    let resolved = config(MODELS);
    for role in ["text", "embed", "rank"] {
        assert_eq!(
            resolved.timeouts().providers[role],
            Duration::from_secs(1700)
        );
    }
    assert_eq!(resolved.timeouts().runner, Duration::from_secs(1700));
    assert_eq!(resolved.timeouts().workflow, Duration::from_secs(5400));
    assert_eq!(resolved.timeouts().queue, Duration::from_secs(300));
    assert!(resolved.timeouts().idle.is_none());
    assert_eq!(resolved.diagnostic()["local_long_timeout_verified"], false);
    let example = resolve(
        include_str!("../examples/config.toml"),
        Some("patient"),
        "",
        &paths(),
    )
    .unwrap();
    assert_eq!(example.timeouts().runner, Duration::from_secs(2000));
    assert_eq!(example.sources()["executors.codex.enabled"], "builtin");
    assert_eq!(example.diagnostic()["selected_profile"], "patient");
}

#[test]
fn profile_and_management_precedence_have_auditable_field_sources() {
    let input = r#"
[timeouts]
runner = "1800s"
[profiles.fast.timeouts]
runner = "1900s"
"#;
    let c = resolve(input, Some("fast"), "[timeouts]\nrunner='2000s'", &paths()).unwrap();
    assert_eq!(c.timeouts().runner, Duration::from_secs(2000));
    assert_eq!(c.sources()["timeouts.runner"], "override");
    assert_eq!(c.sources()["timeouts.local_request"], "builtin");
    let c = resolve(input, Some("fast"), "", &paths()).unwrap();
    assert_eq!(c.timeouts().runner, Duration::from_secs(1900));
    assert_eq!(c.sources()["timeouts.runner"], "profile");
    assert_eq!(
        resolve(input, None, "", &paths()).unwrap().sources()["timeouts.runner"],
        "user"
    );
}

#[test]
fn invalid_configs_fail_closed_and_errors_never_echo_values() {
    for input in [
        "[timeouts",
        "unknown_secret_value = 'DO_NOT_LEAK'",
        "schema_version=2",
        "[timeouts]\nrunner='DO_NOT_LEAK'",
        "[timeouts]\nrunner=1700",
        "[timeouts]\nrunner='0s'",
        "[timeouts]\nrunner='-1s'",
        "[timeouts]\nrunner='NaN'",
        "[timeouts]\nrunner='disabled'",
        "[timeouts]\nrunner='18446744073709551615h'",
        "[timeouts]\nworkflow='45s'",
        "[timeouts]\nworkflow='1700s'",
        "[runtime]\nmax_concurrent_jobs=0",
        "[runtime]\nmemory_budget_mib=2049",
        "[retrieval]\ntop_k=101",
        "[retrieval]\ntop_k=33",
        "[retrieval]\nmax_candidates=101",
        "[models.m]\nkind='embedding'\nendpoint='http://localhost:9999/v1'\nmodel='m'\ndimension=65537\nlocal=true",
        "[retrieval]\nembedding='absent'",
        "[consult]\nexecutor='absent'",
        "[access]\nread_roots=['relative']",
        "[paths]\ndata_dir='relative'",
        "[profiles.unused.timeouts]\nDO_NOT_LEAK=1",
        "[models.m]\nkind='text'\nendpoint='http://DO_NOT_LEAK:secret@host'\nmodel='m'\nlocal=true",
        "[notifiers.n]\nkind='stdio'",
    ] {
        let error = resolve(input, None, "", &paths()).unwrap_err();
        assert_eq!(error.code, "CONFIG_INVALID");
        assert!(!format!("{error:?} {error}").contains("DO_NOT_LEAK"));
        assert!(!error.field.is_empty());
    }
    assert!(resolve("", Some("absent"), "", &paths()).is_err());
}

#[test]
fn fallback_references_cycles_and_slowest_selected_executor_are_checked() {
    let valid = r#"
[consult]
executor = "primary"
[executors.primary]
kind = "codex"
binary = "codex"
fallback = ["slow"]
[executors.slow]
kind = "pi"
binary = "pi"
timeout = "2000s"
"#;
    assert_eq!(config(valid).timeouts().workflow, Duration::from_secs(6300));
    let cyclic = format!("{valid}\nfallback=['primary']");
    assert!(
        resolve(&cyclic, None, "", &paths())
            .unwrap_err()
            .message
            .contains("recursive")
    );
    let missing = valid.replace("[\"slow\"]", "[\"missing\"]");
    assert!(resolve(&missing, None, "", &paths()).is_err());
    let too_short = format!("{valid}\n[timeouts]\nworkflow='2000s'\n");
    assert!(resolve(&too_short, None, "", &paths()).is_err());
}

#[test]
fn diagnostics_redact_credentials_endpoints_and_command_arguments() {
    let input = r#"
[models.m]
kind = "text"
endpoint = "http://example.test/DO_NOT_LEAK"
model = "chat"
local = true
credential_ref = "env:DO_NOT_LEAK"
[executors.one]
kind = "codex"
binary = "codex"
args = ["DO_NOT_LEAK"]
[notifiers.mail]
kind = "stdio"
command = ["DO_NOT_LEAK"]
credential_ref = "env:DO_NOT_LEAK"
"#;
    let c = config(input);
    assert!(!format!("{c:?}").contains("DO_NOT_LEAK"));
    assert!(!c.diagnostic().to_string().contains("DO_NOT_LEAK"));
    let m = ConfigManager::new(1, c, |_| Ok(())).unwrap();
    assert!(!m.status().unwrap().to_string().contains("DO_NOT_LEAK"));
}

#[tokio::test]
async fn accepted_tasks_pin_resources_and_subscribers_see_complete_generations() {
    struct Resource(Arc<AtomicUsize>, String);
    impl Drop for Resource {
        fn drop(&mut self) {
            self.0.fetch_add(1, Ordering::SeqCst);
        }
    }
    let dropped = Arc::new(AtomicUsize::new(0));
    let prepare = |c: &ResolvedConfig| {
        Ok(Resource(
            Arc::clone(&dropped),
            c.config().consult.instructions_version.clone(),
        ))
    };
    let m = ConfigManager::new(10, config(""), prepare).unwrap();
    let queued = m.pin();
    let running = Arc::clone(&queued);
    let mut subscriber = m.subscribe();
    let report = m
        .reload(
            "[consult]\ninstructions_version='v2'\n[timeouts]\nrunner='100s'",
            None,
            prepare,
        )
        .unwrap();
    assert_eq!(report.status, ReloadStatus::Applied);
    assert_eq!(report.active_generation, 11);
    subscriber.changed().await.unwrap();
    let observed = Arc::clone(&subscriber.borrow_and_update());
    assert_eq!(
        observed.resolved().config().consult.instructions_version,
        observed.resources().1
    );
    assert_eq!(observed.resources().1, "v2");
    assert_eq!(running.resources().1, "v1");
    assert_eq!(queued.generation(), 10);
    assert_eq!(
        queued.resolved().timeouts().runner,
        Duration::from_secs(1700)
    );
    assert_eq!(
        m.pin().resolved().timeouts().runner,
        Duration::from_secs(100)
    );
    drop(queued);
    assert_eq!(dropped.load(Ordering::SeqCst), 0);
    drop(running);
    assert_eq!(dropped.load(Ordering::SeqCst), 1);
}

#[test]
fn rejected_or_mixed_restart_changes_never_partially_apply() {
    let m = manager("");
    let initial = m.pin();
    let bad = m
        .reload("[timeouts", None, |_| {
            panic!("invalid candidate reached prepare")
        })
        .unwrap();
    assert_eq!(bad.status, ReloadStatus::Rejected);
    assert!(Arc::ptr_eq(&initial, &m.pin()));
    let failed = m
        .reload("[logging]\nlevel='debug'", None, |_| Err(()))
        .unwrap();
    assert_eq!(failed.status, ReloadStatus::Rejected);
    assert!(Arc::ptr_eq(&initial, &m.pin()));
    let mixed = m
        .reload(
            "[paths]\ndata_dir='/new/path'\n[timeouts]\nrunner='100s'",
            None,
            |_| panic!("restart-required candidate reached prepare"),
        )
        .unwrap();
    assert_eq!(mixed.status, ReloadStatus::RestartRequired);
    assert_eq!(mixed.active_generation, 1);
    assert_eq!(mixed.pending_generation, Some(2));
    assert_eq!(
        m.pin().resolved().timeouts().runner,
        Duration::from_secs(1700)
    );
    let status = m.status().unwrap();
    assert_eq!(
        status["pending"]["desired"]["effective_timeouts"]["runner"]["secs"],
        100
    );
    assert_eq!(
        status["effective"]["effective_timeouts"]["runner"]["secs"],
        1700
    );
    let applied = m
        .reload("[timeouts]\nrunner='2000s'", None, |_| Ok(()))
        .unwrap();
    assert_eq!(applied.active_generation, 2);
    assert!(m.status().unwrap()["pending"].is_null());
    assert_eq!(
        m.status().unwrap()["revision_log"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    println!(
        "{}",
        serde_json::to_string_pretty(&m.status().unwrap()).unwrap()
    );
}

#[test]
fn channel_routes_executor_permissions_and_global_limits_use_live_config() {
    let initial = "[executors.one]\nkind='codex'\nbinary='codex'\n[notifiers.inbox]\nkind='local'";
    let m = manager(initial);
    let old = m.pin();
    assert!(m.executor_allowed("one"));
    assert_eq!(m.delivery_route("inbox").unwrap().0, 1);
    let update = format!(
        "{initial}\nenabled=false\n[runtime]\nmax_concurrent_jobs=2\npaused=true\n[access]\nread_roots=['/new/scope']"
    );
    assert_eq!(
        m.reload(&update, None, |_| Ok(())).unwrap().status,
        ReloadStatus::Applied
    );
    assert!(old.resolved().config().notifiers["inbox"].enabled);
    assert!(m.delivery_route("inbox").is_none());
    assert!(!m.executor_allowed("one"));
    assert_eq!(old.resolved().config().runtime.max_concurrent_jobs, 1);
    assert_eq!(m.pin().resolved().config().runtime.max_concurrent_jobs, 2);
    assert_eq!(
        m.pin().resolved().config().access.read_roots,
        [std::path::PathBuf::from("/new/scope")]
    );
}

#[test]
fn concurrent_reload_commits_unique_complete_versions() {
    let m = Arc::new(
        ConfigManager::new(1, config(""), |c| {
            Ok(c.config().consult.instructions_version.clone())
        })
        .unwrap(),
    );
    let start = Arc::new(Barrier::new(3));
    let threads: Vec<_> = ["a", "b"]
        .into_iter()
        .map(|version| {
            let m = Arc::clone(&m);
            let start = Arc::clone(&start);
            std::thread::spawn(move || {
                start.wait();
                m.reload(
                    &format!("[consult]\ninstructions_version='{version}'"),
                    None,
                    |c| Ok(c.config().consult.instructions_version.clone()),
                )
                .unwrap()
            })
        })
        .collect();
    start.wait();
    let mut generations: Vec<_> = threads
        .into_iter()
        .map(|t| t.join().unwrap().active_generation)
        .collect();
    generations.sort();
    assert_eq!(generations, [2, 3]);
    let final_state = m.pin();
    assert_eq!(
        &final_state.resolved().config().consult.instructions_version,
        final_state.resources()
    );
    assert_eq!(
        m.status().unwrap()["revision_log"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
}

#[test]
fn retrieval_fingerprint_ignores_log_only_changes_but_tracks_model_semantics() {
    let base = config(MODELS);
    let logging = config(&format!("{MODELS}\n[logging]\nlevel='debug'"));
    assert_ne!(base.digest(), logging.digest());
    assert_eq!(
        base.retrieval_fingerprint(),
        logging.retrieval_fingerprint()
    );
    let changed = config(&MODELS.replace("dimension = 128", "dimension = 256"));
    assert_ne!(
        base.retrieval_fingerprint(),
        changed.retrieval_fingerprint()
    );
}

#[tokio::test(start_paused = true)]
async fn queue_time_does_not_shorten_1700_seconds_or_enable_an_idle_watchdog() {
    let c = config("");
    let queue = Deadline::start(Layer::Queue, c.timeouts().queue, None).unwrap();
    assert!(Deadline::start(Layer::Runner, c.timeouts().runner, Some(&queue)).is_err());
    tokio::time::advance(Duration::from_secs(120)).await;
    assert!(queue.expired().is_none());
    let workflow = Deadline::start(Layer::Workflow, c.timeouts().workflow, None).unwrap();
    let runner = Deadline::start(Layer::Runner, c.timeouts().runner, Some(&workflow)).unwrap();
    assert_eq!(runner.remaining(), Duration::from_secs(1700));
    for advance in [46, 45, 1608] {
        tokio::time::advance(Duration::from_secs(advance)).await;
        assert!(runner.expired().is_none());
    }
    assert_eq!(runner.remaining(), Duration::from_secs(1));
    tokio::time::advance(Duration::from_secs(1)).await;
    assert_eq!(runner.expired(), Some(Layer::Runner));
    assert_eq!(runner.trace().triggered_layer, Some(Layer::Runner));
}

#[tokio::test(start_paused = true)]
async fn workflow_covers_three_starts_and_reports_the_actual_limiting_layer() {
    let c = config("");
    let workflow = Deadline::start(Layer::Workflow, c.timeouts().workflow, None).unwrap();
    for _ in 0..3 {
        let runner = Deadline::start(Layer::Runner, c.timeouts().runner, Some(&workflow)).unwrap();
        tokio::time::advance(Duration::from_secs(1699)).await;
        assert!(runner.expired().is_none());
        tokio::time::advance(Duration::from_secs(1)).await;
        assert_eq!(runner.expired(), Some(Layer::Runner));
    }
    assert!(workflow.expired().is_none());
    let runner = Deadline::start(Layer::Runner, Duration::from_secs(100), None).unwrap();
    tokio::time::advance(Duration::from_secs(90)).await;
    let provider =
        Deadline::start(Layer::Provider, Duration::from_secs(1700), Some(&runner)).unwrap();
    assert_eq!(provider.trace().configured_ms, 1_700_000);
    assert_eq!(provider.trace().effective_ms, 10_000);
    tokio::time::advance(Duration::from_secs(10)).await;
    assert_eq!(provider.expired(), Some(Layer::Runner));
    let provider = Deadline::start(Layer::Provider, Duration::from_secs(1700), None).unwrap();
    tokio::time::advance(Duration::from_secs(1700)).await;
    assert_eq!(provider.expired(), Some(Layer::Provider));
}

#[tokio::test(start_paused = true)]
async fn reload_does_not_change_existing_deadlines() {
    let m = manager("");
    let task = m.pin();
    let deadline = Deadline::start(Layer::Runner, task.resolved().timeouts().runner, None).unwrap();
    tokio::time::advance(Duration::from_secs(90)).await;
    for timeout in ["100s", "2000s"] {
        assert_eq!(
            m.reload(&format!("[timeouts]\nrunner='{timeout}'"), None, |_| Ok(()))
                .unwrap()
                .status,
            ReloadStatus::Applied
        );
        assert_eq!(deadline.remaining(), Duration::from_secs(1610));
    }
    assert_eq!(task.generation(), 1);
    assert_eq!(m.pin().generation(), 3);
}

#[test]
fn bounded_file_reads_handle_partial_writes_and_atomic_replacement() {
    let root = std::env::temp_dir().join(format!("akasha-config-{}", std::process::id()));
    std::fs::create_dir(&root).unwrap();
    struct Cleanup(std::path::PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }
    let _cleanup = Cleanup(root.clone());
    let path = root.join("config.toml");
    std::fs::write(&path, "[timeouts").unwrap();
    let m = manager("");
    assert_eq!(
        m.reload(&read_file(&path).unwrap(), None, |_| Ok(()))
            .unwrap()
            .status,
        ReloadStatus::Rejected
    );
    let replacement = root.join("replacement");
    std::fs::write(&replacement, "[timeouts]\nrunner='1800s'").unwrap();
    std::fs::rename(replacement, &path).unwrap();
    assert_eq!(
        m.reload(&read_file(&path).unwrap(), None, |_| Ok(()))
            .unwrap()
            .status,
        ReloadStatus::Applied
    );
    std::fs::write(&path, vec![b'a'; MAX_CONFIG_BYTES + 1]).unwrap();
    assert!(read_file(&path).is_err());
    std::fs::write(&path, [0xff]).unwrap();
    assert!(read_file(&path).is_err());
}

#[test]
fn embedding_preparation_failure_preserves_the_whole_old_bundle() {
    let m = ConfigManager::new(1, config(MODELS), |c| {
        Ok(c.config().models["embed"].dimension.unwrap())
    })
    .unwrap();
    let old_task = m.pin();
    let candidate = MODELS.replace("dimension = 128", "dimension = 256");
    assert_eq!(
        m.reload(&candidate, None, |_| Err(())).unwrap().status,
        ReloadStatus::Rejected
    );
    assert_eq!(
        m.pin().resolved().config().models["embed"].dimension,
        Some(*m.pin().resources())
    );
    assert_eq!(*m.pin().resources(), 128);
    m.reload(&candidate, None, |c| {
        Ok(c.config().models["embed"].dimension.unwrap())
    })
    .unwrap();
    assert_eq!(*m.pin().resources(), 256);
    assert_eq!(*old_task.resources(), 128);
    assert_eq!(
        old_task.resolved().config().models["embed"].dimension,
        Some(128)
    );
}

#[test]
fn profile_count_and_revision_history_are_bounded() {
    let input = (0..65)
        .map(|i| format!("[executors.e{i}]\nkind='codex'\nbinary='codex'\n"))
        .collect::<String>();
    assert!(resolve(&input, None, "", &paths()).is_err());
    let m = manager("");
    for _ in 0..70 {
        m.reload("[broken", None, |_| Ok(())).unwrap();
    }
    assert_eq!(
        m.status().unwrap()["revision_log"]
            .as_array()
            .unwrap()
            .len(),
        64
    );
    assert_eq!(m.pin().generation(), 1);
}

#[test]
fn removing_an_explicit_path_restores_startup_defaults_as_pending() {
    let m = manager("[paths]\ndata_dir='/custom/data'");
    let outcome = m
        .reload("", None, |_| panic!("changed path reached prepare"))
        .unwrap();
    assert_eq!(outcome.status, ReloadStatus::RestartRequired);
    let status = m.status().unwrap();
    assert_eq!(
        status["effective"]["config"]["paths"]["data_dir"],
        "/custom/data"
    );
    assert_eq!(
        status["pending"]["desired"]["config"]["paths"]["data_dir"],
        "/tmp/akasha-config-test/data"
    );
}
