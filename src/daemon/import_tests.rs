use super::*;
use crate::adapter::{
    BindingLevel, CallerProof, CapabilityReport, Invocation, RuntimeBinding, SelfTest,
};

fn capability(host: crate::config::Host) -> CapabilityReport {
    CapabilityReport {
        host,
        version: Some("controlled".into()),
        executable: Some(std::env::current_exe().unwrap()),
        adapter_version: "1".into(),
        binding: BindingLevel::ExactTurn,
        native_children: false,
        history: crate::capture::CoverageLevel::Unavailable,
        machine_protocol: None,
        cancellation: true,
        outer_timeout: true,
        capabilities: BTreeSet::new(),
        self_test: SelfTest::NotRun,
    }
}

async fn seed_capabilities(service: &Service, target: &std::path::Path) {
    private_dir(target).unwrap();
    let source = crate::capture::SourceIdentity {
        adapter: "fixture".into(),
        profile: "default".into(),
        session: "session".into(),
        thread: "session".into(),
        agent: "main".into(),
    };
    Invocation::bind(
        &service.storage,
        RuntimeBinding {
            invocation_id: "old-invocation".into(),
            runtime_instance: "source".into(),
            capability: capability(crate::config::Host::Codex),
            caller: source.clone(),
            principal: principal(),
            scope: owner_scope(),
            proof: CallerProof::RegisteredRoot {
                thread: source.thread.clone(),
            },
            branch: "branch".into(),
            turn_id: Some("turn".into()),
            tool_call_id: Some("tool".into()),
            requested_cursor: 0,
            available_cursor: 0,
            tip: None,
            capture_seq: service.storage.sequence().unwrap(),
            workspace: target.into(),
            config_generation: service.config.pin().generation(),
        },
    )
    .await
    .unwrap();
    let transcript = target.join("transcript.jsonl");
    std::fs::write(&transcript, b"").unwrap();
    let codex = crate::adapter::codex::CodexRegistration {
        capability: capability(crate::config::Host::Codex),
        source: crate::capture::SourceIdentity {
            adapter: "codex".into(),
            ..source.clone()
        },
        scope: owner_scope(),
        principal: principal(),
        transcript: transcript.clone(),
        workspace: target.into(),
        native_home: Some(target.into()),
    };
    let pi = crate::adapter::pi::PiRegistration {
        capability: capability(crate::config::Host::Pi),
        source: crate::capture::SourceIdentity {
            adapter: "pi".into(),
            profile: crate::adapter::pi::profile_key(target),
            ..source.clone()
        },
        scope: owner_scope(),
        principal: principal(),
        transcript: transcript.clone(),
        workspace: target.into(),
        agent_dir: target.into(),
    };
    let claude = crate::adapter::claude::caller::Registration {
        capability: capability(crate::config::Host::ClaudeCode),
        source: crate::capture::SourceIdentity {
            adapter: "claude_code".into(),
            ..source
        },
        scope: owner_scope(),
        transcript,
        workspace: target.into(),
        parent: None,
        start_kind: None,
        native_home: Some(target.into()),
    };
    let token = "a".repeat(64);
    let mut changes: Vec<_> = [
        ("codex-registration", serde_json::to_string(&codex).unwrap()),
        ("pi-registration", serde_json::to_string(&pi).unwrap()),
        (
            "claude-registration",
            serde_json::to_string(&claude).unwrap(),
        ),
    ]
    .into_iter()
    .map(|(prefix, content)| Change {
        key: RecordKey {
            kind: RecordKind::Evidence,
            id: format!("{prefix}/{}", hash(token.as_bytes())),
        },
        expected_revision: None,
        content,
    })
    .collect();
    changes.push(Change {
        key: RecordKey {
            kind: RecordKind::State,
            id: format!("notification/item/{}", "b".repeat(64)),
        },
        expected_revision: None,
        content:
            json!({"schema_version":1,"notification_id":"b".repeat(64),"review_id":"old-review",
            "source_revision":1,"scope":owner_scope(),"state":"open","priority":"normal",
            "summary":"old notice","evidence_locator":"old","occurrences":1,"generation":1,
            "generation_started_at_ms":1,"acknowledged_generation":null,"deliveries":{}})
            .to_string(),
    });
    service
        .storage
        .commit(&CommitRequest {
            request_id: "old-capabilities".into(),
            config_generation: 1,
            policy_revision: None,
            changes,
        })
        .await
        .unwrap();
}

