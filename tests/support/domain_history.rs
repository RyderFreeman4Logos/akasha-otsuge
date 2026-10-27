use super::*;

#[tokio::test]
async fn diff_pages_keep_retirements_and_bind_cursors_to_both_points() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let domain = Domain::new(&store);
    grant(&domain).await;
    for name in ["first", "second"] {
        let request = fact(&domain, name, name).await;
        domain
            .propose(
                &request,
                &agent(Origin::Extraction),
                &policy(ReviewMode::Automatic),
            )
            .await
            .unwrap();
    }
    let before = store.sequence().unwrap();
    let after = review_action(&domain, "first", ReviewAction::Reject, "retire-for-diff").await;
    let retired = domain
        .diff(&reader(), &scope(), before, after, None, budget(8))
        .await
        .unwrap();
    assert_eq!(retired.data.items.len(), 1);
    let change = &retired.data.items[0];
    assert_eq!(
        change.before.as_ref().unwrap().memory.record.lifecycle,
        Lifecycle::Active
    );
    assert_eq!(
        change.after.as_ref().unwrap().memory.record.lifecycle,
        Lifecycle::Retired
    );
    assert_eq!(
        change.after.as_ref().unwrap().memory.record.draft.claim,
        "first"
    );
    let first = domain
        .diff(&reader(), &scope(), 0, after, None, budget(1))
        .await
        .unwrap();
    assert_eq!(first.data.examined, 1);
    assert_eq!(first.data.items.len(), 1);
    let cursor = first.data.next.unwrap();
    let second = domain
        .diff(&reader(), &scope(), 0, after, Some(&cursor), budget(1))
        .await
        .unwrap();
    assert_eq!(second.data.items.len(), 1);
    assert!(second.data.next.is_none());
    assert_ne!(
        first.data.items[0]
            .after
            .as_ref()
            .unwrap()
            .memory
            .revision
            .key,
        second.data.items[0]
            .after
            .as_ref()
            .unwrap()
            .memory
            .revision
            .key
    );
    for (from, to) in [(1, after), (0, before)] {
        assert_eq!(
            domain
                .diff(&reader(), &scope(), from, to, Some(&cursor), budget(1))
                .await
                .unwrap_err()
                .code,
            "INVALID_CURSOR"
        );
    }
    assert_eq!(
        domain
            .state(&reader(), &scope(), after, Some(&cursor), budget(1))
            .await
            .unwrap_err()
            .code,
        "INVALID_CURSOR"
    );
    assert_eq!(
        domain
            .diff(
                &reader(),
                &scope(),
                0,
                after,
                None,
                PageBudget {
                    limit: 1,
                    max_bytes: 1
                }
            )
            .await
            .unwrap_err()
            .code,
        "RESULT_TOO_LARGE"
    );
    let empty = domain
        .diff(&reader(), &scope(), 0, 0, None, budget(1))
        .await
        .unwrap();
    assert!(empty.data.items.is_empty());
    assert!(empty.data.next.is_some());
    assert!(
        domain
            .diff(
                &reader(),
                &scope(),
                0,
                0,
                empty.data.next.as_ref(),
                budget(1)
            )
            .await
            .unwrap()
            .data
            .next
            .is_none()
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn incomplete_outputs_and_empty_pages_cannot_claim_complete_coverage() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let domain = Domain::new(&store);
    grant(&domain).await;
    for name in ["one", "two"] {
        let request = fact(&domain, name, name).await;
        domain
            .propose(
                &request,
                &agent(Origin::Extraction),
                &policy(ReviewMode::Automatic),
            )
            .await
            .unwrap();
    }
    let first = domain
        .state(&reader(), &scope(), 0, None, budget(1))
        .await
        .unwrap();
    assert!(first.data.items.is_empty());
    assert!(first.data.next.is_some());
    let second = domain
        .state(&reader(), &scope(), 0, first.data.next.as_ref(), budget(1))
        .await
        .unwrap();
    assert!(second.data.items.is_empty());
    assert!(second.data.next.is_none());
    let mut cursor = serde_json::to_value(first.data.next.unwrap()).unwrap();
    cursor["after"] = json!("outside-range");
    let cursor: StateCursor = serde_json::from_value(cursor).unwrap();
    assert_eq!(
        domain
            .state(&reader(), &scope(), 0, Some(&cursor), budget(1))
            .await
            .unwrap_err()
            .code,
        "INVALID_CURSOR"
    );
    let mut draft = recall(
        &domain.active("one").unwrap().unwrap(),
        store.sequence().unwrap(),
    );
    draft.truncation = Some(Truncation {
        original_bytes: 1000,
        reason: "response budget".into(),
    });
    let before = store.sequence().unwrap();
    assert_eq!(
        domain
            .prepare_recall(&reader(), &draft)
            .await
            .unwrap_err()
            .code,
        "INVALID_RECALL"
    );
    draft.status = RecallStatus::Incomplete;
    draft.coverage.complete = false;
    let mut malformed = draft.clone();
    malformed.sources = vec![SourceSpan {
        event_id: "too-long-hash".into(),
        revision: 1,
        content_sha256: "a".repeat(65536),
        start: 0,
        end: 1,
    }];
    assert_eq!(
        domain
            .prepare_recall(&reader(), &malformed)
            .await
            .unwrap_err()
            .code,
        "INVALID_RECALL"
    );
    assert_eq!(store.sequence().unwrap(), before);
    domain.prepare_recall(&reader(), &draft).await.unwrap();
    let saved = domain
        .recall_receipt(&reader(), &draft.request_id)
        .await
        .unwrap();
    assert_eq!(saved.data.record.truncation, draft.truncation);
    assert_eq!(saved.data.record.status, RecallStatus::Incomplete);
    assert!(!saved.data.record.coverage.complete);
    let no_hit = RecallDraft {
        request_id: "no-hit".into(),
        output: "No result within the declared scope".into(),
        selected: vec![],
        status: RecallStatus::NoHit,
        coverage: Coverage {
            complete: true,
            gaps: vec![],
        },
        truncation: None,
        index_generation: None,
        ..draft
    };
    domain.prepare_recall(&reader(), &no_hit).await.unwrap();
    assert_eq!(
        domain
            .recall_receipt(&reader(), "no-hit")
            .await
            .unwrap()
            .data
            .record
            .index_generation,
        None
    );
    store.close().await.unwrap();
}

