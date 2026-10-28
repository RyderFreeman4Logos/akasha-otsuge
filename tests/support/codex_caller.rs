use super::*;
use akasha_otsuge::{
    adapter::codex::{CodexCall, CodexCaller, CodexRegistration},
    adapter::*,
    config::{
        Host,
        deadline::{Deadline, Layer},
    },
    domain::Domain,
};
use std::{sync::Arc, time::Duration};

fn native(fixture: &Fixture) -> (CodexRegistration, CodexCall) {
    let raw = include_str!("../fixtures/codex/0.164.0/exec-root.jsonl")
        .replace("/fixture/workspace", fixture.0.to_str().unwrap());
    let path = fixture.0.join("native.jsonl");
    std::fs::write(&path, raw).unwrap();
    let hook: serde_json::Value = serde_json::from_str(include_str!(
        "../fixtures/codex/0.164.0/exec-pre-tool-use.json"
    ))
    .unwrap();
    let session = hook["session_id"].as_str().unwrap().to_owned();
    let capability = CapabilityReport {
        host: Host::Codex,
        version: Some("0.164.0".into()),
        executable: Some("/fixture/codex".into()),
        adapter_version: "1".into(),
        binding: BindingLevel::Unavailable,
        native_children: false,
        history: CoverageLevel::Unavailable,
        machine_protocol: None,
        cancellation: false,
        outer_timeout: false,
        capabilities: Default::default(),
        self_test: SelfTest::NotRun,
    };
    let registration = CodexRegistration {
        capability,
        source: SourceIdentity {
            adapter: "codex".into(),
            profile: "default".into(),
            session: session.clone(),
            thread: session.clone(),
            agent: "main".into(),
        },
        scope: scope(),
        principal: "reader".into(),
        transcript: path,
        workspace: fixture.0.canonicalize().unwrap(),
    };
    let call = CodexCall {
        session_id: session,
        agent_id: None,
        turn_id: hook["turn_id"].as_str().unwrap().into(),
        tool_call_id: hook["tool_use_id"].as_str().unwrap().into(),
        cwd: registration.workspace.clone(),
    };
    (registration, call)
}

#[tokio::test]
async fn native_call_binds_exact_turn_before_its_result_and_retries_keep_the_fence() {
    let fixture = Fixture::new();
    let store = Arc::new(fixture.open().await);
    Domain::new(&store)
        .set_read_grant("grant", "reader", Some(&scope()), None, 1)
        .await
        .unwrap();
    let (registration, call) = native(&fixture);
    let caller = CodexCaller::new(store.clone(), registration.clone(), call.clone(), 1);
    let invocation = caller.bind(&call.tool_call_id).await.unwrap();
    assert_eq!(invocation.diagnostic().caller, registration.source);
    assert_eq!(
        invocation.diagnostic().proof,
        CallerProof::RegisteredRoot {
            thread: call.session_id.clone()
        }
    );
    assert_eq!(
        invocation.diagnostic().turn_id.as_deref(),
        Some(call.turn_id.as_str())
    );
    assert_eq!(invocation.diagnostic().requested_cursor, 10);
    assert_eq!(invocation.diagnostic().available_cursor, 10);
    assert_eq!(
        invocation.diagnostic().tip.as_ref().unwrap().event_id,
        "row:9"
    );
    assert_eq!(invocation.coverage(), FenceCoverage::CompleteToFence);
    let budget = RunBudget::new(
        invocation.id(),
        &invocation.fence_hash(),
        Arc::new(tokio::sync::Semaphore::new(1)),
        BudgetLimits::default(),
        Deadline::start(Layer::Workflow, Duration::from_secs(10), None).unwrap(),
    )
    .unwrap();
    let lease = budget.start().unwrap();
    let mut context = CapturedContext::new(&store, invocation.clone()).unwrap();
    let ContextResponse::Page(page) = context
        .execute(
            &lease,
            ContextRequest::List {
                cursor: None,
                limit: 16,
            },
        )
        .await
        .unwrap()
    else {
        panic!("expected native context page")
    };
    assert_eq!(page.events.len(), 10);
    for event in page.events {
        let ContextResponse::Text(text) = context
            .execute(
                &lease,
                ContextRequest::Read {
                    handle: event.handle,
                    start: 0,
                    max_bytes: 16384,
                },
            )
            .await
            .unwrap()
        else {
            panic!("expected native context text")
        };
        assert!(
            !text.text.contains("synthetic-context"),
            "final answer is later than the call fence"
        );
        assert_ne!(
            text.text, "synthetic-exec",
            "tool output is later than the call fence"
        );
    }
    let retry = CodexCaller::new(store.clone(), registration, call.clone(), 2)
        .bind(&call.tool_call_id)
        .await
        .unwrap();
    assert_eq!(retry.revision(), invocation.revision());
    assert_eq!(retry.fence_hash(), invocation.fence_hash());
    assert_eq!(retry.diagnostic().config_generation, 1);
    store.close().await.unwrap();
}

