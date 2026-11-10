use super::*;
use akasha_otsuge::config::ReviewMode;
use akasha_otsuge::domain::*;
use akasha_otsuge::storage::Storage;
use extraction_tests::{runtime, scope, storage_config};
use notifier_tests::{rpc, wait_items};

fn archive(daemon: &Daemon, operation: &str, directory: &std::path::Path) {
    let output =
        Command::new(std::env::var_os("AKASHA_ACCEPTANCE_BINARY").unwrap_or_else(|| BIN.into()))
            .arg("--config")
            .arg(daemon.root.join("config.toml"))
            .args(["archive", operation])
            .arg(directory)
            .output()
            .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn imported_pi_session_can_register_fresh_without_reusing_its_old_token() {
    let mut source = Daemon::new();
    source.configure(&format!("[access]\nread_roots=[{:?}]\n", source.root));
    let binary = source.root.join("pi");
    executable::write(&binary, "#!/bin/sh\nprintf '1.4.0\\n'\n");
    let agent_dir = source.root.join("agent");
    std::fs::create_dir(&agent_dir).unwrap();
    let transcript = agent_dir.join("native.jsonl");
    let mut rows: Vec<Value> = include_str!("../fixtures/pi/1.4.0/root-before.jsonl")
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    rows[0]["cwd"] = json!(source.root);
    let session = rows[0]["id"].clone();
    let manifest: Value =
        serde_json::from_str(include_str!("../fixtures/pi/1.4.0/manifest.json")).unwrap();
    let turn = manifest["ids"]["assistant"].clone();
    let row = rows.iter_mut().find(|row| row["id"] == turn).unwrap();
    row["message"]["content"][2]["name"] = json!("akasha_consult");
    row["message"]["content"][2]["arguments"] = json!({"hint":"review"});
    std::fs::write(
        &transcript,
        rows.iter()
            .map(|row| format!("{row}\n"))
            .collect::<String>(),
    )
    .unwrap();
    let register = json!({"op":"register","binary":binary,"agent_dir":agent_dir,"session_id":session,"transcript_path":transcript,"cwd":source.root});
    let native = |daemon: &Daemon, event: Value| rpc(daemon, json!({"op":"pi","event":event}));
    source.start();
    let old = native(&source, register.clone());
    let old_token = old["data"]["registration"]
        .as_str()
        .unwrap_or_else(|| panic!("{old}"));
    source.cli(&["daemon", "stop"]);
    source.wait();
    let exported = source.root.join("archive");
    archive(&source, "export", &exported);
    let mut target = Daemon::new();
    // The destination owner explicitly authorizes this existing native session path.
    target.configure(&format!(
        "[access]\nread_roots=[{:?},{:?}]\n",
        target.root, source.root
    ));
    archive(&target, "import", &exported);
    target.start();
    assert_eq!(
        native(
            &target,
            json!({"op":"sync","registration":old_token,"until_entry":"anything"})
        )["error"]["code"],
        "BINDING_UNAVAILABLE"
    );
    let fresh = native(&target, register.clone());
    let token = fresh["data"]["registration"]
        .as_str()
        .unwrap_or_else(|| panic!("{fresh}"));
    assert_ne!(token, old_token);
    assert_eq!(native(&target, register.clone())["data"], fresh["data"]);
    let bind = json!({"op":"bind","registration":token,"call":{"session_id":session,"leaf_id":manifest["ids"]["tool"],"turn_id":turn,"tool_call_id":"call_fixture","agent_dir":agent_dir,"transcript":transcript,"cwd":source.root}});
    let bound = native(&target, bind.clone());
    assert!(bound.get("error").is_none(), "{bound}");
    assert_eq!(bound["data"]["requested_cursor"], 5);
    target.cli(&["daemon", "stop"]);
    target.wait();
    target.start();
    assert_eq!(native(&target, register)["data"], fresh["data"]);
    assert_eq!(native(&target, bind)["data"], bound["data"]);
    assert_eq!(
        native(
            &target,
            json!({"op":"sync","registration":old_token,"until_entry":"anything"})
        )["error"]["code"],
        "BINDING_UNAVAILABLE"
    );
    target.cli(&["daemon", "stop"]);
    target.wait();
}

#[test]
fn imported_daemon_keeps_history_inert_across_restarts_and_accepts_fresh_work() {
    let mut source = Daemon::new();
    source.configure("[notifiers.inbox]\nkind='local'\n");
    review_tests::seed(&source, ReviewMode::Automatic, "retained-memory");
    review_tests::seed(&source, ReviewMode::Manual, "retained-review");
    source.start();
    let original_notice = wait_items(&source, &scope(), |items| {
        items.len() == 1 && items[0]["deliveries"]["inbox"]["status"] == "sent"
    })
    .remove(0);
    source.cli(&["daemon", "stop"]);
    source.wait();
    let (boundary, expected) = runtime().block_on(async {
        let store = Storage::open(&storage_config(&source)).await.unwrap();
        let domain = Domain::new(&store);
        let context = ReadContext::from_verified_binding(&scope().user, "local-management", "local-management").unwrap();
        let memory = domain.active("retained-memory").unwrap().unwrap();
        domain.prepare_recall(&context, &RecallDraft {
            request_id:"retained-receipt".into(), output:"original returned bytes\n原文".into(),
            scope:scope(), read_seq:store.sequence().unwrap(), retrieval_mode:RetrievalMode::CurrentState,
            selected:vec![memory.revision.clone()], sources:memory.record.draft.source_spans.clone(),
            status:RecallStatus::Found, coverage:Coverage { complete:true, gaps:vec![] },
            truncation:None, config_generation:1, index_generation:None,
        }).await.unwrap();
        let at = store.sequence().unwrap();
        let budget = || PageBudget { limit:32, max_bytes:1024*1024 };
        let expected = json!({
            "memory":domain.get(&context,"retained-memory",Some(at)).await.unwrap().data,
            "state":domain.state(&context,&scope(),at,None,budget()).await.unwrap().data,
            "history":domain.history(&context,"retained-memory",1..=at,None,budget()).await.unwrap().data,
            "receipt":domain.recall_receipt(&context,"retained-receipt").await.unwrap().data,
        });
        store.close().await.unwrap();
        (at,expected)
    });
    let exported = source.root.join("archive");
    archive(&source, "export", &exported);
    let original_archive: Vec<_> = ["manifest.json", "commits.jsonl", "blobs.bin"]
        .map(|name| (name, std::fs::read(exported.join(name)).unwrap()))
        .into();
    let mut target = Daemon::new();
    let plugin = target.root.join("notifier.py");
    let script = include_str!("../fixtures/notifier/oneshot.py").replace(
        "root = Path(__file__).parent",
        "root = Path(__file__).parent\n(root / 'plugin-called').write_text('called')",
    );
    executable::write(&plugin, &script);
    target.configure(&format!(
        "[access]\nread_roots=[{:?}]\n[notifiers.plugin]\nkind='stdio'\ncommand=[{plugin:?}]\n",
        target.root
    ));
    archive(&target, "import", &exported);
    let id = original_notice["notification_id"].as_str().unwrap();
    for restart in 0..2 {
        target.start();
        assert_eq!(
            target.cli(&["memory", "show", "retained-memory"])["data"]["data"],
            expected["memory"]
        );
        assert_eq!(
            target.cli(&["receipt", "show", "retained-receipt"])["data"]["data"],
            expected["receipt"]
        );
        assert_eq!(
            rpc(
                &target,
                json!({"op":"state_show","scope":scope(),"at":boundary,"cursor":null})
            )["data"]["data"],
            expected["state"]
        );
        assert_eq!(
            rpc(
                &target,
                json!({"op":"memory_history","id":"retained-memory","from":1,"to":boundary,"cursor":null})
            )["data"]["data"],
            expected["history"]
        );
        let ack = rpc(
            &target,
            json!({"op":"notify_ack","id":id,"generation":original_notice["generation"],"idempotency_key":format!("ack-{restart}")}),
        );
        assert!(ack.get("error").is_none(), "{ack}");
        let binding = target.root.join("binding.json");
        std::fs::write(&binding,json!({"source":{"adapter":"fixture","profile":"test","session":format!("fresh-{restart}"),"thread":"thread","agent":"agent"},"scope":scope(),"stream":"stream","coverage":"available_payloads"}).to_string()).unwrap();
        let input = target.root.join("capture.jsonl");
        std::fs::write(&input, b"").unwrap();
        let job = format!("fresh-{restart}");
        target.cli(&[
            "capture",
            "reconcile",
            binding.to_str().unwrap(),
            input.to_str().unwrap(),
            "--idempotency-key",
            &job,
        ]);
        assert_eq!(
            extraction_tests::finished(&target, &job)["state"],
            "succeeded"
        );
        // A completed job proves the scheduler passed notification delivery admission.
        assert!(
            !target.root.join("plugin-called").exists(),
            "imported history launched a notifier"
        );
        if restart == 0 {
            target.cli(&["daemon", "stop"]);
            target.wait();
        }
    }
    let review_id = original_notice["review_id"].as_str().unwrap();
    let review = target.cli(&["review", "show", review_id]);
    let changed = rpc(
        &target,
        json!({"op":"review_state","id":review_id,"expected_revision":review["data"]["item"]["revision"],"state":"investigating","reason":"fresh local investigation","idempotency_key":"local-review"}),
    );
    assert!(changed.get("error").is_none(), "{changed}");
    let notice = wait_items(&target, &scope(), |items| {
        items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "sent"
    })
    .remove(0);
    assert!(
        notice["generation"].as_u64().unwrap() > original_notice["generation"].as_u64().unwrap()
    );
    assert!(notice["deliveries"].get("inbox").is_none());
    let received = std::fs::read_to_string(target.root.join("received.jsonl")).unwrap();
    let delivered: Vec<Value> = received
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();
    assert_eq!(delivered.len(), 1);
    assert_eq!(delivered[0]["action"], "create");
    assert!(delivered[0]["external_message_id"].is_null());
    target.cli(&["daemon", "stop"]);
    target.wait();
    archive(&target, "import", &exported);
    runtime().block_on(async {
        let store = Storage::open(&storage_config(&target)).await.unwrap();
        let context = ReadContext::from_verified_binding(
            &scope().user,
            "local-management",
            "local-management",
        )
        .unwrap();
        let domain = Domain::new(&store);
        assert_eq!(
            serde_json::to_value(
                domain
                    .get(&context, "retained-memory", Some(boundary))
                    .await
                    .unwrap()
                    .data
            )
            .unwrap(),
            expected["memory"]
        );
        assert_eq!(
            serde_json::to_value(
                domain
                    .recall_receipt(&context, "retained-receipt")
                    .await
                    .unwrap()
                    .data
            )
            .unwrap(),
            expected["receipt"]
        );
        let key = akasha_otsuge::storage::RecordKey {
            kind: akasha_otsuge::storage::RecordKind::State,
            id: format!("notification/item/{id}"),
        };
        assert_eq!(
            serde_json::from_str::<Value>(&store.current(&key).unwrap().unwrap().content).unwrap(),
            notice
        );
        store.close().await.unwrap();
    });
    for (name, bytes) in original_archive {
        assert_eq!(std::fs::read(exported.join(name)).unwrap(), bytes);
    }
}
