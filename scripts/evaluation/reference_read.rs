//! Read one genuinely empty frozen episode through a production Domain grant.
use akasha_otsuge::{
    config::{Paths, resolve},
    domain::{Domain, PageBudget, ReadContext, Scope},
    storage::Storage,
};
use serde_json::{Value, json};
use std::{
    io::{self, BufRead, Write},
    path::PathBuf,
};
fn emit(value: Value) {
    let mut out = io::stdout().lock();
    serde_json::to_writer(&mut out, &value).unwrap();
    writeln!(out).unwrap();
    out.flush().unwrap();
}
#[tokio::main(flavor = "current_thread")]
async fn main() {
    let mut args = std::env::args_os().skip(1);
    let episode: Value =
        serde_json::from_slice(&std::fs::read(args.next().unwrap()).unwrap()).unwrap();
    let root = PathBuf::from(args.next().unwrap());
    assert!(args.next().is_none() && !root.join("data").exists());
    assert!(episode.get("oracle").is_none());
    for field in ["sources", "memories"] {
        assert!(episode[field].as_array().unwrap().is_empty());
    }
    let caller = &episode["caller"];
    assert!(caller["allowed_event_ids"].as_array().unwrap().is_empty());
    assert_eq!(caller["thread"], "root");
    let scope: Scope = serde_json::from_value(caller["scope"].clone()).unwrap();
    let cfg = resolve(
        "",
        None,
        "",
        &Paths {
            data_dir: root.join("data"),
            socket_path: root.join("fixture.sock"),
        },
    )
    .unwrap();
    let store = Storage::open(&cfg).await.unwrap();
    assert_eq!(store.sequence().unwrap(), 0);
    let domain = Domain::new(&store);
    let grant = domain
        .set_read_grant("bind-reference", &scope.user, Some(&scope), None, 1)
        .await
        .unwrap();
    let context =
        ReadContext::from_verified_binding(&scope.user, "reference-caller", "direct-state")
            .unwrap();
    emit(
        json!({"type":"bound","scope":scope,"thread":caller["thread"],"logical_fence":caller["fence_sequence"],"event_ordinals":[],"memory_revisions":[],"grant":grant}),
    );
    for line in io::stdin().lock().lines() {
        let request: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let q = request["query"].as_str().unwrap();
        assert!(!q.is_empty() && q.len() <= 512);
        let result = domain
            .state(
                &context,
                &scope,
                grant.commit_seq,
                None,
                PageBudget {
                    limit: 16,
                    max_bytes: 65536,
                },
            )
            .await
            .unwrap();
        assert!(result.data.items.is_empty() && result.data.next.is_none());
        emit(
            json!({"type":"scoped_read","scope":scope,"items":result.data.items,"next_cursor":result.data.next,"read_seq":result.read_seq,"validated_at_seq":result.validated_at_seq}),
        );
    }
    store.close().await.unwrap();
}