fn reader() -> ReadContext {
    ReadContext::from_verified_binding("reader", "runtime/thread/call", "run-1").unwrap()
}

fn budget(limit: usize) -> PageBudget {
    PageBudget {
        limit,
        max_bytes: 128 * 1024,
    }
}

async fn grant(domain: &Domain<'_>) -> u64 {
    domain
        .set_read_grant(
            "grant",
            "reader",
            Some(&policy(ReviewMode::Manual).scope),
            None,
            1,
        )
        .await
        .unwrap()
        .commit_seq
}

async fn fact(domain: &Domain<'_>, name: &str, claim: &str) -> MemoryWrite {
    let ingress = agent(Origin::Extraction);
    let span = evidence(domain, &ingress, &format!("source-{name}"), claim, scope()).await;
    let mut request = write(name, span, MemoryKind::Fact);
    request.draft.claim = claim.into();
    request
}

fn active(result: &ReadResult<MemoryState>) -> &TimedMemory {
    match &result.data {
        MemoryState::Active(memory) => memory,
        state => panic!("expected active, got {state:?}"),
    }
}

async fn advance_to(store: &Storage, target: u64) {
    use akasha_otsuge::storage::{Change, CommitRequest, RecordKey, RecordKind};
    while store.sequence().unwrap() < target {
        let n = store.sequence().unwrap() + 1;
        store
            .commit(&CommitRequest {
                request_id: format!("padding-{n}"),
                config_generation: 1,
                policy_revision: None,
                changes: vec![Change {
                    key: RecordKey {
                        kind: RecordKind::Job,
                        id: format!("padding-{n}"),
                    },
                    expected_revision: None,
                    content: "{}".into(),
                }],
            })
            .await
            .unwrap();
    }
}

