use super::*;

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
