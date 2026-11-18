//! Native, model-free inspection of one frozen episode in a fresh authority store.
use akasha_otsuge::{
    capture::{
        Capture, CaptureBinding, CoverageLevel, EventDraft, EventKind, EventRef, EventView,
        MessagePhase, SourceIdentity, Visibility,
    },
    config::{Paths, ReviewMode, resolve},
    domain::{
        Authority, Domain, Ingress, MemoryDraft, MemoryKind, MemoryState, MemoryWrite,
        OriginalRange, PageBudget, Policy, ReadContext, ReadResult, ReviewAction, ReviewRequest,
        ReviewTarget, Scope, StatementIntent,
    },
    retrieval::Index,
    storage::Storage,
};
use serde_json::{Value, json};
use std::{
    collections::{BTreeMap, BTreeSet},
    path::PathBuf,
};

fn text<'a>(v: &'a Value, k: &str) -> &'a str {
    v[k].as_str().unwrap()
}
fn number(v: &Value, k: &str) -> u64 {
    v[k].as_u64().unwrap()
}
fn scope(v: &Value) -> Scope {
    serde_json::from_value(v["scope"].clone()).unwrap()
}
fn source_identity(case: &str, thread: &str) -> SourceIdentity {
    SourceIdentity {
        adapter: "fixture".into(),
        profile: "reference-v1".into(),
        session: case.into(),
        thread: thread.into(),
        agent: "main".into(),
    }
}
fn fixture_channel(source: &Value, case: &Value) -> Ingress {
    let bound = scope(source);
    let actor = bound.user.clone();
    let id = text(source, "id");
    let confirmed_source = case["memories"].as_array().unwrap().iter().any(|m| {
        m["authority"] == "human_confirmed"
            && m["source_ids"].as_array().unwrap().iter().any(|v| v == id)
    });
    match text(source, "channel") {
        "verified_human" => Ingress::from_verified_human_channel(
            &actor,
            id,
            bound,
            if confirmed_source {
                StatementIntent::ExplicitDecision
            } else {
                StatementIntent::Other
            },
        )
        .unwrap(),
        "tool" => Ingress::tool("fixture-tool", id, bound).unwrap(),
        _ => panic!("unsupported frozen channel"),
    }
}
fn same_thread_parent<'a>(
    sources: &'a [Value],
    thread: &str,
    scope: &Scope,
    through: u64,
) -> Option<&'a str> {
    sources
        .iter()
        .filter(|s| {
            text(s, "thread") == thread
                && s["available"] == true
                && scope.contains(&serde_json::from_value(s["scope"].clone()).unwrap())
                && number(s, "sequence") <= through
        })
        .max_by_key(|s| number(s, "sequence"))
        .map(|s| text(s, "id"))
}
fn event_kind(s: &Value, events: &BTreeMap<String, EventView>) -> EventKind {
    match s["kind"].as_str() {
        Some("compaction") => {
            let originals: Vec<_> = s["original_event_ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| events[id.as_str().unwrap()].event.reference.clone())
                .collect();
            EventKind::Compaction {
                first: Box::new(originals.first().unwrap().clone()),
                last: Box::new(originals.last().unwrap().clone()),
                originals,
            }
        }
        Some("fork") => EventKind::Fork,
        None | Some("withdrawal") => EventKind::Message {
            message_id: text(s, "id").into(),
            phase: MessagePhase::Final,
        },
        _ => panic!("unmapped event kind"),
    }
}

// The proof contains identifiers and hashes only. Private or withdrawn content stays in the store.
fn read_stamp(read: &ReadResult<MemoryState>) -> Value {
    let (state, revision) = match &read.data {
        MemoryState::Absent => ("absent", None),
        MemoryState::Active(v) => ("active", Some(&v.memory)),
        MemoryState::Retired(v) => ("retired", Some(&v.memory)),
    };
    json!({"state":state, "commit":revision.map(|v|v.revision.commit_seq),
        "authority":revision.map(|v|v.record.authority),
        "semantic_sha256":revision.map(|v|&v.record.semantic_sha256),
        "mode":read.retrieval_mode,"read_seq":read.read_seq,"validated_at_seq":read.validated_at_seq})
}

fn memory_draft(m: &Value, events: &BTreeMap<String, EventView>) -> MemoryDraft {
    let sources_for_m = m["source_ids"].as_array().unwrap();
    let spans = sources_for_m
        .iter()
        .map(|s| events[s.as_str().unwrap()].event.normalized[0].clone())
        .collect();
    MemoryDraft {
        kind: MemoryKind::Fact,
        scope: scope(m),
        claim: text(m, "claim").into(),
        rationale: String::new(),
        conditions: serde_json::from_value(m["conditions"].clone()).unwrap(),
        assumptions: vec![],
        source_spans: spans,
        valid_from_ms: None,
        valid_until_ms: None,
    }
}

#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args_os().skip(1);
    let case: Value =
        serde_json::from_slice(&std::fs::read(args.next().unwrap()).unwrap()).unwrap();
    let root = PathBuf::from(args.next().unwrap());
    assert!(args.next().is_none() && !root.join("data").exists());
    assert!(case.get("oracle").is_none() && case.get("category").is_none());
    let id = text(&case, "id");
    let caller = &case["caller"];
    let bound = scope(caller);
    let cfg = resolve(
        "",
        None,
        "",
        &Paths {
            data_dir: root.join("data"),
            socket_path: root.join("s"),
        },
    )
    .unwrap();
    let store = Storage::open(&cfg).await.unwrap();
    let domain = Domain::new(&store);
    let capture = Capture::new(&store);
    let grant = domain
        .set_read_grant("fixture-grant", &bound.user, Some(&bound), None, 1)
        .await
        .unwrap();
    let reader = ReadContext::from_verified_binding(&bound.user, id, "reference-suite").unwrap();
    let mut sources = case["sources"].as_array().unwrap().clone();
    sources.sort_by_key(|s| number(s, "sequence"));
    let mut events: BTreeMap<String, EventView> = BTreeMap::new();
    let mut source_receipts = Vec::new();
    let mut revisions: BTreeMap<String, u64> = BTreeMap::new();
    let mut memory_rows = Vec::new();
    let mut mems = case["memories"].as_array().unwrap().clone();
    mems.sort_by_key(|m| (number(m, "revision"), text(m, "id").to_owned()));
    let mut ordinal_commits = BTreeMap::new();
    let mut transitions = Vec::new();
    for s in &sources {
        assert!(s["available"] == true);
        let thread = text(s, "thread");
        let eid = text(s, "id");
        let payload = text(s, "text");
        let origin = scope(s);
        let withdrawal = if s["kind"] == "withdrawal" {
            assert_eq!(s["channel"], "verified_human");
            let [target] = mems.as_slice() else {
                panic!("ambiguous frozen withdrawal")
            };
            assert_eq!(scope(target), origin);
            assert_eq!(target["status"], "retired");
            assert!(
                target["source_ids"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .all(|x| events.contains_key(x.as_str().unwrap()))
            );
            domain.memory(text(target, "id")).unwrap()
        } else {
            None
        };
        let channel = if let Some(prior) = &withdrawal {
            Ingress::from_verified_review_channel(
                &origin.user,
                eid,
                origin.clone(),
                ReviewTarget {
                    memory_id: prior.revision.key.id.clone(),
                    proposal_revision: prior.revision.commit_seq,
                    semantic_sha256: prior.record.semantic_sha256.clone(),
                    action: ReviewAction::Reject,
                },
            )
            .unwrap()
        } else {
            fixture_channel(s, &case)
        };
        let binding = CaptureBinding::from_source(
            source_identity(id, thread),
            origin.clone(),
            "original",
            CoverageLevel::AvailablePayloads,
        )
        .unwrap();
        let parent_id = s["parent_event_id"]
            .as_str()
            .or_else(|| same_thread_parent(&sources, thread, &origin, number(s, "sequence") - 1));
        let parent = parent_id.map(|p| events[p].event.reference.clone());
        let draft = EventDraft {
            event_id: eid.into(),
            source_version: "1".into(),
            runtime_instance: id.into(),
            branch: thread.into(),
            parent: parent.clone(),
            fork_cutoff: parent.filter(|p| p.source.thread != thread),
            event_time_ms: None,
            observed_time_ms: number(s, "sequence") as i64,
            visibility: if s["channel"] == "tool" {
                Visibility::Tool
            } else {
                Visibility::User
            },
            kind: event_kind(s, &events),
            text: payload.into(),
        };
        let v = capture
            .ingest(&binding, &draft, payload.as_bytes(), &channel, None, 1)
            .await
            .unwrap();
        assert_eq!(capture.raw(&v).unwrap(), payload.as_bytes());
        assert_eq!(
            store
                .receipt(&v.event.publication_request_id)
                .unwrap()
                .unwrap()
                .commit_seq,
            v.revision
        );
        assert_eq!(v.event.text_sha256, s["sha256"]);
        assert_eq!(v.event.raw_sha256, s["sha256"]);
        let span = &v.event.normalized[0];
        assert_eq!(v.event.normalized.len(), 1);
        let original = domain.source(&span.event_id, span.revision).unwrap();
        assert_eq!(original.content_sha256, s["sha256"]);
        assert_eq!(original.payload.scope, origin);
        let req = OriginalRange {
            event_id: span.event_id.clone(),
            revision: span.revision,
            start: 0,
            end: payload.len(),
        };
        let scoped = domain
            .original_range(&reader, &req, &format!("scope-probe/{eid}"), 8192, 1)
            .await;
        let read = match scoped {
            Ok(o) => {
                assert_eq!(o.output, payload);
                "allowed"
            }
            Err(e) => {
                assert_eq!(e.code, "READ_DENIED");
                "denied"
            }
        };
        source_receipts.push(json!({"id":eid,"ordinal":s["sequence"],"capture_commit":v.revision,"request_id":v.event.publication_request_id,
            "native_source_id":span.event_id,"source_revision":span.revision,"sha256":v.event.text_sha256,
            "actor":original.provenance.actor,"scope_read":read}));
        if let Some(prior) = withdrawal {
            // The fixture supplies this distinct verified withdrawal original. Bind that one event to
            // its sole preceding Memory, without creating a new source or an approving review.
            let mid = prior.revision.key.id.clone();
            let before = domain.get(&reader, &mid, None).await.unwrap();
            let write = MemoryWrite {
                request_id: format!("withdraw/{eid}"),
                memory_id: mid.clone(),
                expected_revision: Some(prior.revision.commit_seq),
                draft: prior.record.draft.clone(),
            };
            let request = ReviewRequest {
                write,
                action: ReviewAction::Reject,
                source: span.clone(),
                semantic_sha256: prior.record.semantic_sha256.clone(),
                reason: payload.into(),
            };
            let policy = Policy {
                revision: "fixture-withdrawal".into(),
                config_generation: 1,
                scope: origin.clone(),
                mode: ReviewMode::Manual,
                adopt_explicit_decisions: false,
            };
            let receipt = domain.review(&request, &channel, &policy).await.unwrap();
            assert_eq!(
                store.receipt(&request.write.request_id).unwrap(),
                Some(receipt.clone())
            );
            let actual = domain.revision(&mid, receipt.commit_seq).unwrap().unwrap();
            let approval = actual.record.approval.as_ref().unwrap();
            assert_eq!(approval.action, ReviewAction::Reject);
            assert_eq!(approval.source, *span);
            let after = domain.get(&reader, &mid, None).await.unwrap();
            let historical = domain
                .get(&reader, &mid, Some(prior.revision.commit_seq))
                .await
                .unwrap();
            transitions.push(json!({"operation":"withdrawal","original_id":eid,"original_sha256":s["sha256"],
                "memory_id":mid,"previous_commit":prior.revision.commit_seq,"previous_authority":prior.record.authority,
                "previous_lifecycle":prior.record.lifecycle,"revision_after":actual.record.lifecycle,
                "commit":receipt.commit_seq,"request_id":request.write.request_id,"action":approval.action,
                "current_before":read_stamp(&before),"current_after":read_stamp(&after),"historical_after":read_stamp(&historical)}));
        } else if s["kind"] == "withdrawal" {
            transitions.push(
                json!({"operation":"withdrawal","original_id":eid,"original_sha256":s["sha256"],
                "outcome":"predecessor_not_imported","commit":null}),
            );
        }
        events.insert(eid.to_owned(), v);
        for m in &mems {
            let inputs = m["source_ids"].as_array().unwrap();
            if !inputs.iter().any(|x| x == eid)
                || !inputs
                    .iter()
                    .all(|x| events.contains_key(x.as_str().unwrap()))
            {
                continue;
            }
            let mid = text(m, "id");
            let rev = number(m, "revision");
            let auth = text(m, "authority");
            if rev > 1 && !revisions.contains_key(mid) {
                memory_rows
                    .push(json!({"id":mid,"ordinal":rev,"error":"PREDECESSOR_NOT_IMPORTED"}));
                continue;
            }
            let src_id = m["source_ids"][0].as_str().unwrap();
            let source = sources.iter().find(|s| s["id"] == src_id).unwrap();
            let ingress = fixture_channel(source, &case);
            let draft = memory_draft(m, &events);
            let write = MemoryWrite {
                request_id: format!("import/{mid}/{rev}"),
                memory_id: mid.to_owned(),
                expected_revision: revisions.get(mid).copied(),
                draft,
            };
            let policy = Policy {
                revision: "fixture-authority".into(),
                config_generation: 1,
                scope: scope(m),
                mode: if auth == "delegated_approved" {
                    ReviewMode::Automatic
                } else {
                    ReviewMode::Manual
                },
                adopt_explicit_decisions: true,
            };
            let receipt = match auth {
                "human_confirmed" => {
                    domain
                        .adopt_statement(&write, &ingress, &policy, &write.draft.source_spans[0])
                        .await
                }
                "candidate" | "delegated_approved" => {
                    domain.propose(&write, &ingress, &policy).await
                }
                _ => panic!("unknown frozen authority"),
            };
            match receipt {
                Ok(r) => {
                    assert_eq!(store.receipt(&write.request_id).unwrap(), Some(r.clone()));
                    let mv = domain.revision(mid, r.commit_seq).unwrap().unwrap();
                    let actual = mv.record.authority;
                    assert_eq!(
                        actual,
                        match auth {
                            "candidate" => Authority::Candidate,
                            "delegated_approved" => Authority::DelegatedApproved,
                            _ => Authority::HumanConfirmed,
                        }
                    );
                    assert_eq!(mv.record.draft.conditions, write.draft.conditions);
                    assert_eq!(mv.record.draft.claim, write.draft.claim);
                    assert_eq!(mv.record.draft.scope, write.draft.scope);
                    assert_eq!(mv.record.draft.source_spans, write.draft.source_spans);
                    revisions.insert(mid.into(), r.commit_seq);
                    memory_rows.push(json!({"id":mid,"ordinal":rev,"commit":r.commit_seq,"request_id":write.request_id,"authority":actual,"native_lifecycle_at_write":mv.record.lifecycle,"frozen_status":m["status"]}));
                }
                Err(e) => {
                    assert_eq!(e.code, "AUTHORITY_DENIED");
                    memory_rows.push(json!({"id":mid,"ordinal":rev,"error":e.code}));
                }
            }
        }
        ordinal_commits.insert(number(s, "sequence"), store.sequence().unwrap());
    }
    assert_eq!(memory_rows.len(), mems.len());
    // Derive lineage from actual topology and cutoffs without using the fixture's expected visible IDs for selection.
    let thread = text(caller, "thread");
    let fence = number(caller, "fence_sequence");
    let mut tip = same_thread_parent(&sources, thread, &bound, fence);
    if tip.is_none()
        && let Some(a) = caller["ancestry"].as_array()
    {
        for link in a {
            tip = same_thread_parent(
                &sources,
                text(link, "thread"),
                &bound,
                fence.min(number(link, "cutoff_sequence")),
            );
            if tip.is_some() {
                break;
            }
        }
    }
    let known_tip = tip.map(|i| events[i].event.reference.clone());
    let missing = case["missing_event_ids"]
        .as_array()
        .and_then(|s| s.first())
        .and_then(Value::as_str);
    assert!(known_tip.is_none() || missing.is_none());
    let parent = known_tip.or_else(|| {
        missing.map(|eid| EventRef {
            event_id: eid.into(),
            source: source_identity(id, thread),
        })
    });
    let current = "fixture-live-request";
    let req = EventDraft {
        event_id: current.into(),
        source_version: "1".into(),
        runtime_instance: id.into(),
        branch: thread.into(),
        parent: parent.clone(),
        fork_cutoff: parent.filter(|p| p.source.thread != thread),
        event_time_ms: None,
        observed_time_ms: fence as i64,
        visibility: Visibility::User,
        kind: EventKind::Message {
            message_id: current.into(),
            phase: MessagePhase::Final,
        },
        text: text(&case, "current_task").into(),
    };
    let binding = CaptureBinding::from_source(
        source_identity(id, thread),
        bound.clone(),
        "original",
        CoverageLevel::AvailablePayloads,
    )
    .unwrap();
    let live = capture
        .ingest(
            &binding,
            &req,
            req.text.as_bytes(),
            &Ingress::unverified("offline-caller", id, bound.clone()).unwrap(),
            None,
            1,
        )
        .await
        .unwrap();
    let page = capture
        .lineage(
            &binding,
            &live.event.reference,
            store.sequence().unwrap(),
            64,
        )
        .unwrap();
    let visible: BTreeSet<_> = page
        .events
        .iter()
        .map(|v| v.event.reference.event_id.as_str())
        .filter(|x| *x != current)
        .collect();
    let expected: BTreeSet<_> = caller["allowed_event_ids"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(visible, expected);
    assert_eq!(page.missing.as_ref().map(|v| v.event_id.as_str()), missing);
    let target = store.sequence().unwrap();
    let index = Index::open(&cfg, &store).await.unwrap();
    assert_eq!(Index::status(&store, &cfg).unwrap().lexical_watermark, 0);
    // The operator owns the isolated fixture and synchronizes its entire index. Verify the original
    // grant remains current at every sync boundary. Search and source reads use the scoped caller.
    let grant_key = &grant.revisions[0].key;
    let authorize_sync = || {
        if store
            .current(grant_key)?
            .is_some_and(|r| r.commit_seq == grant.commit_seq)
        {
            Ok(())
        } else {
            Err(std::io::Error::new(
                std::io::ErrorKind::PermissionDenied,
                "fixture grant changed during index preparation",
            )
            .into())
        }
    };
    let progress = loop {
        let p = index
            .sync_batch(&store, target, 1, &authorize_sync)
            .await
            .unwrap();
        if p.lexical_watermark == target {
            break p;
        }
        assert!(p.lexical_watermark < target);
    };
    let candidates = index
        .keyword(text(&case, "current_task"), &bound, 100)
        .await
        .unwrap();
    let state = domain
        .state(
            &reader,
            &bound,
            store.sequence().unwrap(),
            None,
            PageBudget {
                limit: 32,
                max_bytes: 65536,
            },
        )
        .await
        .unwrap();
    let state_ids: Vec<_> = state
        .data
        .items
        .iter()
        .map(|x| x.memory.revision.key.id.as_str())
        .collect();
    let historical = if let Some(ordinal) = case["as_of"].as_u64() {
        let at = ordinal_commits[&ordinal];
        let [m, ..] = mems.as_slice() else {
            panic!("historical input lacks a Memory")
        };
        let mid = text(m, "id");
        let source = source_receipts
            .iter()
            .find(|s| s["ordinal"] == ordinal)
            .unwrap();
        let then = domain.get(&reader, mid, Some(at)).await.unwrap();
        let now = domain.get(&reader, mid, None).await.unwrap();
        assert_eq!(then.read_seq, at);
        assert!(then.validated_at_seq >= at && now.read_seq >= at);
        Some(
            json!({"as_of_ordinal":ordinal,"captured_original":source["id"],"source_commit":source["capture_commit"],
          "physical_cutoff":at,"memory_id":mid,"historical":read_stamp(&then),"current":read_stamp(&now),"grant_commit":grant.commit_seq}),
        )
    } else {
        None
    };
    let mut rescope = None;
    if let Some(change) = case["changes_during_run"]
        .as_array()
        .and_then(|a| a.iter().find(|v| v["operation"] == "rescope_memory"))
    {
        assert_eq!(change["at"], "after_first_source_read_before_output");
        let mid = text(change, "memory_id");
        let m = mems.iter().find(|m| m["id"] == mid).unwrap();
        let mut draft = memory_draft(m, &events);
        let [span] = draft.source_spans.as_slice() else {
            panic!("ambiguous rescope source")
        };
        let source_id = m["source_ids"][0].as_str().unwrap();
        let original = sources.iter().find(|s| s["id"] == source_id).unwrap();
        let first = domain
            .original_range(
                &reader,
                &OriginalRange {
                    event_id: span.event_id.clone(),
                    revision: span.revision,
                    start: span.start,
                    end: span.end,
                },
                "rescope-first-source",
                8192,
                1,
            )
            .await
            .unwrap();
        assert_eq!(first.output, text(original, "text"));
        let first_receipt = domain
            .recall_receipt(&reader, &first.receipt_id)
            .await
            .unwrap();
        assert_eq!(first_receipt.data.record.output_sha256, original["sha256"]);
        assert_eq!(first_receipt.data.record.scope, bound);
        let source_commit = store
            .receipt(&first.receipt_id)
            .unwrap()
            .unwrap()
            .commit_seq;
        let initial = domain.get(&reader, mid, None).await.unwrap();
        draft.scope.project = Some(text(change, "new_project").into());
        let policy = Policy {
            revision: "fixture-rescope".into(),
            scope: bound.clone(),
            mode: ReviewMode::Manual,
            config_generation: 1,
            adopt_explicit_decisions: false,
        };
        let write = MemoryWrite {
            request_id: "fixture-rescope".into(),
            memory_id: mid.into(),
            expected_revision: revisions.get(mid).copied(),
            draft,
        };
        let ingress = fixture_channel(original, &case);
        let before = store.sequence().unwrap();
        let denied = domain.propose(&write, &ingress, &policy).await.unwrap_err();
        assert_eq!(denied.code, "SCOPE_DENIED");
        assert_eq!(store.sequence().unwrap(), before);
        assert!(store.receipt(&write.request_id).unwrap().is_none());
        let after = domain.get(&reader, mid, None).await.unwrap();
        let private = domain
            .state(
                &reader,
                &write.draft.scope,
                before,
                None,
                PageBudget {
                    limit: 4,
                    max_bytes: 65536,
                },
            )
            .await
            .unwrap_err();
        assert_eq!(private.code, "READ_DENIED");
        rescope = Some(
            json!({"operation":"rescope_memory","memory_id":mid,"source_id":source_id,
          "first_read_id":first.receipt_id,"first_read_commit":source_commit,"receipt_validated_at":first_receipt.validated_at_seq,"source_sha256":first.output_sha256,
          "grant_commit":grant.commit_seq,"before":read_stamp(&initial),"after":read_stamp(&after),"requested_scope":write.draft.scope,
          "mutation_error":denied.code,"mutation_commit":null,"private_read_error":private.code,"model_publication":null}),
        );
    }
    let before_barrier = store.sequence().unwrap();
    let barrier = if case["changes_during_run"]
        .as_array()
        .is_some_and(|a| a.iter().any(|v| v["operation"] == "revoke_read_grant"))
    {
        let sid = visible.iter().next().unwrap();
        let span = &events[*sid].event.normalized[0];
        let range = OriginalRange {
            event_id: span.event_id.clone(),
            revision: span.revision,
            start: 0,
            end: span.end,
        };
        domain
            .original_range(&reader, &range, "barrier-first-read", 8192, 1)
            .await
            .unwrap();
        let revoked = domain
            .set_read_grant(
                "barrier-revoke",
                &bound.user,
                None,
                Some(grant.commit_seq),
                1,
            )
            .await
            .unwrap();
        let seq = store.sequence().unwrap();
        let denied = domain
            .original_range(&reader, &range, "barrier-after-revoke", 8192, 1)
            .await
            .unwrap_err();
        assert_eq!(denied.code, "READ_DENIED");
        assert_eq!(seq, store.sequence().unwrap());
        Some(
            json!({"before":before_barrier,"revoked_at":revoked.commit_seq,"subsequent_read":denied.code,"new_successful_read_receipt":false}),
        )
    } else {
        None
    };
    println!(
        "{}",
        json!({"id":id,"scope":bound,"grant":grant.commit_seq,"current_task_sha256":live.event.text_sha256,
        "sources":source_receipts,"native_visible_ids":visible,"missing":missing,"logical_fence":fence,"capture_seq":live.revision,
        "memories":memory_rows,"index":{"generation":progress.generation,"target":target,"watermark":progress.lexical_watermark,
        "rows":progress.indexed_rows,"candidate_ids":candidates},"native_state_ids":state_ids,"grant_barrier":barrier,"transitions":transitions,"historical_read":historical,"rescope_attempt":rescope})
    );
    drop(index);
    store.close().await.unwrap();
}