#[tokio::test]
async fn as_of_separates_known_state_from_reality_time_and_pending_proposals() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let domain = Domain::new(&store);
    grant(&domain).await;
    let mut request = fact(&domain, "choice", "A").await;
    let ingress = agent(Origin::Extraction);
    let auto = policy(ReviewMode::Automatic);
    advance_to(&store, 9).await;
    let a = domain
        .propose(&request, &ingress, &auto)
        .await
        .unwrap()
        .commit_seq;
    assert_eq!(a, 10);
    assert!(matches!(
        domain
            .get(&reader(), "choice", Some(a - 1))
            .await
            .unwrap()
            .data,
        MemoryState::Absent
    ));
    request.request_id = "replace-b".into();
    request.expected_revision = Some(a);
    request.draft.claim = "B".into();
    request.draft.valid_from_ms = Some(-100_000);
    advance_to(&store, 19).await;
    let b = domain
        .propose(&request, &ingress, &auto)
        .await
        .unwrap()
        .commit_seq;
    assert_eq!(b, 20);
    let old = domain.get(&reader(), "choice", Some(a)).await.unwrap();
    assert_eq!(active(&old).memory.record.draft.claim, "A");
    assert!(active(&old).recorded_at_ms > 1_000_000);
    assert_eq!(
        (old.read_seq, old.validated_at_seq, old.retrieval_mode),
        (a, b, RetrievalMode::HistoricalState)
    );
    let now = domain.get(&reader(), "choice", None).await.unwrap();
    assert_eq!(active(&now).memory.record.draft.claim, "B");
    assert_eq!(now.retrieval_mode, RetrievalMode::CurrentState);
    request.request_id = "pending-c".into();
    request.expected_revision = Some(b);
    request.draft.claim = "C".into();
    let c = domain
        .propose(&request, &ingress, &policy(ReviewMode::Manual))
        .await
        .unwrap()
        .commit_seq;
    assert_eq!(
        active(&domain.get(&reader(), "choice", Some(c)).await.unwrap())
            .memory
            .revision
            .commit_seq,
        b
    );
    let history = domain
        .history(&reader(), "choice", 1..=c, None, budget(8))
        .await
        .unwrap();
    assert_eq!(
        history
            .data
            .items
            .iter()
            .map(|v| v.status)
            .collect::<Vec<_>>(),
        [
            HistoricalStatus::Superseded,
            HistoricalStatus::Active,
            HistoricalStatus::Candidate
        ]
    );
    assert_eq!(
        history.data.items[1]
            .revision
            .memory
            .record
            .previous_adoption,
        Some(a)
    );
    assert_eq!(
        history.data.items[1]
            .revision
            .memory
            .record
            .draft
            .valid_from_ms,
        Some(-100_000)
    );
    store.close().await.unwrap();
    let reopened = fixture.open().await;
    let domain = Domain::new(&reopened);
    assert_eq!(
        active(&domain.get(&reader(), "choice", Some(a)).await.unwrap()),
        active(&old)
    );
    assert_eq!(
        domain
            .history(&reader(), "choice", 1..=c, None, budget(8))
            .await
            .unwrap()
            .data,
        history.data
    );
    reopened.close().await.unwrap();
}

fn recall(memory: &MemoryView, at: u64) -> RecallDraft {
    RecallDraft {
        request_id: "recall-1".into(),
        output: "实际返回：A\n保留空格 \0".into(),
        scope: scope(),
        read_seq: at,
        retrieval_mode: RetrievalMode::CurrentState,
        selected: vec![memory.revision.clone()],
        sources: vec![],
        status: RecallStatus::Found,
        coverage: Coverage {
            complete: true,
            gaps: vec![],
        },
        truncation: None,
        config_generation: 1,
        index_generation: Some("fixture-index-generation-1".into()),
    }
}

