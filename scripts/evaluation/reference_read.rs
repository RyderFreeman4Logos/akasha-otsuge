//! Direct keyword retrieval from the registered episode's scoped authority store.
use akasha_otsuge::{
    config::{Paths, resolve},
    domain::{Domain, MemoryState, OriginalRange, PageBudget, ReadContext, Scope},
    retrieval::Index,
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
    assert!(args.next().is_none() && episode.get("oracle").is_none());
    let caller = &episode["caller"];
    let scope: Scope = serde_json::from_value(caller["scope"].clone()).unwrap();
    let seed_path = root.join("seed.json");
    let seed: Option<Value> = seed_path
        .exists()
        .then(|| serde_json::from_slice(&std::fs::read(seed_path).unwrap()).unwrap());
    assert_eq!(root.join("data").exists(), seed.is_some());
    let cfg = resolve(
        "",
        None,
        "",
        &Paths {
            data_dir: root.join("data"),
            socket_path: root.join("s"),
        },
    )
    .unwrap();
    let store = Storage::open(&cfg).await.unwrap();
    let domain = Domain::new(&store);
    let context =
        ReadContext::from_verified_binding(&scope.user, "reference-caller", "direct-search")
            .unwrap();
    let (grant, at, index) = if let Some(seed) = &seed {
        assert_eq!(seed["id"], episode["id"]);
        let grant = store.receipt("fixture-grant").unwrap().unwrap();
        assert_eq!(seed["grant"], grant.commit_seq);
        (
            grant,
            seed["capture_seq"].as_u64().unwrap(),
            Some(Index::open(&cfg, &store).await.unwrap()),
        )
    } else {
        assert!(
            episode["sources"].as_array().unwrap().is_empty()
                && episode["memories"].as_array().unwrap().is_empty()
        );
        let grant = domain
            .set_read_grant("bind-reference", &scope.user, Some(&scope), None, 1)
            .await
            .unwrap();
        let at = grant.commit_seq;
        (grant, at, None)
    };
    emit(
        json!({"type":"bound","scope":scope,"thread":caller["thread"],"logical_fence":caller["fence_sequence"],"grant":grant,"read_seq":at}),
    );
    for (i, line) in io::stdin().lock().lines().enumerate() {
        let req: Value = serde_json::from_str(&line.unwrap()).unwrap();
        let q = req["query"].as_str().unwrap();
        assert!(!q.is_empty() && q.len() <= 512);
        let page = domain
            .state(
                &context,
                &scope,
                at,
                None,
                PageBudget {
                    limit: 32,
                    max_bytes: 65536,
                },
            )
            .await
            .unwrap();
        assert!(page.data.next.is_none());
        let mut items = Vec::new();
        if let (Some(index), Some(seed)) = (&index, &seed) {
            for (mid, rev) in index.keyword(q, &scope, 32).await.unwrap() {
                let Some(m) = page
                    .data
                    .items
                    .iter()
                    .map(|x| &x.memory)
                    .find(|m| m.revision.key.id == mid && m.revision.commit_seq == rev)
                else {
                    continue;
                };
                let live = domain.get(&context, &mid, None).await.unwrap();
                if !matches!(live.data,MemoryState::Active(ref current) if current.memory.revision==m.revision)
                {
                    continue;
                }
                let logical = seed["memories"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .find(|v| v["id"] == mid && v["commit"] == rev)
                    .unwrap();
                let mut sources = Vec::new();
                for (j, span) in m.record.draft.source_spans.iter().enumerate() {
                    let original = seed["sources"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .find(|s| {
                            s["native_source_id"] == span.event_id
                                && s["source_revision"] == span.revision
                        })
                        .unwrap();
                    assert!(
                        seed["native_visible_ids"]
                            .as_array()
                            .unwrap()
                            .contains(&original["id"])
                    );
                    let read = domain
                        .original_range(
                            &context,
                            &OriginalRange {
                                event_id: span.event_id.clone(),
                                revision: span.revision,
                                start: span.start,
                                end: span.end,
                            },
                            &format!("direct/{i}/{mid}/{j}"),
                            8192,
                            1,
                        )
                        .await
                        .unwrap();
                    assert!(!read.truncated && original["sha256"] == read.output_sha256);
                    sources.push(json!({"id":original["id"],"sha256":read.output_sha256,"text":read.output,"receipt":read.receipt_id}));
                }
                items.push(json!({"id":mid,"revision":logical["ordinal"],"native_commit":rev,"scope":m.record.draft.scope,"authority":m.record.authority,"claim":m.record.draft.claim,"conditions":m.record.draft.conditions,"sources":sources}));
            }
        } else {
            assert!(page.data.items.is_empty());
        }
        emit(
            json!({"type":"scoped_read","scope":scope,"items":items,"next_cursor":null,"read_seq":page.read_seq,"validated_at_seq":store.sequence().unwrap()}),
        );
    }
    drop(index);
    store.close().await.unwrap();
}
