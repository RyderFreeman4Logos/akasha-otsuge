use super::*;
use arrow_array::{StringArray, UInt64Array};
use futures::TryStreamExt;
use lancedb::query::{ExecutableQuery, QueryBase, Select};

fn settings(endpoint: &str) -> String {
    format!(
        "[runtime]\nmax_concurrent_jobs=2\n[retrieval]\nembedding='embed'\n[models.embed]\nkind='embedding'\nendpoint={endpoint:?}\nmodel='vectors-2'\ndimension=2\nlocal=true\ntimeout='15s'"
    )
}

fn native_versions(daemon: &Daemon, generation: &str, expected: &[(String, u64)]) -> [usize; 2] {
    tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let root = daemon.root.join("data/search");
            let connection = lancedb::connect(root.to_str().unwrap())
                .execute()
                .await
                .unwrap();
            let mut versions = [0; 2];
            let mut expected = expected.to_vec();
            expected.sort();
            for (slot, name) in [
                "memory_lexical_v1".to_owned(),
                format!("memory_vector_{generation}"),
            ]
            .into_iter()
            .enumerate()
            {
                let table = connection.open_table(name).execute().await.unwrap();
                let batches: Vec<_> = table
                    .query()
                    .select(Select::Columns(vec!["id".into(), "revision".into()]))
                    .execute()
                    .await
                    .unwrap()
                    .try_collect()
                    .await
                    .unwrap();
                let mut actual = Vec::new();
                for batch in batches {
                    let ids = batch
                        .column(0)
                        .as_any()
                        .downcast_ref::<StringArray>()
                        .unwrap();
                    let revisions = batch
                        .column(1)
                        .as_any()
                        .downcast_ref::<UInt64Array>()
                        .unwrap();
                    actual.extend(
                        (0..batch.num_rows())
                            .map(|i| (ids.value(i).to_owned(), revisions.value(i))),
                    );
                }
                actual.sort();
                assert_eq!(
                    actual, expected,
                    "native table lost or changed authoritative rows"
                );
                versions[slot] = table.list_versions().await.unwrap().len();
            }
            versions
        })
}

#[test]
fn completed_sync_reclaims_native_versions_without_changing_current_or_history() {
    let endpoint = super::super::model_tests::Endpoint::retrieval();
    let mut daemon = Daemon::new();
    let names = (0..32).map(|i| format!("maint-{i}")).collect::<Vec<_>>();
    let records = names
        .iter()
        .map(|id| (id.as_str(), Some("alpha"), "nativeprune keeper"))
        .collect::<Vec<_>>();
    let sequence = seed_records(&daemon, &records, 1);
    assert_eq!(sequence, 64);
    let expected = names
        .iter()
        .enumerate()
        .map(|(i, id)| (id.clone(), (i as u64 + 1) * 2))
        .collect::<Vec<_>>();
    daemon.configure(&settings(&endpoint.address));
    daemon.start();
    let before = names.iter().map(|id| (
        daemon.cli(&["memory", "show", id])["data"]["data"].clone(),
        daemon.cli(&["memory", "history", id, "--to", &sequence.to_string()])["data"]["data"].clone(),
    )).collect::<Vec<_>>();
    daemon.cli(&["index", "sync", "--idempotency-key", "maint-build"]);
    let built = finished(&daemon, "maint-build");
    assert_eq!(built["state"], "succeeded", "{built}");
    let generation = built["result"]["vector"]["generation"].as_str().unwrap();
    for (id, (current, history)) in names.iter().zip(&before) {
        assert_eq!(
            daemon.cli(&["memory", "show", id])["data"]["data"],
            *current
        );
        assert_eq!(
            daemon.cli(&["memory", "history", id, "--to", &sequence.to_string()])["data"]["data"],
            *history
        );
    }
    let versions = native_versions(&daemon, generation, &expected);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    assert_eq!(
        versions,
        [1, 1],
        "completed idle sync retained obsolete native versions"
    );
}

