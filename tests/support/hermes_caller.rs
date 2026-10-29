use super::Fixture;
use akasha_otsuge::{
    adapter::{hermes::*, *},
    capture::*,
    config::Host,
    domain::{Domain, Scope},
    storage::Storage,
};
use std::{os::unix::fs::PermissionsExt, sync::Arc};

fn scope() -> Scope {
    Scope {
        user: "user".into(),
        project: Some("project".into()),
        branch_or_version: None,
        session: None,
    }
}
fn native(f: &Fixture) -> (HermesRegistration, HermesCall) {
    let home = f.0.join("profile");
    std::fs::create_dir(&home).unwrap();
    let mut raw: serde_json::Value =
        serde_json::from_str(include_str!("../fixtures/hermes/0.22.5/root-before.jsonl")).unwrap();
    raw["cwd"] = serde_json::json!(f.0);
    raw["messages"][2]["tool_calls"][0]["function"]["name"] = "akasha_consult".into();
    raw["messages"][2]["tool_calls"][0]["function"]["arguments"] = "{\"hint\":\"review\"}".into();
    let path = f.0.join("snapshot.jsonl");
    std::fs::write(&path, serde_json::to_vec(&raw).unwrap()).unwrap();
    std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600)).unwrap();
    let session = raw["id"].as_str().unwrap().to_string();
    let r = HermesRegistration {
        capability: CapabilityReport {
            host: Host::Hermes,
            version: Some("0.22.5+1914.gfdb04a0".into()),
            executable: Some("/fixture/hermes".into()),
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
            adapter: "hermes".into(),
            profile: profile_key(&home),
            session: session.clone(),
            thread: session.clone(),
            agent: "main".into(),
        },
        scope: scope(),
        principal: "reader".into(),
        native_profile: "custom".into(),
        profile_home: home,
        snapshot: path,
        workspace: f.0.canonicalize().unwrap(),
        task_id: "native-task".into(),
        parent_session: None,
    };
    let c = HermesCall {
        session_id: session,
        task_id: r.task_id.clone(),
        turn_id: "native-turn".into(),
        tool_call_id: "call_alpha".into(),
        profile_home: r.profile_home.clone(),
        cwd: r.workspace.clone(),
    };
    (r, c)
}
async fn store(f: &Fixture) -> Arc<Storage> {
    let s = Arc::new(f.open().await);
    Domain::new(&s)
        .set_read_grant("grant", "reader", Some(&scope()), None, 1)
        .await
        .unwrap();
    s
}

#[tokio::test]
async fn exact_call_fence_omits_later_results_and_retry_survives_source_loss() {
    let f = Fixture::new();
    let s = store(&f).await;
    let grant_seq = s.sequence().unwrap();
    let (r, c) = native(&f);
    let inv = HermesCaller::new(s.clone(), r.clone(), c.clone(), 1)
        .bind(&c.tool_call_id)
        .await
        .unwrap();
    assert_eq!(inv.coverage(), FenceCoverage::CompleteToFence);
    assert_eq!(inv.diagnostic().requested_cursor, 4);
    assert_eq!(inv.diagnostic().available_cursor, 4);
    assert_eq!(inv.diagnostic().turn_id.as_deref(), Some("native-turn"));
    let b = CaptureBinding::from_source(
        r.source.clone(),
        scope(),
        "reader",
        CoverageLevel::VisibleText,
    )
    .unwrap();
    let capture = Capture::new(&s);
    let page = capture
        .lineage(
            &b,
            inv.diagnostic().tip.as_ref().unwrap(),
            inv.diagnostic().capture_seq,
            64,
        )
        .unwrap();
    assert_eq!(page.events.len(), 4);
    let text = page
        .events
        .iter()
        .map(|e| capture.text(e).unwrap())
        .collect::<Vec<_>>()
        .join("\n");
    assert!(text.contains("Original public preference"));
    assert!(text.contains("akasha_consult"));
    assert!(!text.contains("Synthetic file contents"));
    assert!(!text.contains("Answer before interruption"));
    std::fs::remove_file(&r.snapshot).unwrap();
    let retry = HermesCaller::new(s.clone(), r.clone(), c.clone(), 2)
        .bind(&c.tool_call_id)
        .await
        .unwrap();
    assert_eq!(retry.revision(), inv.revision());
    assert_eq!(retry.fence_hash(), inv.fence_hash());
    Domain::new(&s)
        .set_read_grant("revoke", "reader", None, Some(grant_seq), 2)
        .await
        .unwrap();
    assert_eq!(
        HermesCaller::new(s.clone(), r, c.clone(), 2)
            .bind(&c.tool_call_id)
            .await
            .unwrap_err()
            .code,
        ErrorCode::ReadDenied
    );
    s.close().await.unwrap();
}