#[tokio::test]
async fn missing_or_conflicting_runtime_proof_never_binds_to_the_root() {
    for case in [
        "session",
        "turn",
        "call",
        "agent",
        "cwd",
        "source",
        "version",
        "duplicate",
    ] {
        let fixture = Fixture::new();
        let store = Arc::new(fixture.open().await);
        Domain::new(&store)
            .set_read_grant("grant", "reader", Some(&scope()), None, 1)
            .await
            .unwrap();
        let (registration, mut call) = native(&fixture);
        let mut rows: Vec<serde_json::Value> = std::fs::read_to_string(&registration.transcript)
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect();
        let expected = match case {
            "session" => {
                call.session_id = "other-session".into();
                ErrorCode::CallerAmbiguous
            }
            "turn" => {
                call.turn_id = "other-turn".into();
                ErrorCode::CallerAmbiguous
            }
            "call" => {
                call.tool_call_id = "other-call".into();
                ErrorCode::CallerAmbiguous
            }
            "agent" => {
                call.agent_id = Some("child".into());
                ErrorCode::AdapterUnsupported
            }
            "cwd" => {
                call.cwd = fixture.0.join("elsewhere");
                ErrorCode::BindingUnavailable
            }
            "source" => {
                rows[0]["payload"]["source"] = serde_json::json!({"subagent": {"thread_spawn": {"parent_thread_id": "parent"}}});
                ErrorCode::AdapterUnsupported
            }
            "version" => {
                rows[0]["payload"]["cli_version"] = "0.165.0".into();
                ErrorCode::AdapterUnsupported
            }
            "duplicate" => {
                rows.push(rows[9].clone());
                ErrorCode::CallerAmbiguous
            }
            _ => unreachable!(),
        };
        std::fs::write(
            &registration.transcript,
            rows.iter()
                .map(|row| format!("{row}\n"))
                .collect::<String>(),
        )
        .unwrap();
        let at = store.sequence().unwrap();
        let caller = CodexCaller::new(store.clone(), registration, call.clone(), 1);
        assert_eq!(
            caller.bind(&call.tool_call_id).await.unwrap_err().code,
            expected,
            "{case}"
        );
        assert_eq!(
            store.sequence().unwrap(),
            at,
            "invalid identity must not capture: {case}"
        );
        store.close().await.unwrap();
    }
}

#[tokio::test]
async fn missing_ancestor_is_observable_and_permission_revocation_applies_to_retries() {
    let fixture = Fixture::new();
    let store = Arc::new(fixture.open().await);
    let grant = Domain::new(&store)
        .set_read_grant("grant", "reader", Some(&scope()), None, 1)
        .await
        .unwrap();
    let (registration, call) = native(&fixture);
    let raw = std::fs::read_to_string(&registration.transcript).unwrap();
    std::fs::write(
        &registration.transcript,
        raw.lines()
            .enumerate()
            .filter(|(i, _)| *i != 5)
            .map(|(_, s)| format!("{s}\n"))
            .collect::<String>(),
    )
    .unwrap();
    let caller = CodexCaller::new(store.clone(), registration.clone(), call.clone(), 1);
    assert_eq!(
        caller.bind(&call.tool_call_id).await.unwrap_err().code,
        ErrorCode::HistoryGap
    );
    std::fs::write(&registration.transcript, raw).unwrap();
    let bound = caller.bind(&call.tool_call_id).await.unwrap();
    std::fs::remove_file(&registration.transcript).unwrap();
    assert_eq!(
        caller.bind(&call.tool_call_id).await.unwrap().revision(),
        bound.revision()
    );
    Domain::new(&store)
        .set_read_grant("revoke", "reader", None, Some(grant.commit_seq), 2)
        .await
        .unwrap();
    assert_eq!(
        caller.bind(&call.tool_call_id).await.unwrap_err().code,
        ErrorCode::ReadDenied
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn concurrent_registered_sessions_in_one_workspace_keep_separate_fences() {
    let fixture = Fixture::new();
    let store = Arc::new(fixture.open().await);
    Domain::new(&store)
        .set_read_grant("grant", "reader", Some(&scope()), None, 1)
        .await
        .unwrap();
    let (first, first_call) = native(&fixture);
    let mut second = first.clone();
    second.source.session = "separate-native-session".into();
    second.source.thread = second.source.session.clone();
    second.transcript = fixture.0.join("second-native.jsonl");
    std::fs::write(
        &second.transcript,
        std::fs::read_to_string(&first.transcript)
            .unwrap()
            .replace(&first.source.session, &second.source.session),
    )
    .unwrap();
    let mut second_call = first_call.clone();
    second_call.session_id = second.source.session.clone();
    let one = CodexCaller::new(store.clone(), first, first_call.clone(), 1);
    let two = CodexCaller::new(store.clone(), second, second_call.clone(), 1);
    let (left, right) = tokio::join!(
        one.bind(&first_call.tool_call_id),
        two.bind(&second_call.tool_call_id)
    );
    let (left, right) = (left.unwrap(), right.unwrap());
    assert_ne!(left.id(), right.id());
    assert_ne!(left.diagnostic().tip, right.diagnostic().tip);
    assert_ne!(left.diagnostic().caller, right.diagnostic().caller);
    assert_eq!(left.diagnostic().workspace, right.diagnostic().workspace);
    assert_eq!(left.diagnostic().requested_cursor, 10);
    assert_eq!(right.diagnostic().requested_cursor, 10);
    store.close().await.unwrap();
}
