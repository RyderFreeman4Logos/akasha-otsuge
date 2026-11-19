use super::*;
use akasha_otsuge::{
    adapter::codex::{TRANSCRIPT_STREAM, reconcile_transcript},
    capture::{Capture, CaptureBinding, CoverageLevel, EventRef, SourceIdentity},
    config::{Paths, ReviewMode, resolve},
    domain::*,
    storage::Storage,
};
use sha2::{Digest, Sha256};
use std::fs;

fn hash(bytes: &[u8]) -> String {
    akasha_otsuge::capture::encode_hex(&Sha256::digest(bytes))
}

fn frozen_case(id: &str) -> Value {
    include_str!("../../docs/evaluation/reference-v1/tasks.jsonl")
        .lines()
        .map(|line| serde_json::from_str::<Value>(line).unwrap())
        .find(|case| case["id"] == id)
        .unwrap()
}

// Project the frozen contents onto the OS-authenticated native caller. Physical commit
// numbers and Codex event handles are assigned by storage and the transcript decoder.
fn prepare(daemon: &Daemon, case: &Value) -> u64 {
    let workspace = daemon.root.join("workspace");
    let path = workspace.join("native.jsonl");
    let mut rows: Vec<Value> = fs::read_to_string(&path)
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    let original = &case["sources"][0];
    let text = original["text"].as_str().unwrap();
    assert_eq!(original["channel"], "tool");
    assert_eq!(original["sha256"], hash(text.as_bytes()));
    rows[3] = json!({"ordinal":3,"type":"response_item","payload":{
        "type":"function_call_output","call_id":"historical-tool","output":text}});
    if let Some(withdrawal) = case["sources"].as_array().unwrap().get(1) {
        assert_eq!(withdrawal["channel"], "verified_human");
        rows[4] = json!({"ordinal":4,"type":"response_item","payload":{
            "type":"message","role":"user","content":[{"type":"input_text","text":withdrawal["text"]}]}});
    }
    rows[7]["payload"]["content"][0]["text"] = case["current_task"].clone();
    rows[8]["payload"]["item"]["content"][0]["text"] = case["current_task"].clone();
    fs::write(
        &path,
        rows.iter().map(|v| format!("{v}\n")).collect::<String>(),
    )
    .unwrap();
    let query = text
        .split_whitespace()
        .take(3)
        .collect::<Vec<_>>()
        .join(" ");
    fs::write(daemon.root.join("proposal-query"), query).unwrap();
    fs::write(
        daemon.root.join("proposal_evidence.py"),
        include_str!("../fixtures/codex/0.164.0/proposal_evidence.py"),
    )
    .unwrap();
    let session = rows[0]["payload"]["session_id"].as_str().unwrap();
    let source = SourceIdentity {
        adapter: "codex".into(),
        profile: "default".into(),
        session: session.into(),
        thread: session.into(),
        agent: "main".into(),
    };
    let mut scope: Scope = serde_json::from_value(case["caller"]["scope"].clone()).unwrap();
    scope.user = format!("uid:{}", rustix::process::geteuid().as_raw());
    let caller = Scope {
        project: None,
        ..scope.clone()
    };
    let config = resolve(
        "",
        None,
        "",
        &Paths {
            data_dir: daemon.root.join("data"),
            socket_path: daemon.socket(),
        },
    )
    .unwrap();
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let store = Storage::open(&config).await.unwrap();
            reconcile_transcript(&store, &source, caller.clone(), &path, 64, 1)
                .await
                .unwrap();
            let binding = CaptureBinding::from_source(
                source.clone(),
                caller,
                TRANSCRIPT_STREAM,
                CoverageLevel::VisibleText,
            )
            .unwrap();
            let captured = Capture::new(&store)
                .event(
                    &binding,
                    &EventRef {
                        event_id: "row:3".into(),
                        source,
                    },
                    store.sequence().unwrap(),
                )
                .unwrap()
                .unwrap();
            let [span] = captured.event.normalized.as_slice() else {
                panic!("original span missing")
            };
            assert_eq!(
                (span.content_sha256.as_str(), span.start, span.end),
                (original["sha256"].as_str().unwrap(), 0, text.len())
            );
            let memory = &case["memories"][0];
            let write = MemoryWrite {
                request_id: "frozen-candidate".into(),
                memory_id: memory["id"].as_str().unwrap().into(),
                expected_revision: None,
                draft: MemoryDraft {
                    kind: MemoryKind::Fact,
                    scope: scope.clone(),
                    claim: memory["claim"].as_str().unwrap().into(),
                    conditions: serde_json::from_value(memory["conditions"].clone()).unwrap(),
                    rationale: String::new(),
                    assumptions: vec![],
                    source_spans: vec![span.clone()],
                    valid_from_ms: None,
                    valid_until_ms: None,
                },
            };
            let domain = Domain::new(&store);
            let policy = Policy {
                revision: "fixture-authority".into(),
                config_generation: 1,
                scope: scope.clone(),
                mode: ReviewMode::Manual,
                adopt_explicit_decisions: false,
            };
            let ingress = Ingress::tool("fixture", "frozen-original", scope.clone()).unwrap();
            let revision = domain
                .propose(&write, &ingress, &policy)
                .await
                .unwrap()
                .commit_seq;
            let candidate = domain.memory(&write.memory_id).unwrap().unwrap();
            assert_eq!(candidate.record.authority, Authority::Candidate);
            if let Some(withdrawal) = case["sources"].as_array().unwrap().get(1) {
                // A transcript containing a user's words alone cannot sign a rejection.
                let target = ReviewTarget {
                    memory_id: write.memory_id.clone(),
                    proposal_revision: revision,
                    semantic_sha256: candidate.record.semantic_sha256.clone(),
                    action: ReviewAction::Reject,
                };
                let channel = Ingress::from_verified_review_channel(
                    &scope.user,
                    "frozen-withdrawal",
                    scope.clone(),
                    target,
                )
                .unwrap();
                let change = SourceDraft {
                    event_id: withdrawal["id"].as_str().unwrap().into(),
                    scope: scope.clone(),
                    text: withdrawal["text"].as_str().unwrap().into(),
                    reported_role: None,
                    event_time_ms: None,
                    observed_time_ms: 2,
                };
                let receipt = domain
                    .record_source("verified-withdrawal", &change, &channel, 1)
                    .await
                    .unwrap();
                assert_eq!(hash(change.text.as_bytes()), withdrawal["sha256"]);
                let request = ReviewRequest {
                    action: ReviewAction::Reject,
                    semantic_sha256: candidate.record.semantic_sha256,
                    source: SourceSpan {
                        event_id: change.event_id,
                        revision: receipt.commit_seq,
                        content_sha256: hash(change.text.as_bytes()),
                        start: 0,
                        end: change.text.len(),
                    },
                    reason: change.text,
                    write: MemoryWrite {
                        request_id: "apply-withdrawal".into(),
                        expected_revision: Some(revision),
                        ..write.clone()
                    },
                };
                domain.review(&request, &channel, &policy).await.unwrap();
                assert_eq!(
                    domain
                        .memory(&write.memory_id)
                        .unwrap()
                        .unwrap()
                        .record
                        .lifecycle,
                    Lifecycle::Rejected
                );
            }
            assert!(domain.active(&write.memory_id).unwrap().is_none());
            store.close().await.unwrap();
            revision
        })
}

