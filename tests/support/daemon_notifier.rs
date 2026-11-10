use super::*;
use akasha_otsuge::config::ReviewMode;

pub(super) fn rpc(daemon: &Daemon, command: Value) -> Value {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let mut stream = daemon.connect();
    receive(&mut stream);
    send(
        &mut stream,
        &json!({"protocol_version":1,"request_id":format!("notifier-test/{}", NEXT.fetch_add(1, Ordering::Relaxed)),"command":command}),
    );
    receive(&mut stream)
}

fn all(daemon: &Daemon, scope: &akasha_otsuge::domain::Scope) -> Vec<Value> {
    let mut after = Value::Null;
    let mut items = Vec::new();
    for _ in 0..8 {
        let page = rpc(
            daemon,
            json!({"op":"notify_list","scope":scope,"after":after}),
        );
        assert!(page.get("error").is_none(), "{page}");
        let batch = page["data"]["items"].as_array().unwrap();
        assert!(batch.len() <= 32);
        items.extend_from_slice(batch);
        after = page["data"]["next"].clone();
        if after.is_null() {
            return items;
        }
    }
    panic!("notification pagination did not terminate");
}

pub(super) fn wait_items(
    daemon: &Daemon,
    scope: &akasha_otsuge::domain::Scope,
    ready: impl Fn(&[Value]) -> bool,
) -> Vec<Value> {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let items = all(daemon, scope);
        if ready(&items) {
            return items;
        }
        assert!(
            Instant::now() < end,
            "notifications did not converge: {items:?}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

fn list(daemon: &Daemon) -> Value {
    let path = daemon.root.join("scope.json");
    std::fs::write(
        &path,
        serde_json::to_vec(&extraction_tests::scope()).unwrap(),
    )
    .unwrap();
    daemon.cli(&["notify", "list", "--scope", path.to_str().unwrap()])
}

fn wait_local(daemon: &Daemon, count: usize) -> Value {
    let deadline = Instant::now() + Duration::from_secs(15);
    loop {
        let response = list(daemon);
        let items = response["data"]["items"].as_array().unwrap();
        if items.len() == count
            && items
                .iter()
                .all(|item| item["deliveries"]["inbox"]["status"] == "sent")
        {
            return response;
        }
        assert!(
            Instant::now() < deadline,
            "local delivery missing: {response}"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn local_notifications_are_durable_and_ack_cannot_approve_or_close_review() {
    let mut daemon = Daemon::new();
    daemon.configure("[notifiers.inbox]\nkind = 'local'\n");
    let proposal = review_tests::seed(&daemon, ReviewMode::Manual, "notify-candidate");
    // The review and its source outbox exist before any notifier worker runs.
    daemon.start();
    let inbox = wait_local(&daemon, 1);
    let item = &inbox["data"]["items"][0];
    let id = item["notification_id"].as_str().unwrap();
    let review_id = item["review_id"].as_str().unwrap();
    let generation = item["generation"].as_u64().unwrap().to_string();
    assert_eq!(item["priority"], "normal");
    assert_eq!(item["occurrences"], 1);
    assert!(
        !serde_json::to_string(item)
            .unwrap()
            .contains("fixture build passed")
    );
    let review_before = daemon.cli(&["review", "show", review_id])["data"].clone();
    let memory_before = daemon.cli(&["memory", "export", "notify-candidate"])["data"].clone();
    assert_eq!(memory_before["record"]["authority"], "candidate");
    assert_eq!(
        daemon.cli(&["memory", "show", "notify-candidate"])["data"]["data"]["state"],
        "absent"
    );
    assert_eq!(review_before["item"]["proposal_revision"], proposal);
    let args = [
        "notify",
        "ack",
        id,
        "--generation",
        &generation,
        "--idempotency-key",
        "ack-local",
    ];
    let acknowledged = daemon.cli(&args)["data"].clone();
    assert_eq!(daemon.cli(&args)["data"], acknowledged);
    let shown = daemon.cli(&["notify", "show", id])["data"].clone();
    assert_eq!(shown["acknowledged_generation"], item["generation"]);
    assert_eq!(
        daemon.cli(&["review", "show", review_id])["data"],
        review_before
    );
    assert_eq!(
        daemon.cli(&["memory", "export", "notify-candidate"])["data"],
        memory_before
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(daemon.cli(&["notify", "show", id])["data"], shown);
    assert_eq!(wait_local(&daemon, 1)["data"]["items"][0], shown);
}

#[test]
fn repeated_incidents_merge_counts_but_priority_escalation_remains_unacknowledged() {
    let (daemon, _, bound) = codex_tests::consult_fixture_with("editor_ledger", |root| {
        let plugin = root.join("notifier.py");
        executable::write(&plugin, include_str!("../../examples/notifier-file.py"));
        let state = root.join("notifier-inbox.json");
        format!(
            "[incidents]\ntriggers=['dependency_changed']\nescalate_after=50\n[notifiers.inbox]\nkind='local'\n[notifiers.file]\nkind='stdio'\ncommand=[{plugin:?},'--state',{state:?},'--fail-first']\n"
        )
    });
    let mut owner = extraction_tests::scope();
    owner.project = None;
    let wait = |count: u64| {
        let end = Instant::now() + Duration::from_secs(30);
        loop {
            let reply = rpc(
                &daemon,
                json!({"op":"notify_list","scope":owner,"after":null}),
            );
            assert!(reply.get("error").is_none(), "{reply}");
            let items = reply["data"]["items"].as_array().unwrap();
            if items.len() == 1
                && items[0]["occurrences"] == count
                && items[0]["deliveries"]["inbox"]["generation"] == items[0]["generation"]
                && items[0]["deliveries"]["file"]["generation"] == items[0]["generation"]
                && items[0]["deliveries"]["file"]["status"] == "sent"
            {
                return items[0].clone();
            }
            assert!(
                Instant::now() < end,
                "incident notification missing: {reply}"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    };
    let mut event = json!({"event_id":"notify-event-0","invocation_id":bound["data"]["invocation_id"],
        "source":"dependency-check","severity":"info","occurred_at_ms":akasha_otsuge::ipc::now_ms().unwrap(),
        "execution":null,"artifacts":[],"logs":[],"receipt_jobs":[],"claims":[],
        "incident":{"kind":"dependency_changed","key":"notify-dependency","reason":"Private source text must not leave the evidence store."}});
    assert!(
        rpc(&daemon, json!({"op":"outcome_submit","event":event}))
            .get("error")
            .is_none()
    );
    let first = wait(1);
    assert_eq!(first["priority"], "normal");
    assert_eq!(first["generation"], 1);
    assert_eq!(first["deliveries"]["file"]["attempt"]["number"], 2);
    let id = first["notification_id"].as_str().unwrap();
    daemon.cli(&[
        "notify",
        "ack",
        id,
        "--generation",
        "1",
        "--idempotency-key",
        "first-generation",
    ]);
    for n in 1..100 {
        event["event_id"] = json!(format!("notify-event-{n}"));
        let reply = rpc(&daemon, json!({"op":"outcome_submit","event":event}));
        assert!(reply.get("error").is_none(), "{reply}");
    }
    let repeated = wait(100);
    assert_eq!(repeated["notification_id"], first["notification_id"]);
    assert_eq!(repeated["priority"], "high");
    assert_eq!(repeated["generation"], 2);
    assert_eq!(repeated["acknowledged_generation"], 1);
    assert_eq!(
        repeated["deliveries"]["inbox"]["external_message_id"],
        first["deliveries"]["inbox"]["external_message_id"]
    );
    event["event_id"] = json!("notify-critical");
    event["severity"] = json!("critical");
    assert!(
        rpc(&daemon, json!({"op":"outcome_submit","event":event}))
            .get("error")
            .is_none()
    );
    let critical = wait(101);
    assert_eq!(critical["priority"], "urgent");
    assert_eq!(critical["generation"], 3);
    assert!(!critical.to_string().contains("Private source text"));
    let stale = rpc(
        &daemon,
        json!({"op":"notify_ack","id":id,"generation":1,"idempotency_key":"stale"}),
    );
    assert_eq!(stale["error"]["code"], "REVISION_CONFLICT");
    let incident = daemon.cli(&["incident", "show", critical["review_id"].as_str().unwrap()]);
    assert_eq!(incident["data"]["record"]["state"], "open");
    assert_eq!(incident["data"]["record"]["occurrences"], 101);
    assert!(!daemon.root.join("starts").exists());
    let external: Value =
        serde_json::from_slice(&std::fs::read(daemon.root.join("notifier-inbox.json")).unwrap())
            .unwrap();
    assert_eq!(external["messages"].as_object().unwrap().len(), 1);
    assert_eq!(external["receipts"].as_object().unwrap().len(), 3);
    let message = external["messages"]
        .as_object()
        .unwrap()
        .values()
        .next()
        .unwrap();
    assert_eq!(message["priority"], "urgent");
    assert_eq!(message["action"], "update");
    assert!(!external.to_string().contains("Private source text"));
}

#[test]
fn disabled_local_channel_survives_sigkill_and_reload_does_not_resend_success() {
    let mut daemon = Daemon::new();
    daemon.configure("[notifiers.inbox]\nkind='local'\nenabled=false\n");
    review_tests::seed(&daemon, ReviewMode::Manual, "crash-notify");
    daemon.start();
    let items = wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1
    });
    assert_eq!(items[0]["deliveries"], json!({}));
    let id = items[0]["notification_id"].as_str().unwrap();
    let mut child = daemon.child.take().unwrap();
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
    daemon.start();
    assert_eq!(all(&daemon, &extraction_tests::scope()), items);
    daemon.configure("[notifiers.inbox]\nkind='local'\nenabled=true\n");
    assert_eq!(
        daemon.cli(&["config", "reload"])["data"]["status"],
        "applied"
    );
    let delivered = wait_local(&daemon, 1)["data"]["items"][0].clone();
    assert_eq!(delivered["notification_id"], id);
    daemon.configure("[notifiers.inbox]\nkind='local'\nenabled=false\n");
    daemon.cli(&["config", "reload"]);
    let review = daemon.cli(&["review", "show", delivered["review_id"].as_str().unwrap()]);
    let changed = rpc(
        &daemon,
        json!({"op":"review_state","id":delivered["review_id"],
        "expected_revision":review["data"]["item"]["revision"],"state":"investigating",
        "reason":"Operator is investigating","idempotency_key":"notify-investigating"}),
    );
    assert!(changed.get("error").is_none(), "{changed}");
    let updated = wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1 && items[0]["generation"] == 2
    });
    assert_eq!(updated[0]["deliveries"], delivered["deliveries"]);
    daemon.configure("[notifiers.inbox]\nkind='local'\nenabled=true\n");
    daemon.cli(&["config", "reload"]);
    let sent = wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1 && items[0]["deliveries"]["inbox"]["generation"] == 2
    })[0]
        .clone();
    daemon.configure("[notifiers.inbox]\nkind='local'\nenabled=false\n");
    daemon.cli(&["config", "reload"]);
    daemon.configure("[notifiers.inbox]\nkind='local'\nenabled=true\n");
    daemon.cli(&["config", "reload"]);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    assert_eq!(wait_local(&daemon, 1)["data"]["items"][0], sent);
}

#[test]
fn notification_reads_ack_and_dispatch_follow_live_scope() {
    let mut daemon = Daemon::new();
    daemon.configure("[notifiers.inbox]\nkind='local'\nenabled=false\n");
    let alpha = extraction_tests::scope();
    let mut beta = alpha.clone();
    beta.project = Some("beta".into());
    review_tests::seed(&daemon, ReviewMode::Manual, "notify-alpha");
    review_tests::seed_memory(
        &daemon,
        ReviewMode::Manual,
        "notify-beta",
        akasha_otsuge::domain::MemoryKind::Fact,
        beta.clone(),
        "Private beta receipt.",
    );
    daemon.start();
    let beta_item = wait_items(&daemon, &beta, |items| items.len() == 1)[0].clone();
    let id = beta_item["notification_id"].as_str().unwrap();
    assert!(
        rpc(&daemon, json!({"op":"access_set","scope":null}))
            .get("error")
            .is_none()
    );
    for op in [
        json!({"op":"notify_list","scope":alpha,"after":null}),
        json!({"op":"notify_show","id":id}),
        json!({"op":"notify_ack","id":id,"generation":1,"idempotency_key":"denied"}),
    ] {
        assert_eq!(rpc(&daemon, op)["error"]["code"], "READ_DENIED");
    }
    assert!(
        rpc(&daemon, json!({"op":"access_set","scope":alpha}))
            .get("error")
            .is_none()
    );
    daemon.configure("[notifiers.inbox]\nkind='local'\nenabled=true\n");
    daemon.cli(&["config", "reload"]);
    wait_local(&daemon, 1);
    assert_eq!(
        rpc(&daemon, json!({"op":"notify_show","id":id}))["error"]["code"],
        "READ_DENIED"
    );
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    extraction_tests::runtime().block_on(async {
        let store =
            akasha_otsuge::storage::Storage::open(&extraction_tests::storage_config(&daemon))
                .await
                .unwrap();
        let row = store
            .current(&akasha_otsuge::storage::RecordKey {
                kind: akasha_otsuge::storage::RecordKind::State,
                id: format!("notification/item/{id}"),
            })
            .unwrap()
            .unwrap();
        let persisted: Value = serde_json::from_str(&row.content).unwrap();
        assert_eq!(persisted["deliveries"], json!({}));
        assert!(persisted["acknowledged_generation"].is_null());
        store.close().await.unwrap();
    });
    daemon.start();
    assert!(
        rpc(&daemon, json!({"op":"access_set","scope":beta}))
            .get("error")
            .is_none()
    );
    let delivered = wait_items(&daemon, &beta, |items| {
        items.len() == 1 && items[0]["deliveries"]["inbox"]["status"] == "sent"
    });
    assert_eq!(delivered[0]["notification_id"], id);
    assert_eq!(
        rpc(
            &daemon,
            json!({"op":"notify_list","scope":alpha,"after":null})
        )["error"]["code"],
        "READ_DENIED"
    );
}

#[test]
fn notification_pages_and_ack_backlog_do_not_create_an_idle_outbox_loop() {
    let mut daemon = Daemon::new();
    daemon.configure("[notifiers.inbox]\nkind='local'\n");
    let revision = review_tests::seed(&daemon, ReviewMode::Automatic, "notification-pages");
    daemon.start();
    let open = |n: usize| {
        let reply = rpc(
            &daemon,
            json!({"op":"review_open","memory_id":"notification-pages","expected_revision":revision,
            "reason":format!("Page {n}"),"idempotency_key":format!("notification-page-{n}")}),
        );
        assert!(reply.get("error").is_none(), "{reply}");
    };
    for n in 0..35 {
        open(n);
    }
    let scope = extraction_tests::scope();
    let items = wait_items(&daemon, &scope, |items| {
        items.len() == 35
            && items
                .iter()
                .all(|i| i["deliveries"]["inbox"]["status"] == "sent")
    });
    assert_eq!(
        items
            .iter()
            .map(|i| i["notification_id"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        35
    );
    for (n, item) in items.iter().enumerate() {
        let ack = json!({"op":"notify_ack","id":item["notification_id"],"generation":1,"idempotency_key":format!("page-ack-{n}")});
        let first = rpc(&daemon, ack.clone());
        assert!(first.get("error").is_none(), "{first}");
        assert_eq!(rpc(&daemon, ack)["data"], first["data"]);
    }
    let conflict = rpc(
        &daemon,
        json!({"op":"notify_ack","id":items[1]["notification_id"],"generation":1,"idempotency_key":"page-ack-0"}),
    );
    assert_eq!(conflict["error"]["code"], "IDEMPOTENCY_CONFLICT");
    open(35);
    wait_items(&daemon, &scope, |items| {
        items.len() == 36
            && items
                .iter()
                .all(|i| i["deliveries"]["inbox"]["status"] == "sent")
    });
    let end = Instant::now() + Duration::from_secs(20);
    let mut stable_since = Instant::now();
    let mut sequence = daemon.cli(&["daemon", "status"])["data"]["storage_seq"].clone();
    loop {
        std::thread::sleep(Duration::from_millis(100));
        let current = daemon.cli(&["daemon", "status"])["data"]["storage_seq"].clone();
        if current != sequence {
            sequence = current;
            stable_since = Instant::now();
        }
        if stable_since.elapsed() >= Duration::from_secs(3) {
            break;
        }
        assert!(
            Instant::now() < end,
            "idle notifications keep writing the outbox"
        );
    }
}

#[test]
fn notification_channel_history_limit_is_visible_without_stopping_new_delivery() {
    let mut daemon = Daemon::new();
    let routes: String = (0..64)
        .map(|n| format!("[notifiers.route{n}]\nkind='local'\n"))
        .collect();
    daemon.configure(&routes);
    let revision = review_tests::seed(&daemon, ReviewMode::Manual, "many-routes");
    daemon.start();
    let scope = extraction_tests::scope();
    let original = wait_items(&daemon, &scope, |items| {
        items.len() == 1 && items[0]["deliveries"].as_object().unwrap().len() == 64
    })[0]
        .clone();
    daemon.configure("[notifiers.later]\nkind='local'\n");
    daemon.cli(&["config", "reload"]);
    let limited = wait_items(&daemon, &scope, |items| {
        items.len() == 1 && items[0]["delivery_error"] == "CHANNEL_HISTORY_LIMIT"
    })[0]
        .clone();
    assert_eq!(limited["deliveries"], original["deliveries"]);
    let opened = rpc(
        &daemon,
        json!({"op":"review_open","memory_id":"many-routes","expected_revision":revision,
        "reason":"New notification can use the new route","idempotency_key":"later-route"}),
    );
    assert!(opened.get("error").is_none(), "{opened}");
    let items = wait_items(&daemon, &scope, |items| {
        items.len() == 2
            && items
                .iter()
                .any(|i| i["deliveries"]["later"]["status"] == "sent")
    });
    assert_eq!(
        items
            .iter()
            .filter(|i| i["delivery_error"] == "CHANNEL_HISTORY_LIMIT")
            .count(),
        1
    );
    assert_eq!(
        daemon.cli(&["daemon", "status"])["data"]["state"],
        "running"
    );
}

#[test]
fn stdio_notifier_sends_minimal_envelopes_and_updates_the_provider_message() {
    let mut daemon = Daemon::new();
    let plugin = daemon.root.join("notifier.py");
    executable::write(&plugin, include_str!("../fixtures/notifier/oneshot.py"));
    daemon.configure(&format!(
        "[notifiers.plugin]\nkind='stdio'\ncommand=[{plugin:?}]\n"
    ));
    review_tests::seed(&daemon, ReviewMode::Manual, "external-notice");
    daemon.start();
    let scope = extraction_tests::scope();
    let first = wait_items(&daemon, &scope, |items| {
        items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "sent"
    })[0]
        .clone();
    let id = first["notification_id"].as_str().unwrap();
    assert_eq!(
        first["deliveries"]["plugin"]["external_message_id"],
        format!("fixture-{id}")
    );
    let read_received = || -> Vec<Value> {
        std::fs::read_to_string(daemon.root.join("received.jsonl"))
            .unwrap()
            .lines()
            .map(|line| serde_json::from_str(line).unwrap())
            .collect()
    };
    let received = read_received();
    assert_eq!(received.len(), 1);
    let expected = [
        "notification_id",
        "idempotency_key",
        "review_id",
        "priority",
        "summary",
        "evidence_locator",
        "action",
        "external_message_id",
    ];
    assert_eq!(
        received[0]
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        expected.into_iter().collect()
    );
    assert_eq!(received[0]["action"], "create");
    assert_eq!(received[0]["notification_id"], id);
    assert!(received[0]["external_message_id"].is_null());
    assert!(!received[0].to_string().contains("fixture build passed"));
    let review = daemon.cli(&["review", "show", first["review_id"].as_str().unwrap()]);
    let changed = rpc(
        &daemon,
        json!({"op":"review_state","id":first["review_id"],
        "expected_revision":review["data"]["item"]["revision"],"state":"investigating",
        "reason":"Private investigation details","idempotency_key":"plugin-update"}),
    );
    assert!(changed.get("error").is_none(), "{changed}");
    let update = wait_items(&daemon, &scope, |items| {
        items.len() == 1
            && items[0]["deliveries"]["plugin"]["generation"] == 2
            && items[0]["deliveries"]["plugin"]["status"] == "sent"
    })[0]
        .clone();
    assert_eq!(
        update["deliveries"]["plugin"]["external_message_id"],
        first["deliveries"]["plugin"]["external_message_id"]
    );
    let received = read_received();
    assert_eq!(received.len(), 2);
    assert_eq!(received[1]["action"], "update");
    assert_eq!(
        received[1]["external_message_id"],
        first["deliveries"]["plugin"]["external_message_id"]
    );
    assert_ne!(
        received[0]["idempotency_key"],
        received[1]["idempotency_key"]
    );
    assert!(!received[1].to_string().contains("Private investigation"));
}

#[test]
fn waiting_stdio_plugin_does_not_hold_admission_and_live_disable_cancels_it() {
    let mut daemon = Daemon::new();
    let plugin = daemon.root.join("notifier.py");
    executable::write(&plugin, include_str!("../fixtures/notifier/oneshot.py"));
    let settings = |enabled: bool| {
        format!("[notifiers.plugin]\nkind='stdio'\nenabled={enabled}\ncommand=[{plugin:?}]\n")
    };
    daemon.configure(&settings(true));
    let revision = review_tests::seed(&daemon, ReviewMode::Manual, "blocked-plugin");
    std::fs::write(daemon.root.join("hold-plugin"), "wait for cancellation").unwrap();
    daemon.start();
    let end = Instant::now() + Duration::from_secs(15);
    while !daemon.root.join("plugin-started").exists() {
        assert!(Instant::now() < end, "stdio plugin never started");
        std::thread::sleep(Duration::from_millis(25));
    }
    let pid: u32 = std::fs::read_to_string(daemon.root.join("plugin.pid"))
        .unwrap()
        .parse()
        .unwrap();
    let started = Instant::now();
    let opened = rpc(
        &daemon,
        json!({"op":"review_open","memory_id":"blocked-plugin","expected_revision":revision,
        "reason":"Independent operation while notifier is waiting","idempotency_key":"while-plugin-waits"}),
    );
    assert!(opened.get("error").is_none(), "{opened}");
    assert!(
        started.elapsed() < Duration::from_secs(5),
        "plugin held the daemon admission lock"
    );
    daemon.configure(&settings(false));
    assert_eq!(
        daemon.cli(&["config", "reload"])["data"]["status"],
        "applied"
    );
    let end = Instant::now() + Duration::from_secs(10);
    while std::path::Path::new(&format!("/proc/{pid}")).exists() {
        assert!(
            Instant::now() < end,
            "disabled notifier child was not reaped"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(!daemon.root.join("received.jsonl").exists());
    let items = wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 2
            && items
                .iter()
                .any(|item| item["deliveries"]["plugin"]["status"] == "blocked")
    });
    assert!(
        items
            .iter()
            .all(|item| item["deliveries"]["plugin"]["status"] != "sent")
    );
    std::fs::remove_file(daemon.root.join("hold-plugin")).unwrap();
    daemon.configure(&settings(true));
    daemon.cli(&["config", "reload"]);
    wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 2
            && items
                .iter()
                .all(|item| item["deliveries"]["plugin"]["status"] == "sent")
    });
}

fn plugin_fixture(mode: &str) -> Daemon {
    let daemon = Daemon::new();
    let plugin = daemon.root.join("notifier.py");
    executable::write(&plugin, include_str!("../fixtures/notifier/oneshot.py"));
    std::fs::write(daemon.root.join("plugin-mode"), mode).unwrap();
    daemon.configure(&format!(
        "[notifiers.plugin]\nkind='stdio'\ncommand=[{plugin:?}]\n"
    ));
    review_tests::seed(&daemon, ReviewMode::Manual, "recovery-notice");
    daemon
}

fn plugin_rows(daemon: &Daemon, name: &str) -> Vec<Value> {
    std::fs::read_to_string(daemon.root.join(name))
        .unwrap()
        .lines()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect()
}

#[test]
fn memory_status_charges_guard_plugin_and_detached_grandchild() {
    let mut daemon = plugin_fixture("grandchild");
    std::fs::write(
        daemon.root.join("hold-plugin"),
        "memory observation barrier",
    )
    .unwrap();
    daemon.start();
    let end = Instant::now() + Duration::from_secs(15);
    while !daemon.root.join("plugin-started").exists() {
        assert!(Instant::now() < end, "plugin did not reach barrier");
        std::thread::sleep(Duration::from_millis(25));
    }
    let child: u32 = std::fs::read_to_string(daemon.root.join("grandchild.pid"))
        .unwrap()
        .parse()
        .unwrap();
    let plugin: u32 = std::fs::read_to_string(daemon.root.join("plugin.pid"))
        .unwrap()
        .parse()
        .unwrap();
    let observed = daemon.cli(&["daemon", "status"]);
    std::fs::remove_file(daemon.root.join("hold-plugin")).unwrap();
    wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "sent"
    });
    let sample = &observed["data"]["resources"]["measurement"];
    assert_eq!(sample["complete"], true);
    let pids = sample["runner_group"]["pids"].as_array().unwrap();
    assert!(
        pids.len() >= 3,
        "guardian and both descendants must be charged"
    );
    assert!(pids.contains(&json!(child)) && pids.contains(&json!(plugin)));
    assert!(!pids.contains(&json!(std::process::id())));
    for metric in ["rss_bytes", "pss_bytes"] {
        let daemon = sample["daemon_group"][metric].as_u64().unwrap();
        let runner = sample["runner_group"][metric].as_u64().unwrap();
        assert!(runner > 0);
        assert_eq!(sample["service_total"][metric], daemon + runner);
    }
    let end = Instant::now() + Duration::from_secs(5);
    loop {
        let current = daemon.cli(&["daemon", "status"]);
        if current["data"]["resources"]["measurement"]["runner_group"]["pids"] == json!([]) {
            break;
        }
        assert!(Instant::now() < end, "reaped children remained charged");
        std::thread::sleep(Duration::from_millis(25));
    }
}

