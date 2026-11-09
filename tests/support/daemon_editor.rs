use super::extraction_tests::scope;
use super::*;
use akasha_otsuge::config::ReviewMode;

fn editor_config(root: &std::path::Path, mode: &str) -> String {
    format!(
        "[review]\nmode='{mode}'\n[executors.editor]\nkind='codex'\nbinary={:?}\nnative_home={:?}\n",
        root.join("codex"),
        root.join("native-home")
    )
}

fn feedback(daemon: &Daemon, bound: &Value, expires: u64) -> Value {
    let target =
        daemon.cli(&["memory", "export", "editable"])["data"]["revision"]["commit_seq"].clone();
    json!({"invocation_id":bound["data"]["invocation_id"],"executor":"editor",
        "targets":[{"memory_id":"editable","expected_revision":target}],
        "feedback":"Check the platform claim against the original build evidence and notes.rs.",
        "expires_at_ms":expires})
}

fn finished(daemon: &Daemon, id: &str) -> Value {
    let end = Instant::now() + Duration::from_secs(20);
    loop {
        let job = daemon.cli(&["job", "show", id])["data"].clone();
        if !matches!(job["state"].as_str(), Some("queued" | "running")) {
            return job;
        }
        assert!(Instant::now() < end, "{job}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn editor_refuses_unread_unlisted_scope_and_forged_changes_and_keeps_nonmemory_failures() {
    for (case, error) in [
        ("editor_unread", "INSUFFICIENT_EVIDENCE"),
        ("editor_target", "READ_DENIED"),
        ("editor_scope", "SCOPE_DENIED"),
        ("editor_forged", "INVALID_PATCH"),
        ("editor_environment", ""),
        ("editor_insufficient", ""),
        ("editor_uncertain", ""),
    ] {
        let (daemon, _, bound) =
            super::codex_tests::consult_fixture_with(case, |root| editor_config(root, "automatic"));
        let original = daemon.cli(&["memory", "show", "editable"])["data"]["data"].clone();
        let input = feedback(
            &daemon,
            &bound,
            akasha_otsuge::ipc::now_ms().unwrap() + 60_000,
        );
        let submit = rpc(
            &daemon,
            case,
            json!({"op":"editor_run","feedback":input,"idempotency_key":case}),
        );
        assert!(submit.get("error").is_none(), "{submit}");
        let job = finished(&daemon, submit["data"]["id"].as_str().unwrap());
        assert_eq!(
            job["state"],
            if error.is_empty() {
                "succeeded"
            } else {
                "failed"
            },
            "{case}: {job}"
        );
        if !error.is_empty() {
            assert_eq!(job["error"], error, "{case}: {job}");
        }
        assert_eq!(
            daemon.cli(&["memory", "show", "editable"])["data"]["data"],
            original,
            "{case}"
        );
        let queue = rpc(
            &daemon,
            "list",
            json!({"op":"review_list","scope":input_scope(),"after":null}),
        );
        assert_eq!(
            queue["data"]["items"].as_array().unwrap().len(),
            usize::from(matches!(case, "editor_insufficient" | "editor_uncertain")),
            "{case}: {queue}"
        );
    }
}

fn input_scope() -> Value {
    json!({"user":format!("uid:{}",rustix::process::geteuid().as_raw()),"project":null,"branch_or_version":null,"session":null})
}

#[test]
#[ignore = "requires an explicitly supplied authenticated Pi profile and receipt path"]
fn native_editor_actual() {
    let binary = PathBuf::from(std::env::var_os("AKASHA_NATIVE_PI_BINARY").unwrap());
    let home = PathBuf::from(std::env::var_os("AKASHA_NATIVE_PI_HOME").unwrap());
    let output = PathBuf::from(std::env::var_os("AKASHA_NATIVE_EDITOR_RECEIPT").unwrap());
    assert!(!output.exists());
    let (mut daemon, _, bound) =
        super::codex_tests::consult_fixture_with("editor_actual", |_| String::new());
    let original = daemon.cli(&["memory", "export", "editable"])["data"].clone();
    daemon.configure(&format!("[access]\nread_roots=[{:?},{home:?}]\n[review]\nmode='automatic'\n[consult]\nmax_delegation_depth=0\nmax_runner_starts=1\n[executors.editor]\nkind='pi'\nbinary={binary:?}\nnative_home={home:?}\n",daemon.root));
    daemon.cli(&["config", "reload"]);
    daemon.cli(&[
        "adapter",
        "install",
        "pi",
        "--binary",
        binary.to_str().unwrap(),
        "--agent-dir",
        home.to_str().unwrap(),
    ]);
    let mut input = feedback(
        &daemon,
        &bound,
        akasha_otsuge::ipc::now_ms().unwrap() + 3_600_000,
    );
    input["feedback"] = json!(
        "The editable memory claims the fixture passed on all platforms. Investigate the original build evidence and notes.rs. If the evidence establishes an overclaim, submit a precise correction limited to what was actually observed; preserve original scope and do not claim unobserved tests. This is synthetic acceptance data, not a human approval."
    );
    let file = daemon.root.join("feedback.json");
    std::fs::write(&file, serde_json::to_vec(&input).unwrap()).unwrap();
    let accepted = daemon.cli(&[
        "editor",
        "run",
        file.to_str().unwrap(),
        "--idempotency-key",
        "actual-editor",
    ])["data"]
        .clone();
    let id = accepted["id"].as_str().unwrap();
    let mut receipt = json!({"schema_version":1,"status":"RUNNING","source":"controlled caller binding and synthetic fixture; actual authenticated external Pi Runner","scratch":daemon.root,"input":input,"before":original,"accepted":accepted});
    std::fs::write(&output, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    let end = Instant::now()
        + Duration::from_millis(accepted["execution_budget_ms"].as_u64().unwrap() + 60_000);
    let mut previous = Value::Null;
    let done = loop {
        let job = daemon.cli(&["job", "show", id])["data"].clone();
        let progress = json!({"state":job["state"],"run_trace":job["run_trace"]});
        if progress != previous {
            receipt["progress"] = progress.clone();
            previous = progress;
            std::fs::write(&output, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
        }
        if !matches!(job["state"].as_str(), Some("queued" | "running")) {
            break job;
        }
        assert!(
            Instant::now() < end,
            "native editor exceeded original workflow"
        );
        std::thread::sleep(Duration::from_millis(500));
    };
    receipt["job"] = done.clone();
    receipt["status"] = json!("FAILED");
    std::fs::write(&output, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
    assert_eq!(done["state"], "succeeded", "{done}");
    assert!(
        done["result"]["patch_receipt"]["commit_seq"].is_u64(),
        "{done}"
    );
    let after = daemon.cli(&["memory", "export", "editable"])["data"].clone();
    assert_ne!(
        after["record"]["draft"]["claim"],
        original["record"]["draft"]["claim"]
    );
    assert_eq!(
        after["record"]["draft"]["scope"],
        original["record"]["draft"]["scope"]
    );
    assert_eq!(after["record"]["provenance"]["origin"], "memory_edit");
    assert!(after["record"]["approval"].is_null());
    assert_ne!(after["record"]["authority"], "human_confirmed");
    assert_eq!(done["run_trace"]["runs"][0]["host"], "pi");
    assert_eq!(done["run_trace"]["usage"]["starts"], 1);
    let patch = daemon.cli(&["patch", "show", id])["data"].clone();
    assert_eq!(patch["config_generation"], accepted["config_generation"]);
    receipt["after"] = after;
    receipt["patch_audit"] = patch;
    let repeat = daemon.cli(&[
        "editor",
        "run",
        file.to_str().unwrap(),
        "--idempotency-key",
        "actual-editor",
    ]);
    assert_eq!(repeat["data"]["id"], id);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    receipt["status"] = json!("PASS_REAL_NATIVE_EDITOR");
    std::fs::write(&output, serde_json::to_vec_pretty(&receipt).unwrap()).unwrap();
}

#[test]
fn patch_filters_old_search_versions_but_preserves_receipts_and_original_bytes() {
    let mut daemon = Daemon::new();
    let old = super::review_tests::seed(&daemon, ReviewMode::Automatic, "indexed");
    daemon.configure("[review]\nmode='automatic'\n");
    let scope_file = daemon.root.join("scope.json");
    std::fs::write(&scope_file, serde_json::to_vec(&scope()).unwrap()).unwrap();
    daemon.start();
    daemon.cli(&["index", "sync", "--idempotency-key", "before-index"]);
    assert_eq!(finished(&daemon, "before-index")["state"], "succeeded");
    daemon.cli(&[
        "search",
        "fixture",
        "--scope",
        scope_file.to_str().unwrap(),
        "--idempotency-key",
        "old-search",
    ]);
    let prior = finished(&daemon, "old-search");
    assert_eq!(prior["state"], "succeeded", "{prior}");
    let prior_output: Value =
        serde_json::from_str(prior["result"]["output"].as_str().unwrap()).unwrap();
    assert_eq!(
        prior_output["items"][0]["memory"]["revision"]["commit_seq"],
        old
    );
    let edit = patch(
        &daemon,
        "indexed",
        old,
        "Only the controlled Linux fixture is supported by this receipt.",
    );
    let source = &edit["evidence"][0];
    let original = || {
        daemon.cli(&[
            "show",
            source["event_id"].as_str().unwrap(),
            "--revision",
            &source["revision"].to_string(),
            "--range",
            &source["start"].to_string(),
            &source["end"].to_string(),
        ])["data"]["output"]
            .clone()
    };
    let bytes = original();
    let receipt = submit(&daemon, "indexed-edit", &edit);
    assert!(receipt.get("error").is_none(), "{receipt}");
    assert_eq!(original(), bytes);
    daemon.cli(&[
        "search",
        "fixture",
        "--scope",
        scope_file.to_str().unwrap(),
        "--idempotency-key",
        "current-search",
    ]);
    let current = finished(&daemon, "current-search");
    assert_eq!(current["state"], "succeeded", "{current}");
    let current_output: Value =
        serde_json::from_str(current["result"]["output"].as_str().unwrap()).unwrap();
    for item in current_output["items"].as_array().unwrap() {
        assert_ne!(item["memory"]["revision"]["commit_seq"], old);
    }
    let replay = daemon
        .command()
        .args([
            "receipt",
            "replay",
            prior["result"]["receipt_id"].as_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(replay.status.success());
    assert_eq!(
        replay.stdout,
        prior["result"]["output"].as_str().unwrap().as_bytes()
    );
    daemon.cli(&["index", "sync", "--idempotency-key", "after-index"]);
    assert_eq!(finished(&daemon, "after-index")["state"], "succeeded");
    daemon.cli(&[
        "search",
        "fixture",
        "--scope",
        scope_file.to_str().unwrap(),
        "--idempotency-key",
        "fresh-search",
    ]);
    let fresh = finished(&daemon, "fresh-search");
    assert_eq!(fresh["state"], "succeeded", "{fresh}");
    let output: Value = serde_json::from_str(fresh["result"]["output"].as_str().unwrap()).unwrap();
    assert_eq!(
        output["items"][0]["memory"]["revision"]["commit_seq"],
        receipt["data"]["commit_seq"]
    );
}

#[test]
fn editor_rechecks_live_executor_policy_and_expiry_but_pins_its_adoption_mode() {
    for case in ["mode", "revoked", "expiry"] {
        let (mut daemon, _, bound) =
            super::codex_tests::consult_fixture_with("editor_wait", |root| {
                editor_config(root, "manual")
            });
        let expires =
            akasha_otsuge::ipc::now_ms().unwrap() + if case == "expiry" { 5000 } else { 60_000 };
        let input = feedback(&daemon, &bound, expires);
        let original = daemon.cli(&["memory", "export", "editable"])["data"].clone();
        let accepted = rpc(
            &daemon,
            case,
            json!({"op":"editor_run","feedback":input,"idempotency_key":case}),
        );
        let id = accepted["data"]["id"]
            .as_str()
            .unwrap_or_else(|| panic!("{accepted}"));
        let end = Instant::now() + Duration::from_secs(10);
        while !daemon.root.join("editor-ready").exists() {
            assert!(Instant::now() < end);
            std::thread::sleep(Duration::from_millis(10));
        }
        if case == "expiry" {
            let wait = expires.saturating_sub(akasha_otsuge::ipc::now_ms().unwrap());
            std::thread::sleep(Duration::from_millis(wait + 5));
        } else {
            let suffix = if case == "revoked" {
                "enabled=false\n"
            } else {
                ""
            };
            daemon.configure(&format!(
                "[access]\nread_roots=[{:?}]\n{}{}",
                daemon.root,
                editor_config(&daemon.root, "automatic"),
                suffix
            ));
            daemon.cli(&["config", "reload"]);
        }
        std::fs::write(daemon.root.join("editor-release"), "").unwrap();
        if case == "revoked" {
            let shown = rpc(&daemon, "revoked", json!({"op":"job_show","id":id}));
            assert_eq!(shown["error"]["code"], "EXECUTOR_UNAVAILABLE", "{shown}");
            daemon.cli(&["daemon", "stop"]);
            daemon.wait();
            daemon.configure(&format!(
                "[access]\nread_roots=[{:?}]\n{}",
                daemon.root,
                editor_config(&daemon.root, "automatic")
            ));
            daemon.start();
            let done = finished(&daemon, id);
            assert_eq!(done["state"], "failed", "{done}");
        } else {
            let done = finished(&daemon, id);
            assert_eq!(
                done["state"],
                if case == "mode" {
                    "succeeded"
                } else {
                    "failed"
                },
                "{done}"
            );
            if case == "expiry" {
                assert_eq!(done["error"], "PATCH_EXPIRED");
            }
        }
        let latest = daemon.cli(&["memory", "export", "editable"])["data"].clone();
        if case == "mode" {
            assert_eq!(latest["record"]["authorization"]["mode"], "manual");
            assert_eq!(latest["record"]["authority"], "candidate");
        } else {
            assert_eq!(latest, original);
        }
    }
}

#[test]
fn configured_editor_investigates_bound_evidence_and_applies_only_its_pinned_patch() {
    let (daemon, _, bound) = super::codex_tests::consult_fixture_with("editor", |root| {
        format!(
            "[review]\nmode='automatic'\n[executors.editor]\nkind='codex'\nbinary={:?}\nnative_home={:?}\n",
            root.join("codex"),
            root.join("native-home")
        )
    });
    let original = daemon.cli(&["memory", "export", "editable"])["data"].clone();
    let feedback = json!({"invocation_id":bound["data"]["invocation_id"],"executor":"editor",
        "targets":[{"memory_id":"editable","expected_revision":original["revision"]["commit_seq"]}],
        "feedback":"Check the exact platform claim against the original build evidence and notes.rs. Do not assume a local failure changes global preferences.",
        "expires_at_ms":akasha_otsuge::ipc::now_ms().unwrap()+60_000});
    let submitted = rpc(
        &daemon,
        "editor",
        json!({"op":"editor_run","feedback":feedback,"idempotency_key":"editor-fixture"}),
    );
    assert!(
        submitted.get("error").is_none(),
        "editor workflow unavailable: {submitted}"
    );
    let id = submitted["data"]["id"].as_str().unwrap();
    let end = Instant::now() + Duration::from_secs(20);
    let done = loop {
        let job = daemon.cli(&["job", "show", id])["data"].clone();
        if !matches!(job["state"].as_str(), Some("queued" | "running")) {
            break job;
        }
        assert!(Instant::now() < end, "{job}");
        std::thread::sleep(Duration::from_millis(20));
    };
    assert_eq!(done["state"], "succeeded", "{done}");
    assert_eq!(done["result"]["editor"]["classification"], "memory_error");
    assert!(
        done["result"]["patch_receipt"]["commit_seq"]
            .as_u64()
            .unwrap()
            > original["revision"]["commit_seq"].as_u64().unwrap()
    );
    let changed = daemon.cli(&["memory", "export", "editable"])["data"].clone();
    assert_eq!(
        changed["record"]["draft"]["claim"],
        "Only the Linux fixture build passed; other platforms were not tested."
    );
    assert_eq!(changed["record"]["authority"], "delegated_approved");
    assert_eq!(
        changed["record"]["provenance"]["generator"]["runner"],
        "executor/editor"
    );
    assert!(changed["record"]["approval"].is_null());
    let repeat = rpc(
        &daemon,
        "repeat",
        json!({"op":"editor_run","feedback":feedback,"idempotency_key":"editor-fixture"}),
    );
    assert_eq!(repeat["data"]["id"], id, "{repeat}");
    assert_eq!(
        std::fs::read_to_string(daemon.root.join("starts")).unwrap(),
        "start\n"
    );
}

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
