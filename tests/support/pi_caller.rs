use super::Fixture;
use akasha_otsuge::{
    adapter::{pi::*, *},
    capture::*,
    config::Host,
    domain::{Domain, Scope},
    storage::Storage,
};
use serde_json::{Value, json};
use std::sync::Arc;

pub(super) fn scope() -> Scope {
    Scope {
        user: "user".into(),
        project: Some("project".into()),
        branch_or_version: None,
        session: None,
    }
}
pub(super) fn native(f: &Fixture, suffix: &str) -> (PiRegistration, PiCall) {
    let agent_dir = f.0.join(format!("agent-{suffix}"));
    std::fs::create_dir(&agent_dir).unwrap();
    let workspace = f.0.canonicalize().unwrap();
    let agent_dir = agent_dir.canonicalize().unwrap();
    let path = agent_dir.join("session.jsonl");
    let manifest: Value =
        serde_json::from_str(include_str!("../fixtures/pi/1.4.0/manifest.json")).unwrap();
    let session = format!("session-{suffix}");
    let mut rows: Vec<Value> = include_str!("../fixtures/pi/1.4.0/root-tree.jsonl")
        .split_terminator('\n')
        .map(|s| serde_json::from_str(s).unwrap())
        .collect();
    rows[0]["id"] = json!(session);
    rows[0]["cwd"] = json!(workspace);
    let assistant = rows
        .iter_mut()
        .find(|v| v["id"] == manifest["ids"]["assistant"])
        .unwrap();
    assistant["message"]["content"][2]["name"] = json!("akasha_consult");
    assistant["message"]["content"][2]["arguments"] = json!({"hint":"review"});
    std::fs::write(
        &path,
        rows.iter().map(|v| format!("{v}\n")).collect::<String>(),
    )
    .unwrap();
    let registration = PiRegistration {
        capability: CapabilityReport {
            host: Host::Pi,
            version: Some("1.4.0".into()),
            executable: Some("/fixture/pi".into()),
            adapter_version: "1".into(),
            binding: BindingLevel::Unavailable,
            native_children: false,
            history: CoverageLevel::Unavailable,
            machine_protocol: None,
            cancellation: false,
            outer_timeout: false,
            capabilities: Default::default(),
            self_test: SelfTest::NotRun,
        },
        source: SourceIdentity {
            adapter: "pi".into(),
            profile: profile_key(&agent_dir),
            session: session.clone(),
            thread: session.clone(),
            agent: "main".into(),
        },
        scope: scope(),
        principal: "reader".into(),
        agent_dir: agent_dir.clone(),
        transcript: path.clone(),
        workspace: workspace.clone(),
    };
    let call = PiCall {
        session_id: session,
        leaf_id: manifest["ids"]["final"].as_str().unwrap().into(),
        turn_id: manifest["ids"]["assistant"].as_str().unwrap().into(),
        tool_call_id: "call_fixture".into(),
        agent_dir,
        transcript: path,
        cwd: workspace,
    };
    (registration, call)
}
pub(super) async fn store(f: &Fixture) -> Arc<Storage> {
    let store = Arc::new(f.open().await);
    Domain::new(&store)
        .set_read_grant("grant", "reader", Some(&scope()), None, 1)
        .await
        .unwrap();
    store
}

async fn captured(store: &Storage, registration: &PiRegistration) {
    // These tests isolate immutable binding and concurrent identities from cold-ingest timing.
    // Cold native capture and daemon-driven binding are covered by their own integration tests.
    let report = reconcile_transcript(
        store,
        &registration.source,
        registration.scope.clone(),
        &registration.transcript,
        64,
        1,
    )
    .await
    .unwrap();
    assert!(report.processed > 0);
    assert!(report.checkpoint.checkpoint.pending.is_none());
}