#[test]
fn stdio_invalid_responses_are_bounded_terminal_and_do_not_echo_plugin_output() {
    for (mode, code) in [
        ("unsupported", "NOTIFIER_UNSUPPORTED"),
        ("malformed", "NOTIFIER_PROTOCOL_INVALID"),
        ("bad-id", "NOTIFIER_PROTOCOL_INVALID"),
        ("oversize", "OUTPUT_LIMIT"),
    ] {
        let mut daemon = plugin_fixture(mode);
        daemon.start();
        let failed = wait_items(&daemon, &extraction_tests::scope(), |items| {
            items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "failed"
        });
        assert_eq!(failed[0]["deliveries"]["plugin"]["attempt"]["error"], code);
        assert_eq!(failed[0]["deliveries"]["plugin"]["attempt"]["number"], 1);
        assert!(
            !serde_json::to_string(&failed)
                .unwrap()
                .contains("DO_NOT_ECHO")
        );
        assert_eq!(
            daemon.cli(&["daemon", "status"])["data"]["state"],
            "running"
        );
    }
}

#[test]
fn stdio_provider_without_update_receives_a_new_create_for_escalation() {
    let mut daemon = plugin_fixture("no-update");
    daemon.start();
    let first = wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "sent"
    })[0]
        .clone();
    let review = daemon.cli(&["review", "show", first["review_id"].as_str().unwrap()]);
    assert!(rpc(&daemon,json!({"op":"review_state","id":first["review_id"],"expected_revision":review["data"]["item"]["revision"],"state":"investigating","reason":"Update fallback","idempotency_key":"without-update"})).get("error").is_none());
    wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1
            && items[0]["deliveries"]["plugin"]["generation"] == 2
            && items[0]["deliveries"]["plugin"]["status"] == "sent"
    });
    let received = plugin_rows(&daemon, "received.jsonl");
    assert_eq!(received.len(), 2);
    assert_eq!(received[1]["action"], "create");
    assert!(received[1]["external_message_id"].is_null());
    assert_ne!(
        received[1]["idempotency_key"],
        received[0]["idempotency_key"]
    );
}

