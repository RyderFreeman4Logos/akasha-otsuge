use super::*;
use crate::domain::{Authority, Generator, Ingress, MemoryKind, Origin, SourceDraft, SourceSpan};
use crate::grill::{Discussion, PROMPT_VERSION, Pending, Plan, Question, Role, Stage};

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn settings(root: &Path) -> String {
    format!(
        "[grill]\ninterviewer='one'\nexpert='one'\nreviewer='one'\nmax_runner_starts=1\n[review]\nmode='automatic'\n[executors.one]\nkind='codex'\nbinary='this-fixture-must-never-launch'\nnative_home={root:?}"
    )
}
async fn seed(service: &Service, id: &str) -> Plan {
    let scope = owner_scope();
    let ingress = Ingress::tool("compiler", "controlled-channel", scope.clone()).unwrap();
    let text = "Offline keyword search passed the recorded test.";
    let receipt = Domain::new(&service.storage)
        .record_source(
            &format!("source/{id}"),
            &SourceDraft {
                event_id: id.into(),
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
    Plan {
        scope,
        sources: vec![SourceSpan {
            event_id: id.into(),
            revision: receipt.commit_seq,
            content_sha256: hash(text.as_bytes()),
            start: 0,
            end: text.len(),
        }],
        question: "Which search mode passed?".into(),
        resume_from: None,
    }
}
async fn saved_response(service: &Arc<Service>, id: &str, plan: &Plan) {
    service.submit_grill(id.into(), plan.clone()).await.unwrap();
    assert!(service.claim(id, 0).await.unwrap());
    let (revision, mut job) = service.job(id).unwrap();
    let generator = Generator {
        runner: "controlled-fixture".into(),
        model: "fixture".into(),
        prompt_version: PROMPT_VERSION.into(),
    };
    let ingress = Ingress::agent(
        "auto-grill",
        id,
        plan.scope.clone(),
        Origin::AutoGrill,
        generator.clone(),
    )
    .unwrap();
    let prompt = "Controlled saved-response checkpoint; no model request is claimed.";
    let receipt = Domain::new(&service.storage)
        .record_source(
            &format!("prompt/{id}"),
            &SourceDraft {
                event_id: format!("prompt/{id}"),
                scope: plan.scope.clone(),
                text: prompt.into(),
                reported_role: Some("prompt".into()),
                event_time_ms: None,
                observed_time_ms: 1,
            },
            &ingress,
            1,
        )
        .await
        .unwrap();
    let span = SourceSpan {
        event_id: format!("prompt/{id}"),
        revision: receipt.commit_seq,
        content_sha256: hash(prompt.as_bytes()),
        start: 0,
        end: prompt.len(),
    };
    let text=json!({"schema_version":1,"questions":[],"answers":[{"question":"q","kind":"decision","claim":"Use keyword search offline.","rationale":"Recorded result only.","strongest_opposition":"Other queries remain untested.","conditions":["offline"],"assumptions":[],"unresolved":[],"sources":[{"source":0,"start":0,"end":plan.sources[0].end}],"resolved":true}]}).to_string();
    let state = Discussion {
        questions: vec![Question {
            id: "q".into(),
            text: "Which mode?".into(),
            depends_on: vec![],
        }],
        pending: Some(Pending {
            stage: Stage {
                role: Role::Expert,
                round: 0,
            },
            prompt: span.clone(),
            text,
            generator,
        }),
        attempt_prompt: Some(span),
        usage: crate::adapter::BudgetUsage {
            starts: 1,
            tokens: 123,
            ..Default::default()
        },
        ..Default::default()
    };
    let pointer = models::save_result(
        &service.storage,
        &serde_json::to_value(state).unwrap(),
        job.config_generation,
    )
    .await
    .unwrap();
    job.result = Some(json!({"checkpoint":pointer}));
    service
        .save_job(&job, Some(revision), vec![])
        .await
        .unwrap();
}

#[test]
#[ignore = "owned subprocess launched by the Auto-Grill crash supervisor"]
fn grill_crash_child() {
    let root = PathBuf::from(std::env::var_os("AKASHA_TEST_CRASH_ROOT").unwrap());
    runtime().block_on(async {
        let (service, _) = tests::service_at(&root, &settings(&root)).await;
        service.recover().await.unwrap();
        assert!(service.claim("crash", 0).await.unwrap());
        service.execute_job("crash".into()).await.unwrap();
        panic!("missing requested crash checkpoint");
    });
}

#[test]
fn grill_saved_response_and_partial_candidates_survive_sigkill_without_new_starts() {
    for phase in ["grill-phase", "grill-candidate"] {
        let root =
            std::env::temp_dir().join(format!("akasha-grill-{}-{phase}", std::process::id()));
        let _fixture = tests::Fixture(root.clone());
        runtime().block_on(async {
            let (service, _) = tests::service_at(&root, &settings(&root)).await;
            let plan = seed(&service, "evidence").await;
            saved_response(&service, "crash", &plan).await;
            service.storage.close().await.unwrap();
        });
        crate::storage::crash_tests::kill_at(
            &root,
            phase,
            "daemon::jobs::grill_tests::grill_crash_child",
        );
        runtime().block_on(async {
            let (service, _) = tests::service_at(&root, &settings(&root)).await;
            let original = service.job("crash").unwrap().1;
            let memory_id = format!("grill/{}/0", hash(b"crash"));
            let before = Domain::new(&service.storage).memory(&memory_id).unwrap();
            assert_eq!(before.is_some(), phase == "grill-candidate");
            service.recover().await.unwrap();
            assert!(service.claim("crash", 0).await.unwrap());
            service.clone().execute_job("crash".into()).await.unwrap();
            let recovered = service.job("crash").unwrap().1;
            assert_eq!(recovered.state, "succeeded", "{:?}", recovered.error);
            assert_eq!(recovered.started_at_ms, original.started_at_ms);
            assert_eq!(
                recovered.execution_deadline_ms,
                original.execution_deadline_ms
            );
            assert_eq!(recovered.result.as_ref().unwrap()["usage"]["starts"], 1);
            assert_eq!(recovered.result.as_ref().unwrap()["usage"]["tokens"], 123);
            let memory = Domain::new(&service.storage)
                .memory(&memory_id)
                .unwrap()
                .unwrap();
            assert_eq!(memory.record.authority, Authority::Candidate);
            assert_eq!(memory.record.draft.kind, MemoryKind::Hypothesis);
            assert!(
                service
                    .storage
                    .current(&memory.revision.key)
                    .unwrap()
                    .unwrap()
                    .previous
                    .is_none()
            );
            if let Some(before) = before {
                assert_eq!(before.revision, memory.revision);
            }
            service.storage.close().await.unwrap();
        });
    }
}

#[tokio::test]
async fn grill_ambiguous_native_call_fails_without_requeue_or_new_clock() {
    let root = std::env::temp_dir().join(format!("akasha-grill-ambiguous-{}", std::process::id()));
    let _fixture = tests::Fixture(root.clone());
    let (service, _) = tests::service_at(&root, &settings(&root)).await;
    let plan = seed(&service, "evidence").await;
    saved_response(&service, "ambiguous", &plan).await;
    let (revision, mut job) = service.job("ambiguous").unwrap();
    let mut state = service.grill_checkpoint(&job, revision).unwrap();
    state.inflight = Some(state.pending.take().unwrap().stage);
    job.result = Some(
        json!({"checkpoint":models::save_result(&service.storage,&serde_json::to_value(&state).unwrap(),job.config_generation).await.unwrap()}),
    );
    service
        .save_job(&job, Some(revision), vec![])
        .await
        .unwrap();
    service.recover().await.unwrap();
    let recovered = service.job("ambiguous").unwrap().1;
    assert_eq!(recovered.state, "failed");
    assert_eq!(recovered.error.as_deref(), Some("RUNNER_INTERRUPTED"));
    assert_eq!(recovered.execution_deadline_ms, job.execution_deadline_ms);
    assert!(!service.claim("ambiguous", 0).await.unwrap());
    service.storage.close().await.unwrap();
}

#[tokio::test]
async fn grill_publication_obeys_review_policy_without_claiming_human_approval() {
    for mode in ["manual", "policy", "automatic"] {
        let root =
            std::env::temp_dir().join(format!("akasha-grill-policy-{}-{mode}", std::process::id()));
        let _fixture = tests::Fixture(root.clone());
        let input = settings(&root).replace("mode='automatic'", &format!("mode='{mode}'"));
        let (service, _) = tests::service_at(&root, &input).await;
        let plan = seed(&service, "evidence").await;
        saved_response(&service, "policy", &plan).await;
        let (revision, mut job) = service.job("policy").unwrap();
        let mut state = service.grill_checkpoint(&job, revision).unwrap();
        let pending = state.pending.take().unwrap();
        let records = plan.sources(&service.storage, &context().unwrap()).unwrap();
        let report = crate::grill::Report::parse(
            &pending.text,
            pending.stage,
            &state.questions,
            4,
            &plan,
            &records,
        )
        .unwrap();
        let ingress = Ingress::agent(
            "auto-grill",
            "policy",
            plan.scope.clone(),
            Origin::AutoGrill,
            pending.generator.clone(),
        )
        .unwrap();
        let receipt = Domain::new(&service.storage)
            .record_source(
                "response-policy",
                &SourceDraft {
                    event_id: "response-policy".into(),
                    scope: plan.scope.clone(),
                    text: pending.text.clone(),
                    reported_role: Some("response".into()),
                    event_time_ms: None,
                    observed_time_ms: 1,
                },
                &ingress,
                1,
            )
            .await
            .unwrap();
        let output = SourceSpan {
            event_id: "response-policy".into(),
            revision: receipt.commit_seq,
            content_sha256: hash(pending.text.as_bytes()),
            start: 0,
            end: pending.text.len(),
        };
        state.steps = [Role::Expert, Role::Reviewer]
            .into_iter()
            .map(|role| crate::grill::Step {
                stage: Stage { role, round: 0 },
                report: report.clone(),
                prompt: pending.prompt.clone(),
                output: output.clone(),
                generator: pending.generator.clone(),
            })
            .collect();
        state.stop_reason = Some("resolved".into());
        job.result = Some(
            json!({"checkpoint":models::save_result(&service.storage,&serde_json::to_value(state).unwrap(),job.config_generation).await.unwrap()}),
        );
        service
            .save_job(&job, Some(revision), vec![])
            .await
            .unwrap();
        service.clone().execute_job("policy".into()).await.unwrap();
        let done = service.job("policy").unwrap().1;
        assert_eq!(done.state, "succeeded", "{:?}", done.error);
        for candidate in done.result.unwrap()["candidates"].as_array().unwrap() {
            let memory = Domain::new(&service.storage)
                .memory(candidate["memory_id"].as_str().unwrap())
                .unwrap()
                .unwrap();
            assert_eq!(
                memory.record.authority,
                if mode == "automatic" {
                    Authority::DelegatedApproved
                } else {
                    Authority::Candidate
                }
            );
            assert!(memory.record.approval.is_none());
            assert_eq!(memory.record.provenance.origin, Origin::AutoGrill);
        }
        service.storage.close().await.unwrap();
    }
}