#[tokio::test]
async fn native_call_fence_survives_source_loss_and_rechecks_current_permission() {
    let f = Fixture::new();
    let s = store(&f).await;
    let grant = s.sequence().unwrap();
    let (r, c) = native(&f, "root");
    captured(&s, &r).await;
    let invocation = PiCaller::new(s.clone(), r.clone(), c.clone(), 1)
        .bind(&c.tool_call_id)
        .await
        .unwrap();
    assert_eq!(invocation.coverage(), FenceCoverage::CompleteToFence);
    assert_eq!(invocation.diagnostic().requested_cursor, 5);
    assert_eq!(invocation.diagnostic().available_cursor, 5);
    assert_eq!(
        invocation.diagnostic().turn_id.as_deref(),
        Some(c.turn_id.as_str())
    );
    let b = CaptureBinding::from_source(
        r.source.clone(),
        scope(),
        TRANSCRIPT_STREAM,
        CoverageLevel::VisibleText,
    )
    .unwrap();
    let capture = Capture::new(&s);
    let page = capture
        .lineage(
            &b,
            invocation.diagnostic().tip.as_ref().unwrap(),
            invocation.diagnostic().capture_seq,
            64,
        )
        .unwrap();
    let text = page
        .events
        .iter()
        .map(|v| capture.text(v).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Original intent"));
    assert!(text.contains("akasha_consult"));
    for forbidden in [
        "Original tool result",
        "SIBLING_",
        "OTHER_ROOT_ONLY",
        "LATER_PARENT_ONLY",
        "Current branch final",
    ] {
        assert!(!text.contains(forbidden));
    }
    std::fs::remove_file(&r.transcript).unwrap();
    let retry = PiCaller::new(s.clone(), r.clone(), c.clone(), 2)
        .bind(&c.tool_call_id)
        .await
        .unwrap();
    assert_eq!(retry.fence_hash(), invocation.fence_hash());
    assert_eq!(retry.revision(), invocation.revision());
    Domain::new(&s)
        .set_read_grant("revoke", "reader", None, Some(grant), 2)
        .await
        .unwrap();
    assert_eq!(
        PiCaller::new(s, r, c.clone(), 2)
            .bind(&c.tool_call_id)
            .await
            .unwrap_err()
            .code,
        ErrorCode::ReadDenied
    );
}

#[tokio::test]
async fn native_context_cannot_borrow_another_branch_profile_workspace_or_nested_call() {
    let f = Fixture::new();
    let s = store(&f).await;
    let (r, c) = native(&f, "controls");
    for (field, value, expected) in [
        ("session_id", "another-session", ErrorCode::CallerAmbiguous),
        (
            "agent_dir",
            "/different/profile",
            ErrorCode::CallerAmbiguous,
        ),
        (
            "transcript",
            "/different/session",
            ErrorCode::CallerAmbiguous,
        ),
        ("cwd", "/different/workspace", ErrorCode::BindingUnavailable),
        ("tool_call_id", "call_fixture/1", ErrorCode::CallerAmbiguous),
    ] {
        let mut v = serde_json::to_value(&c).unwrap();
        v[field] = json!(value);
        let changed: PiCall = serde_json::from_value(v).unwrap();
        assert_eq!(
            PiCaller::new(s.clone(), r.clone(), changed.clone(), 1)
                .bind(&changed.tool_call_id)
                .await
                .unwrap_err()
                .code,
            expected,
            "{field}"
        );
    }
    let manifest: Value =
        serde_json::from_str(include_str!("../fixtures/pi/1.4.0/manifest.json")).unwrap();
    for leaf in ["sibling", "other_root"] {
        let mut changed = c.clone();
        changed.leaf_id = manifest["ids"][leaf].as_str().unwrap().into();
        assert_eq!(
            PiCaller::new(s.clone(), r.clone(), changed, 1)
                .bind(&c.tool_call_id)
                .await
                .unwrap_err()
                .code,
            ErrorCode::CallerAmbiguous
        );
    }
    let mut older = r.clone();
    older.capability.version = Some("0.78.0".into());
    assert_eq!(
        PiCaller::new(s.clone(), older, c.clone(), 1)
            .bind(&c.tool_call_id)
            .await
            .unwrap_err()
            .code,
        ErrorCode::AdapterUnsupported
    );
    let mut child = r;
    child.source.agent = "unregistered-child".into();
    assert_eq!(
        PiCaller::new(s, child, c.clone(), 1)
            .bind(&c.tool_call_id)
            .await
            .unwrap_err()
            .code,
        ErrorCode::CallerAmbiguous
    );
}

#[tokio::test]
async fn actual_native_tool_name_unique_call_id_and_short_hint_schema_are_required() {
    let f = Fixture::new();
    let s = store(&f).await;
    for mode in ["name", "duplicate", "authority"] {
        let (r, c) = native(&f, mode);
        let mut rows: Vec<Value> = std::fs::read_to_string(&r.transcript)
            .unwrap()
            .split_terminator('\n')
            .map(|v| serde_json::from_str(v).unwrap())
            .collect();
        let row = rows.iter_mut().find(|v| v["id"] == c.turn_id).unwrap();
        match mode {
            "name" => row["message"]["content"][2]["name"] = json!("other_tool"),
            "duplicate" => {
                let copy = row["message"]["content"][2].clone();
                row["message"]["content"].as_array_mut().unwrap().push(copy);
            }
            _ => {
                row["message"]["content"][2]["arguments"] =
                    json!({"hint":"review", "session_id":"forged"})
            }
        }
        std::fs::write(
            &r.transcript,
            rows.iter().map(|v| format!("{v}\n")).collect::<String>(),
        )
        .unwrap();
        let expected = if mode == "authority" {
            ErrorCode::InvalidArgument
        } else {
            ErrorCode::CallerAmbiguous
        };
        assert_eq!(
            PiCaller::new(s.clone(), r, c.clone(), 1)
                .bind(&c.tool_call_id)
                .await
                .unwrap_err()
                .code,
            expected
        );
    }
}

#[tokio::test]
async fn two_native_sessions_share_a_workspace_without_sharing_context_fences() {
    let f = Fixture::new();
    let s = store(&f).await;
    let (r1, c1) = native(&f, "first");
    let (r2, c2) = native(&f, "second");
    captured(&s, &r1).await;
    captured(&s, &r2).await;
    let first = PiCaller::new(s.clone(), r1, c1.clone(), 1);
    let second = PiCaller::new(s, r2, c2.clone(), 1);
    let (a, b) = tokio::join!(first.bind(&c1.tool_call_id), second.bind(&c2.tool_call_id));
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(a.diagnostic().workspace, b.diagnostic().workspace);
    assert_ne!(a.diagnostic().caller, b.diagnostic().caller);
    assert_ne!(a.fence_hash(), b.fence_hash());
    assert_ne!(a.diagnostic().tip, b.diagnostic().tip);
}