#[test]
fn stdio_credential_rotation_and_read_revocation_cancel_owned_processes() {
    let mut daemon = plugin_fixture("normal");
    let plugin = daemon.root.join("notifier.py");
    let settings = |credential: &str| {
        format!("[notifiers.plugin]\nkind='stdio'\ncommand=[{plugin:?}]\n{credential}\n")
    };
    // HOME is explicitly isolated by Daemon::command; no real secret is involved.
    daemon.configure(&settings("credential_ref='env:HOME'"));
    std::fs::write(daemon.root.join("hold-plugin"), "wait for revocation").unwrap();
    daemon.start();
    let wait_attempts = |count| {
        let end = Instant::now() + Duration::from_secs(15);
        loop {
            if daemon.root.join("attempts.jsonl").exists()
                && plugin_rows(&daemon, "attempts.jsonl").len() >= count
            {
                break;
            }
            assert!(Instant::now() < end, "plugin did not reach attempt {count}");
            std::thread::sleep(Duration::from_millis(25));
        }
    };
    wait_attempts(1);
    let old = std::fs::read_to_string(daemon.root.join("plugin.pid")).unwrap();
    daemon.configure(&settings(""));
    daemon.cli(&["config", "reload"]);
    wait_attempts(2);
    assert!(!std::path::Path::new(&format!("/proc/{old}")).exists());
    let current = std::fs::read_to_string(daemon.root.join("plugin.pid")).unwrap();
    let mut beta = extraction_tests::scope();
    beta.project = Some("beta".into());
    assert!(
        rpc(&daemon, json!({"op":"access_set","scope":beta}))
            .get("error")
            .is_none()
    );
    let end = Instant::now() + Duration::from_secs(10);
    while std::path::Path::new(&format!("/proc/{current}")).exists() {
        assert!(Instant::now() < end, "revoked plugin remained live");
        std::thread::sleep(Duration::from_millis(25));
    }
    assert!(!daemon.root.join("received.jsonl").exists());
    std::fs::remove_file(daemon.root.join("hold-plugin")).unwrap();
    assert!(
        rpc(
            &daemon,
            json!({"op":"access_set","scope":extraction_tests::scope()})
        )
        .get("error")
        .is_none()
    );
    wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "sent"
    });
    let attempts = plugin_rows(&daemon, "attempts.jsonl");
    assert_eq!(attempts.len(), 3);
    assert_eq!(attempts[0]["credential_present"], true);
    assert_eq!(attempts[1]["credential_present"], false);
    assert_eq!(attempts[2]["credential_present"], false);
    assert_ne!(
        attempts[0]["idempotency_key"],
        attempts[1]["idempotency_key"]
    );
    assert_eq!(
        attempts[1]["idempotency_key"],
        attempts[2]["idempotency_key"]
    );
    assert_eq!(plugin_rows(&daemon, "received.jsonl").len(), 1);
}

