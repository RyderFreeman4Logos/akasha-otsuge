use super::*;
use crate::adapter::{BindingLevel, CallerProof, CapabilityReport, RuntimeBinding, SelfTest};
use crate::domain::{
    Ingress, MemoryDraft, MemoryKind, MemoryWrite, Policy, SourceDraft, SourceSpan,
};
use crate::editor::{Feedback, Target};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

fn id() -> String {
    format!("editor/{}", hash(b"crash"))
}

async fn setup(root: &std::path::Path, settings: &str, checkpoint: bool) {
    let (service, _) = tests::service_at(root, settings).await;
    let domain = Domain::new(&service.storage);
    let scope = owner_scope();
    let ingress = Ingress::tool("fixture", "build", scope.clone()).unwrap();
    let text = "The fixture passed on Linux.";
    let source = domain
        .record_source(
            "source",
            &SourceDraft {
                event_id: "source".into(),
                scope: scope.clone(),
                text: text.into(),
                reported_role: None,
                event_time_ms: None,
                observed_time_ms: 1,
            },
            &ingress,
            1,
        )
        .await
        .unwrap();
    let span = SourceSpan {
        event_id: "source".into(),
        revision: source.commit_seq,
        content_sha256: hash(text.as_bytes()),
        start: 0,
        end: text.len(),
    };
    let mut draft = MemoryDraft {
        kind: MemoryKind::Fact,
        scope: scope.clone(),
        claim: text.into(),
        rationale: "controlled build".into(),
        conditions: vec!["Linux".into()],
        assumptions: vec![],
        source_spans: vec![span.clone()],
        valid_from_ms: None,
        valid_until_ms: None,
    };
    let memory = domain
        .propose(
            &MemoryWrite {
                request_id: "seed".into(),
                memory_id: "editable".into(),
                expected_revision: None,
                draft: draft.clone(),
            },
            &ingress,
            &Policy {
                revision: "fixture".into(),
                config_generation: 1,
                mode: crate::config::ReviewMode::Automatic,
                scope: scope.clone(),
                adopt_explicit_decisions: false,
            },
        )
        .await
        .unwrap();
    Invocation::bind(
        &service.storage,
        RuntimeBinding {
            invocation_id: "bound".into(),
            runtime_instance: "fixture".into(),
            capability: CapabilityReport {
                host: crate::config::Host::Codex,
                version: Some("controlled".into()),
                executable: Some(std::env::current_exe().unwrap()),
                adapter_version: "1".into(),
                binding: BindingLevel::ExactTurn,
                native_children: false,
                history: crate::capture::CoverageLevel::Unavailable,
                machine_protocol: None,
                cancellation: true,
                outer_timeout: true,
                capabilities: BTreeSet::new(),
                self_test: SelfTest::NotRun,
            },
            caller: crate::capture::SourceIdentity {
                adapter: "fixture".into(),
                profile: "default".into(),
                session: "s".into(),
                thread: "s".into(),
                agent: "a".into(),
            },
            principal: principal(),
            scope: scope.clone(),
            proof: CallerProof::RegisteredRoot { thread: "s".into() },
            branch: "s".into(),
            turn_id: Some("turn".into()),
            tool_call_id: Some("call".into()),
            requested_cursor: 0,
            available_cursor: 0,
            tip: None,
            capture_seq: service.storage.sequence().unwrap(),
            workspace: root.into(),
            config_generation: 1,
        },
    )
    .await
    .unwrap();
    let input = Feedback {
        invocation_id: "bound".into(),
        executor: "editor".into(),
        targets: vec![Target {
            memory_id: "editable".into(),
            expected_revision: memory.commit_seq,
        }],
        feedback: "Verify the platform boundary".into(),
        expires_at_ms: ipc::now_ms().unwrap() + 60_000,
    };
    service
        .submit_editor("crash".into(), input.clone())
        .await
        .unwrap();
    if checkpoint {
        draft.claim = "Only the controlled fixture passed on Linux.".into();
        let patch = crate::domain::MemoryPatch {
            scope,
            changes: vec![crate::domain::MemoryEdit {
                memory_id: "editable".into(),
                expected_revision: memory.commit_seq,
                draft,
            }],
            reason: "Narrow the claim".into(),
            evidence: vec![span.clone()],
            expires_at_ms: input.expires_at_ms,
        };
        let answer = json!({"status":"found","answer":serde_json::to_string(&json!({"classification":"memory_error","reason":"Narrow the claim","patch":patch})).unwrap(),"sources":[span],"memories":[{"memory_id":"editable","revision":memory.commit_seq}],"conditions":[],"conflicts":[],"gaps":[]});
        // Controlled durable-response fixture; the subprocess tests publication, not model quality.
        let ready = json!({"schema_version":1,"origin":"editor","answer":answer,"investigation_gaps":["INDEX_NOT_READY"],"trace":{"runs":[],"workspace":null},"executor":"editor","generator":{"runner":"executor/editor","model":"controlled","prompt_version":crate::editor::PROMPT_VERSION}});
        let pointer = models::save_result(&service.storage, &ready, 1)
            .await
            .unwrap();
        let (revision, mut job) = service.job(&id()).unwrap();
        job.result = Some(json!({"editor_checkpoint":pointer}));
        service
            .save_job(&job, Some(revision), vec![])
            .await
            .unwrap();
    }
    service.storage.close().await.unwrap();
}

#[test]
#[ignore = "owned subprocess launched by the editor crash supervisor"]
fn editor_crash_child() {
    let root = PathBuf::from(std::env::var_os("AKASHA_TEST_CRASH_ROOT").unwrap());
    let settings = std::fs::read_to_string(root.join("settings.toml")).unwrap();
    runtime().block_on(async {
        let (service, _) = tests::service_at(&root, &settings).await;
        service.recover().await.unwrap();
        assert!(service.claim(&id(), 0).await.unwrap());
        service.execute_job(id()).await.unwrap();
        panic!("editor missed the requested crash checkpoint");
    });
}

