//! Native, model-free inspection of one frozen episode in a fresh authority store.
use akasha_otsuge::{
    capture::{
        Capture, CaptureBinding, CoverageLevel, EventDraft, EventKind, EventRef, EventView,
        MessagePhase, SourceIdentity, Visibility,
    },
    config::{Paths, ReviewMode, resolve},
    domain::{
        Authority, Domain, Ingress, MemoryDraft, MemoryKind, MemoryWrite, OriginalRange,
        PageBudget, Policy, ReadContext, Scope, StatementIntent,
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
    for s in &sources {
        assert!(s["available"] == true);
        let thread = text(s, "thread");
        let eid = text(s, "id");
        let payload = text(s, "text");
        let channel = fixture_channel(s, &case);
        let origin = scope(s);
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
        events.insert(eid.to_owned(), v);
    }
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
    let mut revisions: BTreeMap<String, u64> = BTreeMap::new();
    let mut memory_rows = Vec::new();
    let mut mems = case["memories"].as_array().unwrap().clone();
    mems.sort_by_key(|m| (number(m, "revision"), text(m, "id").to_owned()));
    for m in &mems {
        let mid = text(m, "id");
        let rev = number(m, "revision");
        let auth = text(m, "authority");
        if rev > 1 && !revisions.contains_key(mid) {
            memory_rows.push(json!({"id":mid,"ordinal":rev,"error":"PREDECESSOR_NOT_IMPORTED"}));
            continue;
        }
        let sources_for_m = m["source_ids"].as_array().unwrap();
        let spans = sources_for_m
            .iter()
            .map(|s| events[s.as_str().unwrap()].event.normalized[0].clone())
            .collect();
        let src_id = sources_for_m[0].as_str().unwrap();
        let source = sources.iter().find(|s| s["id"] == src_id).unwrap();
        let ingress = fixture_channel(source, &case);
        let draft: MemoryDraft = MemoryDraft {
            kind: MemoryKind::Fact,
            scope: scope(m),
            claim: text(m, "claim").into(),
            rationale: String::new(),
            conditions: serde_json::from_value(m["conditions"].clone()).unwrap(),
            assumptions: vec![],
            source_spans: spans,
            valid_from_ms: None,
            valid_until_ms: None,
        };
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
            "candidate" | "delegated_approved" => domain.propose(&write, &ingress, &policy).await,
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
        "rows":progress.indexed_rows,"candidate_ids":candidates},"native_state_ids":state_ids,"grant_barrier":barrier})
    );
    drop(index);
    store.close().await.unwrap();
}
