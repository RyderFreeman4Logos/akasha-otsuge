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
        let ready = json!({"schema_version":1,"origin":"editor","answer":answer,"investigation_gaps":["INDEX_NOT_READY", "HISTORY_GAP", "context_read_budget", "child_limit", "unresolved_conflict"],"trace":{"runs":[],"workspace":null},"executor":"editor","generator":{"runner":"executor/editor","model":"controlled","prompt_version":crate::editor::PROMPT_VERSION}});
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
        if std::env::var("AKASHA_TEST_CRASH_PHASE").as_deref() == Ok("incident-admitted") {
            service.advance_incidents().await.unwrap();
            panic!("incident missed the admission checkpoint");
        }
        assert!(service.claim(&id(), 0).await.unwrap());
        service.execute_job(id()).await.unwrap();
        panic!("editor missed the requested crash checkpoint");
    });
}

#[test]
fn incident_admitted_job_survives_sigkill_and_keeps_its_original_queue_deadline() {
    let root =
        std::env::temp_dir().join(format!("akasha-incident-admission-{}", std::process::id()));
    private_dir(&root).unwrap();
    let _cleanup = tests::Fixture(root.clone());
    let settings = format!(
        "[timeouts]\nqueue='5s'\n[incidents]\nexecutor='editor'\ntriggers=['user_correction']\n[executors.editor]\nkind='codex'\nbinary='/missing-native-must-not-run'\nnative_home={root:?}\n"
    );
    std::fs::write(root.join("settings.toml"), &settings).unwrap();
    runtime().block_on(async {
        setup(&root,&settings,false).await;
        let (service, _) = tests::service_at(&root,&settings).await;
        service.submit_outcome(serde_json::from_value(json!({
            "event_id":"admission-gap","invocation_id":"bound","source":"controlled-crash","severity":"warning",
            "occurred_at_ms":ipc::now_ms().unwrap(),"execution":null,"artifacts":[],"logs":[],"receipt_jobs":[],"claims":[],
            "incident":{"kind":"user_correction","key":"crash-window","reason":"Retain the original admitted job across the missing incident link."}
        })).unwrap()).await.unwrap();
        // Persist the admitted-but-unlinked state before restart can outlast its queue window.
        let outcome = service.read_outcome("admission-gap").await.unwrap();
        let record = service.incident_record(&outcome.incident.unwrap()).unwrap().1;
        let accepted = service.submit_incident_editor(&record).await.unwrap();
        let admitted = service.job(accepted["id"].as_str().unwrap()).unwrap().1;
        assert_eq!(admitted.accepted_at_ms, record.first_report_at_ms);
        assert_eq!(admitted.queue_deadline_ms, admitted.accepted_at_ms + 5000);
        service.storage.close().await.unwrap();
        tokio::time::sleep(Duration::from_millis(
            admitted.queue_deadline_ms.saturating_sub(ipc::now_ms().unwrap()) + 1,
        )).await;
    });
    crate::storage::crash_tests::kill_at(
        &root,
        "incident-admitted",
        "daemon::jobs::consult::editor_tests::editor_crash_child",
    );
    runtime().block_on(async {
        let (service, _) = tests::service_at(&root, &settings).await;
        let outcome = service.read_outcome("admission-gap").await.unwrap();
        let incident_id = outcome.incident.unwrap();
        let before = service.incident_record(&incident_id).unwrap().1;
        assert!(before.job_id.is_none());
        let job_id = format!(
            "editor/{}",
            hash(format!("incident/{incident_id}").as_bytes())
        );
        let admitted = service.job(&job_id).unwrap().1;
        assert_eq!(admitted.accepted_at_ms, before.first_report_at_ms);
        assert_eq!(admitted.queue_deadline_ms, admitted.accepted_at_ms + 5000);
        assert!(ipc::now_ms().unwrap() >= admitted.queue_deadline_ms);
        service.recover().await.unwrap();
        assert!(!service.claim(&job_id, 0).await.unwrap());
        service.advance_incidents().await.unwrap();
        service.advance_incidents().await.unwrap();
        let after = service.incident_record(&incident_id).unwrap().1;
        assert_eq!(
            after.job_id.as_deref(),
            Some(job_id.as_str()),
            "admitted job was orphaned after its queue deadline"
        );
        assert_eq!(after.first_report_at_ms, before.first_report_at_ms);
        assert_eq!(after.expires_at_ms, before.expires_at_ms);
        assert_eq!(
            after.investigation,
            crate::incident::Investigation::Finished
        );
        assert_eq!(after.error.as_deref(), Some("QUEUE_TIMEOUT"));
        let job = service.job(&job_id).unwrap().1;
        assert_eq!(job.attempt, 0);
        assert_eq!(job.queue_deadline_ms, admitted.queue_deadline_ms);
        assert!(service.storage.receipt(&job_id).unwrap().is_none());
        service.storage.close().await.unwrap();
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
                json!([
                    "INDEX_NOT_READY",
                    "HISTORY_GAP",
                    "context_read_budget",
                    "child_limit",
                    "unresolved_conflict"
                ])
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
async fn research_retains_errors_after_a_successful_dispatch_without_swallowing_them() {
    use crate::adapter::{ContextRequest, ContextSource, ErrorCode};
    let root = std::env::temp_dir().join(format!("akasha-editor-budget-{}", std::process::id()));
    private_dir(&root).unwrap();
    let _cleanup = tests::Fixture(root.clone());
    let settings = format!(
        "[executors.editor]\nkind='codex'\nbinary='/unused-native'\nnative_home={root:?}\n"
    );
    setup(&root, &settings, false).await;
    let (service, _) = tests::service_at(&root, &settings).await;
    let job_id = id();
    let plan = service.job(&job_id).unwrap().1.consult_plan.unwrap();
    let invocation = service.consult_invocation(&plan).unwrap();
    let workflow = Deadline::start(
        crate::config::deadline::Layer::Workflow,
        Duration::from_secs(30),
        None,
    )
    .unwrap();
    let probe = RunBudget::new(
        invocation.id(),
        &invocation.fence_hash(),
        service.runner_slots.clone(),
        BudgetLimits::default(),
        workflow.clone(),
    )
    .unwrap();
    let bytes = {
        let lease = probe.start().unwrap();
        let mut captured = CapturedContext::new(&service.storage, invocation.clone()).unwrap();
        let response = captured
            .execute(&lease, ContextRequest::Describe {})
            .await
            .unwrap();
        serde_json::to_vec(&response).unwrap().len() as u64
    };
    let budget = RunBudget::new(
        invocation.id(),
        &invocation.fence_hash(),
        service.runner_slots.clone(),
        BudgetLimits {
            output_bytes: bytes,
            ..BudgetLimits::default()
        },
        workflow.clone(),
    )
    .unwrap();
    let lease = budget.start().unwrap();
    let mut research = research::Research::new(
        &service,
        &job_id,
        invocation.clone(),
        service.config.pin(),
        &workflow,
        plan.clone(),
        budget.clone(),
    )
    .unwrap();
    let error = research
        .execute(&lease, ContextRequest::Describe {})
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::BudgetExhausted);
    assert_eq!(budget.usage().unwrap().output_bytes, bytes);
    assert!(research.gaps.iter().any(|gap| gap == "BUDGET_EXHAUSTED"));
    let budget = RunBudget::new(
        invocation.id(),
        &invocation.fence_hash(),
        service.runner_slots.clone(),
        BudgetLimits {
            output_bytes: 1,
            ..BudgetLimits::default()
        },
        workflow.clone(),
    )
    .unwrap();
    let lease = budget.start().unwrap();
    let mut research = research::Research::new(
        &service,
        &job_id,
        invocation,
        service.config.pin(),
        &workflow,
        plan,
        budget,
    )
    .unwrap();
    let error = research
        .execute(
            &lease,
            ContextRequest::MemoryGet {
                memory_id: "unissued".into(),
                revision: 1,
            },
        )
        .await
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::BudgetExhausted);
    assert!(research.gaps.iter().any(|gap| gap == "BUDGET_EXHAUSTED"));
    assert!(research.gaps.iter().any(|gap| gap == "READ_DENIED"));
    service.storage.close().await.unwrap();
}

#[tokio::test]
async fn incident_context_reserves_read_budget_before_outcome_payload_access() {
    use crate::adapter::{
        ContextRequest, ContextResponse, ContextSource, ErrorCode, MAX_FRAME_BYTES,
    };
    use std::sync::atomic::Ordering;
    let root = std::env::temp_dir().join(format!("akasha-incident-budget-{}", std::process::id()));
    private_dir(&root).unwrap();
    let _cleanup = tests::Fixture(root.clone());
    let settings = format!(
        "[incidents]\nexecutor='editor'\ntriggers=['user_correction']\n[executors.editor]\nkind='codex'\nbinary='/unused-native'\nnative_home={root:?}\n"
    );
    setup(&root, &settings, false).await;
    let (service, _) = tests::service_at(&root, &settings).await;
    service.submit_outcome(serde_json::from_value(json!({
        "event_id":"budget-probe","invocation_id":"bound","source":"controlled-budget","severity":"warning",
        "occurred_at_ms":ipc::now_ms().unwrap(),"execution":null,"artifacts":[],"logs":[],"receipt_jobs":[],"claims":[],
        "incident":{"kind":"user_correction","key":"budget-probe","reason":"Read only within the admitted budget."}
    })).unwrap()).await.unwrap();
    let incident_id = service
        .read_outcome("budget-probe")
        .await
        .unwrap()
        .incident
        .unwrap();
    let incident = service.incident_record(&incident_id).unwrap().1;
    let accepted = service.submit_incident_editor(&incident).await.unwrap();
    let job_id = accepted["id"].as_str().unwrap();
    let plan = service.job(job_id).unwrap().1.consult_plan.unwrap();
    let invocation = service.consult_invocation(&plan).unwrap();
    let workflow = Deadline::start(
        crate::config::deadline::Layer::Workflow,
        Duration::from_secs(30),
        None,
    )
    .unwrap();
    for describe in [false, true] {
        let request = if describe {
            ContextRequest::Describe {}
        } else {
            ContextRequest::Read {
                handle: "incident:outcome".into(),
                start: 0,
                max_bytes: 512,
            }
        };
        for mode in ["read_count", "read_bytes", "permitted"] {
            let limits = match mode {
                "read_count" => BudgetLimits {
                    reads: 1 + u64::from(describe),
                    ..BudgetLimits::default()
                },
                "read_bytes" => BudgetLimits {
                    read_bytes: MAX_FRAME_BYTES as u64 - 1 + u64::from(describe) * 2048,
                    ..BudgetLimits::default()
                },
                _ => BudgetLimits::default(),
            };
            let budget = RunBudget::new(
                invocation.id(),
                &invocation.fence_hash(),
                service.runner_slots.clone(),
                limits,
                workflow.clone(),
            )
            .unwrap();
            let lease = budget.start().unwrap();
            if mode == "read_count" {
                lease.charge_read(1).unwrap();
            }
            let mut research = research::Research::new(
                &service,
                job_id,
                invocation.clone(),
                service.config.pin(),
                &workflow,
                plan.clone(),
                budget,
            )
            .unwrap();
            let before = service.outcome_payload_reads.load(Ordering::Relaxed);
            for _ in 0..if mode == "permitted" { 1 } else { 3 } {
                let response = research.execute(&lease, request.clone()).await.unwrap();
                if mode == "permitted" {
                    assert!(
                        !matches!(response, ContextResponse::Error(_)),
                        "{response:?}"
                    );
                    assert_eq!(
                        service.outcome_payload_reads.load(Ordering::Relaxed),
                        before + 1
                    );
                } else {
                    assert_eq!(
                        service.outcome_payload_reads.load(Ordering::Relaxed),
                        before,
                        "{mode}, describe={describe}: denied read accessed outcome payload"
                    );
                    assert!(
                        matches!(response, ContextResponse::Error(error) if error.code == ErrorCode::BudgetExhausted)
                    );
                }
            }
        }
    }
    service.storage.close().await.unwrap();
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