async fn imported_fixture(name: &str) -> (tests::Fixture, PathBuf, u64, CommitRequest) {
    let (fixture, source, binding) = tests::fixture(name, "").await;
    let path = fixture.0.join("capture.jsonl");
    std::fs::write(&path, b"").unwrap();
    source
        .submit("archived-job".into(), binding, path)
        .await
        .unwrap();
    let input = std::fs::read_to_string(&source.config_path).unwrap();
    std::fs::write(
        &source.config_path,
        format!("{input}\n[runtime]\npaused=true\n"),
    )
    .unwrap();
    source.reload().await.unwrap();
    seed_capabilities(&source, &fixture.0.join("target")).await;
    source
        .storage
        .commit(&CommitRequest {
            request_id: "foreign-controls".into(),
            config_generation: 1,
            policy_revision: None,
            changes: [
                (
                    "daemon/config-generation/00000000000000001000",
                    "null".into(),
                ),
                ("daemon/incidents", json!(["a".repeat(64)]).to_string()),
                ("notification/cursor", "0".into()),
                (
                    "search/lexical-v1",
                    json!({"generation":"lexical-icu-v1","lexical_watermark":1,"indexed_rows":17})
                        .to_string(),
                ),
            ]
            .into_iter()
            .map(|(id, content)| Change {
                key: RecordKey {
                    kind: RecordKind::State,
                    id: id.into(),
                },
                expected_revision: None,
                content,
            })
            .collect(),
        })
        .await
        .unwrap();
    Domain::new(&source.storage)
        .change_local_reader("foreign-revocation", &principal(), None, 1)
        .await
        .unwrap();
    let original = CommitRequest {
        request_id: "original-content".into(),
        config_generation: 1,
        policy_revision: None,
        changes: vec![Change {
            key: RecordKey {
                kind: RecordKind::Evidence,
                id: "original".into(),
            },
            expected_revision: None,
            content: "原文\0exact".into(),
        }],
    };
    source.storage.commit(&original).await.unwrap();
    let boundary = source.storage.sequence().unwrap();
    let config = source.config.pin().resolved().clone();
    source.storage.close().await.unwrap();
    drop(source);
    let archive = fixture.0.join("archive");
    Storage::export_archive(&config, &archive).await.unwrap();
    let target = fixture.0.join("target");
    let initial = crate::config::resolve(
        "",
        None,
        "",
        &crate::config::Paths {
            data_dir: target.join("data"),
            socket_path: target.join("run/socket"),
        },
    )
    .unwrap();
    Storage::import_archive(&initial, &archive).await.unwrap();
    (fixture, target, boundary, original)
}

