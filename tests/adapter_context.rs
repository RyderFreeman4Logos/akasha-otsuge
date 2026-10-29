#[path = "support/executable.rs"]
mod executable;

use akasha_otsuge::{
    adapter::*,
    capture::*,
    config::{Host, Paths, resolve},
    domain::{Domain, Ingress, Scope},
    storage::Storage,
};
use std::{
    collections::BTreeSet,
    path::PathBuf,
    sync::atomic::{AtomicU64, Ordering},
};

#[path = "support/adapter_matrix.rs"]
mod matrix;

#[path = "support/claude_runner.rs"]
mod claude_runner;

#[path = "support/runner_bridge_deadlines.rs"]
mod bridge_deadlines;

struct Fixture(PathBuf);
impl Fixture {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let p = std::env::temp_dir().join(format!(
            "akasha-adapter-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir(&p).unwrap();
        Self(p)
    }
    async fn open(&self) -> Storage {
        Storage::open(
            &resolve(
                "",
                None,
                "",
                &Paths {
                    data_dir: self.0.join("data"),
                    socket_path: self.0.join("service.sock"),
                },
            )
            .unwrap(),
        )
        .await
        .unwrap()
    }
}
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn scope() -> Scope {
    Scope {
        user: "user".into(),
        project: Some("project".into()),
        branch_or_version: None,
        session: None,
    }
}
fn source(thread: &str) -> SourceIdentity {
    SourceIdentity {
        adapter: "fixture".into(),
        profile: "default".into(),
        session: "family".into(),
        thread: thread.into(),
        agent: thread.into(),
    }
}
fn report() -> CapabilityReport {
    CapabilityReport {
        host: Host::Codex,
        version: Some("fixture-1".into()),
        executable: Some("/fixture/codex".into()),
        adapter_version: "1".into(),
        binding: BindingLevel::ExactTurn,
        native_children: true,
        history: CoverageLevel::VisibleText,
        machine_protocol: Some("jsonl-1".into()),
        cancellation: true,
        outer_timeout: true,
        capabilities: BTreeSet::from(["context.read".into()]),
        self_test: SelfTest::NotRun,
    }
}
async fn event(
    store: &Storage,
    thread: &str,
    id: &str,
    text: &str,
    parent: Option<EventRef>,
    fork: bool,
) -> EventRef {
    let binding = CaptureBinding::from_source(
        source(thread),
        scope(),
        "events",
        CoverageLevel::VisibleText,
    )
    .unwrap();
    let draft = EventDraft {
        event_id: id.into(),
        source_version: "1".into(),
        runtime_instance: "runtime".into(),
        branch: thread.into(),
        fork_cutoff: if fork { parent.clone() } else { None },
        parent,
        event_time_ms: None,
        observed_time_ms: 1,
        visibility: Visibility::User,
        kind: EventKind::Message {
            message_id: id.into(),
            phase: MessagePhase::Final,
        },
        text: text.into(),
    };
    Capture::new(store)
        .ingest(
            &binding,
            &draft,
            text.as_bytes(),
            &Ingress::unverified("fixture", "runtime", scope()).unwrap(),
            None,
            1,
        )
        .await
        .unwrap()
        .event
        .reference
}
fn runtime(f: &Fixture, store: &Storage, thread: &str, tip: EventRef, id: &str) -> RuntimeBinding {
    RuntimeBinding {
        invocation_id: id.into(),
        runtime_instance: "runtime".into(),
        capability: report(),
        caller: source(thread),
        principal: "reader".into(),
        scope: scope(),
        proof: CallerProof::RegisteredRoot {
            thread: thread.into(),
        },
        branch: thread.into(),
        turn_id: Some("turn".into()),
        tool_call_id: Some(id.into()),
        requested_cursor: 10,
        available_cursor: 10,
        tip: Some(tip),
        capture_seq: store.sequence().unwrap(),
        workspace: f.0.clone(),
        config_generation: 1,
    }
}

#[tokio::test]
async fn invocation_is_durable_immutable_and_concurrent_same_cwd_callers_do_not_mix() {
    let f = Fixture::new();
    let store = f.open().await;
    Domain::new(&store)
        .set_read_grant("grant", "reader", Some(&scope()), None, 1)
        .await
        .unwrap();
    let a = event(&store, "a", "a-1", "private-a", None, false).await;
    let b = event(&store, "b", "b-1", "private-b", None, false).await;
    let one = runtime(&f, &store, "a", a, "call-a");
    let two = runtime(&f, &store, "b", b, "call-b");
    let before = store.sequence().unwrap();
    let (a, b) = tokio::join!(
        Invocation::bind(&store, one.clone()),
        Invocation::bind(&store, two)
    );
    let a = a.unwrap();
    let b = b.unwrap();
    assert_eq!(
        store.sequence().unwrap(),
        before + 2,
        "persist before research starts"
    );
    assert_ne!(a.id(), b.id());
    assert_eq!(a.diagnostic().caller.thread, "a");
    assert_eq!(b.diagnostic().caller.thread, "b");
    assert_eq!(
        Invocation::bind(&store, one.clone())
            .await
            .unwrap()
            .revision(),
        a.revision()
    );
    let mut changed = one;
    changed.tool_call_id = Some("other".into());
    assert_eq!(
        Invocation::bind(&store, changed).await.unwrap_err().code,
        ErrorCode::BindingConflict
    );
    store.close().await.unwrap();
    let reopened = f.open().await;
    assert_eq!(
        Invocation::load(&reopened, "call-a", "reader")
            .await
            .unwrap()
            .diagnostic()
            .caller
            .thread,
        "a"
    );
    assert_eq!(
        Invocation::load(&reopened, "call-a", "other-reader")
            .await
            .unwrap_err()
            .code,
        ErrorCode::ReadDenied
    );
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn binding_rejects_missing_ambiguous_future_and_false_root_identity() {
    let f = Fixture::new();
    let store = f.open().await;
    Domain::new(&store)
        .set_read_grant("grant", "reader", Some(&scope()), None, 1)
        .await
        .unwrap();
    let tip = event(&store, "a", "event", "private", None, false).await;
    let good = runtime(&f, &store, "a", tip, "call");
    for code in [
        ErrorCode::CallerAmbiguous,
        ErrorCode::BindingUnavailable,
        ErrorCode::AdapterUnsupported,
        ErrorCode::ProtocolInvalid,
    ] {
        let mut bad = good.clone();
        match code {
            ErrorCode::CallerAmbiguous => bad.proof = CallerProof::Unknown,
            ErrorCode::BindingUnavailable => bad.caller.thread.clear(),
            ErrorCode::AdapterUnsupported => bad.capability.binding = BindingLevel::SessionOnly,
            _ => bad.available_cursor = 11,
        }
        assert_eq!(Invocation::bind(&store, bad).await.unwrap_err().code, code);
    }
    let mut wrong = good.clone();
    wrong.proof = CallerProof::RegisteredRoot {
        thread: "parent".into(),
    };
    assert_eq!(
        Invocation::bind(&store, wrong).await.unwrap_err().code,
        ErrorCode::CallerAmbiguous
    );
    let mut lag = good;
    lag.available_cursor = 9;
    let inv = Invocation::bind(&store, lag).await.unwrap();
    assert_eq!(inv.coverage(), FenceCoverage::Lagging);
    store.close().await.unwrap();
}

fn lease(inv: &Invocation) -> RunLease {
    use akasha_otsuge::config::deadline::{Deadline, Layer};
    RunBudget::new(
        inv.id(),
        &inv.fence_hash(),
        std::sync::Arc::new(tokio::sync::Semaphore::new(3)),
        BudgetLimits::default(),
        Deadline::start(Layer::Workflow, std::time::Duration::from_secs(30), None).unwrap(),
    )
    .unwrap()
    .start()
    .unwrap()
}

#[tokio::test]
async fn description_reports_actual_binding_precision_even_on_capable_hosts() {
    let f = Fixture::new();
    let store = f.open().await;
    Domain::new(&store)
        .set_read_grant("grant", "reader", Some(&scope()), None, 1)
        .await
        .unwrap();
    let tip = event(&store, "root", "event", "original", None, false).await;
    for (id, exact, expected) in [
        ("session", false, "session_only"),
        ("turn", true, "exact_turn"),
    ] {
        let mut binding = runtime(&f, &store, "root", tip.clone(), id);
        if !exact {
            binding.turn_id = None;
            binding.tool_call_id = None;
        }
        let inv = Invocation::bind(&store, binding).await.unwrap();
        let permit = lease(&inv);
        let mut context = CapturedContext::new(&store, inv).unwrap();
        let value = serde_json::to_value(
            context
                .execute(&permit, ContextRequest::Describe {})
                .await
                .unwrap(),
        )
        .unwrap();
        assert_eq!(value["value"]["binding_level"], expected);
    }
    store.close().await.unwrap();
}
async fn list(
    context: &mut impl ContextSource,
    lease: &RunLease,
    cursor: Option<String>,
    limit: usize,
) -> ContextPage {
    match context
        .execute(lease, ContextRequest::List { cursor, limit })
        .await
        .unwrap()
    {
        ContextResponse::Page(page) => page,
        _ => panic!("expected page"),
    }
}

#[tokio::test]
async fn context_handles_follow_only_fenced_ancestry_and_recheck_live_grants() {
    let f = Fixture::new();
    let store = f.open().await;
    let grant = Domain::new(&store)
        .set_read_grant("grant", "reader", Some(&scope()), None, 1)
        .await
        .unwrap();
    let p0 = event(&store, "parent", "p0", "inherited parent", None, false).await;
    let c0 = event(
        &store,
        "child",
        "c0",
        "child secret",
        Some(p0.clone()),
        true,
    )
    .await;
    event(
        &store,
        "parent",
        "p1",
        "parent after spawn",
        Some(p0.clone()),
        false,
    )
    .await;
    let sibling = event(&store, "sibling", "s0", "sibling secret", Some(p0), true).await;
    event(
        &store,
        "child",
        "c1",
        "future child message",
        Some(c0.clone()),
        false,
    )
    .await;
    let mut binding = runtime(&f, &store, "child", c0, "child-call");
    binding.proof = CallerProof::RegisteredChild {
        thread: "child".into(),
        parent: "parent".into(),
    };
    let inv = Invocation::bind(&store, binding).await.unwrap();
    let permit = lease(&inv);
    let mut context = CapturedContext::new(&store, inv.clone()).unwrap();
    let grand_tip = event(
        &store,
        "grandchild",
        "g0",
        "grandchild secret",
        inv.diagnostic().tip.clone(),
        true,
    )
    .await;
    let mut grand_binding = runtime(&f, &store, "grandchild", grand_tip, "grandchild-call");
    grand_binding.proof = CallerProof::RegisteredChild {
        thread: "grandchild".into(),
        parent: "child".into(),
    };
    let grand_inv = Invocation::bind(&store, grand_binding).await.unwrap();
    let grand_lease = lease(&grand_inv);
    let mut grand_context = CapturedContext::new(&store, grand_inv).unwrap();
    let grand_page = list(&mut grand_context, &grand_lease, None, 16).await;
    assert_eq!(
        grand_page
            .events
            .iter()
            .map(|e| e.reference.event_id.as_str())
            .collect::<Vec<_>>(),
        vec!["g0", "c0", "p0"],
        "each ancestor keeps its own cutoff even when later parent records precede this call"
    );
    let page = list(&mut context, &permit, None, 1).await;
    assert_eq!(page.events.len(), 1);
    assert_eq!(page.events[0].reference.event_id, "c0");
    let handle = page.events[0].handle.clone();
    let next = list(&mut context, &permit, page.next, 16).await;
    assert_eq!(
        next.events
            .iter()
            .map(|v| v.reference.event_id.as_str())
            .collect::<Vec<_>>(),
        vec!["p0"]
    );
    assert!(next.next.is_none());
    let result = context
        .execute(
            &permit,
            ContextRequest::Read {
                handle: handle.clone(),
                start: 0,
                max_bytes: 4096,
            },
        )
        .await
        .unwrap();
    let ContextResponse::Text(text) = result else {
        panic!("expected text")
    };
    assert_eq!(text.text, "child secret");
    assert_eq!(text.total_bytes, 12);
    assert_eq!(
        context
            .execute(
                &permit,
                ContextRequest::Read {
                    handle: "forged-sibling".into(),
                    start: 0,
                    max_bytes: 4096
                }
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::ReadDenied
    );
    let other = Invocation::bind(
        &store,
        runtime(&f, &store, "sibling", sibling, "other-call"),
    )
    .await
    .unwrap();
    let other_permit = lease(&other);
    let mut other = CapturedContext::new(&store, other).unwrap();
    assert_eq!(
        other
            .execute(
                &other_permit,
                ContextRequest::Read {
                    handle: handle.clone(),
                    start: 0,
                    max_bytes: 4096
                }
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::ReadDenied
    );
    let mut narrow = scope();
    narrow.session = Some("another-session".into());
    Domain::new(&store)
        .set_read_grant("shrink", "reader", Some(&narrow), Some(grant.commit_seq), 2)
        .await
        .unwrap();
    assert_eq!(
        context
            .execute(
                &permit,
                ContextRequest::Read {
                    handle,
                    start: 0,
                    max_bytes: 4096
                }
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::ReadDenied
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn context_reports_missing_history_and_search_never_reads_future_messages() {
    let f = Fixture::new();
    let store = f.open().await;
    Domain::new(&store)
        .set_read_grant("grant", "reader", Some(&scope()), None, 1)
        .await
        .unwrap();
    let missing = EventRef {
        source: source("root"),
        event_id: "deleted-original".into(),
    };
    let tip = event(
        &store,
        "root",
        "tip",
        "retained 中文 original",
        Some(missing),
        false,
    )
    .await;
    event(
        &store,
        "root",
        "later",
        "future needle",
        Some(tip.clone()),
        false,
    )
    .await;
    let inv = Invocation::bind(&store, runtime(&f, &store, "root", tip, "call"))
        .await
        .unwrap();
    let permit = lease(&inv);
    let mut context = CapturedContext::new(&store, inv).unwrap();
    let page = list(&mut context, &permit, None, 16).await;
    assert_eq!(page.gaps, vec![ErrorCode::HistoryGap]);
    assert_eq!(page.events.len(), 1);
    let response = context
        .execute(
            &permit,
            ContextRequest::Search {
                cursor: None,
                query: "future needle".into(),
                limit: 16,
            },
        )
        .await
        .unwrap();
    let ContextResponse::Page(result) = response else {
        panic!("expected page")
    };
    assert!(result.events.is_empty());
    assert_eq!(result.gaps, vec![ErrorCode::HistoryGap]);
    let handle = page.events[0].handle.clone();
    assert_eq!(
        context
            .execute(
                &permit,
                ContextRequest::Read {
                    handle: handle.clone(),
                    start: 10,
                    max_bytes: 1
                }
            )
            .await
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument,
        "do not split UTF-8"
    );
    let result = context
        .execute(
            &permit,
            ContextRequest::Read {
                handle,
                start: 0,
                max_bytes: 9,
            },
        )
        .await
        .unwrap();
    let ContextResponse::Text(text) = result else {
        panic!("expected text")
    };
    assert_eq!(text.text, "retained ");
    assert!(text.truncated);
    store.close().await.unwrap();
}

#[tokio::test]
async fn code_reads_pin_workspace_reject_escape_and_report_changed_content() {
    let f = Fixture::new();
    let store = f.open().await;
    Domain::new(&store)
        .set_read_grant("grant", "reader", Some(&scope()), None, 1)
        .await
        .unwrap();
    let workspace = f.0.join("workspace");
    std::fs::create_dir(&workspace).unwrap();
    std::fs::write(f.0.join("secret"), "outside marker").unwrap();
    std::fs::write(workspace.join("file.rs"), "before\nneedle\n").unwrap();
    std::os::unix::fs::symlink("../secret", workspace.join("escape")).unwrap();
    std::os::unix::fs::symlink("file.rs", workspace.join("inside")).unwrap();
    rustix::fs::mkfifoat(
        rustix::fs::CWD,
        workspace.join("pipe"),
        rustix::fs::Mode::RUSR | rustix::fs::Mode::WUSR,
    )
    .unwrap();
    let config = akasha_otsuge::config::ConfigManager::new(
        1,
        resolve(
            &format!("[access]\nread_roots=[{:?}]", workspace),
            None,
            "",
            &Paths {
                data_dir: f.0.join("data"),
                socket_path: f.0.join("service.sock"),
            },
        )
        .unwrap(),
        |_| Ok(()),
    )
    .unwrap();
    let tip = event(&store, "root", "tip", "read code", None, false).await;
    let mut binding = runtime(&f, &store, "root", tip, "code-call");
    binding.workspace = workspace.clone();
    let inv = Invocation::bind(&store, binding).await.unwrap();
    let permit = lease(&inv);
    let mut code = BoundWorkspace::new(&store, inv, &config).unwrap();
    let before = code.read(&permit, "file.rs", 0, 4096).await.unwrap();
    assert_eq!(before.text, "before\nneedle\n");
    assert!(!before.changed);
    assert_eq!(before.sha256.len(), 64);
    for path in ["../secret", "escape", "/etc/passwd", "pipe", "."] {
        assert_eq!(
            code.read(&permit, path, 0, 4096).await.unwrap_err().code,
            ErrorCode::ReadDenied,
            "path {path}"
        );
    }
    assert_eq!(
        code.read(&permit, "inside", 0, 4096).await.unwrap().text,
        before.text
    );
    std::fs::write(workspace.join("file.rs"), "after\nneedle\n").unwrap();
    let after = code.read(&permit, "file.rs", 0, 4096).await.unwrap();
    assert!(after.changed);
    assert_ne!(before.sha256, after.sha256);
    let found = code
        .search(&permit, &["file.rs".into()], "needle")
        .await
        .unwrap();
    assert_eq!(found.matches.len(), 1);
    assert_eq!(found.matches[0].line, 2);
    assert_eq!(found.matches[0].sha256, after.sha256);
    assert!(!found.matches[0].changed_during_read);
    assert!(found.matches[0].observed_at_unix_ms > 0);
    config
        .reload("[access]\nread_roots=[]", None, |_| Ok(()))
        .unwrap();
    assert_eq!(
        code.read(&permit, "file.rs", 0, 4096)
            .await
            .unwrap_err()
            .code,
        ErrorCode::ReadDenied
    );
    store.close().await.unwrap();
}

#[test]
fn context_requests_reject_model_supplied_identity_fields() {
    for request in [
        serde_json::json!({"operation":"describe"}),
        serde_json::json!({"operation":"list","limit":1}),
        serde_json::json!({"operation":"read","handle":"issued","start":0,"max_bytes":32}),
        serde_json::json!({"operation":"search","query":"q","limit":1}),
        serde_json::json!({"operation":"memory_search","query":"q"}),
        serde_json::json!({"operation":"memory_get","memory_id":"memory","revision":1}),
        serde_json::json!({"operation":"code_search","paths":["src/lib.rs"],"query":"q"}),
        serde_json::json!({"operation":"code_read","path":"src/lib.rs","start":0,"max_bytes":32}),
        serde_json::json!({"operation":"delegate","hint":"Check the alternative"}),
    ] {
        assert!(serde_json::from_value::<ContextRequest>(request.clone()).is_ok());
        for field in ["session_id", "token", "principal", "home"] {
            let mut forged = request.clone();
            forged[field] = serde_json::json!("model-supplied");
            assert!(
                serde_json::from_value::<ContextRequest>(forged).is_err(),
                "{field}"
            );
        }
    }
}