async fn review_action(domain: &Domain<'_>, memory: &str, action: ReviewAction, name: &str) -> u64 {
    let current = domain.memory(memory).unwrap().unwrap();
    let target = ReviewTarget {
        memory_id: memory.into(),
        proposal_revision: current.revision.commit_seq,
        semantic_sha256: current.record.semantic_sha256.clone(),
        action,
    };
    let ingress =
        Ingress::from_verified_review_channel("user-1", name, scope(), target.clone()).unwrap();
    let span = evidence(
        domain,
        &ingress,
        name,
        "Review this exact revision",
        scope(),
    )
    .await;
    domain
        .review(
            &ReviewRequest {
                write: MemoryWrite {
                    request_id: format!("review-{name}"),
                    memory_id: memory.into(),
                    expected_revision: Some(current.revision.commit_seq),
                    draft: current.record.draft,
                },
                semantic_sha256: target.semantic_sha256,
                source: span,
                reason: "explicit review".into(),
                action,
            },
            &ingress,
            &policy(ReviewMode::Manual),
        )
        .await
        .unwrap()
        .commit_seq
}

#[tokio::test]
async fn retired_and_rejected_history_is_bounded_and_keeps_approval_sources() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let domain = Domain::new(&store);
    grant(&domain).await;
    let request = fact(&domain, "reviewed", "A").await;
    let proposed = domain
        .propose(
            &request,
            &agent(Origin::Extraction),
            &policy(ReviewMode::Manual),
        )
        .await
        .unwrap()
        .commit_seq;
    let approved = review_action(&domain, "reviewed", ReviewAction::Approve, "approve").await;
    let view = domain.active("reviewed").unwrap().unwrap();
    let draft = recall(&view, approved);
    domain.prepare_recall(&reader(), &draft).await.unwrap();
    let receipt = domain
        .recall_receipt(&reader(), &draft.request_id)
        .await
        .unwrap();
    assert!(
        receipt
            .data
            .record
            .sources
            .contains(&view.record.approval.unwrap().source)
    );
    let retired = review_action(&domain, "reviewed", ReviewAction::Reject, "retire").await;
    assert!(matches!(
        domain
            .get(&reader(), "reviewed", Some(retired))
            .await
            .unwrap()
            .data,
        MemoryState::Retired(_)
    ));
    assert_eq!(
        active(
            &domain
                .get(&reader(), "reviewed", Some(approved))
                .await
                .unwrap()
        )
        .memory
        .revision
        .commit_seq,
        approved
    );
    assert!(
        domain
            .state(&reader(), &scope(), retired, None, budget(8))
            .await
            .unwrap()
            .data
            .items
            .is_empty()
    );
    let first = domain
        .history(&reader(), "reviewed", 1..=retired, None, budget(1))
        .await
        .unwrap();
    assert_eq!(
        first.data.items[0].revision.memory.revision.commit_seq,
        proposed
    );
    let next = first.data.next.unwrap();
    assert_eq!(
        domain
            .history(
                &reader(),
                "reviewed",
                approved..=retired,
                Some(&next),
                budget(1)
            )
            .await
            .unwrap_err()
            .code,
        "INVALID_CURSOR"
    );
    let second = domain
        .history(&reader(), "reviewed", 1..=retired, Some(&next), budget(1))
        .await
        .unwrap();
    assert_eq!(second.data.items[0].status, HistoricalStatus::Superseded);
    let third = domain
        .history(
            &reader(),
            "reviewed",
            1..=retired,
            second.data.next.as_ref(),
            budget(1),
        )
        .await
        .unwrap();
    assert_eq!(third.data.items[0].status, HistoricalStatus::Retired);
    assert!(third.data.next.is_none());
    let tiny = PageBudget {
        limit: 8,
        max_bytes: 1,
    };
    assert_eq!(
        domain
            .history(&reader(), "reviewed", 1..=retired, None, tiny)
            .await
            .unwrap_err()
            .code,
        "RESULT_TOO_LARGE"
    );
    assert_eq!(
        domain
            .state(&reader(), &scope(), approved, None, tiny)
            .await
            .unwrap_err()
            .code,
        "RESULT_TOO_LARGE"
    );
    let historical = RecallDraft {
        request_id: "historical".into(),
        retrieval_mode: RetrievalMode::HistoricalState,
        ..draft.clone()
    };
    domain.prepare_recall(&reader(), &historical).await.unwrap();
    let unsupported = RecallDraft {
        request_id: "unsupported".into(),
        retrieval_mode: RetrievalMode::HistoricalSemantic,
        ..draft
    };
    assert_eq!(
        domain
            .prepare_recall(&reader(), &unsupported)
            .await
            .unwrap_err()
            .code,
        "HISTORICAL_SEMANTIC_UNSUPPORTED"
    );
    let mut pending = request;
    pending.request_id = "new-candidate".into();
    pending.expected_revision = Some(retired);
    domain
        .propose(
            &pending,
            &agent(Origin::Extraction),
            &policy(ReviewMode::Manual),
        )
        .await
        .unwrap();
    let rejected = review_action(&domain, "reviewed", ReviewAction::Reject, "reject-pending").await;
    assert!(matches!(
        domain
            .get(&reader(), "reviewed", Some(rejected))
            .await
            .unwrap()
            .data,
        MemoryState::Retired(_)
    ));
    let last = domain
        .history(&reader(), "reviewed", rejected..=rejected, None, budget(1))
        .await
        .unwrap();
    assert_eq!(last.data.items[0].status, HistoricalStatus::Rejected);
    store.close().await.unwrap();
}