#[test]
fn held_query_defers_native_pruning_until_its_index_owner_finishes() {
    let old = super::super::model_tests::Endpoint::retrieval();
    let next = super::super::model_tests::Endpoint::retrieval();
    let mut daemon = Daemon::new();
    seed(&daemon);
    let expected = vec![
        ("offline".into(), 2),
        ("private".into(), 4),
        ("weather".into(), 6),
    ];
    daemon.configure(&settings(&old.address));
    let scope_file = daemon.root.join("scope.json");
    std::fs::write(&scope_file, serde_json::to_vec(&scope("alpha")).unwrap()).unwrap();
    daemon.start();
    daemon.cli(&["index", "sync", "--idempotency-key", "initial"]);
    assert_eq!(finished(&daemon, "initial")["state"], "succeeded");
    old.pause();
    let initial_requests = old.requests.lock().unwrap().len();
    daemon.cli(&[
        "search",
        "offline",
        "--scope",
        scope_file.to_str().unwrap(),
        "--idempotency-key",
        "held-reader",
    ]);
    let until = Instant::now() + Duration::from_secs(5);
    while old.requests.lock().unwrap().len() == initial_requests {
        assert!(
            Instant::now() < until,
            "query never acquired its index and sent embedding"
        );
        std::thread::sleep(Duration::from_millis(10));
    }
    daemon.configure(&settings(&next.address));
    daemon.cli(&["config", "reload"]);
    daemon.cli(&["index", "sync", "--idempotency-key", "during-reader"]);
    let built = finished(&daemon, "during-reader");
    assert_eq!(built["state"], "succeeded", "{built}");
    let generation = built["result"]["vector"]["generation"].as_str().unwrap();
    let during = native_versions(&daemon, generation, &expected);
    let held = daemon.cli(&["job", "show", "held-reader"])["data"].clone();
    old.resume();
    let read = finished(&daemon, "held-reader");
    assert_eq!(held["state"], "running", "{held}");
    assert_eq!(read["state"], "succeeded", "{read}");
    assert!(
        during[1] > 1,
        "pruned native versions while a query owned the shared index"
    );
    daemon.cli(&["index", "sync", "--idempotency-key", "after-reader"]);
    let after = finished(&daemon, "after-reader");
    assert_eq!(after["state"], "succeeded", "{after}");
    let versions = native_versions(&daemon, generation, &expected);
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    assert_eq!(
        versions,
        [1, 1],
        "idle successor did not reclaim deferred native versions"
    );
}

#[test]
fn completed_sync_indexes_merge_keys_for_both_native_tables() {
    use sha2::Digest;

    let key = sha2::Sha256::digest(b"offline")
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    let endpoint = super::super::model_tests::Endpoint::retrieval();
    let mut daemon = Daemon::new();
    seed(&daemon);
    daemon.configure(&settings(&endpoint.address));
    daemon.start();
    daemon.cli(&["index", "sync", "--idempotency-key", "indexed-keys"]);
    let built = finished(&daemon, "indexed-keys");
    assert_eq!(built["state"], "succeeded", "{built}");
    let generation = built["result"]["vector"]["generation"].as_str().unwrap();
    let observations = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .unwrap()
        .block_on(async {
            let root = daemon.root.join("data/search");
            let connection = lancedb::connect(root.to_str().unwrap())
                .execute()
                .await
                .unwrap();
            let mut observations = Vec::new();
            for name in [
                "memory_lexical_v1".to_owned(),
                format!("memory_vector_{generation}"),
            ] {
                let table = connection.open_table(&name).execute().await.unwrap();
                let indexed = table
                    .list_indices()
                    .await
                    .unwrap()
                    .iter()
                    .any(|index| index.columns == ["key"]);
                let query = table
                    .query()
                    .only_if(format!("key = '{key}'"))
                    .select(Select::Columns(vec!["id".into()]));
                let plan = query.explain_plan(false).await.unwrap();
                let batches: Vec<_> = query.execute().await.unwrap().try_collect().await.unwrap();
                let ids = batches
                    .iter()
                    .flat_map(|batch| {
                        let ids = batch
                            .column(0)
                            .as_any()
                            .downcast_ref::<StringArray>()
                            .unwrap();
                        (0..batch.num_rows()).map(move |i| ids.value(i).to_owned())
                    })
                    .collect::<Vec<_>>();
                assert_eq!(ids, ["offline"]);
                observations.push((name, indexed, plan));
            }
            observations
        });
    daemon.cli(&["daemon", "stop"]);
    daemon.wait();
    assert!(
        observations
            .iter()
            .all(|(_, indexed, plan)| *indexed && plan.contains("ScalarIndexQuery")),
        "native merge keys lack indexed lookup: {observations:?}"
    );
}
