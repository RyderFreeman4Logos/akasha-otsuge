use std::{
    collections::BTreeSet, fs::File, io::Read, os::unix::fs::FileExt, path::Path, time::Instant,
};

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
    pub corpus_manifest_sha256: Option<String>,
    vector_file: Option<VectorFile>,
    #[serde(skip)]
    opened_vectors: Option<File>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VectorFile {
    name: String,
    sha256: String,
    bytes: u64,
}

#[derive(Clone, Deserialize)]
pub struct Query {
    pub key: String,
    pub partition: String,
    pub term: String,
    #[serde(default)]
    pub embedding: Vec<f32>,
    vector_offset: Option<u64>,
    exact_neighbors: Option<Vec<String>>,
}

impl Fixture {
    pub fn population(&self) -> bool {
        self.vector_file.is_some()
    }

    pub fn query(&self, index: usize) -> Result<Query, Error> {
        let mut query = self
            .queries
            .get(index)
            .ok_or("query outside population")?
            .clone();
        if let Some(file) = &self.opened_vectors {
            let mut bytes = [0; 1536 * 4];
            file.read_exact_at(
                &mut bytes,
                query.vector_offset.ok_or("missing vector offset")?,
            )?;
            query.embedding = bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|v| f32::from_le_bytes(*v))
                .collect();
        }
        if query.embedding.len() != 1536
            || !query.embedding.iter().all(|v| v.is_finite())
            || !query.embedding.iter().any(|v| *v != 0.0)
        {
            return Err("invalid query vector".into());
        }
        Ok(query)
    }
}

pub fn load(path: &Path) -> Result<(Fixture, String), Error> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(4 * 1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("search fixture exceeds 4 MiB".into());
    }
    let mut fixture: Fixture = serde_json::from_slice(&bytes)?;
    if fixture.dimension != 1536
        || fixture.model != "text-embedding-3-large"
        || fixture.source_sha256.len() != 64
        || if fixture.population() {
            !(8..=4096).contains(&fixture.queries.len())
        } else {
            fixture.queries.len() != 8
        }
    {
        return Err("unexpected search fixture identity".into());
    }
    let mut partitions = BTreeSet::new();
    let mut keys = BTreeSet::new();
    for (index, query) in fixture.queries.iter().enumerate() {
        if query.key.is_empty()
            || query.key.len() > 256
            || !matches!(
                query.partition.as_str(),
                "p0" | "p1" | "p2" | "p3" | "p4" | "p5" | "p6" | "p7"
            )
            || !keys.insert(&query.key)
            || query.term.is_empty()
            || query.term.len() > 128
            || if fixture.population() {
                !query.embedding.is_empty()
                    || query.vector_offset != Some((index * 1536 * 4) as u64)
            } else {
                query.vector_offset.is_some()
                    || query.embedding.len() != 1536
                    || !query.embedding.iter().all(|v| v.is_finite())
                    || !query.embedding.iter().any(|v| *v != 0.0)
            }
        {
            return Err("invalid real-document search query".into());
        }
        partitions.insert(&query.partition);
        if let Some(neighbors) = &query.exact_neighbors
            && (neighbors.len() != 20
                || neighbors.iter().collect::<BTreeSet<_>>().len() != 20
                || neighbors
                    .iter()
                    .any(|key| key.is_empty() || key.len() > 256))
        {
            return Err("invalid exact neighbor reference".into());
        }
    }
    if partitions.len() != 8 {
        return Err("query population must cover eight partitions".into());
    }
    if let Some(vector) = &fixture.vector_file {
        if Path::new(&vector.name).file_name().and_then(|v| v.to_str())
            != Some(vector.name.as_str())
            || vector.bytes != (fixture.queries.len() * 1536 * 4) as u64
            || fixture.corpus_manifest_sha256.as_ref() != Some(&fixture.source_sha256)
        {
            return Err("invalid query sidecar identity".into());
        }
        let mut file = File::open(
            path.parent()
                .ok_or("query fixture has no parent")?
                .join(&vector.name),
        )?;
        if file.metadata()?.len() != vector.bytes {
            return Err("query vector file size mismatch".into());
        }
        let mut digest = Sha256::new();
        let mut block = [0; 64 * 1024];
        loop {
            let count = file.read(&mut block)?;
            if count == 0 {
                break;
            }
            digest.update(&block[..count]);
        }
        let actual: String = digest
            .finalize()
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        if actual != vector.sha256 {
            return Err("query vector file hash mismatch".into());
        }
        fixture.opened_vectors = Some(file);
        for index in 0..fixture.queries.len() {
            fixture.query(index)?;
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
    for index in 0..fixture.queries.len() {
        let query = fixture.query(index)?;
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
