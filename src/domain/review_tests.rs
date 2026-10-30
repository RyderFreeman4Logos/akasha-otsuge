use super::*;
use std::path::{Path, PathBuf};

struct Fixture(PathBuf);
impl Drop for Fixture {
    fn drop(&mut self) {
        std::fs::remove_dir_all(&self.0).unwrap();
    }
}
fn config(root: &Path) -> crate::config::ResolvedConfig {
    crate::config::resolve(
        "",
        None,
        "",
        &crate::config::Paths {
            data_dir: root.join("data"),
            socket_path: root.join("run/socket"),
        },
    )
    .unwrap()
}
fn runtime() -> tokio::runtime::Runtime {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
}
fn scope() -> Scope {
    Scope {
        user: "operator".into(),
        project: None,
        branch_or_version: None,
        session: None,
    }
}
fn context() -> ReadContext {
    ReadContext::from_verified_binding("operator", "fixture-control", "fixture-control").unwrap()
}
fn policy() -> Policy {
    Policy {
        revision: "controlled-review".into(),
        config_generation: 1,
        mode: ReviewMode::Manual,
        scope: scope(),
        adopt_explicit_decisions: false,
    }
}

async fn seed(store: &Storage) -> ReviewDecision {
    let domain = Domain::new(store);
    domain
        .set_read_grant("grant", "operator", Some(&scope()), None, 1)
        .await
        .unwrap();
    let ingress = Ingress::tool("compiler", "controlled-fixture", scope()).unwrap();
    let original = SourceDraft {
        event_id: "original".into(),
        scope: scope(),
        text: "The controlled build passed".into(),
        reported_role: None,
        event_time_ms: None,
        observed_time_ms: 1,
    };
    let source = domain
        .record_source("source", &original, &ingress, 1)
        .await
        .unwrap();
    domain
        .propose(
            &MemoryWrite {
                request_id: "proposal".into(),
                memory_id: "memory".into(),
                expected_revision: None,
                draft: MemoryDraft {
                    kind: MemoryKind::Fact,
                    scope: scope(),
                    claim: original.text.clone(),
                    rationale: "build receipt".into(),
                    conditions: vec![],
                    assumptions: vec![],
                    source_spans: vec![SourceSpan {
                        event_id: original.event_id,
                        revision: source.commit_seq,
                        content_sha256: hash(original.text.as_bytes()),
                        start: 0,
                        end: original.text.len(),
                    }],
                    valid_from_ms: None,
                    valid_until_ms: None,
                },
            },
            &ingress,
            &policy(),
        )
        .await
        .unwrap();
    let page = domain
        .review_list(&context(), &scope(), None)
        .await
        .unwrap();
    ReviewDecision {
        id: page.items[0].id.clone(),
        expected_revision: page.items[0].revision,
        action: ReviewAction::Approve,
        draft: None,
        reason: "Synthetic exact operator decision".into(),
    }
}

#[test]
#[ignore = "launched only by the review crash supervisor"]
fn review_crash_child() {
    let root = PathBuf::from(std::env::var_os("AKASHA_TEST_CRASH_ROOT").unwrap());
    runtime().block_on(async {
        let store = Storage::open(&config(&root)).await.unwrap();
        let decision: ReviewDecision =
            serde_json::from_slice(&std::fs::read(root.join("decision.json")).unwrap()).unwrap();
        let domain = Domain::new(&store);
        let preview = domain
            .preview_decision(&context(), "approve", &decision, &policy())
            .await
            .unwrap();
        domain
            .apply_decision(
                &context(),
                "approve",
                &decision,
                &preview.preview_sha256,
                "fixture-operator",
                &policy(),
            )
            .await
            .unwrap();
        panic!("crash checkpoint not reached");
    });
}

#[test]
fn review_crash_keeps_evidence_memory_queue_and_outbox_atomic() {
    for phase in [
        "partial-stage",
        "before-commit",
        "commit-pending",
        "after-commit",
    ] {
        let root = std::env::temp_dir().join(format!(
            "akasha-review-crash-{}-{phase}",
            std::process::id()
        ));
        std::fs::create_dir(&root).unwrap();
        let _fixture = Fixture(root.clone());
        let (decision, before) = runtime().block_on(async {
            let store = Storage::open(&config(&root)).await.unwrap();
            let decision = seed(&store).await;
            let before = store.sequence().unwrap();
            store.close().await.unwrap();
            (decision, before)
        });
        std::fs::write(
            root.join("decision.json"),
            serde_json::to_vec(&decision).unwrap(),
        )
        .unwrap();
        crate::storage::crash_tests::kill_at(
            &root,
            phase,
            "domain::review_tests::review_crash_child",
        );
        runtime().block_on(async {
            let store = Storage::open(&config(&root)).await.unwrap();
            let domain = Domain::new(&store);
            let receipt = store.receipt("approve").unwrap();
            let committed = receipt.is_some();
            if phase == "after-commit" {
                assert!(committed);
            }
            if matches!(phase, "partial-stage" | "before-commit") {
                assert!(!committed);
            }
            assert_eq!(store.sequence().unwrap(), before + u64::from(committed));
            let detail = domain
                .review_show(&context(), &decision.id, None)
                .await
                .unwrap();
            assert_eq!(
                detail.item.state,
                if committed {
                    ReviewState::Resolved
                } else {
                    ReviewState::Open
                }
            );
            assert_eq!(domain.active("memory").unwrap().is_some(), committed);
            assert_eq!(store.outbox(before + 1).unwrap().is_some(), committed);
            let source_id = format!("review-operation/{}", hash(b"approve"));
            assert_eq!(
                store.current(&source_key(&source_id)).unwrap().is_some(),
                committed
            );
            let preview = domain
                .preview_decision(&context(), "approve", &decision, &policy())
                .await
                .unwrap();
            assert_eq!(
                domain
                    .apply_decision(
                        &context(),
                        "approve",
                        &decision,
                        "wrong-hash",
                        "fixture-operator",
                        &policy()
                    )
                    .await
                    .unwrap_err()
                    .code,
                "REVIEW_PREVIEW_CHANGED"
            );
            let recovered = domain
                .apply_decision(
                    &context(),
                    "approve",
                    &decision,
                    &preview.preview_sha256,
                    "fixture-operator",
                    &policy(),
                )
                .await
                .unwrap();
            assert_eq!(recovered.commit_seq, before + 1);
            if let Some(receipt) = receipt {
                assert_eq!(recovered, receipt);
            }
            let memory = domain.active("memory").unwrap().unwrap();
            assert_eq!(
                memory.record.approval.unwrap().source.revision,
                recovered.commit_seq
            );
            assert_eq!(
                domain
                    .review_show(&context(), &decision.id, None)
                    .await
                    .unwrap()
                    .item
                    .revision,
                recovered.commit_seq
            );
            assert_eq!(store.sequence().unwrap(), recovered.commit_seq);
            store.close().await.unwrap();
        });
    }
}
