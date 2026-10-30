use super::extraction_tests::scope;
use super::*;
use akasha_otsuge::config::ReviewMode;

fn patch(daemon: &Daemon, memory: &str, revision: u64, claim: &str) -> Value {
    let shown = daemon.cli(&["memory", "show", memory]);
    let mut draft = shown["data"]["data"]["revision"]["memory"]["record"]["draft"].clone();
    let evidence = draft["source_spans"].clone();
    draft["claim"] = json!(claim);
    json!({"scope":scope(),"reason":"Limit the claim to its actual fixture evidence",
        "evidence":evidence,"expires_at_ms":akasha_otsuge::ipc::now_ms().unwrap()+60_000,
        "changes":[{"memory_id":memory,"expected_revision":revision,"draft":draft}]})
}

fn submit(daemon: &Daemon, id: &str, patch: &Value) -> Value {
    rpc(
        daemon,
        id,
        json!({"op":"patch_apply","idempotency_key":id,"patch":patch}),
    )
}

fn rpc(daemon: &Daemon, id: &str, command: Value) -> Value {
    let mut stream = daemon.connect();
    receive(&mut stream);
    send(
        &mut stream,
        &json!({"protocol_version":1,"request_id":id,"command":command}),
    );
    receive(&mut stream)
}

#[test]
fn exact_editor_patch_uses_policy_and_replays_without_rewriting_history() {
    let mut daemon = Daemon::new();
    let old = super::review_tests::seed(&daemon, ReviewMode::Automatic, "editable");
    daemon.configure("[review]\nmode='automatic'\n");
    daemon.start();
    let original = daemon.cli(&["memory", "show", "editable"]);
    let patch = patch(
        &daemon,
        "editable",
        old,
        "Only the controlled fixture build passed on Linux.",
    );
    let file = daemon.root.join("patch.json");
    std::fs::write(&file, serde_json::to_vec(&patch).unwrap()).unwrap();
    let applied = daemon.cli(&[
        "memory",
        "patch",
        file.to_str().unwrap(),
        "--idempotency-key",
        "edit-one",
    ]);
    assert!(
        applied.get("error").is_none(),
        "editor patch unavailable: {applied}"
    );
    let seq = applied["data"]["commit_seq"].as_u64().unwrap();
    assert!(seq > old);
    let exported = daemon.cli(&["memory", "export", "editable"]);
    assert_eq!(
        exported["data"]["record"]["draft"],
        patch["changes"][0]["draft"]
    );
    assert_eq!(
        exported["data"]["record"]["authority"],
        "delegated_approved"
    );
    assert_eq!(
        exported["data"]["record"]["provenance"]["origin"],
        "memory_edit"
    );
    assert!(exported["data"]["record"]["approval"].is_null());
    assert_eq!(
        daemon.cli(&["memory", "show", "editable", "--at", &old.to_string()])["data"]["data"],
        original["data"]["data"]
    );
    let audit = daemon.cli(&["patch", "show", "edit-one"]);
    assert_eq!(audit["data"]["patch"], patch);
    daemon.configure("[review]\nmode='manual'\n");
    daemon.cli(&["config", "reload"]);
    assert_eq!(submit(&daemon, "edit-one", &patch)["data"], applied["data"]);
    assert_eq!(
        submit(&daemon, "stale", &patch)["error"]["code"],
        "REVISION_CONFLICT"
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(submit(&daemon, "edit-one", &patch)["data"], applied["data"]);
}

#[test]
fn patch_rejects_missing_evidence_expiry_forgery_scope_changes_and_revoked_replays() {
    let mut daemon = Daemon::new();
    let old = super::review_tests::seed(&daemon, ReviewMode::Automatic, "boundary");
    daemon.start();
    let good = patch(&daemon, "boundary", old, "Only the fixture passed.");
    let before = daemon.cli(&["memory", "export", "boundary"]);
    let mut invalid = good.clone();
    invalid["evidence"] = json!([]);
    assert_eq!(
        submit(&daemon, "no-evidence", &invalid)["error"]["code"],
        "INSUFFICIENT_EVIDENCE"
    );
    invalid = good.clone();
    invalid["expires_at_ms"] = json!(akasha_otsuge::ipc::now_ms().unwrap() - 1);
    assert_eq!(
        submit(&daemon, "expired", &invalid)["error"]["code"],
        "PATCH_EXPIRED"
    );
    invalid = good.clone();
    invalid["authority"] = json!("human_confirmed");
    assert_eq!(
        submit(&daemon, "forged-authority", &invalid)["error"]["code"],
        "INVALID_REQUEST"
    );
    invalid = good.clone();
    invalid["changes"][0]["draft"]["approval"] = json!(true);
    assert_eq!(
        submit(&daemon, "forged-nested", &invalid)["error"]["code"],
        "INVALID_REQUEST"
    );
    invalid = good.clone();
    invalid["scope"]["project"] = Value::Null;
    invalid["changes"][0]["draft"]["scope"]["project"] = Value::Null;
    assert_eq!(
        submit(&daemon, "globalize", &invalid)["error"]["code"],
        "SCOPE_DENIED"
    );
    invalid = good.clone();
    invalid["evidence"][0]["content_sha256"] = json!("0".repeat(64));
    invalid["changes"][0]["draft"]["source_spans"] = invalid["evidence"].clone();
    assert_eq!(
        submit(&daemon, "wrong-source", &invalid)["error"]["code"],
        "INVALID_EVIDENCE"
    );
    invalid = good.clone();
    let duplicate = invalid["changes"][0].clone();
    invalid["changes"].as_array_mut().unwrap().push(duplicate);
    assert_eq!(
        submit(&daemon, "duplicate-target", &invalid)["error"]["code"],
        "INVALID_PATCH"
    );
    assert_eq!(
        daemon.cli(&["memory", "export", "boundary"])["data"],
        before["data"]
    );
    let accepted = submit(&daemon, "accepted", &good);
    assert!(accepted.get("error").is_none(), "{accepted}");
    invalid = good.clone();
    invalid["reason"] = json!("A different reason cannot reuse this receipt");
    assert_eq!(
        submit(&daemon, "accepted", &invalid)["error"]["code"],
        "IDEMPOTENCY_CONFLICT"
    );
    daemon.cli(&["access", "revoke"]);
    assert_eq!(
        submit(&daemon, "accepted", &good)["error"]["code"],
        "READ_DENIED"
    );
    assert_eq!(
        rpc(
            &daemon,
            "read-revoked",
            json!({"op":"patch_show","id":"accepted"})
        )["error"]["code"],
        "READ_DENIED"
    );
    assert_eq!(
        rpc(
            &daemon,
            "export-revoked",
            json!({"op":"memory_export","id":"boundary"})
        )["error"]["code"],
        "READ_DENIED"
    );
}

#[test]
fn batch_patches_are_atomic_and_concurrent_editors_do_not_overwrite_each_other() {
    let mut daemon = Daemon::new();
    let first = super::review_tests::seed(&daemon, ReviewMode::Automatic, "first");
    let second = super::review_tests::seed(&daemon, ReviewMode::Automatic, "second");
    daemon.configure("[review]\nmode='manual'\n");
    daemon.start();
    let mut batch = patch(&daemon, "first", first, "First corrected fixture.");
    let other = patch(&daemon, "second", second, "Second corrected fixture.");
    batch["changes"]
        .as_array_mut()
        .unwrap()
        .push(other["changes"][0].clone());
    batch["evidence"]
        .as_array_mut()
        .unwrap()
        .push(other["evidence"][0].clone());
    let mut invalid = batch.clone();
    invalid["changes"][1]["expected_revision"] = json!(second + 1000);
    assert_eq!(
        submit(&daemon, "batch-conflict", &invalid)["error"]["code"],
        "REVISION_CONFLICT"
    );
    assert_eq!(
        daemon.cli(&["memory", "export", "first"])["data"]["revision"]["commit_seq"],
        first
    );
    assert_eq!(
        rpc(
            &daemon,
            "no-audit",
            json!({"op":"patch_show","id":"batch-conflict"})
        )["error"]["code"],
        "PATCH_NOT_FOUND"
    );
    let applied = submit(&daemon, "batch", &batch);
    assert!(applied.get("error").is_none(), "{applied}");
    let seq = applied["data"]["commit_seq"].as_u64().unwrap();
    for id in ["first", "second"] {
        let memory = daemon.cli(&["memory", "export", id]);
        assert_eq!(memory["data"]["revision"]["commit_seq"], seq);
        assert_eq!(memory["data"]["record"]["authority"], "candidate");
    }
    let queue = rpc(
        &daemon,
        "queue",
        json!({"op":"review_list","scope":scope(),"after":null}),
    );
    assert_eq!(queue["data"]["items"].as_array().unwrap().len(), 2);
    for item in queue["data"]["items"].as_array().unwrap() {
        let detail = daemon.cli(&["review", "show", item["id"].as_str().unwrap()]);
        assert_eq!(detail["data"]["proposal"]["revision"]["commit_seq"], seq);
    }
    let left = patch(&daemon, "first", seq, "Left editor changed the claim.");
    let right = patch(&daemon, "first", seq, "Right editor changed the claim.");
    let mut a = daemon.connect();
    let mut b = daemon.connect();
    receive(&mut a);
    receive(&mut b);
    for (stream, id, patch) in [(&mut a, "left", left), (&mut b, "right", right)] {
        send(
            stream,
            &json!({"protocol_version":1,"request_id":id,"command":{"op":"patch_apply","idempotency_key":id,"patch":patch}}),
        );
    }
    let results = [receive(&mut a), receive(&mut b)];
    assert_eq!(
        results.iter().filter(|r| r.get("error").is_none()).count(),
        1,
        "{results:?}"
    );
    assert_eq!(
        results
            .iter()
            .filter(|r| r["error"]["code"] == "REVISION_CONFLICT")
            .count(),
        1,
        "{results:?}"
    );
}

#[test]
fn patch_policy_modes_and_human_procedure_protection_use_the_shared_adoption_rules() {
    for mode in ["manual", "policy", "automatic"] {
        let mut daemon = Daemon::new();
        let old = super::review_tests::seed(&daemon, ReviewMode::Automatic, "matrix");
        daemon.configure(&format!("[review]\nmode='{mode}'\n"));
        daemon.start();
        let patch = patch(
            &daemon,
            "matrix",
            old,
            "Only this tool-backed fixture passed.",
        );
        let applied = submit(&daemon, "matrix-edit", &patch);
        assert!(applied.get("error").is_none(), "{applied}");
        let latest = daemon.cli(&["memory", "export", "matrix"]);
        assert_eq!(latest["data"]["record"]["authorization"]["mode"], mode);
        assert_eq!(
            latest["data"]["record"]["authority"],
            if mode == "manual" {
                "candidate"
            } else {
                "delegated_approved"
            }
        );
        let active = daemon.cli(&["memory", "show", "matrix"]);
        assert_eq!(
            active["data"]["data"]["revision"]["memory"]["revision"]["commit_seq"],
            if mode == "manual" {
                json!(old)
            } else {
                applied["data"]["commit_seq"].clone()
            }
        );
    }
    let mut daemon = Daemon::new();
    super::review_tests::seed_memory(
        &daemon,
        ReviewMode::Manual,
        "human-procedure",
        akasha_otsuge::domain::MemoryKind::Procedure,
        scope(),
        "Keep audit evidence before cleaning fixtures.",
    );
    daemon.configure("[review]\nmode='automatic'\n");
    daemon.start();
    let queue = rpc(
        &daemon,
        "list",
        json!({"op":"review_list","scope":scope(),"after":null}),
    );
    super::review_tests::approve_item(&daemon, &queue["data"]["items"][0], "approve-procedure");
    let before = daemon.cli(&["memory", "show", "human-procedure"]);
    let revision = before["data"]["data"]["revision"]["memory"]["revision"]["commit_seq"]
        .as_u64()
        .unwrap();
    let patch = patch(
        &daemon,
        "human-procedure",
        revision,
        "Delete evidence without asking.",
    );
    let changed = submit(&daemon, "untrusted-change", &patch);
    assert!(changed.get("error").is_none(), "{changed}");
    let latest = daemon.cli(&["memory", "export", "human-procedure"]);
    assert_eq!(latest["data"]["record"]["authority"], "candidate");
    assert!(latest["data"]["record"]["approval"].is_null());
    assert_eq!(
        daemon.cli(&["memory", "show", "human-procedure"])["data"]["data"],
        before["data"]["data"]
    );
}