#[test]
fn notification_pages_bound_serialized_delivery_history_bytes() {
    use akasha_otsuge::storage::{Change, CommitRequest, RecordKey, RecordKind, Storage};
    use sha2::Digest;
    let hash = |bytes: &[u8]| akasha_otsuge::capture::encode_hex(&sha2::Sha256::digest(bytes));
    let mut daemon = plugin_fixture("normal");
    daemon.start();
    let template = wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "sent"
    })[0]
        .clone();
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.configure("");
    extraction_tests::runtime().block_on(async {
        let store = Storage::open(&extraction_tests::storage_config(&daemon))
            .await
            .unwrap();
        for n in 0..34 {
            let mut item = template.clone();
            let review = hash(format!("large-review-{n}").as_bytes());
            let id = hash(format!("review/{review}").as_bytes());
            item["notification_id"] = json!(id);
            item["review_id"] = json!(review);
            let mut delivery = template["deliveries"]["plugin"].clone();
            delivery["external_message_id"] = json!("x".repeat(256));
            delivery["attempt"]["envelope"]["notification_id"] = item["notification_id"].clone();
            delivery["attempt"]["envelope"]["review_id"] = item["review_id"].clone();
            item["deliveries"] = Value::Object(
                (0..60)
                    .map(|channel| (format!("historical-{channel}"), delivery.clone()))
                    .collect(),
            );
            let content = serde_json::to_string(&item).unwrap();
            assert!(content.len() > 70 * 1024 && content.len() < 128 * 1024);
            store
                .commit(&CommitRequest {
                    request_id: format!("large-notification-{n}"),
                    config_generation: 1,
                    policy_revision: None,
                    changes: vec![Change {
                        key: RecordKey {
                            kind: RecordKind::State,
                            id: format!("notification/item/{id}"),
                        },
                        expected_revision: None,
                        content,
                    }],
                })
                .await
                .unwrap();
        }
        store.close().await.unwrap();
    });
    daemon.start();
    let items = all(&daemon, &extraction_tests::scope());
    assert_eq!(items.len(), 35);
    assert_eq!(
        items
            .iter()
            .map(|i| i["notification_id"].as_str().unwrap())
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        35
    );
    assert_eq!(
        daemon.cli(&["daemon", "status"])["data"]["state"],
        "running"
    );
}

