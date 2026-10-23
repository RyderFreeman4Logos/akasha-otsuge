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
    #[serde(skip)]
    vector_indices: Vec<usize>,
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
    pub memory: Option<u64>,
    pub body_sha256: Option<String>,
    #[serde(default)]
    pub text_only: bool,
    #[serde(default)]
    pub embedding: Vec<f32>,
    vector_offset: Option<u64>,
    exact_neighbors: Option<Vec<String>>,
}

impl Fixture {
    pub fn population(&self) -> bool {
        self.vector_file.is_some()
    }

    pub fn select_index(&self, seed: usize, vector: bool) -> usize {
        if vector {
            self.vector_indices[seed % self.vector_indices.len()]
        } else {
            seed % self.queries.len()
        }
    }

    pub fn query(&self, index: usize) -> Result<Query, Error> {
        let mut query = self
            .queries
            .get(index)
            .ok_or("query outside population")?
            .clone();
        if query.text_only {
            return Ok(query);
        }
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
        || !(8..=4096).contains(&fixture.queries.len())
    {
        return Err("unexpected search fixture identity".into());
    }
    let mut partitions = BTreeSet::new();
    let mut keys = BTreeSet::new();
    let mut memories = BTreeSet::new();
    let mut vector_partitions = BTreeSet::new();
    for (index, query) in fixture.queries.iter().enumerate() {
        if fixture.population()
            || query.text_only
            || query.memory.is_some()
            || query.body_sha256.is_some()
        {
            let memory = query.memory.ok_or("missing current corpus identity")?;
            if !(1_000_000..101_000_000).contains(&memory)
                || !memories.insert(memory)
                || !query.body_sha256.as_ref().is_some_and(|hash| {
                    hash.len() == 64
                        && hash
                            .bytes()
                            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
                })
            {
                return Err("invalid current corpus identity".into());
            }
        }
        if query.key.is_empty()
            || query.key.len() > 256
            || !matches!(
                query.partition.as_str(),
                "p0" | "p1" | "p2" | "p3" | "p4" | "p5" | "p6" | "p7"
            )
            || !keys.insert(&query.key)
            || query.term.is_empty()
            || query.term.len() > 128
            || if query.text_only {
                !query.embedding.is_empty()
                    || query.vector_offset.is_some()
                    || query.exact_neighbors.is_some()
            } else if fixture.population() {
                !query.embedding.is_empty()
                    || query.vector_offset != Some((fixture.vector_indices.len() * 1536 * 4) as u64)
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
        if !query.text_only {
            fixture.vector_indices.push(index);
            vector_partitions.insert(&query.partition);
        }
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
    if partitions.len() != 8 || vector_partitions.len() != 8 {
        return Err("query population must cover eight partitions".into());
    }
    if let Some(vector) = &fixture.vector_file {
        if Path::new(&vector.name).file_name().and_then(|v| v.to_str())
            != Some(vector.name.as_str())
            || vector.bytes != (fixture.vector_indices.len() * 1536 * 4) as u64
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