fn exercise(id: &str) {
    let case = frozen_case(id);
    let mut revision = 0;
    let (mut daemon, mut event, _) = consult_fixture_prepared(
        "conclusion_proposal_evidence",
        |_| String::new(),
        |d| revision = prepare(d, &case),
    );
    event["args"]["hint"] = case["hint"].clone();
    let sync = daemon.cli(&[
        "index",
        "sync",
        "--idempotency-key",
        "frozen-proposal-index",
    ]);
    let sync_id = sync["data"]["id"].as_str().unwrap();
    assert_eq!(
        crate::extraction_tests::finished(&daemon, sync_id)["state"],
        "succeeded"
    );
    let job = named_terminal(&daemon, event);
    assert_eq!(job["state"], "succeeded", "{job}");
    let job_id = job["id"].as_str().unwrap();
    let observed: Value =
        serde_json::from_slice(&fs::read(daemon.root.join("proposal-observed.json")).unwrap())
            .unwrap();
    let live = daemon.cli(&["consult", "show", job_id])["data"].clone();
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(daemon.cli(&["consult", "show", job_id])["data"], live);
    let archived = daemon.cli(&["consult", "receipt", job_id])["data"]["data"].clone();
    assert_eq!(
        serde_json::from_str::<Value>(archived["output"].as_str().unwrap()).unwrap(),
        live
    );
    let record = &archived["record"];
    let source = &case["sources"][0];
    let index = record["sources"]
        .as_array()
        .unwrap()
        .iter()
        .position(|s| {
            s["content_sha256"] == source["sha256"]
                && s["start"] == 0
                && s["end"] == source["text"].as_str().unwrap().len()
        })
        .unwrap();
    assert_eq!(
        daemon.cli(&["consult", "original", job_id, &index.to_string()])["data"]["output"],
        source["text"]
    );
    eprintln!(
        "PROPOSAL_FIXTURE {}",
        json!({
            "case": id, "memory_id":case["memories"][0]["id"], "native_revision":revision,
            "status":live["status"], "original_reopened":true, "observed":observed,
            "selected":record["selected"], "proposals":record["proposals"],
            "public_authority":live["memories"][0]["authority"], "public_origin":live["memories"][0]["origin"],
            "conditions":live["conditions"],
            "answer_matches_registered_claim":live["answer"] == case["memories"][0]["claim"],
        })
    );
    assert_eq!(observed["original"]["sha256"], source["sha256"]);
    assert_eq!(observed["adopted"], json!([]));
    assert_eq!(record["selected"], json!([]));
    for phase in ["unread", "partial", "complete"] {
        assert_eq!(observed[phase]["error"], Value::Null, "{phase}: {observed}");
    }
    for phase in ["unread", "partial"] {
        assert_eq!(observed[phase]["handles"], json!([]));
    }
    if id == "C03" {
        assert_eq!(observed["complete"]["handles"], json!([]));
        assert!(record["proposals"].as_array().is_none_or(Vec::is_empty));
        assert!(live["memories"].as_array().is_none_or(Vec::is_empty));
    } else {
        let memory = &case["memories"][0];
        assert_eq!(
            observed["complete"]["handles"],
            json!([{"memory_id":memory["id"],"revision":revision,"authority":"candidate"}])
        );
        assert_eq!(record["proposals"][0]["revision"]["commit_seq"], revision);
        assert_eq!(
            record["proposals"][0]["revision"]["key"]["id"],
            memory["id"]
        );
        assert_eq!(live["memories"][0]["authority"], "candidate");
        assert_eq!(observed["expanded_origin"], "technical_observation");
        assert_eq!(live["memories"][0]["origin"], observed["expanded_origin"]);
        assert_eq!(live["memories"][0]["lifecycle"], "candidate");
        assert_eq!(live["memories"][0]["reference"]["commit_seq"], revision);
        assert_eq!(live["memories"][0]["conditions"], memory["conditions"]);
        assert_eq!(live["conditions"], memory["conditions"]);
        assert_eq!(live["answer"], memory["claim"]);
    }
}

#[test]
fn c02_untrusted_tool_text_is_only_candidate_evidence() {
    exercise("C02")
}
#[test]
fn h04_tentative_proposal_preserves_conditions() {
    exercise("H04")
}
#[test]
fn c03_withdrawn_candidate_is_absent() {
    exercise("C03")
}
