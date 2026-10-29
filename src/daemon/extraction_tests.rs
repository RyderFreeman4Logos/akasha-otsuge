use super::*;
use crate::domain::{
    Generator, Ingress, MemoryWrite, Origin, Policy, SourceDraft, SourceSpan, StatementIntent,
};

use std::io::{Read, Write};
use std::sync::atomic::Ordering;
use std::time::Instant;

#[allow(dead_code)]
#[path = "../../tests/support/model_endpoint.rs"]
mod endpoint;

fn scope() -> Scope {
    Scope {
        project: Some("alpha".into()),
        ..owner_scope()
    }
}
fn generator() -> Generator {
    Generator {
        runner: "fixture".into(),
        model: "controlled".into(),
        prompt_version: crate::extraction::PROMPT_VERSION.into(),
    }
}

fn workflow() -> crate::config::deadline::Deadline {
    crate::config::deadline::Deadline::start(
        crate::config::deadline::Layer::Workflow,
        Duration::from_secs(30),
        None,
    )
    .unwrap()
}

async fn source_plan(
    service: &Service,
    id: &str,
    text: &str,
    origin: Option<Origin>,
) -> crate::extraction::Plan {
    let scope = scope();
    let ingress = match origin {
        Some(origin) => Ingress::agent(
            "internal-memory-service",
            id,
            scope.clone(),
            origin,
            generator(),
        )
        .unwrap(),
        None => Ingress::from_verified_human_channel(
            &scope.user,
            id,
            scope.clone(),
            StatementIntent::Tentative,
        )
        .unwrap(),
    };
    let domain = Domain::new(&service.storage);
    let receipt = domain
        .record_source(
            &format!("source/{id}"),
            &SourceDraft {
                event_id: id.into(),
                scope: scope.clone(),
                text: text.into(),
                reported_role: Some("user".into()),
                event_time_ms: Some(1),
                observed_time_ms: 2,
            },
            &ingress,
            1,
        )
        .await
        .unwrap();
    crate::extraction::Plan {
        scope,
        sources: vec![SourceSpan {
            event_id: id.into(),
            revision: receipt.commit_seq,
            content_sha256: hash(text.as_bytes()),
            start: 0,
            end: text.len(),
        }],
    }
}

fn batch(plan: &crate::extraction::Plan, claim: &str) -> Value {
    json!({"schema_version":1,"candidates":[{"kind":"preference","scope":plan.scope,"claim":claim,"rationale":"Preserve the explicit condition.","conditions":["offline"],"assumptions":[],"sources":[{"source":0,"start":0,"end":plan.sources[0].end}],"valid_from_ms":null,"valid_until_ms":null}]})
}

