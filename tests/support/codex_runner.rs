use super::*;
use akasha_otsuge::{
    adapter::{
        codex::{CodexCaller, CodexRunner},
        *,
    },
    config::deadline::{Deadline, Layer},
    domain::Domain,
};
use std::{sync::Arc, time::Duration};

fn host(fixture: &Fixture, scenario: &str) -> PathBuf {
    let binary = fixture.0.join("codex");
    crate::executable::write(
        &binary,
        include_str!("../fixtures/codex/0.164.0/runner_host.py"),
    );
    std::fs::write(fixture.0.join("scenario"), scenario).unwrap();
    std::fs::write(fixture.0.join("models_cache.json"), r#"{"models":[{"slug":"fixture","shell_type":"unified_exec","apply_patch_tool_type":"freeform","experimental_supported_tools":["clock"],"tool_mode":"code_mode"}]}"#).unwrap();
    binary
}

async fn invocation(fixture: &Fixture) -> (Arc<Storage>, Invocation) {
    let store = Arc::new(fixture.open().await);
    Domain::new(&store)
        .set_read_grant("grant", "reader", Some(&scope()), None, 1)
        .await
        .unwrap();
    let (registration, call) = caller::native(fixture);
    // Runner fixtures start after ingestion; caller tests own the bounded catch-up path.
    reconcile_transcript(
        &store,
        &registration.source,
        registration.scope.clone(),
        &registration.transcript,
        64,
        1,
    )
    .await
    .unwrap();
    let invocation = CodexCaller::new(store.clone(), registration, call.clone(), 1)
        .bind(&call.tool_call_id)
        .await
        .unwrap();
    (store, invocation)
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
            executor_profile: "codex".into(),
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
async fn standalone_runner_uses_scoped_native_tools_and_requires_structured_final_and_zero_exit() {
    let fixture = Fixture::new();
    let binary = host(&fixture, "success");
    let (store, inv) = invocation(&fixture).await;
    let mut runner = CodexRunner::new(&binary, &fixture.0, &fixture.0, "Read the bound context.")
        .await
        .unwrap();
    runner
        .probe()
        .unwrap()
        .negotiate(
            &[
                "context.describe",
                "context.list",
                "context.read",
                "context.search",
            ],
            &[],
        )
        .unwrap();
    let spec = spec(&inv, 10);
    let lease = spec.lease_handle();
    let mut context = CapturedContext::new(&store, inv).unwrap();
    let mut handle = runner.start(spec, &mut context).await.unwrap();
    let mut collector = RunCollector::new(&lease, 1024);
    let mut last_usage = None;
    while let Some(event) = handle.next().await.unwrap() {
        if let RunnerEvent::Usage {
            input_tokens,
            output_tokens,
        } = &event
        {
            last_usage = Some((*input_tokens, *output_tokens));
        }
        collector.push(event).unwrap();
    }
    assert_eq!(last_usage, Some((20, 4)));
    assert_eq!(
        collector
            .finish(handle.exit_status().await.unwrap())
            .unwrap(),
        "bounded-context"
    );
    drop(handle);
    assert!(!std::fs::read_dir(&fixture.0).unwrap().any(|p| {
        p.unwrap()
            .file_name()
            .to_string_lossy()
            .starts_with("akasha-run-")
    }));
    drop(context);
    Arc::try_unwrap(store).ok().unwrap().close().await.unwrap();
}

#[tokio::test]
async fn shared_budget_stop_wakes_a_runner_waiting_on_native_output() {
    for exhausted in [true, false] {
        let f = Fixture::new();
        let binary = host(&f, "cancel");
        let (store, inv) = invocation(&f).await;
        let budget = RunBudget::new(
            inv.id(),
            &inv.fence_hash(),
            Arc::new(tokio::sync::Semaphore::new(2)),
            BudgetLimits::default(),
            Deadline::start(Layer::Workflow, Duration::from_secs(20), None).unwrap(),
        )
        .unwrap();
        let parent = budget.start().unwrap();
        let plan = RunSpec::new(
            &inv,
            RunOptions {
                job_id: "wake".into(),
                executor_profile: "codex".into(),
                instructions_version: "test".into(),
                runner_timeout: Duration::from_secs(10),
                args: ConsultArgs::new(None).unwrap(),
            },
            ReadCapability::issue(&inv).unwrap(),
            parent.delegate().unwrap(),
        )
        .unwrap();
        let mut runner = CodexRunner::new(&binary, &f.0, &f.0, "Read context.")
            .await
            .unwrap();
        let mut context = CapturedContext::new(&store, inv).unwrap();
        let mut handle = runner.start(plan, &mut context).await.unwrap();
        assert!(matches!(
            handle.next().await.unwrap(),
            Some(RunnerEvent::Started)
        ));
        let mut next = Box::pin(handle.next());
        assert!(futures::poll!(next.as_mut()).is_pending());
        let expected = if exhausted {
            assert_eq!(
                parent.charge_tokens(200_000, 0).unwrap_err().code,
                ErrorCode::BudgetExhausted
            );
            ErrorCode::BudgetExhausted
        } else {
            budget.cancel();
            ErrorCode::Cancelled
        };
        let result = tokio::time::timeout(Duration::from_secs(2), next).await;
        drop(handle);
        drop(context);
        Arc::try_unwrap(store).ok().unwrap().close().await.unwrap();
        assert_eq!(
            result
                .expect("shared stop did not wake native read")
                .unwrap_err()
                .code,
            expected
        );
    }
}

#[tokio::test]
async fn native_runner_refuses_protocol_tool_output_and_exit_failures_and_keeps_original_deadline()
{
    for (scenario, expected) in [
        ("token_budget", ErrorCode::BudgetExhausted),
        ("usage_regression", ErrorCode::ProtocolInvalid),
        ("wrong_thread", ErrorCode::ProtocolInvalid),
        ("wrong_turn", ErrorCode::ProtocolInvalid),
        ("forged_args", ErrorCode::InvalidArgument),
        ("denied_tool", ErrorCode::ToolDenied),
        ("duplicate_call", ErrorCode::ProtocolInvalid),
        ("missing_id", ErrorCode::ProtocolInvalid),
        ("invalid_id", ErrorCode::ProtocolInvalid),
        ("auth", ErrorCode::AuthRequired),
        ("auth_http", ErrorCode::AuthRequired),
        ("request_as_notification", ErrorCode::ToolDenied),
        ("malformed_final", ErrorCode::ProtocolInvalid),
        ("double_final", ErrorCode::ProtocolInvalid),
        ("nonzero", ErrorCode::RunnerFailed),
        ("no_final", ErrorCode::RunnerIncomplete),
        ("partial", ErrorCode::ProtocolInvalid),
        ("oversized", ErrorCode::OutputLimit),
        ("stderr", ErrorCode::OutputLimit),
        ("eof_live", ErrorCode::RunnerTimeout),
        ("timeout", ErrorCode::RunnerTimeout),
    ] {
        let fixture = Fixture::new();
        let binary = host(&fixture, scenario);
        let (store, inv) = invocation(&fixture).await;
        let mut runner = CodexRunner::new(&binary, &fixture.0, &fixture.0, "Read context.")
            .await
            .unwrap();
        let mut context = CapturedContext::new(&store, inv.clone()).unwrap();
        let start = std::time::Instant::now();
        let mut handle = runner.start(spec(&inv, 1), &mut context).await.unwrap();
        let error = loop {
            match handle.next().await {
                Ok(Some(_)) => {}
                Err(error) => break error,
                Ok(None) => panic!("{scenario} returned success"),
            }
        };
        assert_eq!(error.code, expected, "{scenario}");
        assert!(!format!("{error:?}").contains("private provider error"));
        assert!(
            start.elapsed() < Duration::from_secs(4),
            "{scenario} refreshed its deadline"
        );
        drop(handle);
        drop(context);
        Arc::try_unwrap(store).ok().unwrap().close().await.unwrap();
    }
}

#[tokio::test]
async fn native_runner_cancel_kills_the_owned_group_and_waits_before_cleanup() {
    let fixture = Fixture::new();
    let binary = host(&fixture, "cancel");
    let (store, inv) = invocation(&fixture).await;
    let mut runner = CodexRunner::new(&binary, &fixture.0, &fixture.0, "Read context.")
        .await
        .unwrap();
    let mut context = CapturedContext::new(&store, inv.clone()).unwrap();
    let mut handle = runner.start(spec(&inv, 10), &mut context).await.unwrap();
    assert!(matches!(
        handle.next().await.unwrap(),
        Some(RunnerEvent::Started)
    ));
    let pid = std::fs::read_to_string(fixture.0.join("child.pid")).unwrap();
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
#[ignore = "requires explicit native Codex binary/home and isolated acceptance environment"]
async fn installed_native_runner_denies_unissued_context_handles() {
    let fixture = Fixture::new();
    let binary = PathBuf::from(std::env::var_os("AKASHA_CODEX_BINARY").unwrap());
    let home = PathBuf::from(std::env::var_os("AKASHA_CODEX_HOME").unwrap());
    let (store, inv) = invocation(&fixture).await;
    let mut runner = CodexRunner::new(&binary, &home, &fixture.0, "Read only authorized context.")
        .await
        .unwrap();
    let mut context = CapturedContext::new(&store, inv.clone()).unwrap();
    let mut handle = runner.start(spec(&inv, 25), &mut context).await.unwrap();
    loop {
        match handle.next().await {
            Ok(Some(_)) => {}
            Err(error) => {
                assert_eq!(error.code, ErrorCode::ReadDenied);
                break;
            }
            Ok(None) => panic!("unissued handle was accepted"),
        }
    }
    drop(handle);
    drop(context);
    Arc::try_unwrap(store).ok().unwrap().close().await.unwrap();
}

#[tokio::test]
#[ignore = "requires explicit native Codex binary/home and isolated acceptance environment"]
async fn installed_native_runner_roundtrip() {
    let fixture = Fixture::new();
    let binary = PathBuf::from(std::env::var_os("AKASHA_CODEX_BINARY").unwrap());
    let home = PathBuf::from(std::env::var_os("AKASHA_CODEX_HOME").unwrap());
    let (store, inv) = invocation(&fixture).await;
    let mut runner = CodexRunner::new(&binary, &home, &fixture.0,
        "Call context_describe exactly once, then return {\"answer\":\"bounded-context\"}. Do not call any other tool.").await.unwrap();
    let seconds = std::env::var("AKASHA_CODEX_ACCEPTANCE_SECONDS")
        .ok()
        .map(|s| s.parse::<u64>().unwrap())
        .unwrap_or(25);
    assert!((10..=1700).contains(&seconds));
    let spec = spec(&inv, seconds);
    let lease = spec.lease_handle();
    let mut context = CapturedContext::new(&store, inv).unwrap();
    let mut handle = runner.start(spec, &mut context).await.unwrap();
    let mut collector = RunCollector::new(&lease, 1024);
    let mut calls = 0;
    while let Some(event) = handle.next().await.unwrap() {
        if matches!(&event, RunnerEvent::Tool{name} if name=="context.describe") {
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
    Arc::try_unwrap(store).ok().unwrap().close().await.unwrap();
}