#[tokio::test]
async fn queued_reads_and_receipt_preparation_observe_permission_shrink() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let domain = Domain::new(&store);
    let permission = grant(&domain).await;
    let request = fact(&domain, "choice", "A").await;
    let at = domain
        .propose(
            &request,
            &agent(Origin::Extraction),
            &policy(ReviewMode::Automatic),
        )
        .await
        .unwrap()
        .commit_seq;
    let draft = recall(&domain.active("choice").unwrap().unwrap(), at);
    let context = reader();
    let narrower = Scope {
        project: Some("other-project".into()),
        ..scope()
    };
    let (shrink, read, prepare) = tokio::join!(biased;
        domain.set_read_grant("shrink", "reader", Some(&narrower), Some(permission), 2),
        domain.get(&context, "choice", Some(at)),
        domain.prepare_recall(&context, &draft),
    );
    assert_eq!(shrink.unwrap().commit_seq, at + 1);
    assert_eq!(read.unwrap_err().code, "READ_DENIED");
    assert_eq!(prepare.unwrap_err().code, "READ_DENIED");
    assert_eq!(store.sequence().unwrap(), at + 1);
    assert!(store.receipt(&draft.request_id).unwrap().is_none());
    store.close().await.unwrap();
}

#[tokio::test]
async fn receipt_phase_updates_cannot_rewrite_the_original_retrieval_metadata() {
    use akasha_otsuge::storage::{Change, CommitRequest};
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let domain = Domain::new(&store);
    grant(&domain).await;
    let request = fact(&domain, "choice", "A").await;
    let at = domain
        .propose(
            &request,
            &agent(Origin::Extraction),
            &policy(ReviewMode::Automatic),
        )
        .await
        .unwrap()
        .commit_seq;
    let draft = recall(&domain.active("choice").unwrap().unwrap(), at);
    let prepared = domain.prepare_recall(&reader(), &draft).await.unwrap();
    let mut record = domain
        .recall_receipt(&reader(), &draft.request_id)
        .await
        .unwrap()
        .data
        .record;
    record.index_generation = Some("rewritten-history".into());
    record.delivery = DeliveryPhase::Delivered {
        request_id: "injected-delivery".into(),
    };
    let key = prepared
        .revisions
        .iter()
        .find(|v| v.key.kind == akasha_otsuge::storage::RecordKind::State)
        .unwrap()
        .key
        .clone();
    store
        .commit(&CommitRequest {
            request_id: "injected-delivery".into(),
            config_generation: 1,
            policy_revision: None,
            changes: vec![Change {
                key,
                expected_revision: Some(prepared.commit_seq),
                content: serde_json::to_string(&record).unwrap(),
            }],
        })
        .await
        .unwrap();
    assert_eq!(
        domain
            .recall_receipt(&reader(), &draft.request_id)
            .await
            .unwrap_err()
            .code,
        "RECALL_CORRUPT"
    );
    store.close().await.unwrap();
}

