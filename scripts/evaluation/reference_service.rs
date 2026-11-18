// Evaluation-only entry compiled into an isolated copy of the frozen daemon module.
use super::*;
use crate::{
    adapter::{BindingLevel, CallerProof, ConsultArgs, Invocation, RuntimeBinding},
    capture::{
        Capture, CaptureBinding, CoverageLevel, EventDraft, EventKind, MessagePhase,
        SourceIdentity, Visibility,
    },
    domain::{Domain, Ingress, Scope},
};
use serde_json::Value;
use std::io::{BufRead, Write};

fn emit(v: Value) -> Result<()> {
    let mut out = std::io::stdout().lock();
    serde_json::to_writer(&mut out, &v)?;
    writeln!(out)?;
    out.flush()?;
    Ok(())
}

async fn sync_private_index(service: &Arc<Service>) -> Result<Value> {
    let id = "reference-index-preparation";
    let plan = crate::retrieval::Request::Sync;
    service.submit_retrieval(id.to_owned(), plan).await?;
    // Only the sync may execute here. Starting the full scheduler could start a paid consult.
    if !service.claim(id, 0).await? {
        return Err(Error("REFERENCE_SYNC_NOT_CLAIMED"));
    }
    service.clone().execute_job(id.to_owned()).await?;
    let job = service.show_job(id).await?;
    if job["state"] != "succeeded" {
        return Err(Error("REFERENCE_INDEX_SYNC_FAILED"));
    }
    Ok(json!({"job":id,"state":job["state"],"result":job["result"]}))
}

async fn scoped_search(
    service: &Arc<Service>,
    id: &str,
    scope: &Scope,
    query: &str,
) -> Result<Value> {
    use crate::config::deadline::{Deadline, Layer};
    let snapshot = service.config.pin();
    let workflow = Deadline::start(Layer::Workflow, Duration::from_secs(60), None)?;
    let index = service.retrieval_index(id, &workflow).await?;
    let outcome = service
        .search(
            id,
            &workflow,
            index,
            snapshot.clone(),
            Some(snapshot.generation()),
            crate::retrieval::Request::Search {
                query: query.to_owned(),
                scope: scope.clone(),
            },
        )
        .await?;
    let job = service.job(id)?.1;
    let receipt = Domain::new(&service.storage)
        .recall_receipt(
            &service.job_read_context(&job)?,
            outcome["receipt_id"].as_str().ok_or(Error("JOB_CORRUPT"))?,
        )
        .await?;
    let payload: Value = serde_json::from_str(&receipt.data.output)?;
    Ok(
        json!({"status":payload["status"],"scope":payload["scope"],"items":payload["items"],"rules":payload["rules"],
        "model_calls":payload["model_calls"],"coverage":payload["coverage"],"permission_revision":receipt.data.record.permission_revision,
        "receipt_id":outcome["receipt_id"],"read_seq":receipt.data.record.read_seq}),
    )
}

