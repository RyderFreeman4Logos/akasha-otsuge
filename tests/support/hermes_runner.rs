use super::*;
use akasha_otsuge::{
    adapter::{hermes::*, *},
    config::deadline::{Deadline, Layer},
};
use std::{sync::Arc, time::Duration};

async fn host(f: &Fixture, scenario: &str) -> PathBuf {
    let binary = f.0.join("hermes");
    crate::executable::write(
        &binary,
        include_str!("../fixtures/hermes/0.22.5/runner_host.py"),
    );
    std::fs::write(f.0.join("scenario"), scenario).unwrap();
    akasha_otsuge::adapter::hermes::installation::install(&binary, &f.0, &f.0.join("unused.sock"))
        .await
        .unwrap();
    binary
}
async fn invocation(f: &Fixture) -> (Arc<akasha_otsuge::storage::Storage>, Invocation) {
    let store = caller::store(f).await;
    let (registration, call) = caller::native(f);
    let inv = HermesCaller::new(store.clone(), registration, call.clone(), 1)
        .bind(&call.tool_call_id)
        .await
        .unwrap();
    (store, inv)
}
fn spec(inv: &Invocation, seconds: u64) -> RunSpec {
    let budget = RunBudget::new(
        inv.id(),
        &inv.fence_hash(),
        Arc::new(tokio::sync::Semaphore::new(1)),
        BudgetLimits::default(),
        Deadline::start(Layer::Workflow, Duration::from_secs(seconds + 5), None).unwrap(),
    )
    .unwrap();
    RunSpec::new(
        inv,
        RunOptions {
            job_id: "job".into(),
            executor_profile: "hermes".into(),
            instructions_version: "test".into(),
            runner_timeout: Duration::from_secs(seconds),
            args: ConsultArgs::new(None).unwrap(),
        },
        ReadCapability::issue(inv).unwrap(),
        budget.start().unwrap(),
    )
    .unwrap()
}

#[tokio::test]
async fn standalone_runner_uses_profile_plugin_scoped_context_and_structured_final() {
    let f = Fixture::new();
    let binary = host(&f, "success").await;
    let (store, inv) = invocation(&f).await;
    let mut runner = HermesRunner::new(&binary, &f.0, &f.0, "Read context.")
        .await
        .unwrap();
    runner
        .probe()
        .unwrap()
        .negotiate(&["context.describe", "context.read", "runner.cancel"], &[])
        .unwrap();
    let spec = spec(&inv, 10);
    let lease = spec.lease_handle();
    let mut context = CapturedContext::new(&store, inv).unwrap();
    let mut handle = runner.start(spec, &mut context).await.unwrap();
    let mut collector = RunCollector::new(&lease, 1024);
    let mut calls = 0;
    while let Some(event) = handle.next().await.unwrap() {
        if matches!(&event,RunnerEvent::Tool{name} if name=="context.describe") {
            calls += 1;
        }
        collector.push(event).unwrap();
    }
    assert_eq!(calls, 1);
    assert_eq!(
        collector
            .finish(handle.exit_status().await.unwrap())
            .unwrap(),
        "bounded-context"
    );
    drop(handle);
    drop(context);
    assert!(!std::fs::read_dir(&f.0).unwrap().any(|p| {
        p.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("akasha-run-")
    }));
    Arc::try_unwrap(store).ok().unwrap().close().await.unwrap();
}