#[test]
fn editor_checkpoint_and_committed_patch_survive_sigkill_without_runner_replay() {
    for phase in ["editor-response", "editor-applied"] {
        let root =
            std::env::temp_dir().join(format!("akasha-editor-{}-{phase}", std::process::id()));
        private_dir(&root).unwrap();
        let _cleanup = tests::Fixture(root.clone());
        let settings = format!(
            "[review]\nmode='automatic'\n[executors.editor]\nkind='codex'\nbinary='/missing-native-must-not-run'\nnative_home={root:?}\n"
        );
        std::fs::write(root.join("settings.toml"), &settings).unwrap();
        runtime().block_on(setup(&root, &settings, true));
        crate::storage::crash_tests::kill_at(
            &root,
            phase,
            "daemon::jobs::consult::editor_tests::editor_crash_child",
        );
        runtime().block_on(async {
            let (service, _) = tests::service_at(&root, &settings).await;
            let original = service.job(&id()).unwrap().1;
            assert_eq!(original.state, "running");
            let before = service.storage.receipt(&id()).unwrap();
            assert_eq!(before.is_some(), phase == "editor-applied");
            service.recover().await.unwrap();
            assert!(service.claim(&id(), 0).await.unwrap());
            service.clone().execute_job(id()).await.unwrap();
            let resumed = service.job(&id()).unwrap().1;
            assert_eq!(resumed.state, "succeeded", "{:?}", resumed.error);
            assert_eq!(resumed.started_at_ms, original.started_at_ms);
            assert_eq!(
                resumed.execution_deadline_ms,
                original.execution_deadline_ms
            );
            let receipt = service.storage.receipt(&id()).unwrap().unwrap();
            if let Some(before) = before {
                assert_eq!(receipt, before);
            }
            let latest = Domain::new(&service.storage)
                .memory("editable")
                .unwrap()
                .unwrap();
            assert_eq!(latest.record.authority, crate::domain::Authority::Candidate);
            assert_eq!(
                resumed.result.as_ref().unwrap()["gaps"],
                json!(["INDEX_NOT_READY"])
            );
            assert_eq!(latest.revision.commit_seq, receipt.commit_seq);
            let initial = latest.previous.unwrap();
            assert!(
                service
                    .storage
                    .revision(&latest.revision.key, initial)
                    .unwrap()
                    .unwrap()
                    .previous
                    .is_none()
            );
            service.storage.close().await.unwrap();
        });
    }
}

#[tokio::test]
async fn editor_legacy_checkpoint_without_coverage_stays_for_review() {
    let root = std::env::temp_dir().join(format!("akasha-editor-legacy-{}", std::process::id()));
    private_dir(&root).unwrap();
    let _cleanup = tests::Fixture(root.clone());
    let settings = format!(
        "[review]\nmode='automatic'\n[executors.editor]\nkind='codex'\nbinary='/missing-native-must-not-run'\nnative_home={root:?}\n"
    );
    setup(&root, &settings, true).await;
    let (service, _) = tests::service_at(&root, &settings).await;
    let (revision, mut job) = service.job(&id()).unwrap();
    let pointer = &job.result.as_ref().unwrap()["editor_checkpoint"];
    let mut ready = models::load_result(&service.storage, pointer, revision).unwrap();
    ready.as_object_mut().unwrap().remove("investigation_gaps");
    job.result = Some(
        json!({"editor_checkpoint":models::save_result(&service.storage, &ready, 1).await.unwrap()}),
    );
    service
        .save_job(&job, Some(revision), vec![])
        .await
        .unwrap();
    service.storage.close().await.unwrap();
    drop(service);
    let (service, _) = tests::service_at(&root, &settings).await;
    service.recover().await.unwrap();
    assert!(service.claim(&id(), 0).await.unwrap());
    service.clone().execute_job(id()).await.unwrap();
    let done = service.job(&id()).unwrap().1;
    assert_eq!(done.state, "succeeded", "{:?}", done.error);
    assert_eq!(
        Domain::new(&service.storage)
            .memory("editable")
            .unwrap()
            .unwrap()
            .record
            .authority,
        crate::domain::Authority::Candidate
    );
    assert_eq!(
        done.result.unwrap()["gaps"],
        json!(["investigation_coverage_unavailable"])
    );
    service.storage.close().await.unwrap();
}

#[tokio::test]
async fn editor_without_a_response_checkpoint_requires_explicit_retry_after_restart() {
    let root = std::env::temp_dir().join(format!("akasha-editor-ambiguous-{}", std::process::id()));
    private_dir(&root).unwrap();
    let _cleanup = tests::Fixture(root.clone());
    let settings = format!(
        "[executors.editor]\nkind='codex'\nbinary='/missing-native-must-not-run'\nnative_home={root:?}\n"
    );
    setup(&root, &settings, false).await;
    let (service, _) = tests::service_at(&root, &settings).await;
    assert!(service.claim(&id(), 0).await.unwrap());
    service.storage.close().await.unwrap();
    drop(service);
    let (service, _) = tests::service_at(&root, &settings).await;
    service.recover().await.unwrap();
    let job = service.job(&id()).unwrap().1;
    assert_eq!(job.state, "failed");
    assert_eq!(job.error.as_deref(), Some("RUNNER_INTERRUPTED"));
    assert!(service.storage.receipt(&id()).unwrap().is_none());
    service.storage.close().await.unwrap();
}