#[test]
fn notification_record_limit_defers_large_delivery_history_without_stopping_daemon() {
    use akasha_otsuge::storage::{Change, CommitRequest, RecordKey, RecordKind, Storage};
    let mut daemon = plugin_fixture("normal");
    daemon.start();
    let mut item = wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "sent"
    })[0]
        .clone();
    let id = item["notification_id"].as_str().unwrap().to_owned();
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    let plugin = daemon.root.join("notifier.py");
    daemon.configure(&format!(
        "[notifiers.new]\nkind='stdio'\ncommand=[{plugin:?}]\n"
    ));
    let mut delivery = item["deliveries"]["plugin"].clone();
    delivery["external_message_id"] = json!("\\".repeat(256));
    delivery["attempt"]["envelope"]["external_message_id"] = json!("\\".repeat(256));
    item["deliveries"] = json!({});
    for n in 0..63 {
        item["deliveries"][format!("history-{n}-{}", "\\".repeat(100))] = delivery.clone();
        if serde_json::to_vec(&item).unwrap().len() >= 121 * 1024 {
            break;
        }
    }
    let size = serde_json::to_vec(&item).unwrap().len();
    assert!(
        (121 * 1024..126 * 1024).contains(&size),
        "fixture byte size {size}"
    );
    extraction_tests::runtime().block_on(async {
        let store = Storage::open(&extraction_tests::storage_config(&daemon))
            .await
            .unwrap();
        let key = RecordKey {
            kind: RecordKind::State,
            id: format!("notification/item/{id}"),
        };
        let previous = store.current(&key).unwrap().unwrap().commit_seq;
        store
            .commit(&CommitRequest {
                request_id: "notification-capacity-fixture".into(),
                config_generation: 1,
                policy_revision: None,
                changes: vec![Change {
                    key,
                    expected_revision: Some(previous),
                    content: serde_json::to_string(&item).unwrap(),
                }],
            })
            .await
            .unwrap();
        store.close().await.unwrap();
    });
    daemon.start();
    let limited = wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1 && items[0]["delivery_error"] == "DELIVERY_RECORD_LIMIT"
    });
    assert_eq!(limited[0]["deliveries"], item["deliveries"]);
    assert_eq!(plugin_rows(&daemon, "received.jsonl").len(), 1);
    assert_eq!(
        daemon.cli(&["daemon", "status"])["data"]["state"],
        "running"
    );
}