#[tokio::test]
async fn foreign_profile_session_task_or_missing_call_cannot_read_the_root() {
    for case in ["profile", "session", "task", "call", "cwd", "duplicate"] {
        let f = Fixture::new();
        let s = store(&f).await;
        let (r, mut c) = native(&f);
        match case {
            "profile" => c.profile_home = f.0.join("other-profile"),
            "session" => c.session_id = "foreign-session".into(),
            "task" => c.task_id = "foreign-task".into(),
            "call" => c.tool_call_id = "foreign-call".into(),
            "cwd" => c.cwd = f.0.join("other-workspace"),
            "duplicate" => {
                let mut v: serde_json::Value =
                    serde_json::from_slice(&std::fs::read(&r.snapshot).unwrap()).unwrap();
                let mut row = v["messages"][2].clone();
                row["id"] = 6.into();
                row["message_uid"] = "different-native-uid".into();
                v["messages"].as_array_mut().unwrap().push(row);
                std::fs::write(&r.snapshot, serde_json::to_vec(&v).unwrap()).unwrap();
            }
            _ => unreachable!(),
        }
        let at = s.sequence().unwrap();
        assert_eq!(
            HermesCaller::new(s.clone(), r, c.clone(), 1)
                .bind(&c.tool_call_id)
                .await
                .unwrap_err()
                .code,
            if case == "cwd" {
                ErrorCode::BindingUnavailable
            } else {
                ErrorCode::CallerAmbiguous
            },
            "{case}"
        );
        assert_eq!(
            s.sequence().unwrap(),
            at,
            "invalid identity wrote history: {case}"
        );
        s.close().await.unwrap();
    }
}

#[tokio::test]
async fn simultaneous_homes_with_identical_native_labels_and_ids_remain_isolated() {
    let first = Fixture::new();
    let second = Fixture::new();
    let s = store(&first).await;
    let (r1, c1) = native(&first);
    let (r2, c2) = native(&second);
    assert_eq!(r1.native_profile, r2.native_profile);
    assert_eq!(c1.session_id, c2.session_id);
    let one = HermesCaller::new(s.clone(), r1, c1.clone(), 1);
    let two = HermesCaller::new(s.clone(), r2, c2.clone(), 1);
    let (a, b) = tokio::join!(one.bind(&c1.tool_call_id), two.bind(&c2.tool_call_id));
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_ne!(a.id(), b.id());
    assert_ne!(a.diagnostic().caller.profile, b.diagnostic().caller.profile);
    assert_ne!(a.diagnostic().tip, b.diagnostic().tip);
    assert_eq!(a.diagnostic().caller.session, b.diagnostic().caller.session);
    s.close().await.unwrap();
}

#[tokio::test]
async fn child_proof_uses_its_native_parent_without_substituting_root_history() {
    let f = Fixture::new();
    let s = store(&f).await;
    let (mut r, c) = native(&f);
    let mut v: serde_json::Value =
        serde_json::from_slice(&std::fs::read(&r.snapshot).unwrap()).unwrap();
    v["source"] = "delegate_task".into();
    v["parent_session_id"] = "native-parent".into();
    std::fs::write(&r.snapshot, serde_json::to_vec(&v).unwrap()).unwrap();
    r.parent_session = Some("native-parent".into());
    r.source.agent = r.source.session.clone();
    let inv = HermesCaller::new(s.clone(), r, c.clone(), 1)
        .bind(&c.tool_call_id)
        .await
        .unwrap();
    assert_eq!(
        inv.diagnostic().proof,
        CallerProof::RegisteredChild {
            thread: c.session_id.clone(),
            parent: "native-parent".into(),
        }
    );
    assert_eq!(
        inv.diagnostic().tip.as_ref().unwrap().source.session,
        c.session_id
    );
    inv.diagnostic()
        .capability
        .negotiate(&["binding.children"], &[])
        .unwrap();
    s.close().await.unwrap();
}