#[tokio::test]
async fn incremental_runtime_keeps_newly_imported_jobs_inactive_across_restarts() {
    let (fixture, target, _, _) = imported_fixture("import-incremental-runtime").await;
    let (source, binding) = tests::service_at(&fixture.0, "").await;
    // The shared fixture deliberately revoked its source grant before the first export.
    Domain::new(&source.storage)
        .change_local_reader(
            "incremental-source-grant",
            &principal(),
            Some(&owner_scope()),
            source.config.pin().generation(),
        )
        .await
        .unwrap();
    let path = fixture.0.join("incremental-capture.jsonl");
    std::fs::write(&path, b"").unwrap();
    source
        .submit("incremental-job".into(), binding, path)
        .await
        .unwrap();
    let boundary = source.storage.sequence().unwrap();
    let source_config = source.config.pin().resolved().clone();
    source.storage.close().await.unwrap();
    drop(source);
    let archive = fixture.0.join("incremental-archive");
    Storage::export_archive(&source_config, &archive)
        .await
        .unwrap();
    let target_config = crate::config::resolve(
        "",
        None,
        "",
        &crate::config::Paths {
            data_dir: target.join("data"),
            socket_path: target.join("run/socket"),
        },
    )
    .unwrap();
    Storage::import_archive(&target_config, &archive)
        .await
        .unwrap();
    let mut sequence = None;
    for _ in 0..2 {
        let (service, _) = tests::service_at(&target, "").await;
        assert_eq!(
            service
                .storage
                .import_identity()
                .unwrap()
                .unwrap()
                .commit_seq,
            boundary
        );
        service.recover().await.unwrap();
        service.advance_incidents().await.unwrap();
        assert!(service.active().unwrap().1.is_empty());
        for id in ["archived-job", "incremental-job"] {
            assert_eq!(service.job(id).unwrap().1.state, "queued");
            assert_eq!(
                service.claim(id, 0).await.unwrap_err().0,
                "IMPORTED_RUNTIME_AUTHORITY"
            );
        }
        assert!(
            Domain::new(&service.storage)
                .permission(&context().unwrap(), Some(&owner_scope()))
                .is_ok()
        );
        assert!(!service.config.pin().resolved().config().runtime.paused);
        if let Some(previous) = sequence {
            assert_eq!(service.storage.sequence().unwrap(), previous);
        }
        sequence = Some(service.storage.sequence().unwrap());
        service.storage.close().await.unwrap();
    }
}

#[tokio::test]
async fn imported_runtime_bootstrap_preserves_history_and_starts_fresh_once() {
    let (_fixture, target, boundary, original) = imported_fixture("import-bootstrap").await;
    let mut generation = None;
    let mut sequence = None;
    for _ in 0..2 {
        let (service, _) = tests::service_at(&target, "").await;
        assert!(
            service.config.pin().generation() > 1000,
            "reused an imported configuration generation"
        );
        assert_eq!(
            service.config.pin().resolved().config().paths.data_dir,
            target.join("data")
        );
        assert!(!service.config.pin().resolved().config().runtime.paused);
        assert!(
            service.active().unwrap().1.is_empty(),
            "recovered an imported queue"
        );
        assert!(
            Domain::new(&service.storage)
                .permission(&context().unwrap(), Some(&owner_scope()))
                .is_ok(),
            "reused imported revoked grant"
        );
        service.recover().await.unwrap();
        service.advance_incidents().await.unwrap();
        assert_eq!(service.job("archived-job").unwrap().1.state, "queued");
        let progress = crate::retrieval::status(&service.storage).unwrap();
        assert_eq!(progress.lexical_watermark, 0);
        assert_eq!(progress.indexed_rows, 0);
        assert_ne!(progress.generation, "lexical-icu-v1");
        assert_eq!(
            service
                .storage
                .at(&original.changes[0].key, boundary)
                .unwrap()
                .unwrap()
                .content,
            original.changes[0].content
        );
        assert_eq!(
            service.storage.commit(&original).await.unwrap().commit_seq,
            boundary
        );
        if let Some(old) = generation {
            assert_eq!(service.config.pin().generation(), old);
        }
        if let Some(old) = sequence {
            assert_eq!(
                service.storage.sequence().unwrap(),
                old,
                "restart repeated initialization"
            );
        }
        generation = Some(service.config.pin().generation());
        sequence = Some(service.storage.sequence().unwrap());
        service.storage.close().await.unwrap();
        drop(service);
    }
}

#[tokio::test]
async fn imported_job_identity_cannot_be_rearmed_by_a_local_revision() {
    let (_fixture, target, _, _) = imported_fixture("import-job-origin").await;
    let (service, binding) = tests::service_at(&target, "").await;
    let (previous, job) = service.job("archived-job").unwrap();
    service
        .save_job(&job, Some(previous), vec![])
        .await
        .unwrap();
    assert_eq!(
        service.claim("archived-job", 0).await.unwrap_err().0,
        "IMPORTED_RUNTIME_AUTHORITY"
    );
    let path = target.join("new.jsonl");
    std::fs::write(&path, b"").unwrap();
    assert_eq!(
        service
            .submit("archived-job".into(), binding.clone(), path.clone())
            .await
            .unwrap_err()
            .0,
        "IMPORTED_RUNTIME_AUTHORITY"
    );
    service
        .submit("fresh-job".into(), binding, path)
        .await
        .unwrap();
    assert!(service.claim("fresh-job", 0).await.unwrap());
    service.storage.close().await.unwrap();
}