async fn publish(
    service: &Service,
    plan: &crate::extraction::Plan,
    id: &str,
    claim: &str,
) -> Vec<Value> {
    let records = plan.sources(&service.storage, &context().unwrap()).unwrap();
    let drafts = crate::extraction::parse(plan, &batch(plan, claim).to_string(), &records).unwrap();
    crate::extraction::publish(
        &service.storage,
        plan,
        id,
        1,
        generator(),
        drafts,
        &workflow(),
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn extraction_rejects_forged_authority_scope_and_citations_before_publication() {
    let (_fixture, service, _) = tests::fixture("extraction-schema", "").await;
    let plan = source_plan(&service, "unicode", "只在离线时使用关键词检索。", None).await;
    let records = plan.sources(&service.storage, &context().unwrap()).unwrap();
    let valid = batch(&plan, "Use keyword search only offline.");
    assert_eq!(
        crate::extraction::parse(&plan, &valid.to_string(), &records).unwrap()[0].source_spans,
        plan.sources
    );
    for (pointer, value) in [
        ("/schema_version", json!(2)),
        ("/candidates/0/scope/project", Value::Null),
        ("/candidates/0/sources/0/source", json!(1)),
        ("/candidates/0/sources/0/end", json!(1)),
        (
            "/candidates/0/sources/0/end",
            json!(plan.sources[0].end + 1),
        ),
        ("/candidates/0/sources", json!([])),
    ] {
        let mut invalid = valid.clone();
        *invalid.pointer_mut(pointer).unwrap() = value;
        assert!(
            crate::extraction::parse(&plan, &invalid.to_string(), &records).is_err(),
            "{pointer}"
        );
    }
    let mut forged = valid.clone();
    forged["candidates"][0]["authority"] = json!("human_confirmed");
    assert!(crate::extraction::parse(&plan, &forged.to_string(), &records).is_err());
    let mut mixed = valid.clone();
    mixed["candidates"]
        .as_array_mut()
        .unwrap()
        .push(forged["candidates"][0].clone());
    assert!(crate::extraction::parse(&plan, &mixed.to_string(), &records).is_err());
    service.storage.close().await.unwrap();
}

#[tokio::test]
async fn extraction_internal_origins_cannot_feed_another_extraction() {
    let settings = "[retrieval]\nextraction='fixture'\n[models.fixture]\nkind='text'\nendpoint='http://127.0.0.1:1/v1'\nmodel='fixture'\nlocal=true\n";
    let (_fixture, service, _) = tests::fixture("extraction-origin", settings).await;
    for (id, origin) in [
        ("internal-extraction", Origin::Extraction),
        ("internal-grill", Origin::AutoGrill),
    ] {
        let plan = source_plan(
            &service,
            id,
            "A derived internal discussion, not a human instruction.",
            Some(origin),
        )
        .await;
        let result = service
            .submit_extraction(format!("recursive/{id}"), plan.clone())
            .await;
        assert_eq!(result.unwrap_err().0, "EXTRACTION_INTERNAL_SOURCE");
        assert_eq!(
            Domain::new(&service.storage)
                .source(id, plan.sources[0].revision)
                .unwrap()
                .payload
                .text,
            "A derived internal discussion, not a human instruction."
        );
    }
    service.storage.close().await.unwrap();
}

#[tokio::test]
async fn extraction_merges_distinct_sources_once_and_keeps_contradictory_candidates() {
    let (_fixture, service, _) = tests::fixture("extraction-merge", "").await;
    let first = source_plan(
        &service,
        "first-occurrence",
        "Use keyword search only offline.",
        None,
    )
    .await;
    let second = source_plan(
        &service,
        "second-occurrence",
        "Use keyword search only offline.",
        None,
    )
    .await;
    let claim = "Use keyword search only offline.";
    let initial = publish(&service, &first, "first-job", claim).await;
    let merged = publish(&service, &second, "second-job", claim).await;
    assert_eq!(initial[0]["memory_id"], merged[0]["memory_id"]);
    assert_ne!(initial[0]["revision"], merged[0]["revision"]);
    let repeated = publish(&service, &second, "third-job", claim).await;
    assert_eq!(repeated[0]["revision"], merged[0]["revision"]);
    let memory = Domain::new(&service.storage)
        .memory(merged[0]["memory_id"].as_str().unwrap())
        .unwrap()
        .unwrap();
    assert_eq!(
        memory.record.draft.source_spans,
        [first.sources[0].clone(), second.sources[0].clone()]
    );
    let contrary = source_plan(
        &service,
        "contrary",
        "Do not use keyword search while offline.",
        None,
    )
    .await;
    let conflict = publish(
        &service,
        &contrary,
        "contradiction",
        "Do not use keyword search while offline.",
    )
    .await;
    assert_ne!(conflict[0]["memory_id"], initial[0]["memory_id"]);
    assert_eq!(
        Domain::new(&service.storage)
            .memory(initial[0]["memory_id"].as_str().unwrap())
            .unwrap()
            .unwrap()
            .revision
            .commit_seq,
        memory.revision.commit_seq
    );
    service.storage.close().await.unwrap();
}

#[tokio::test]
async fn extraction_does_not_merge_new_sources_into_a_revised_candidate_identity() {
    for (name, origin, revised_claim) in [
        (
            "edited",
            Origin::Extraction,
            "Do not use keyword search offline.",
        ),
        (
            "other-origin",
            Origin::AutoGrill,
            "Use keyword search only offline.",
        ),
    ] {
        let (_fixture, service, _) = tests::fixture(&format!("extraction-{name}"), "").await;
        let first = source_plan(&service, "first", "Use keyword search only offline.", None).await;
        let second =
            source_plan(&service, "second", "Use keyword search only offline.", None).await;
        let claim = "Use keyword search only offline.";
        let initial = publish(&service, &first, "first-job", claim).await;
        let id = initial[0]["memory_id"].as_str().unwrap();
        let domain = Domain::new(&service.storage);
        let old = domain.memory(id).unwrap().unwrap();
        let mut draft = old.record.draft.clone();
        draft.claim = revised_claim.into();
        let ingress = Ingress::agent("editor", "edit", scope(), origin, generator()).unwrap();
        let policy = Policy {
            revision: "manual".into(),
            config_generation: 1,
            mode: crate::config::ReviewMode::Manual,
            scope: scope(),
            adopt_explicit_decisions: false,
        };
        let edited = domain
            .propose(
                &MemoryWrite {
                    request_id: "edit".into(),
                    memory_id: id.into(),
                    expected_revision: Some(old.revision.commit_seq),
                    draft,
                },
                &ingress,
                &policy,
            )
            .await
            .unwrap();
        publish(&service, &second, "later-extraction", claim).await;
        let after = domain.memory(id).unwrap().unwrap();
        assert_eq!(
            after.revision.commit_seq, edited.commit_seq,
            "extraction overwrote {name}"
        );
        assert_eq!(after.record.draft.source_spans, first.sources);
        service.storage.close().await.unwrap();
    }
}

fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}

#[tokio::test]
async fn extraction_interrupted_before_checkpoint_needs_explicit_retry() {
    let endpoint = endpoint::Endpoint::extraction();
    let settings = format!(
        "[retrieval]\nextraction='fixture'\n[models.fixture]\nkind='text'\nendpoint={:?}\nmodel='fixture'\nlocal=true\n",
        endpoint.address
    );
    let (_fixture, service, _) = tests::fixture("extraction-ambiguous", &settings).await;
    let plan = source_plan(
        &service,
        "conditional",
        "Only if offline, use keyword search.",
        None,
    )
    .await;
    service
        .submit_extraction("interrupted".into(), plan.clone())
        .await
        .unwrap();
    assert!(service.claim("interrupted", 0).await.unwrap());
    let old = service.job("interrupted").unwrap().1;
    service.recover().await.unwrap();
    let recovered = service.job("interrupted").unwrap().1;
    assert_eq!(
        recovered.state, "failed",
        "a possibly admitted provider request must not be replayed automatically"
    );
    assert_eq!(recovered.error.as_deref(), Some("PROVIDER_INTERRUPTED"));
    assert_eq!(recovered.execution_deadline_ms, old.execution_deadline_ms);
    assert!(endpoint.requests.lock().unwrap().is_empty());
    service
        .submit_extraction("explicit-retry".into(), plan.clone())
        .await
        .unwrap();
    assert!(service.claim("explicit-retry", 0).await.unwrap());
    service
        .clone()
        .execute_job("explicit-retry".into())
        .await
        .unwrap();
    assert_eq!(service.job("explicit-retry").unwrap().1.state, "succeeded");
    assert_eq!(endpoint.requests.lock().unwrap().len(), 1);
    assert_eq!(
        Domain::new(&service.storage)
            .source("conditional", plan.sources[0].revision)
            .unwrap()
            .payload
            .text,
        "Only if offline, use keyword search."
    );
    service.storage.close().await.unwrap();
}

#[tokio::test]
async fn extraction_model_failure_keeps_originals_and_a_durable_failed_job() {
    let endpoint = endpoint::Endpoint::with_mode(endpoint::Mode::Disconnect);
    let settings = format!(
        "[retrieval]\nextraction='fixture'\n[models.fixture]\nkind='text'\nendpoint={:?}\nmodel='fixture'\nlocal=true\n",
        endpoint.address
    );
    let (_fixture, service, _) = tests::fixture("extraction-offline", &settings).await;
    let plan = source_plan(
        &service,
        "raw",
        "Only if offline, use keyword search.",
        None,
    )
    .await;
    service
        .submit_extraction("offline".into(), plan.clone())
        .await
        .unwrap();
    assert!(service.claim("offline", 0).await.unwrap());
    service.clone().execute_job("offline".into()).await.unwrap();
    let job = service.job("offline").unwrap().1;
    assert_eq!(job.state, "failed");
    assert!(job.error.is_some());
    assert!(job.result.as_ref().unwrap()["candidates"].is_null());
    assert_eq!(
        Domain::new(&service.storage)
            .source("raw", plan.sources[0].revision)
            .unwrap()
            .payload
            .text,
        "Only if offline, use keyword search."
    );
    assert_eq!(
        service
            .submit_extraction("offline".into(), plan)
            .await
            .unwrap()["state"],
        "failed"
    );
    assert_eq!(endpoint.requests.lock().unwrap().len(), 1);
    service.storage.close().await.unwrap();
}

#[tokio::test]
async fn extraction_source_union_limit_fails_the_job_without_losing_old_sources() {
    let endpoint = endpoint::Endpoint::extraction();
    let settings = format!(
        "[retrieval]\nextraction='fixture'\n[models.fixture]\nkind='text'\nendpoint={:?}\nmodel='fixture'\nlocal=true\n",
        endpoint.address
    );
    let (_fixture, service, _) = tests::fixture("extraction-source-limit", &settings).await;
    let mut last = None;
    for n in 0..32 {
        let plan = source_plan(
            &service,
            &format!("source-{n}"),
            "Only if offline, use keyword search.",
            None,
        )
        .await;
        let records = plan.sources(&service.storage, &context().unwrap()).unwrap();
        let mut value = batch(&plan, "Use keyword search when offline.");
        value["candidates"][0]["kind"] = json!("decision");
        let drafts = crate::extraction::parse(&plan, &value.to_string(), &records).unwrap();
        last = Some(
            crate::extraction::publish(
                &service.storage,
                &plan,
                &format!("job-{n}"),
                1,
                generator(),
                drafts,
                &workflow(),
            )
            .await
            .unwrap(),
        );
    }
    let result = last.unwrap();
    let id = result[0]["memory_id"].as_str().unwrap();
    let original = Domain::new(&service.storage).memory(id).unwrap().unwrap();
    assert_eq!(original.record.draft.source_spans.len(), 32);
    let next = source_plan(
        &service,
        "source-33",
        "Only if offline, use keyword search.",
        None,
    )
    .await;
    service
        .submit_extraction("full-union".into(), next)
        .await
        .unwrap();
    assert!(service.claim("full-union", 0).await.unwrap());
    service
        .clone()
        .execute_job("full-union".into())
        .await
        .unwrap();
    let job = service.job("full-union").unwrap().1;
    assert_eq!(job.state, "failed");
    assert_eq!(job.error.as_deref(), Some("EXTRACTION_SOURCE_LIMIT"));
    let after = Domain::new(&service.storage).memory(id).unwrap().unwrap();
    assert_eq!(after.revision, original.revision);
    assert_eq!(
        after.record.draft.source_spans,
        original.record.draft.source_spans
    );
    service.storage.close().await.unwrap();
}

#[tokio::test]
async fn extraction_publication_cannot_start_writes_after_its_workflow_expires() {
    let (_fixture, service, _) = tests::fixture("extraction-expired", "").await;
    let plan = source_plan(
        &service,
        "conditional",
        "Only if offline, use keyword search.",
        None,
    )
    .await;
    let records = plan.sources(&service.storage, &context().unwrap()).unwrap();
    let drafts = crate::extraction::parse(
        &plan,
        &batch(&plan, "Use keyword search only offline.").to_string(),
        &records,
    )
    .unwrap();
    let before = service.storage.sequence().unwrap();
    let clock = crate::config::deadline::Deadline::start(
        crate::config::deadline::Layer::Workflow,
        Duration::from_millis(1),
        None,
    )
    .unwrap();
    tokio::time::sleep(Duration::from_millis(5)).await;
    assert!(clock.expired().is_some());
    let result = crate::extraction::publish(
        &service.storage,
        &plan,
        "expired",
        1,
        generator(),
        drafts,
        &clock,
    )
    .await;
    assert_eq!(result.unwrap_err().code, "WORKFLOW_TIMEOUT");
    assert_eq!(service.storage.sequence().unwrap(), before);
    service.storage.close().await.unwrap();
}

#[test]
#[ignore = "owned subprocess launched by the extraction crash supervisor"]
fn extraction_crash_child() {
    let root = PathBuf::from(std::env::var_os("AKASHA_TEST_CRASH_ROOT").unwrap());
    let settings = std::fs::read_to_string(root.join("settings.toml")).unwrap();
    runtime().block_on(async {
        let (service, _) = tests::service_at(&root, &settings).await;
        service.recover().await.unwrap();
        assert!(service.claim("extract-crash", 0).await.unwrap());
        service.execute_job("extract-crash".into()).await.unwrap();
        panic!("extraction did not reach the requested crash checkpoint");
    });
}

#[test]
fn extraction_response_and_partial_publication_survive_sigkill_without_model_replay() {
    for phase in ["extraction-response", "extraction-candidate"] {
        let root =
            std::env::temp_dir().join(format!("akasha-extraction-{}-{phase}", std::process::id()));
        private_dir(&root).unwrap();
        let _fixture = tests::Fixture(root.clone());
        let endpoint = endpoint::Endpoint::extraction();
        let settings = format!(
            "[retrieval]\nextraction='fixture'\n[models.fixture]\nkind='text'\nendpoint={:?}\nmodel='controlled-extractor'\nlocal=true\n",
            endpoint.address
        );
        std::fs::write(root.join("settings.toml"), &settings).unwrap();
        runtime().block_on(async {
            let (service, _) = tests::service_at(&root, &settings).await;
            let scope = Scope {
                project: Some("alpha".into()),
                ..owner_scope()
            };
            let ingress = Ingress::from_verified_human_channel(
                &scope.user,
                "source-channel",
                scope.clone(),
                StatementIntent::Tentative,
            )
            .unwrap();
            let domain = Domain::new(&service.storage);
            let text = "Only if offline, use keyword search.";
            let receipt = domain
                .record_source(
                    "source-crash",
                    &SourceDraft {
                        event_id: "source-crash".into(),
                        scope: scope.clone(),
                        text: text.into(),
                        reported_role: Some("user".into()),
                        event_time_ms: None,
                        observed_time_ms: 1,
                    },
                    &ingress,
                    1,
                )
                .await
                .unwrap();
            let plan = crate::extraction::Plan {
                scope,
                sources: vec![SourceSpan {
                    event_id: "source-crash".into(),
                    revision: receipt.commit_seq,
                    content_sha256: hash(text.as_bytes()),
                    start: 0,
                    end: text.len(),
                }],
            };
            service
                .submit_extraction("extract-crash".into(), plan)
                .await
                .unwrap();
            service.storage.close().await.unwrap();
        });
        crate::storage::crash_tests::kill_at(
            &root,
            phase,
            "daemon::jobs::extraction_tests::extraction_crash_child",
        );
        runtime().block_on(async {
            let (service, _) = tests::service_at(&root, &settings).await;
            let original = service.job("extract-crash").unwrap().1;
            assert_eq!(original.state, "running");
            let keys = service
                .storage
                .head_keys(
                    &RecordKey {
                        kind: RecordKind::Memory,
                        id: format!("extracted/{}", "0".repeat(64)),
                    },
                    &RecordKey {
                        kind: RecordKind::Memory,
                        id: format!("extracted/{}", "g".repeat(64)),
                    },
                    None,
                    32,
                )
                .unwrap();
            let before: Vec<_> = keys
                .iter()
                .map(|key| service.storage.current(key).unwrap().unwrap())
                .collect();
            assert_eq!(before.len(), usize::from(phase == "extraction-candidate"));
            service.recover().await.unwrap();
            assert!(service.claim("extract-crash", 0).await.unwrap());
            service
                .clone()
                .execute_job("extract-crash".into())
                .await
                .unwrap();
            let resumed = service.job("extract-crash").unwrap().1;
            assert_eq!(resumed.state, "succeeded");
            assert_eq!(resumed.started_at_ms, original.started_at_ms);
            assert_eq!(
                resumed.execution_deadline_ms,
                original.execution_deadline_ms
            );
            assert_eq!(resumed.attempt, original.attempt + 1);
            assert_eq!(
                endpoint.requests.lock().unwrap().len(),
                1,
                "{phase}: a completed model response was replayed"
            );
            for previous in before {
                assert_eq!(
                    service.storage.current(&previous.key).unwrap(),
                    Some(previous)
                );
            }
            let candidates = resumed.result.unwrap()["candidates"]
                .as_array()
                .unwrap()
                .clone();
            assert_eq!(candidates.len(), 1);
            let record = service
                .storage
                .current(&RecordKey {
                    kind: RecordKind::Memory,
                    id: candidates[0]["memory_id"].as_str().unwrap().into(),
                })
                .unwrap()
                .unwrap();
            assert!(
                record.previous.is_none(),
                "recovery created a duplicate effective version"
            );
            service.storage.close().await.unwrap();
        });
    }
}