#[test]
fn stdio_live_policy_defers_delivery_and_policy_reload_does_not_replay_success() {
    let mut daemon = plugin_fixture("normal");
    let plugin = daemon.root.join("notifier.py");
    let settings = |policy: &str| {
        format!(
            "[notifiers.inbox]\nkind='local'\n[notifiers.plugin]\nkind='stdio'\ncommand=[{plugin:?}]\n{policy}\n"
        )
    };
    daemon.configure(&settings("min_priority='high'"));
    daemon.start();
    wait_local(&daemon, 1);
    assert!(!daemon.root.join("received.jsonl").exists());
    for policy in ["quiet_hours_utc=[0,24]", "digest_seconds=86400"] {
        daemon.configure(&settings(policy));
        assert_eq!(
            daemon.cli(&["config", "reload"])["data"]["status"],
            "applied"
        );
        std::thread::sleep(Duration::from_millis(1200));
        assert!(
            !daemon.root.join("received.jsonl").exists(),
            "policy did not defer delivery: {policy}"
        );
    }
    daemon.configure(&settings("digest_seconds=0"));
    daemon.cli(&["config", "reload"]);
    wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "sent"
    });
    daemon.configure(&settings("timeout='2s'"));
    daemon.cli(&["config", "reload"]);
    std::thread::sleep(Duration::from_millis(1200));
    assert_eq!(plugin_rows(&daemon, "received.jsonl").len(), 1);
    assert_eq!(plugin_rows(&daemon, "attempts.jsonl").len(), 1);
}

#[test]
fn stdio_timeout_is_pinned_and_policy_only_reload_does_not_cancel_inflight_send() {
    let mut daemon = plugin_fixture("normal");
    let plugin = daemon.root.join("notifier.py");
    let settings = |policy: &str| {
        format!("[notifiers.plugin]\nkind='stdio'\ncommand=[{plugin:?}]\n{policy}\n")
    };
    daemon.configure(&settings("timeout='30s'"));
    std::fs::write(daemon.root.join("after-send"), "hold response").unwrap();
    daemon.start();
    let end = Instant::now() + Duration::from_secs(15);
    while !daemon.root.join("plugin-sent").exists() {
        assert!(Instant::now() < end);
        std::thread::sleep(Duration::from_millis(25));
    }
    daemon.configure(&settings(
        "timeout='1ms'\nquiet_hours_utc=[0,24]\nmin_priority='urgent'",
    ));
    daemon.cli(&["config", "reload"]);
    std::thread::sleep(Duration::from_millis(200));
    let before = all(&daemon, &extraction_tests::scope());
    assert_eq!(before[0]["deliveries"]["plugin"]["status"], "sending");
    std::fs::remove_file(daemon.root.join("after-send")).unwrap();
    let sent = wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "sent"
    });
    assert_eq!(sent[0]["deliveries"]["plugin"]["attempt"]["number"], 1);
    assert_eq!(plugin_rows(&daemon, "attempts.jsonl").len(), 1);
}

#[test]
fn stdio_timeout_reaps_the_plugin_and_persists_retry() {
    let mut daemon = plugin_fixture("normal");
    let plugin = daemon.root.join("notifier.py");
    daemon.configure(&format!(
        "[notifiers.plugin]\nkind='stdio'\ncommand=[{plugin:?}]\ntimeout='5s'\n"
    ));
    std::fs::write(daemon.root.join("hold-plugin"), "wait past timeout").unwrap();
    daemon.start();
    let end = Instant::now() + Duration::from_secs(10);
    while !daemon.root.join("plugin-started").exists() {
        assert!(
            Instant::now() < end,
            "timeout fixture never reached delivery"
        );
        std::thread::sleep(Duration::from_millis(25));
    }
    let pid = std::fs::read_to_string(daemon.root.join("plugin.pid")).unwrap();
    let waiting = wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "retry_wait"
    });
    assert_eq!(
        waiting[0]["deliveries"]["plugin"]["attempt"]["error"],
        "NOTIFIER_TIMEOUT"
    );
    assert!(!std::path::Path::new(&format!("/proc/{pid}")).exists());
    assert!(!daemon.root.join("received.jsonl").exists());
    std::fs::remove_file(daemon.root.join("hold-plugin")).unwrap();
    let sent = wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "sent"
    });
    assert_eq!(sent[0]["deliveries"]["plugin"]["attempt"]["number"], 2);
}

fn crash_plugin_daemon(daemon: &mut Daemon) {
    let plugin: u32 = std::fs::read_to_string(daemon.root.join("plugin.pid"))
        .unwrap()
        .parse()
        .unwrap();
    let guard = std::fs::read_to_string(format!("/proc/{plugin}/stat"))
        .ok()
        .map(|stat| {
            stat.rsplit_once(") ")
                .unwrap()
                .1
                .split_whitespace()
                .nth(1)
                .unwrap()
                .parse::<u32>()
                .unwrap()
        });
    let directory = guard.map(|pid| std::fs::read_link(format!("/proc/{pid}/cwd")).unwrap());
    let mut child = daemon.child.take().unwrap();
    child.kill().unwrap();
    assert!(!child.wait().unwrap().success());
    let end = Instant::now() + Duration::from_secs(10);
    let grandchild = std::fs::read_to_string(daemon.root.join("grandchild.pid"))
        .ok()
        .map(|s| s.parse::<u32>().unwrap());
    for pid in std::iter::once(plugin).chain(guard).chain(grandchild) {
        loop {
            let live = std::fs::read_to_string(format!("/proc/{pid}/stat")).is_ok_and(|stat| {
                !matches!(
                    stat.rsplit_once(") ").unwrap().1.split_whitespace().next(),
                    Some("Z" | "X")
                )
            });
            if !live {
                break;
            }
            assert!(
                Instant::now() < end,
                "orphaned notifier process {pid} survived daemon SIGKILL"
            );
            std::thread::sleep(Duration::from_millis(25));
        }
    }
    if let Some(directory) = directory {
        assert!(
            !directory.exists(),
            "guardian left its owned scratch directory: {directory:?}"
        );
    }
}

