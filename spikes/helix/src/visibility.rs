use std::{collections::BTreeMap, path::Path};

use serde_json::{Value, json};

use crate::{
    authority::{Authority, Change},
    engine::Error,
    search,
};

pub async fn probe(authority: &Authority, fixture: &Path, verify: bool) -> Result<Value, Error> {
    let (fixture, fixture_hash) = search::load(fixture)?;
    let query = fixture.query(0)?;
    let scope = query.partition.as_bytes()[1] - b'0';
    let rows = authority
        .engine
        .cypher(
            "MATCH (d:Document {key:$key}) RETURN d.memory,d.body",
            BTreeMap::from([("key".into(), json!(query.key))]),
            None,
        )
        .await?
        .rows;
    let [row] = rows.as_slice() else {
        return Err("visibility fixture document is not unique".into());
    };
    let memory = row[0].as_u64().ok_or("missing corpus memory")?;
    let body = row[1].as_str().ok_or("missing corpus body")?;
    if !verify {
        assert_eq!(
            authority.read_point().await?,
            0,
            "visibility fixture must have a fresh authority"
        );
        authority.set_permission("alice", scope, true).await?;
        authority.set_permission("bob", scope, true).await?;
        let mut candidates = Vec::new();
        let read_seq = authority.read_point().await?;
        for vector in [false, true] {
            let raw = search::query(&authority.engine, &query, &query.partition, vector).await?;
            let hits = raw["hits"].as_array().ok_or("invalid candidates")?.clone();
            assert!(hits.iter().any(|hit| hit["key"] == query.key));
            let visible = authority
                .revalidate("alice", scope, read_seq, read_seq, &hits)
                .await?;
            assert!(
                visible["hits"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|hit| hit["key"] == query.key)
            );
            assert!(
                authority
                    .revalidate("ungranted", scope, read_seq, read_seq, &hits)
                    .await?["hits"]
                    .as_array()
                    .unwrap()
                    .is_empty()
            );
            candidates.push(hits);
        }
        assert_eq!(authority.set_permission("alice", scope, false).await?, 2);
        for hits in &candidates {
            let denied = authority.revalidate("alice", scope, 0, 0, hits).await?;
            assert!(denied["hits"].as_array().unwrap().is_empty());
            assert_eq!(denied["permission_revision"], 2);
            let allowed = authority.revalidate("bob", scope, 0, 0, hits).await?;
            assert!(
                allowed["hits"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .any(|hit| hit["key"] == query.key)
            );
        }
        assert_eq!(authority.set_permission("alice", scope, true).await?, 3);
        for (index, (active, next_scope)) in [
            (false, scope),
            (true, scope),
            (true, (scope + 1) % 8),
            (false, (scope + 1) % 8),
        ]
        .into_iter()
        .enumerate()
        {
            let expected = index as u64;
            let result = authority
                .apply(&Change {
                    request_id: format!("visibility:{index}"),
                    memory,
                    expected,
                    scope: next_scope,
                    active,
                    body: body.to_owned(),
                    text_only: false,
                })
                .await?;
            assert_eq!(result["commit_seq"], expected + 1);
            // Reuse candidates from a query begun before this correction committed.
            for hits in &candidates {
                let visible = authority
                    .revalidate("alice", scope, read_seq, expected + 1, hits)
                    .await?;
                assert_eq!(visible["read_seq"], 0);
                assert_eq!(visible["validated_at_seq"], expected + 1);
                assert!(
                    !visible["hits"]
                        .as_array()
                        .unwrap()
                        .iter()
                        .any(|hit| hit["key"] == query.key)
                );
            }
            if expected == 1 {
                assert_eq!(
                    authority.current("alice", memory).await?["entry"]["body"],
                    body
                );
            }
            if expected == 2 {
                assert!(
                    authority.current("alice", memory).await?["entry"].is_null(),
                    "new scope requires its current permission"
                );
                authority.set_permission("alice", next_scope, true).await?;
                assert_eq!(
                    authority.current("alice", memory).await?["entry"]["scope"],
                    next_scope
                );
            }
        }
        let old = authority.history_for("alice", memory, 2, scope).await?;
        assert_eq!(
            old["entry"]["body"], body,
            "later scope/retraction cannot rewrite historical content"
        );
        assert_eq!(old["read_seq"], 2);
        assert_eq!(old["validated_at_seq"], 4);
        assert_eq!(
            authority.state_for("alice", scope, 2, None, 100).await?["entries"]
                .as_array()
                .unwrap()
                .len(),
            1
        );
        assert_eq!(authority.set_permission("alice", scope, false).await?, 4);
        assert_eq!(
            authority.set_permission("alice", scope, false).await?,
            4,
            "same permission is idempotent"
        );
        assert!(
            authority
                .revalidate("bob", scope, 0, 5, &candidates[0])
                .await
                .is_err()
        );
        for (principal, invalid_scope) in [("alice/other", scope), ("", scope), ("alice", 8)] {
            assert!(
                authority
                    .set_permission(principal, invalid_scope, true)
                    .await
                    .is_err()
            );
        }
    }
    assert_eq!(authority.read_point().await?, 4);
    assert!(authority.current("bob", memory).await?["entry"].is_null());
    let denied = authority.history_for("alice", memory, 2, scope).await?;
    assert!(denied["entry"].is_null());
    assert_eq!(denied["permission_revision"], 4);
    assert!(
        authority.state_for("alice", scope, 2, None, 100).await?["entries"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let allowed = authority.history_for("bob", memory, 2, scope).await?;
    assert_eq!(
        allowed["entry"],
        json!({"memory":memory,"seq":2,"scope":scope,"active":true,"body":body})
    );
    assert_eq!(allowed["permission_revision"], 1);
    for vector in [false, true] {
        let raw = search::query(&authority.engine, &query, &query.partition, vector).await?;
        let hits = raw["hits"].as_array().ok_or("invalid stale candidates")?;
        assert!(
            hits.iter().any(|hit| hit["key"] == query.key),
            "diagnostic must retain a genuinely stale index entry"
        );
        let visible = authority.revalidate("bob", scope, 4, 4, hits).await?;
        assert!(
            !visible["hits"]
                .as_array()
                .unwrap()
                .iter()
                .any(|hit| hit["key"] == query.key)
        );
    }
    if verify {
        let params = BTreeMap::from([("memory".into(), json!(memory))]);
        authority
            .engine
            .cypher(
                "MATCH (h:Head {key:$memory}) SET h.scope=99",
                params.clone(),
                None,
            )
            .await?;
        assert!(
            authority.current("bob", memory).await.is_err(),
            "corrupt authority must not look like an empty result"
        );
        authority
            .engine
            .cypher("MATCH (h:Head {key:$memory}) DELETE h", params, None)
            .await?;
        let raw = search::query(&authority.engine, &query, &query.partition, true).await?;
        assert!(
            authority
                .revalidate("bob", scope, 4, 4, raw["hits"].as_array().unwrap())
                .await
                .is_err(),
            "a missing head must not revive revision-zero corpus content"
        );
        authority.engine.cypher("CREATE (:Revision {order:$order,memory:$memory,seq:5,scope:0,active:true,body:'corrupt fixture'})",
            BTreeMap::from([("order".into(),json!(format!("{:020}:{:020}",memory+1,5))),
                ("memory".into(),json!(memory+1))]),None).await?;
        assert!(
            authority.current("bob", memory + 1).await.is_err(),
            "an orphaned future revision must not be hidden by the global counter"
        );
    }
    Ok(
        json!({"status":"passed","phase":if verify {"separate_process_reopen"} else {"seed_and_invalidate"},
        "fixture_sha256":fixture_hash,"memory":memory,"scope":scope,"changes":4,
        "checks":["stale_bm25","stale_ann","old_read_seq_new_validation","scope_move","retraction",
            "restoration_does_not_revive_old_index_revision","historical_content_preserved","live_permission_revocation",
            "principal_isolation","default_deny","minimum_commit_point","permission_idempotency",
            "invalid_or_missing_head_explicit_error_on_reopen","orphaned_future_revision_error"],
        "not_tested":["full_size","indexed_receipts","generation_watermarks","power_loss"]}),
    )
}
