use super::*;
use akasha_otsuge::config::{Paths, resolve};
use akasha_otsuge::domain::*;
use akasha_otsuge::storage::Storage;

fn scope() -> Scope {
    Scope {
        user: format!("uid:{}", rustix::process::geteuid().as_raw()),
        project: Some("alpha".into()),
        branch_or_version: None,
        session: None,
    }
}

fn storage_config(daemon: &Daemon) -> akasha_otsuge::config::ResolvedConfig {
    resolve(
        "",
        None,
        "",
        &Paths {
            data_dir: daemon.root.join("data"),
            socket_path: daemon.socket(),
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

fn seed_source(daemon: &Daemon, event_id: &str, text: &str) -> SourceSpan {
    runtime().block_on(async {
        let store = Storage::open(&storage_config(daemon)).await.unwrap();
        let domain = Domain::new(&store);
        let ingress = Ingress::from_verified_human_channel(
            &scope().user,
            "fixture-channel",
            scope(),
            StatementIntent::Tentative,
        )
        .unwrap();
        let receipt = domain
            .record_source(
                &format!("source-{event_id}"),
                &SourceDraft {
                    event_id: event_id.into(),
                    scope: scope(),
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
        let span = SourceSpan {
            event_id: event_id.into(),
            revision: receipt.commit_seq,
            content_sha256: domain
                .source(event_id, receipt.commit_seq)
                .unwrap()
                .content_sha256,
            start: 0,
            end: text.len(),
        };
        store.close().await.unwrap();
        span
    })
}

fn submit(daemon: &Daemon, id: &str, plan: &Value) -> Value {
    let mut stream = daemon.connect();
    receive(&mut stream);
    send(
        &mut stream,
        &json!({"protocol_version":1,"request_id":id,
            "command":{"op":"extraction_run","idempotency_key":id,"plan":plan}}),
    );
    receive(&mut stream)
}

fn finished(daemon: &Daemon, id: &str) -> Value {
    let end = Instant::now() + Duration::from_secs(30);
    loop {
        let job = daemon.cli(&["job", "show", id])["data"].clone();
        if matches!(
            job["state"].as_str(),
            Some("succeeded" | "failed" | "cancelled")
        ) {
            return job;
        }
        assert!(Instant::now() < end, "{job}");
        std::thread::sleep(Duration::from_millis(20));
    }
}

#[test]
fn text_extraction_is_durable_source_backed_candidate_only_and_idempotent() {
    let endpoint = super::model_tests::Endpoint::extraction();
    let mut daemon = Daemon::new();
    let source = seed_source(
        &daemon,
        "conditional",
        "Only if offline, use keyword search.",
    );
    daemon.configure(&format!(
        "[retrieval]\nextraction='extractor'\n[review]\nmode='automatic'\n[models.extractor]\nkind='text'\nendpoint={:?}\nmodel='controlled-extractor'\nlocal=true\n",
        endpoint.address
    ));
    daemon.start();
    let plan = json!({"scope":scope(),"sources":[source]});
    let accepted = submit(&daemon, "extract-first", &plan);
    assert!(
        accepted.get("error").is_none(),
        "extraction was not admitted: {accepted}"
    );
    let job = finished(&daemon, "extract-first");
    assert_eq!(job["state"], "succeeded", "{job}");
    let candidates = job["result"]["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 1);
    let memory_id = candidates[0]["memory_id"].as_str().unwrap().to_owned();
    assert_eq!(
        submit(&daemon, "extract-first", &plan)["data"]["result"],
        job["result"]
    );
    assert_eq!(endpoint.requests.lock().unwrap().len(), 1);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    runtime().block_on(async {
        let store = Storage::open(&storage_config(&daemon)).await.unwrap();
        let domain = Domain::new(&store);
        let memory = domain.memory(&memory_id).unwrap().unwrap();
        assert_eq!(memory.record.authority, Authority::Candidate);
        assert_eq!(memory.record.lifecycle, Lifecycle::Candidate);
        assert_eq!(memory.record.provenance.origin, Origin::Extraction);
        assert_eq!(memory.record.draft.conditions, ["offline"]);
        assert_eq!(memory.record.draft.source_spans, [source]);
        assert_eq!(
            memory.record.provenance.generator.unwrap().prompt_version,
            "extraction-v1"
        );
        assert_eq!(
            domain
                .source(
                    "conditional",
                    plan["sources"][0]["revision"].as_u64().unwrap()
                )
                .unwrap()
                .payload
                .text,
            "Only if offline, use keyword search."
        );
        store.close().await.unwrap();
    });
}