#[test]
fn stdio_recovery_missing_executable_preserves_envelope_and_exhausts_finite_budget() {
    for restore in [true, false] {
        let mut daemon = plugin_fixture("normal");
        let plugin = daemon.root.join("notifier.py");
        let backup = daemon.root.join("notifier.saved");
        std::fs::rename(&plugin, &backup).unwrap();
        daemon.start();
        let first = wait_items(&daemon, &extraction_tests::scope(), |items| {
            items.len() == 1
                && matches!(
                    items[0]["deliveries"]["plugin"]["status"].as_str(),
                    Some("retry_wait" | "failed")
                )
        })[0]
            .clone();
        let accepted = &first["deliveries"]["plugin"]["attempt"];
        assert_eq!(first["deliveries"]["plugin"]["status"], "retry_wait");
        assert_eq!(accepted["error"], "NOTIFIER_UNAVAILABLE");
        assert_eq!(accepted["transient_failures"], 1);
        assert!(!daemon.root.join("attempts.jsonl").exists());
        if restore {
            daemon.cli(&["daemon", "stop"]);
            daemon.wait();
            std::fs::rename(&backup, &plugin).unwrap();
            daemon.start();
            let sent = wait_items(&daemon, &extraction_tests::scope(), |items| {
                items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "sent"
            })[0]
                .clone();
            let attempt = &sent["deliveries"]["plugin"]["attempt"];
            assert_eq!(attempt["envelope"], accepted["envelope"]);
            assert_eq!(
                attempt["first_started_at_ms"],
                accepted["first_started_at_ms"]
            );
            assert_eq!(attempt["number"], 2);
            assert_eq!(attempt["transient_failures"], 1);
            assert_eq!(plugin_rows(&daemon, "received.jsonl").len(), 1);
        } else {
            let failed = wait_items(&daemon, &extraction_tests::scope(), |items| {
                items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "failed"
            })[0]
                .clone();
            assert_eq!(failed["deliveries"]["plugin"]["attempt"]["number"], 5);
            assert_eq!(
                failed["deliveries"]["plugin"]["attempt"]["transient_failures"],
                5
            );
            daemon.cli(&["daemon", "stop"]);
            daemon.wait();
            std::fs::rename(&backup, &plugin).unwrap();
            daemon.start();
            std::thread::sleep(Duration::from_millis(1100));
            assert_eq!(
                all(&daemon, &extraction_tests::scope())[0]["deliveries"],
                failed["deliveries"]
            );
            assert!(!daemon.root.join("attempts.jsonl").exists());
        }
    }
}

#[test]
fn stdio_recovery_destination_rotation_retains_exhausted_budget_and_success() {
    let mut daemon = plugin_fixture("always-fail");
    let a = daemon.root.join("notifier.py");
    let b_root = daemon.root.join("destination-b");
    std::fs::create_dir(&b_root).unwrap();
    let b = b_root.join("notifier.py");
    executable::write(&b, include_str!("../fixtures/notifier/oneshot.py"));
    std::fs::write(b_root.join("hold-plugin"), "hold destination B").unwrap();
    let settings =
        |path: &std::path::Path| format!("[notifiers.plugin]\nkind='stdio'\ncommand=[{path:?}]\n");
    daemon.start();
    let failed = wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "failed"
    })[0]
        .clone();
    assert_eq!(plugin_rows(&daemon, "attempts.jsonl").len(), 5);
    daemon.configure(&settings(&b));
    daemon.cli(&["config", "reload"]);
    let pending = wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1
            && items[0]["deliveries"]["plugin"]["status"] == "sending"
            && b_root.join("plugin-started").exists()
    })[0]
        .clone();
    let accepted = pending["deliveries"]["plugin"]["attempt"].clone();
    assert_ne!(
        accepted["envelope"]["idempotency_key"],
        failed["deliveries"]["plugin"]["attempt"]["envelope"]["idempotency_key"]
    );
    assert!(accepted["envelope"]["external_message_id"].is_null());
    assert_eq!(accepted["number"], 1);
    daemon.configure(&settings(&a));
    daemon.cli(&["config", "reload"]);
    wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "blocked"
    });
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    std::fs::write(daemon.root.join("plugin-mode"), "normal").unwrap();
    daemon.start();
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(plugin_rows(&daemon, "attempts.jsonl").len(), 5);
    assert!(!daemon.root.join("received.jsonl").exists());
    std::fs::remove_file(b_root.join("hold-plugin")).unwrap();
    daemon.configure(&settings(&b));
    daemon.cli(&["config", "reload"]);
    let sent = wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "sent"
    })[0]
        .clone();
    assert_eq!(
        sent["deliveries"]["plugin"]["attempt"]["envelope"],
        accepted["envelope"]
    );
    assert_eq!(sent["deliveries"]["plugin"]["attempt"]["number"], 2);
    daemon.configure(&settings(&a));
    daemon.cli(&["config", "reload"]);
    std::thread::sleep(Duration::from_millis(1100));
    assert_eq!(
        all(&daemon, &extraction_tests::scope())[0]["deliveries"],
        sent["deliveries"]
    );
    assert_eq!(plugin_rows(&daemon, "attempts.jsonl").len(), 5);
    let review = daemon.cli(&["review", "show", sent["review_id"].as_str().unwrap()]);
    assert!(rpc(&daemon,json!({"op":"review_state","id":sent["review_id"],"expected_revision":review["data"]["item"]["revision"],"state":"investigating","reason":"New notification generation","idempotency_key":"route-generation"})).get("error").is_none());
    let next = wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1
            && items[0]["deliveries"]["plugin"]["generation"] == 2
            && items[0]["deliveries"]["plugin"]["status"] == "sent"
    })[0]
        .clone();
    assert_eq!(next["deliveries"]["plugin"]["attempt"]["number"], 1);
    assert_eq!(
        next["deliveries"]["plugin"]["attempt"]["transient_failures"],
        0
    );
    assert_eq!(plugin_rows(&daemon, "received.jsonl").len(), 1);
}

#[test]
fn stdio_recovery_returned_destination_replays_old_envelope_before_new_generation() {
    let mut daemon = plugin_fixture("normal");
    let a = daemon.root.join("notifier.py");
    std::fs::write(daemon.root.join("hold-plugin"), "hold A").unwrap();
    let b_root = daemon.root.join("destination-b");
    std::fs::create_dir(&b_root).unwrap();
    let b = b_root.join("notifier.py");
    executable::write(&b, include_str!("../fixtures/notifier/oneshot.py"));
    std::fs::write(b_root.join("hold-plugin"), "hold B").unwrap();
    let settings =
        |path: &std::path::Path| format!("[notifiers.plugin]\nkind='stdio'\ncommand=[{path:?}]\n");
    daemon.start();
    let first = wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1
            && items[0]["deliveries"]["plugin"]["status"] == "sending"
            && daemon.root.join("plugin-started").exists()
    })[0]
        .clone();
    daemon.configure(&settings(&b));
    daemon.cli(&["config", "reload"]);
    wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1
            && items[0]["deliveries"]["plugin"]["status"] == "sending"
            && b_root.join("plugin-started").exists()
    });
    let review = daemon.cli(&["review", "show", first["review_id"].as_str().unwrap()]);
    assert!(rpc(&daemon,json!({"op":"review_state","id":first["review_id"],"expected_revision":review["data"]["item"]["revision"],"state":"investigating","reason":"Changed while another destination is active","idempotency_key":"return-route-generation"})).get("error").is_none());
    wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1 && items[0]["generation"] == 2
    });
    daemon.configure(&settings(&a));
    daemon.cli(&["config", "reload"]);
    let resumed = wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1
            && items[0]["deliveries"]["plugin"]["status"] == "sending"
            && plugin_rows(&daemon, "attempts.jsonl").len() == 2
    })[0]
        .clone();
    assert_eq!(
        resumed["deliveries"]["plugin"]["attempt"]["envelope"],
        first["deliveries"]["plugin"]["attempt"]["envelope"]
    );
    assert_eq!(resumed["deliveries"]["plugin"]["attempt"]["number"], 2);
    assert_eq!(resumed["deliveries"]["plugin"]["generation"], 1);
    std::fs::remove_file(daemon.root.join("hold-plugin")).unwrap();
    wait_items(&daemon, &extraction_tests::scope(), |items| {
        items.len() == 1
            && items[0]["deliveries"]["plugin"]["generation"] == 2
            && items[0]["deliveries"]["plugin"]["status"] == "sent"
    });
    let received = plugin_rows(&daemon, "received.jsonl");
    assert_eq!(received.len(), 2);
    assert_eq!(
        received[0],
        first["deliveries"]["plugin"]["attempt"]["envelope"]
    );
    assert_eq!(received[1]["action"], "update");
    assert_ne!(
        received[0]["idempotency_key"],
        received[1]["idempotency_key"]
    );
    assert!(!b_root.join("received.jsonl").exists());
}