pub async fn run() -> Result<()> {
    let root = Path::new("/fixture");
    let workspace = Path::new("/work");
    let binary = Path::new("/host/codex");
    let home = Path::new("/home/fixture/.codex");
    let case: Value = serde_json::from_slice(&fs::read(root.join("episode.json"))?)?;
    let caller = &case["caller"];
    assert_eq!(case["id"], "H01");
    assert!(case.get("oracle").is_none() && case["hint"].is_null());
    for items in [
        &case["sources"],
        &case["memories"],
        &caller["allowed_event_ids"],
    ] {
        assert_eq!(items.as_array().unwrap().len(), 0);
    }
    assert_eq!(caller["thread"], "root");
    let scope: Scope = serde_json::from_value(caller["scope"].clone())?;
    let input = format!(
        "[access]\nread_roots=[{workspace:?},{home:?}]\n[runtime]\nmax_concurrent_jobs=2\nservice_total_budget_mib=3072\n"
    );
    let config_file = root.join("config.toml");
    fs::write(&config_file, &input)?;
    let config = crate::config::resolve(
        &input,
        None,
        "",
        &crate::config::Paths {
            data_dir: root.join("data"),
            socket_path: root.join("socket"),
        },
    )?;
    let service = open_service(
        config,
        config_file,
        None,
        None,
        Arc::new(resources::Resources::open().await),
    )
    .await?;
    let gen_id = service.config.pin().generation();
    let grant = Domain::new(&service.storage)
        .set_read_grant("bind-reference", &scope.user, Some(&scope), None, gen_id)
        .await?;
    let mut capability = crate::adapter::codex::probe(binary).await?;
    assert_eq!(capability.version.as_deref(), Some("0.164.0"));
    emit(
        json!({"type":"bound","scope":scope,"thread":caller["thread"],"logical_fence":caller["fence_sequence"],"grant":grant,
        "event_ordinals":[],"memory_revisions":[],"native_version": capability.version}),
    )?;

    let line = std::io::stdin().lock().lines().next().transpose()?;
    let Some(line) = line else {
        return service.close(Ok(())).await;
    };
    let transport: Value = serde_json::from_str(&line)?;
    let get = |k: &str| transport[k].as_str().ok_or(Error("INVALID_REQUEST"));
    let thread_id = get("thread_id")?;
    let turn_id = get("turn_id")?;
    let tool_call_id = get("call_id")?;
    let task = get("current_task")?;
    assert_eq!(task, case["current_task"]);
    assert!(transport["hint"].is_null());
    let branch = thread_id.to_owned();
    let source = SourceIdentity {
        adapter: "codex".into(),
        profile: "frozen-reference".into(),
        session: thread_id.into(),
        thread: thread_id.into(),
        agent: "main".into(),
    };
    let binding = CaptureBinding::from_source(
        source.clone(),
        scope.clone(),
        "live-caller",
        CoverageLevel::VisibleText,
    )?;
    // This accepted user input belongs to the new live turn; it is the only captured event.
    let draft = EventDraft {
        event_id: format!("user/{turn_id}"),
        source_version: "1".into(),
        runtime_instance: thread_id.into(),
        branch: branch.clone(),
        parent: None,
        fork_cutoff: None,
        event_time_ms: None,
        observed_time_ms: i64::try_from(ipc::now_ms()?).unwrap(),
        visibility: Visibility::User,
        kind: EventKind::Message {
            message_id: turn_id.into(),
            phase: MessagePhase::Final,
        },
        text: task.into(),
    };
    let captured = Capture::new(&service.storage)
        .ingest(
            &binding,
            &draft,
            task.as_bytes(),
            &Ingress::unverified("live-codex", thread_id, scope.clone())?,
            None,
            gen_id,
        )
        .await?;
    let tip = captured.event.reference;
    capability.binding = BindingLevel::ExactTurn;
    capability.history = CoverageLevel::VisibleText;
    let inv = Invocation::bind(
        &service.storage,
        RuntimeBinding {
            invocation_id: format!(
                "reference/{}",
                crate::storage::hash(tool_call_id.as_bytes())
            ),
            runtime_instance: thread_id.into(),
            capability,
            caller: source,
            principal: scope.user.clone(),
            scope: scope.clone(),
            proof: CallerProof::RegisteredRoot {
                thread: thread_id.into(),
            },
            branch,
            turn_id: Some(turn_id.into()),
            tool_call_id: Some(tool_call_id.into()),
            requested_cursor: 1,
            available_cursor: 1,
            tip: Some(tip.clone()),
            capture_seq: service.storage.sequence()?,
            workspace: workspace.into(),
            config_generation: gen_id,
        },
    )
    .await?;
    emit(
        json!({"type":"invocation","revision":inv.revision(),"fence_sha256":inv.fence_hash(),"binding":inv.diagnostic(),
        "current_request":{"event":tip,"utf8_sha256":crate::storage::hash(task.as_bytes())},"historical_ordinals":[],
        "logical_fence":caller["fence_sequence"]}),
    )?;
    let queued = service
        .submit_consult(
            inv,
            jobs::consult::NativeExecutor {
                host: crate::config::Host::Codex,
                binary: binary.into(),
                home: home.into(),
            },
            ConsultArgs::new(None)?,
        )
        .await?;
    let id = queued["id"].as_str().ok_or(Error("JOB_CORRUPT"))?;
    emit(json!({"type":"queued","id":id,"state":queued["state"]}))?;
    let snapshot = service.config.pin();
    let preflight = transport["preflight"] == true;
    let before = crate::retrieval::Index::status(&service.storage, snapshot.resolved())?;
    let cold = if preflight {
        let actual = scoped_search(&service, id, &scope, task).await;
        match actual {
            Err(Error("INDEX_NOT_READY")) => Some("INDEX_NOT_READY"),
            _ => return Err(Error("REFERENCE_COLD_STATE_MISMATCH")),
        }
    } else {
        None
    };
    let sync = sync_private_index(&service).await?;
    let after = crate::retrieval::Index::status(&service.storage, snapshot.resolved())?;
    assert!(before.lexical_watermark == 0 && after.lexical_watermark > 0);
    let ready = scoped_search(&service, id, &scope, task).await?;
    assert_eq!(ready["status"], "no_hit");
    assert_eq!(ready["scope"], json!(scope));
    for key in ["items", "rules", "model_calls"] {
        assert_eq!(ready[key], json!([]));
    }
    assert_eq!(ready["coverage"], json!({"complete":true,"gaps":[]}));
    let runner_admission = if preflight {
        let first = service
            .reserve_runner("caller", snapshot.resolved())
            .await?;
        let second = service
            .reserve_runner("caller", snapshot.resolved())
            .await?;
        drop((first, second));
        Some(2)
    } else {
        None
    };
    emit(
        json!({"type":"prepared","cold_search_error":cold,"before":before,"after":after,"sync":sync,
        "ready_search":ready,"concurrent_reservations_accepted":runner_admission,
        "max_concurrent_jobs":snapshot.resolved().config().runtime.max_concurrent_jobs,
        "service_total_budget_mib":snapshot.resolved().config().runtime.memory_limits().service_total_budget_mib}),
    )?;
    if preflight {
        return service.close(Ok(())).await;
    }
    let worker = tokio::spawn(service.clone().jobs());
    let job = loop {
        let j = service.show_job(id).await?;
        if !matches!(j["state"].as_str(), Some("queued" | "running")) {
            break j;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    };
    let public = service.show_consult(id).await?;
    let receipt = if job["state"] == "succeeded" {
        service.consult_receipt(id, true).await?
    } else {
        Value::Null
    };
    emit(
        json!({"type":"terminal","id":id,"job_state":job["state"],"job_error":job["error"],"run_trace":job["run_trace"],
        "publication":public,"receipt":receipt}),
    )?;
    service
        .stopping
        .store(true, std::sync::atomic::Ordering::Release);
    service.wake_jobs();
    worker.await.map_err(|_| Error("JOB_ERROR"))??;
    service.close(Ok(())).await
}