#[tokio::test]
async fn imported_invocation_is_history_only_and_fresh_binding_works() {
    let (_fixture, target, _, _) = imported_fixture("import-binding").await;
    let (service, _) = tests::service_at(&target, "").await;
    let old = Invocation::load_verified(&service.storage, "old-invocation", &principal()).unwrap();
    service.outcome_binding("old-invocation").unwrap();
    assert_eq!(
        service
            .outcome_context("old-invocation")
            .await
            .unwrap_err()
            .0,
        "BINDING_UNAVAILABLE"
    );
    assert_eq!(
        Invocation::load(&service.storage, "old-invocation", &principal())
            .await
            .unwrap_err()
            .code,
        crate::adapter::ErrorCode::BindingUnavailable
    );
    assert_eq!(
        Invocation::bind(&service.storage, old.diagnostic().clone())
            .await
            .unwrap_err()
            .code,
        crate::adapter::ErrorCode::BindingUnavailable
    );
    let mut fresh = old.diagnostic().clone();
    fresh.invocation_id = "new-invocation".into();
    fresh.runtime_instance = "destination".into();
    fresh.capture_seq = service.storage.sequence().unwrap();
    fresh.config_generation = service.config.pin().generation();
    Invocation::bind(&service.storage, fresh).await.unwrap();
    Invocation::load(&service.storage, "new-invocation", &principal())
        .await
        .unwrap();
    service.storage.close().await.unwrap();
}

#[tokio::test]
async fn imported_native_tokens_cannot_admit_capture_jobs() {
    let (_fixture, target, _, _) = imported_fixture("import-tokens").await;
    let (service, _) = tests::service_at(&target, "").await;
    let commands = |token: &str| {
        [
            Command::Codex {
                event: crate::adapter::codex::NativeEvent::Sync {
                    registration: token.into(),
                    until_ordinal: 1,
                },
            },
            Command::Pi {
                event: crate::adapter::pi::NativeEvent::Sync {
                    registration: token.into(),
                    until_entry: "entry".into(),
                },
            },
            Command::Claude {
                event: crate::adapter::claude::NativeEvent::Sync {
                    registration: token.into(),
                    tip: "tip".into(),
                },
            },
        ]
    };
    for command in commands(&"a".repeat(64)) {
        assert_eq!(
            service.dispatch("old-token", command).await.unwrap_err().0,
            "BINDING_UNAVAILABLE"
        );
    }
    assert!(service.active().unwrap().1.is_empty());
    let changes = [
        "codex-registration",
        "pi-registration",
        "claude-registration",
    ]
    .into_iter()
    .map(|prefix| {
        let old_key = RecordKey {
            kind: RecordKind::Evidence,
            id: format!("{prefix}/{}", hash("a".repeat(64).as_bytes())),
        };
        Change {
            key: RecordKey {
                kind: RecordKind::Evidence,
                id: format!("{prefix}/{}", hash("d".repeat(64).as_bytes())),
            },
            expected_revision: None,
            content: service.storage.current(&old_key).unwrap().unwrap().content,
        }
    })
    .collect();
    service
        .storage
        .commit(&CommitRequest {
            request_id: "fresh-tokens".into(),
            config_generation: service.config.pin().generation(),
            policy_revision: None,
            changes,
        })
        .await
        .unwrap();
    for command in commands(&"d".repeat(64)) {
        service.dispatch("fresh-token", command).await.unwrap();
    }
    assert_eq!(service.active().unwrap().1.len(), 3);
    service.storage.close().await.unwrap();
}

