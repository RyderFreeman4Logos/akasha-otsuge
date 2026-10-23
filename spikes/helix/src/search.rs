use crate::engine::{Engine, Error};
pub use crate::query_fixture::{Query, load};
use helix_db::prelude::*;
use serde_json::{Value, json};
#[cfg(test)]
use sha2::{Digest, Sha256};
use std::{path::Path, time::Instant};

pub async fn query(
    engine: &Engine,
    query: &Query,
    scope: &str,
    vector: bool,
) -> Result<Value, Error> {
    if vector && query.text_only {
        return Err("text-only query has no source vector".into());
    }
    let traversal = if vector {
        g().vector_search_nodes(
            "Document",
            "embedding",
            query.embedding.clone(),
            20,
            Some(PropertyValue::from(scope.to_owned())),
        )
    } else {
        g().text_search_nodes(
            "Document",
            "body",
            query.term.clone(),
            20,
            Some(PropertyValue::from(scope.to_owned())),
        )
    };
    engine
        .query(QueryRequest::read(
            read_batch()
                .var_as(
                    "hits",
                    traversal.value_map(Some(vec!["key", "partition", "memory", "seq"])),
                )
                .returning(["hits"]),
        ))
        .await
}

pub async fn verify(engine: &Engine, path: &Path) -> Result<Value, Error> {
    let (fixture, digest) = load(path)?;
    let started = Instant::now();
    let mut results = Vec::new();
    for index in 0..fixture.queries.len() {
        let query = fixture.query(index)?;
        for mode in ["bm25", "ann", "excluded_scope"] {
            if query.text_only && mode == "ann" {
                continue;
            }
            let scope = if mode == "excluded_scope" {
                format!("p{}", (query.partition.as_bytes()[1] - b'0' + 1) % 8)
            } else {
                query.partition.clone()
            };
            let query_started = Instant::now();
            let response = self::query(engine, &query, &scope, mode == "ann").await?;
            let elapsed = query_started.elapsed().as_secs_f64();
            let hits = response["hits"]
                .as_array()
                .ok_or("search result is not an array")?;
            let contains_self = hits.iter().any(|hit| hit["key"] == query.key);
            if hits.iter().any(|hit| hit["partition"] != scope)
                || (mode == "excluded_scope" && contains_self)
                || (mode != "excluded_scope" && hits.is_empty())
                || (!fixture.population() && mode != "excluded_scope" && !contains_self)
            {
                return Err(format!(
                    "search oracle mismatch for {} / {mode}: {response}",
                    query.key
                )
                .into());
            }
            let recall = if mode == "ann" {
                query.exact_neighbors.as_ref().map(|expected| {
                    hits.iter()
                        .filter(|hit| {
                            hit["key"]
                                .as_str()
                                .is_some_and(|key| expected.iter().any(|value| value == key))
                        })
                        .count() as f64
                        / 20.0
                })
            } else {
                None
            };
            let result = json!({"key":query.key,"partition":scope,"mode":mode,"seconds":elapsed,"hits":hits,"self_at_20":contains_self,"ann_recall_at_20":recall});
            eprintln!("SEARCH_VERIFIED:{result}");
            results.push(result);
        }
    }
    let recalls: Vec<_> = results
        .iter()
        .filter_map(|row| row["ann_recall_at_20"].as_f64())
        .collect();
    Ok(
        json!({"phase":"real_search_verified", "queries":results.len(), "results":results,
        "exact_ann_queries":recalls.len(),"mean_ann_recall_at_20":if recalls.is_empty() {None} else {Some(recalls.iter().sum::<f64>() / recalls.len() as f64)},
        "fixture_sha256":digest,"source_sha256":fixture.source_sha256,
        "seconds":started.elapsed().as_secs_f64(),"not_tested":["50GB","100_clients","retraction","model_generations"]}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mixed_population_keeps_text_out_of_vector_storage_and_selection() {
        let path = std::env::temp_dir().join(format!("akasha-mixed-query-{}", std::process::id()));
        std::fs::create_dir(&path).unwrap();
        let bytes: Vec<_> = (0..8 * 1536).flat_map(|_| 1.0_f32.to_le_bytes()).collect();
        std::fs::write(path.join("queries.f32"), &bytes).unwrap();
        let mut queries = Vec::new();
        for i in 0..8 {
            queries.push(json!({"key":format!("vector:{i}"),"partition":format!("p{i}"),"term":"verification","memory":1000000+i*2,"body_sha256":"a".repeat(64),"vector_offset":i*1536*4}));
            queries.push(json!({"key":format!("text:{i}"),"partition":format!("p{i}"),"term":"verification","memory":1000001+i*2,"body_sha256":"b".repeat(64),"text_only":true}));
        }
        let vector_hash: String = Sha256::digest(&bytes)
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        let value = json!({"source_sha256":"c".repeat(64),"corpus_manifest_sha256":"c".repeat(64),"model":"text-embedding-3-large","dimension":1536,"queries":queries,"vector_file":{"name":"queries.f32","bytes":bytes.len(),"sha256":vector_hash}});
        let metadata = path.join("queries.json");
        let check = |value: &Value| {
            std::fs::write(&metadata, serde_json::to_vec(value).unwrap()).unwrap();
            load(&metadata)
        };
        let result = check(&value);
        if result.is_err() {
            std::fs::remove_dir_all(&path).unwrap();
        }
        let (fixture, _) = result.unwrap();
        assert!(fixture.query(1).unwrap().embedding.is_empty());
        for seed in 0..32 {
            assert_eq!(fixture.select_index(seed, true), (seed % 8) * 2);
            assert_eq!(fixture.select_index(seed, false), seed % 16);
        }
        for (field, bad) in [
            ("embedding", json!([1.0])),
            ("vector_offset", json!(0)),
            ("exact_neighbors", json!(vec!["fake"; 20])),
            ("text_only", json!(false)),
        ] {
            let mut invalid = value.clone();
            invalid["queries"][1][field] = bad;
            assert!(check(&invalid).is_err(), "{field}");
        }
        std::fs::remove_dir_all(&path).unwrap();
    }
}