#[tokio::test]
async fn runner_rejects_forged_authority_protocol_errors_and_unfinished_native_processes() {
    for (scenario, expected) in [
        ("token_budget", ErrorCode::BudgetExhausted),
        ("usage_regression", ErrorCode::ProtocolInvalid),
        ("wrong_token", ErrorCode::ReadDenied),
        ("wrong_home", ErrorCode::ReadDenied),
        ("double_ready", ErrorCode::ProtocolInvalid),
        ("no_ready", ErrorCode::AdapterUnsupported),
        ("denied_tool", ErrorCode::ToolDenied),
        ("forged_args", ErrorCode::InvalidArgument),
        ("unissued_handle", ErrorCode::ReadDenied),
        ("malformed_final", ErrorCode::ProtocolInvalid),
        ("double_final", ErrorCode::ProtocolInvalid),
        ("nonzero", ErrorCode::RunnerFailed),
        ("no_final", ErrorCode::RunnerIncomplete),
        ("partial", ErrorCode::ProtocolInvalid),
        ("oversized", ErrorCode::OutputLimit),
        ("stderr", ErrorCode::OutputLimit),
        ("timeout", ErrorCode::RunnerTimeout),
        ("eof_live", ErrorCode::RunnerTimeout),
    ] {
        let f = Fixture::new();
        let binary = host(&f, scenario).await;
        let (store, inv) = invocation(&f).await;
        let mut runner = HermesRunner::new(&binary, &f.0, &f.0, "Read context.")
            .await
            .unwrap();
        let mut context = CapturedContext::new(&store, inv.clone()).unwrap();
        let start = std::time::Instant::now();
        let mut handle = runner.start(spec(&inv, 1), &mut context).await.unwrap();
        let error = loop {
            match handle.next().await {
                Ok(Some(_)) => {}
                Err(e) => break e,
                Ok(None) => panic!("{scenario} passed"),
            }
        };
        assert_eq!(error.code, expected, "{scenario}");
        assert!(
            start.elapsed() < Duration::from_secs(4),
            "{scenario} renewed deadline"
        );
        drop(handle);
        drop(context);
        Arc::try_unwrap(store).ok().unwrap().close().await.unwrap();
    }
}

