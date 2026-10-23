use std::{collections::BTreeSet, io::Read, path::Path, time::Instant};

use helix_db::prelude::*;
use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::engine::{Engine, Error};

#[derive(Deserialize)]
pub struct Fixture {
    source_sha256: String,
    model: String,
    dimension: usize,
    pub queries: Vec<Query>,
}

#[derive(Clone, Deserialize)]
pub struct Query {
    pub key: String,
    pub partition: String,
    pub term: String,
    pub embedding: Vec<f32>,
}

pub fn load(path: &Path) -> Result<(Fixture, String), Error> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err("search fixture exceeds 1 MiB".into());
    }
    let fixture: Fixture = serde_json::from_slice(&bytes)?;
    if fixture.dimension != 1536
        || fixture.model != "text-embedding-3-large"
        || fixture.source_sha256.len() != 64
        || fixture.queries.len() != 8
    {
        return Err("unexpected search fixture identity".into());
    }
    let mut partitions = BTreeSet::new();
    for query in &fixture.queries {
        if query.key.is_empty()
            || query.key.len() > 256
            || !matches!(
                query.partition.as_str(),
                "p0" | "p1" | "p2" | "p3" | "p4" | "p5" | "p6" | "p7"
            )
            || !partitions.insert(&query.partition)
            || query.term.is_empty()
            || query.term.len() > 128
            || query.embedding.len() != 1536
            || !query.embedding.iter().all(|v| v.is_finite())
            || !query.embedding.iter().any(|v| *v != 0.0)
        {
            return Err("invalid real-document search query".into());
        }
    }
    let digest = Sha256::digest(&bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    Ok((fixture, digest))
}

pub async fn query(
    engine: &Engine,
    query: &Query,
    scope: &str,
    vector: bool,
) -> Result<Value, Error> {
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
                .var_as("hits", traversal.value_map(Some(vec!["key", "partition"])))
                .returning(["hits"]),
        ))
        .await
}

pub async fn verify(engine: &Engine, path: &Path) -> Result<Value, Error> {
    let (fixture, digest) = load(path)?;
    let started = Instant::now();
    let mut results = Vec::new();
    for query in fixture.queries {
        for mode in ["bm25", "ann", "excluded_scope"] {
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
                || (mode != "excluded_scope" && !contains_self)
            {
                return Err(format!(
                    "search oracle mismatch for {} / {mode}: {response}",
                    query.key
                )
                .into());
            }
            let result = json!({"key":query.key,"partition":scope,"mode":mode,"seconds":elapsed,"hits":hits});
            eprintln!("SEARCH_VERIFIED:{result}");
            results.push(result);
        }
    }
    Ok(
        json!({"phase":"real_search_verified", "queries":results.len(), "results":results,
        "fixture_sha256":digest,"source_sha256":fixture.source_sha256,
        "seconds":started.elapsed().as_secs_f64(),"not_tested":["50GB","100_clients","retraction","model_generations"]}),
    )
}