#[tokio::test]
async fn receipt_replays_exact_bytes_after_replacement_index_switch_and_reopen() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let domain = Domain::new(&store);
    let permission = grant(&domain).await;
    let mut request = fact(&domain, "choice", "A").await;
    let ingress = agent(Origin::Extraction);
    let p = policy(ReviewMode::Automatic);
    let a = domain
        .propose(&request, &ingress, &p)
        .await
        .unwrap()
        .commit_seq;
    let draft = recall(&domain.active("choice").unwrap().unwrap(), a);
    let prepared = domain.prepare_recall(&reader(), &draft).await.unwrap();
    assert_eq!(
        domain.prepare_recall(&reader(), &draft).await.unwrap(),
        prepared
    );
    let first = domain
        .recall_receipt(&reader(), &draft.request_id)
        .await
        .unwrap();
    assert_eq!(first.data.output.as_bytes(), draft.output.as_bytes());
    assert_eq!(first.data.record.output_bytes, draft.output.len());
    assert_eq!(first.data.record.delivery, DeliveryPhase::Prepared);
    assert_eq!(first.data.record.sources, request.draft.source_spans);
    assert_eq!(first.data.record.permission_revision, permission);
    assert_eq!(
        (
            first.data.record.read_seq,
            first.data.record.validated_at_seq
        ),
        (a, a)
    );
    assert_eq!(first.data.record.storage_format, 2);
    assert_eq!(first.retrieval_mode, RetrievalMode::ReceiptReplay);
    let mut conflicting = draft.clone();
    conflicting.output.push('!');
    let before = store.sequence().unwrap();
    assert_eq!(
        domain
            .prepare_recall(&reader(), &conflicting)
            .await
            .unwrap_err()
            .code,
        "IDEMPOTENCY_CONFLICT"
    );
    assert_eq!(
        domain
            .record_delivery(&reader(), &draft.request_id, "bad-hash", &"0".repeat(64))
            .await
            .unwrap_err()
            .code,
        "DELIVERY_HASH_MISMATCH"
    );
    assert_eq!(store.sequence().unwrap(), before);
    let delivered = domain
        .record_delivery(
            &reader(),
            &draft.request_id,
            "delivered-1",
            &first.data.record.output_sha256,
        )
        .await
        .unwrap();
    assert_eq!(
        domain
            .record_delivery(
                &reader(),
                &draft.request_id,
                "delivered-1",
                &first.data.record.output_sha256
            )
            .await
            .unwrap(),
        delivered
    );
    let receipt = domain
        .recall_receipt(&reader(), &draft.request_id)
        .await
        .unwrap()
        .data;
    assert_eq!(
        receipt.record.delivery,
        DeliveryPhase::Delivered {
            request_id: "delivered-1".into()
        }
    );
    request.request_id = "replace".into();
    request.expected_revision = Some(a);
    request.draft.claim = "B".into();
    domain.propose(&request, &ingress, &p).await.unwrap();
    let mut new_draft = recall(
        &domain.active("choice").unwrap().unwrap(),
        store.sequence().unwrap(),
    );
    new_draft.request_id = "recall-2".into();
    new_draft.output = "B".into();
    new_draft.index_generation = Some("fixture-index-generation-2".into());
    new_draft.config_generation = 2;
    domain.prepare_recall(&reader(), &new_draft).await.unwrap();
    assert_eq!(
        domain
            .prepare_recall(&reader(), &draft)
            .await
            .unwrap_err()
            .code,
        "STALE_RECALL"
    );
    let backup = fixture.0.join("receipt-backup");
    store.checkpoint(&backup).await.unwrap();
    store.close().await.unwrap();
    let restored_config = resolve(
        "",
        None,
        "",
        &Paths {
            data_dir: fixture.0.join("restored"),
            socket_path: fixture.0.join("restored.sock"),
        },
    )
    .unwrap();
    let restored = Storage::restore(&restored_config, &backup).await.unwrap();
    assert_eq!(
        Domain::new(&restored)
            .recall_receipt(&reader(), &draft.request_id)
            .await
            .unwrap()
            .data,
        receipt
    );
    assert_eq!(
        active(
            &Domain::new(&restored)
                .get(&reader(), "choice", Some(a))
                .await
                .unwrap()
        )
        .memory
        .record
        .draft
        .claim,
        "A"
    );
    restored.close().await.unwrap();
    let reopened = fixture.open().await;
    let domain = Domain::new(&reopened);
    assert_eq!(
        domain
            .recall_receipt(&reader(), &draft.request_id)
            .await
            .unwrap()
            .data,
        receipt
    );
    let other = ReadContext::from_verified_binding("reader", "other-call", "run-1").unwrap();
    assert_eq!(
        domain
            .recall_receipt(&other, &draft.request_id)
            .await
            .unwrap_err()
            .code,
        "READ_DENIED"
    );
    domain
        .set_read_grant("revoke", "reader", None, Some(permission), 3)
        .await
        .unwrap();
    assert_eq!(
        domain
            .recall_receipt(&reader(), &draft.request_id)
            .await
            .unwrap_err()
            .code,
        "READ_DENIED"
    );
    reopened.close().await.unwrap();
}