#[tokio::test]
async fn acknowledging_imported_notification_does_not_rearm_delivery() {
    let (_fixture, target, boundary, _) = imported_fixture("import-notice").await;
    let (service, _) = tests::service_at(
        &target,
        "[notifiers.fixture]\nkind='stdio'\ncommand=['/bin/false']\n",
    )
    .await;
    service
        .ack_notification(&"b".repeat(64), 1, "local-ack")
        .await
        .unwrap();
    assert!(service.storage.sequence().unwrap() > boundary);
    let mut scan = 0;
    let mut after = None;
    assert!(
        service
            .advance_notifications(&mut scan, &mut after, true)
            .await
            .unwrap()
            .is_none(),
        "old source acquired a fresh dispatch"
    );
    let old = service.show_notification(&"b".repeat(64)).await.unwrap();
    assert_eq!(old["deliveries"], json!({}));
    let mut fresh = old;
    fresh["notification_id"] = json!("c".repeat(64));
    fresh["source_revision"] = json!(service.storage.sequence().unwrap());
    fresh["acknowledged_generation"] = Value::Null;
    service
        .storage
        .commit(&CommitRequest {
            request_id: "fresh-notice".into(),
            config_generation: service.config.pin().generation(),
            policy_revision: None,
            changes: vec![Change {
                key: RecordKey {
                    kind: RecordKind::State,
                    id: format!("notification/item/{}", "c".repeat(64)),
                },
                expected_revision: None,
                content: fresh.to_string(),
            }],
        })
        .await
        .unwrap();
    assert!(
        service
            .advance_notifications(&mut scan, &mut after, true)
            .await
            .unwrap()
            .is_some(),
        "fresh local notification was blocked"
    );
    service.storage.close().await.unwrap();
}

#[test]
#[ignore = "launched only by the import runtime crash supervisor"]
fn import_runtime_child() {
    let root = PathBuf::from(std::env::var_os("AKASHA_TEST_CRASH_ROOT").unwrap());
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    runtime.block_on(async {
        let (service, _) = tests::service_at(&root, "").await;
        service.storage.close().await.unwrap();
        panic!("requested import runtime checkpoint was not reached");
    });
}

#[test]
fn killed_import_runtime_initialization_resumes_without_rearming_history() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap();
    for (number, phase) in [
        "import-runtime-config",
        "import-runtime-controls",
        "import-runtime-index",
        "import-runtime-grant",
    ]
    .into_iter()
    .enumerate()
    {
        let (_fixture, target, boundary, original) =
            runtime.block_on(imported_fixture(&format!("import-crash-{number}")));
        crate::storage::crash_tests::kill_at(
            &target,
            phase,
            "daemon::jobs::import_tests::import_runtime_child",
        );
        runtime.block_on(async {
            let mut sequence = None;
            for _ in 0..2 {
                let (service, _) = tests::service_at(&target, "").await;
                assert_eq!(service.config.pin().generation(), 1001);
                assert!(service.active().unwrap().1.is_empty());
                service.recover().await.unwrap();
                service.advance_incidents().await.unwrap();
                assert_eq!(service.job("archived-job").unwrap().1.attempt, 0);
                assert!(
                    Invocation::load(&service.storage, "old-invocation", &principal())
                        .await
                        .is_err()
                );
                assert!(
                    Domain::new(&service.storage)
                        .permission(&context().unwrap(), Some(&owner_scope()))
                        .is_ok()
                );
                assert_eq!(
                    service.storage.commit(&original).await.unwrap().commit_seq,
                    boundary
                );
                let progress = crate::retrieval::status(&service.storage).unwrap();
                assert_eq!(progress.lexical_watermark, 0);
                if let Some(old) = sequence {
                    assert_eq!(service.storage.sequence().unwrap(), old);
                }
                sequence = Some(service.storage.sequence().unwrap());
                service.storage.close().await.unwrap();
            }
        });
        println!("import runtime checkpoint={phase} SIGKILL recovered with two stable restarts");
    }
}
