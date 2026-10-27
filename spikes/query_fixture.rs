//! Frozen source/query validation shared by the two storage candidates.
use crate::Error;
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{collections::BTreeSet, fs::File, io::Read, os::unix::fs::FileExt, path::Path};

#[derive(Deserialize)]
pub struct Fixture {
    pub(crate) source_sha256: String,
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
    pub(crate) exact_neighbors: Option<Vec<String>>,
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