#[test]
fn stdio_retry_survives_restart_and_stops_after_five_attempts() {
    let mut daemon = plugin_fixture("fail-once");
    daemon.start();
    let scope = extraction_tests::scope();
    let waiting = wait_items(&daemon, &scope, |items| {
        items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "retry_wait"
    })[0]
        .clone();
    let planned = waiting["deliveries"]["plugin"]["attempt"]["next_retry_at_ms"]
        .as_u64()
        .unwrap();
    let first_started = waiting["deliveries"]["plugin"]["attempt"]["first_started_at_ms"].clone();
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    daemon.start();
    let sent = wait_items(&daemon, &scope, |items| {
        items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "sent"
    })[0]
        .clone();
    assert_eq!(sent["deliveries"]["plugin"]["attempt"]["number"], 2);
    assert_eq!(
        sent["deliveries"]["plugin"]["attempt"]["first_started_at_ms"],
        first_started
    );
    let attempts = plugin_rows(&daemon, "attempts.jsonl");
    assert_eq!(attempts.len(), 2);
    assert_eq!(
        attempts[0]["idempotency_key"],
        attempts[1]["idempotency_key"]
    );
    assert!(attempts[1]["at_ms"].as_u64().unwrap() >= planned);
    assert_eq!(plugin_rows(&daemon, "received.jsonl").len(), 1);

    let mut failing = plugin_fixture("always-fail");
    failing.start();
    let end = Instant::now() + Duration::from_secs(60);
    loop {
        let items = all(&failing, &scope);
        if items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "failed" {
            assert_eq!(items[0]["deliveries"]["plugin"]["attempt"]["number"], 5);
            assert_eq!(
                items[0]["deliveries"]["plugin"]["attempt"]["error"],
                "NOTIFIER_FAILED"
            );
            break;
        }
        assert!(
            Instant::now() < end,
            "finite retry budget never settled: {items:?}"
        );
        std::thread::sleep(Duration::from_millis(100));
    }
    assert_eq!(plugin_rows(&failing, "attempts.jsonl").len(), 5);
    failing.cli(&["daemon", "stop"]);
    failing.wait();
    failing.start();
    let state = all(&failing, &scope);
    assert_eq!(state[0]["deliveries"]["plugin"]["status"], "failed");
    assert_eq!(plugin_rows(&failing, "attempts.jsonl").len(), 5);
}

#[test]
fn stdio_fifth_attempt_policy_cancellation_can_resume_without_renewing_failure_budget() {
    for (restored_mode, expected_state) in [("normal", "sent"), ("always-fail", "failed")] {
        let mut daemon = plugin_fixture("fail-four");
        let plugin = daemon.root.join("notifier.py");
        let settings = |enabled| {
            format!("[notifiers.plugin]\nkind='stdio'\nenabled={enabled}\ncommand=[{plugin:?}]\n")
        };
        std::fs::write(daemon.root.join("hold-plugin"), "hold fifth delivery").unwrap();
        daemon.start();
        let end = Instant::now() + Duration::from_secs(60);
        loop {
            if daemon.root.join("attempts.jsonl").exists()
                && plugin_rows(&daemon, "attempts.jsonl").len() == 5
            {
                break;
            }
            assert!(Instant::now() < end, "fifth attempt was not reached");
            std::thread::sleep(Duration::from_millis(25));
        }
        let accepted =
            all(&daemon, &extraction_tests::scope())[0]["deliveries"]["plugin"]["attempt"].clone();
        daemon.configure(&settings(false));
        daemon.cli(&["config", "reload"]);
        wait_items(&daemon, &extraction_tests::scope(), |items| {
            items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "blocked"
        });
        std::fs::write(daemon.root.join("plugin-mode"), restored_mode).unwrap();
        std::fs::remove_file(daemon.root.join("hold-plugin")).unwrap();
        daemon.configure(&settings(true));
        daemon.cli(&["config", "reload"]);
        let final_item = wait_items(&daemon, &extraction_tests::scope(), |items| {
            items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == expected_state
        })[0]
            .clone();
        let attempt = &final_item["deliveries"]["plugin"]["attempt"];
        assert_eq!(attempt["number"], 6);
        assert_eq!(
            attempt["first_started_at_ms"],
            accepted["first_started_at_ms"]
        );
        assert_eq!(attempt["envelope"], accepted["envelope"]);
        assert_eq!(
            attempt["transient_failures"],
            if restored_mode == "normal" { 4 } else { 5 }
        );
        assert_eq!(plugin_rows(&daemon, "attempts.jsonl").len(), 6);
        daemon.cli(&["daemon", "stop"]);
        daemon.wait();
        daemon.start();
        assert_eq!(
            all(&daemon, &extraction_tests::scope())[0]["deliveries"]["plugin"]["status"],
            expected_state
        );
        assert_eq!(plugin_rows(&daemon, "attempts.jsonl").len(), 6);
    }
}

#[test]
fn stdio_crash_windows_replay_with_idempotency_or_expose_duplicate_risk() {
    for (barrier, mode, expected_deliveries) in [
        ("hold-plugin", "normal", 1),
        ("after-send", "normal", 1),
        ("after-send", "no-idempotency", 2),
        ("hold-plugin", "grandchild", 1),
    ] {
        let mut daemon = plugin_fixture(mode);
        std::fs::write(daemon.root.join(barrier), "crash barrier").unwrap();
        daemon.start();
        let marker = if barrier == "after-send" {
            "plugin-sent"
        } else {
            "plugin-started"
        };
        let end = Instant::now() + Duration::from_secs(15);
        while !daemon.root.join(marker).exists() {
            assert!(Instant::now() < end, "plugin did not reach {barrier}");
            std::thread::sleep(Duration::from_millis(25));
        }
        let before = all(&daemon, &extraction_tests::scope())[0].clone();
        assert_eq!(before["deliveries"]["plugin"]["status"], "sending");
        let first_key =
            before["deliveries"]["plugin"]["attempt"]["envelope"]["idempotency_key"].clone();
        crash_plugin_daemon(&mut daemon);
        std::fs::remove_file(daemon.root.join(barrier)).unwrap();
        daemon.start();
        let recovered = wait_items(&daemon, &extraction_tests::scope(), |items| {
            items.len() == 1 && items[0]["deliveries"]["plugin"]["status"] == "sent"
        })[0]
            .clone();
        let attempt = &recovered["deliveries"]["plugin"]["attempt"];
        assert_eq!(attempt["number"], 2);
        assert_eq!(attempt["envelope"]["idempotency_key"], first_key);
        assert_eq!(attempt["duplicate_risk"], mode == "no-idempotency");
        assert_eq!(
            plugin_rows(&daemon, "received.jsonl").len(),
            expected_deliveries
        );
        assert_eq!(plugin_rows(&daemon, "attempts.jsonl").len(), 2);
    }
}