#[tokio::test]
async fn runner_cancellation_reaps_owned_group_before_releasing_resources() {
    let f = Fixture::new();
    let binary = host(&f, "cancel").await;
    let (store, inv) = invocation(&f).await;
    let mut runner = HermesRunner::new(&binary, &f.0, &f.0, "Read context.")
        .await
        .unwrap();
    let mut context = CapturedContext::new(&store, inv.clone()).unwrap();
    let mut handle = runner.start(spec(&inv, 10), &mut context).await.unwrap();
    assert!(matches!(
        handle.next().await.unwrap(),
        Some(RunnerEvent::Started)
    ));
    let pid = std::fs::read_to_string(f.0.join("child.pid")).unwrap();
    assert!(
        handle
            .cancel(CancelReason::User)
            .await
            .unwrap()
            .acknowledged
    );
    assert_eq!(handle.next().await.unwrap_err().code, ErrorCode::Cancelled);
    if let Ok(stat) = std::fs::read_to_string(format!("/proc/{}/stat", pid.trim())) {
        assert_eq!(
            stat.rsplit_once(") ").unwrap().1.split_whitespace().next(),
            Some("Z")
        );
    }
    drop(handle);
    drop(context);
    Arc::try_unwrap(store).ok().unwrap().close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires explicit native Hermes binary/profile and authenticated isolated acceptance"]
async fn installed_native_runner_roundtrip() {
    let f = Fixture::new();
    let binary = PathBuf::from(std::env::var_os("AKASHA_HERMES_BINARY").unwrap());
    let home = PathBuf::from(std::env::var_os("AKASHA_HERMES_HOME").unwrap());
    let (store, inv) = invocation(&f).await;
    let mut runner = HermesRunner::new(&binary, &home, &f.0,
        "Read context. Call context_describe, context_list with limit16, context_read on the first returned handle with start0 and max_bytes1024, and context_search with query Original and limit16. Call each of those four tools exactly once. Then return {\"answer\":\"bounded-context\"}. Do not call any other tool.").await.unwrap();
    let seconds = std::env::var("AKASHA_HERMES_ACCEPTANCE_SECONDS")
        .ok()
        .map(|s| s.parse::<u64>().unwrap())
        .unwrap_or(180);
    assert!((10..=1700).contains(&seconds));
    let spec = spec(&inv, seconds);
    let lease = spec.lease_handle();
    let mut context = CapturedContext::new(&store, inv).unwrap();
    let mut handle = runner.start(spec, &mut context).await.unwrap();
    let mut collector = RunCollector::new(&lease, 1024);
    let mut calls = std::collections::BTreeMap::new();
    while let Some(event) = handle.next().await.unwrap() {
        if let RunnerEvent::Tool { name } = &event {
            *calls.entry(name.clone()).or_insert(0) += 1;
        }
        collector.push(event).unwrap();
    }
    assert_eq!(
        calls,
        [
            "context.describe",
            "context.list",
            "context.read",
            "context.search"
        ]
        .into_iter()
        .map(|n| (n.to_owned(), 1))
        .collect()
    );
    assert_eq!(
        collector
            .finish(handle.exit_status().await.unwrap())
            .unwrap(),
        "bounded-context"
    );
    drop(handle);
    drop(context);
    Arc::try_unwrap(store).ok().unwrap().close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires explicit native Hermes binary/profile and authenticated isolated acceptance"]
async fn installed_native_rejects_unissued_context() {
    let f = Fixture::new();
    let binary = PathBuf::from(std::env::var_os("AKASHA_HERMES_BINARY").unwrap());
    let home = PathBuf::from(std::env::var_os("AKASHA_HERMES_HOME").unwrap());
    let (store, inv) = invocation(&f).await;
    let mut runner = HermesRunner::new(&binary, &home, &f.0,
        "Permission acceptance: immediately call context_read with handle /outside/unissued, start0 and max_bytes32. Do not list or search first. This deliberate invalid handle must be refused.").await.unwrap();
    let mut context = CapturedContext::new(&store, inv.clone()).unwrap();
    let mut handle = runner.start(spec(&inv, 180), &mut context).await.unwrap();
    let error = loop {
        match handle.next().await {
            Ok(Some(_)) => {}
            Err(error) => break error,
            Ok(None) => panic!("unissued handle was not refused"),
        }
    };
    assert_eq!(error.code, ErrorCode::ReadDenied);
    drop(handle);
    drop(context);
    Arc::try_unwrap(store).ok().unwrap().close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires explicit native Hermes binary/profile and authenticated isolated acceptance"]
async fn installed_native_cancellation_ends_executor_after_context_tool() {
    let f = Fixture::new();
    let binary = PathBuf::from(std::env::var_os("AKASHA_HERMES_BINARY").unwrap());
    let home = PathBuf::from(std::env::var_os("AKASHA_HERMES_HOME").unwrap());
    let (store, inv) = invocation(&f).await;
    let mut runner = HermesRunner::new(&binary, &home, &f.0,
        "Call context_describe first, then context_list limit16, then read the first handle start0 max_bytes1024. After all three tools return {\"answer\":\"checked\"}.").await.unwrap();
    let mut context = CapturedContext::new(&store, inv.clone()).unwrap();
    let mut handle = runner.start(spec(&inv, 180), &mut context).await.unwrap();
    loop {
        match handle.next().await.unwrap() {
            Some(RunnerEvent::Tool { name }) => {
                assert_eq!(name, "context.describe");
                break;
            }
            Some(_) => {}
            None => panic!("native executor ended before its context tool"),
        }
    }
    let owned: Vec<_> = std::fs::read_dir("/proc")
        .unwrap()
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| std::fs::read_link(path.join("cwd")).is_ok_and(|cwd| cwd.starts_with(&f.0)))
        .collect();
    assert!(
        !owned.is_empty(),
        "native executor ownership was not observed"
    );
    assert!(
        handle
            .cancel(CancelReason::User)
            .await
            .unwrap()
            .acknowledged
    );
    assert_eq!(handle.next().await.unwrap_err().code, ErrorCode::Cancelled);
    for process in owned {
        assert!(
            !std::fs::read_link(process.join("cwd")).is_ok_and(|cwd| cwd.starts_with(&f.0)),
            "owned native process survived acknowledged cancellation"
        );
    }
    drop(handle);
    drop(context);
    assert!(!std::fs::read_dir(&f.0).unwrap().any(|entry| {
        entry
            .unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("akasha-run-")
    }));
    Arc::try_unwrap(store).ok().unwrap().close().await.unwrap();
}