#[tokio::test]
async fn scope_pages_bind_the_point_and_permission_revocation_remains_live() {
    let fixture = Fixture::new();
    let store = fixture.open().await;
    let domain = Domain::new(&store);
    let permission = grant(&domain).await;
    let ingress = agent(Origin::Extraction);
    for name in ["one", "two", "three"] {
        let request = fact(&domain, name, name).await;
        domain
            .propose(&request, &ingress, &policy(ReviewMode::Automatic))
            .await
            .unwrap();
    }
    let at = store.sequence().unwrap();
    let first = domain
        .state(&reader(), &scope(), at, None, budget(1))
        .await
        .unwrap();
    assert_eq!(first.data.items.len(), 1);
    let cursor = first.data.next.clone().unwrap();
    let later = fact(&domain, "four", "four").await;
    domain
        .propose(&later, &ingress, &policy(ReviewMode::Automatic))
        .await
        .unwrap();
    assert_eq!(
        domain
            .state(&reader(), &scope(), at + 1, Some(&cursor), budget(1))
            .await
            .unwrap_err()
            .code,
        "INVALID_CURSOR"
    );
    assert_eq!(
        domain
            .state(
                &reader(),
                &policy(ReviewMode::Manual).scope,
                at,
                Some(&cursor),
                budget(1)
            )
            .await
            .unwrap_err()
            .code,
        "INVALID_CURSOR"
    );
    store.close().await.unwrap();
    let reopened = fixture.open().await;
    let domain = Domain::new(&reopened);
    let mut ids = vec![first.data.items[0].memory.revision.key.id.clone()];
    let mut next = Some(cursor);
    while let Some(cursor) = next {
        let page = domain
            .state(&reader(), &scope(), at, Some(&cursor), budget(1))
            .await
            .unwrap();
        assert_eq!(page.read_seq, at);
        assert!(page.data.examined <= 1);
        ids.extend(
            page.data
                .items
                .iter()
                .map(|v| v.memory.revision.key.id.clone()),
        );
        next = page.data.next;
    }
    ids.sort();
    assert_eq!(ids, ["one", "three", "two"]);
    domain
        .set_read_grant("revoke", "reader", None, Some(permission), 2)
        .await
        .unwrap();
    assert_eq!(
        domain
            .get(&reader(), "one", Some(at))
            .await
            .unwrap_err()
            .code,
        "READ_DENIED"
    );
    assert_eq!(
        domain
            .state(&reader(), &scope(), at, None, budget(4))
            .await
            .unwrap_err()
            .code,
        "READ_DENIED"
    );
    assert_eq!(
        domain
            .history(&reader(), "one", 1..=at, None, budget(4))
            .await
            .unwrap_err()
            .code,
        "READ_DENIED"
    );
    reopened.close().await.unwrap();
}
