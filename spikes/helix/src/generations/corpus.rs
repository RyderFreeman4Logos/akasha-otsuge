//! Copy a pinned immutable import in bounded transactions before native index build.
use super::*;
use std::{fs::File, io::Write, time::Instant};

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub(super) struct Coverage {
    pub manifest_sha256: String,
    documents: u64,
    body_bytes: u64,
    vectors: u64,
    prepared_sha256: String,
    model_fingerprint: String,
    completed: u64,
    copied_bytes: u64,
    copied_vectors: u64,
    pub complete: bool,
}

impl Coverage {
    pub fn same_source(&self, other: &Self) -> bool {
        self.manifest_sha256 == other.manifest_sha256
            && self.documents == other.documents
            && self.body_bytes == other.body_bytes
            && self.vectors == other.vectors
            && self.prepared_sha256 == other.prepared_sha256
            && self.model_fingerprint == other.model_fingerprint
    }
    pub fn validate(&self, bundle: &Bundle) -> Result<(), Error> {
        if self.documents == 0
            || self.documents > 100_000_000
            || self.completed > self.documents
            || self.copied_bytes > self.body_bytes
            || self.copied_vectors > self.vectors
            || self.vectors > self.documents
            || self.model_fingerprint != bundle.model_fingerprint
            || self.complete != (self.completed == self.documents)
            || (self.complete
                && (self.copied_bytes != self.body_bytes || self.copied_vectors != self.vectors))
            || [&self.manifest_sha256, &self.prepared_sha256]
                .iter()
                .any(|s| {
                    s.len() != 64
                        || !s
                            .bytes()
                            .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
                })
        {
            return Err("corpus coverage integrity mismatch".into());
        }
        Ok(())
    }
}

fn coverage(bytes: &[u8], bundle: &Bundle) -> Result<Coverage, Error> {
    bundle.validate()?;
    let m: Value = serde_json::from_slice(bytes)?;
    if m["dimensions"] != bundle.dimension
        || m["embedding_model"] != bundle.model
        || bundle.dimension != 1536
        || bundle.model != "text-embedding-3-large"
        || m["precomputed_at"] != "2024-02 (publisher)"
        || m["chunk_bytes"] != 65536
        || bundle.chunker != "source-document-v1"
    {
        return Err("corpus encoder/chunker provenance mismatch".into());
    }
    let inputs = m["corpus"]["inputs"]
        .as_array()
        .ok_or("missing corpus source provenance")?;
    let vectors: Vec<_> = inputs
        .iter()
        .filter(|r| {
            r["file"]
                .as_str()
                .is_some_and(|s| s.starts_with("vectors-"))
        })
        .collect();
    if vectors.is_empty()
        || vectors.iter().any(|r| {
            r["repository"] != "Qdrant/dbpedia-entities-openai3-text-embedding-3-large-1536-1M"
                || r["revision"] != "4a9731217921bc476a0f03544f11f22ae4903fa5"
        })
    {
        return Err("unrecognized precomputed vector provenance".into());
    }
    let identity = json!({"model":bundle.model,"repository":vectors[0]["repository"],"revision":vectors[0]["revision"],"precomputed_at":"2024-02","precision":"float32"});
    let source = &m["corpus"];
    let record = Coverage {
        manifest_sha256: hash(bytes),
        documents: number(&source["documents"])?,
        body_bytes: number(&source["indexed_body_utf8_bytes"])?,
        vectors: number(&source["vectors"])?,
        prepared_sha256: source["prepared_sha256"]
            .as_str()
            .ok_or("missing prepared hash")?
            .into(),
        model_fingerprint: hash(serde_json::to_vec(&identity)?),
        completed: 0,
        copied_bytes: 0,
        copied_vectors: 0,
        complete: false,
    };
    record.validate(bundle)?;
    Ok(record)
}

async fn verify_import(a: &Authority, base: &Coverage) -> Result<(), Error> {
    let rows = a.engine.cypher("MATCH (s:ImportState) RETURN s.manifest,s.documents,s.content_bytes,s.vectors,s.prefix,s.complete", Default::default(), None).await?.rows;
    if rows
        != vec![vec![
            json!(base.manifest_sha256),
            json!(base.documents),
            json!(base.body_bytes),
            json!(base.vectors),
            json!(base.prepared_sha256),
            json!(true),
        ]]
    {
        return Err("complete authoritative import does not match corpus generation".into());
    }
    let rows = a
        .engine
        .cypher(
            "MATCH (d:Document) RETURN count(d)",
            Default::default(),
            None,
        )
        .await?
        .rows;
    if rows != vec![vec![json!(base.documents)]] {
        return Err("corpus source count differs from durable import".into());
    }
    Ok(())
}

pub(super) async fn begin(
    a: &Authority,
    id: u64,
    bundle: Bundle,
    manifest: &Path,
    root: &Path,
) -> Result<Value, Error> {
    let mut bytes = Vec::new();
    File::open(manifest)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err("corpus manifest exceeds 1 MiB".into());
    }
    let base = coverage(&bytes, &bundle)?;
    verify_import(a, &base).await?;
    for spec in [
        IndexSpec::node_unique_equality("Document", "memory"),
        IndexSpec::node_range("Document", "memory"),
    ] {
        indexes::create_measured(
            &a.engine,
            spec,
            7200,
            Some(&root.join("corpus-cursor-index-progress.json")),
        )
        .await?;
    }
    let _writer = a.writer.lock().await;
    verify_import(a, &base).await?;
    begin_record(a, id, bundle, Some(base)).await
}

pub(super) async fn backfill(
    a: &Authority,
    id: u64,
    waves: u64,
    hold: Option<&str>,
    root: &Path,
) -> Result<Value, Error> {
    if !(1..=12_500_000).contains(&waves)
        || hold.is_some_and(|h| !["before-backfill-commit", "after-backfill-commit"].contains(&h))
    {
        return Err("invalid bounded backfill request".into());
    }
    let initial = generation(a, id).await?;
    let base = initial
        .base
        .as_ref()
        .ok_or("generation has no imported corpus")?;
    verify_import(a, base).await?;
    if initial.ready || !matches!(initial.status.as_str(), "building" | "failed") {
        return Err("cannot backfill a built generation".into());
    }
    for family in [Family::Text, Family::Vector] {
        indexes::create(
            &a.engine,
            IndexSpec::node_unique_equality(family.label(id), "memory"),
        )
        .await?;
    }
    let started = Instant::now();
    let before = base.completed;
    for wave in 0..waves {
        let record = step(a, id, if wave == 0 { hold } else { None }).await?;
        let base = record.base.as_ref().ok_or("missing corpus progress")?;
        // Persist the external observation at least every second; the database cursor commits each wave.
        let path = root.join(format!("generation-{id}-backfill.json"));
        let stale = path
            .metadata()
            .and_then(|m| m.modified())
            .ok()
            .and_then(|t| t.elapsed().ok())
            .is_none_or(|age| age.as_secs() >= 1);
        if wave == 0 || wave + 1 == waves || base.complete || stale {
            let elapsed = started.elapsed().as_secs_f64();
            let rate = (base.completed - before) as f64 / elapsed.max(0.000001);
            let stat = std::fs::read_to_string("/proc/self/stat")?;
            let ticks = stat
                .rsplit_once(')')
                .and_then(|(_, tail)| tail.split_whitespace().nth(19))
                .ok_or("invalid process identity")?;
            let progress = json!({"phase":"corpus_backfill","generation":id,"completed":base.completed,"total":base.documents,"elapsed_s":elapsed,"rate_per_s":rate,"eta_s":if base.complete {Some(0.0)} else if rate>0.0 {Some((base.documents-base.completed) as f64/rate)} else {None},"pid":std::process::id(),"start_ticks":ticks,"base":base});
            let temporary = path.with_extension("tmp");
            let mut file = File::create(&temporary)?;
            writeln!(file, "{progress}")?;
            file.sync_all()?;
            std::fs::rename(temporary, path)?;
            File::open(root)?.sync_all()?;
        }
        if base.complete {
            return Ok(serde_json::to_value(record)?);
        }
        tokio::task::yield_now().await;
    }
    Ok(serde_json::to_value(generation(a, id).await?)?)
}

async fn step(a: &Authority, id: u64, hold: Option<&str>) -> Result<Generation, Error> {
    let _writer = a.writer.lock().await;
    let mut record = generation(a, id).await?;
    if record.ready || !matches!(record.status.as_str(), "building" | "failed") {
        return Err("generation changed during backfill".into());
    }
    let base = record.base.as_mut().ok_or("missing corpus coverage")?;
    if base.complete {
        return Ok(record);
    }
    let response = a
        .engine
        .query(QueryRequest::read(
            read_batch()
                .var_as(
                    "documents",
                    g().n_with_label_where(
                        "Document",
                        SourcePredicate::between(
                            "memory",
                            i64::try_from(1_000_000 + base.completed)?,
                            i64::MAX,
                        ),
                    )
                    .order_by("memory", Order::Asc)
                    .limit(8)
                    .value_map(Some(vec![
                        "memory",
                        "key",
                        "body",
                        "partition",
                        "embedding",
                    ])),
                )
                .returning(["documents"]),
        ))
        .await?;
    let rows = response["documents"]
        .as_array()
        .ok_or("missing corpus continuation")?;
    let expected = (base.documents - base.completed).min(8) as usize;
    if rows.len() != expected {
        return Err("corpus backfill count or range mismatch".into());
    }
    let mut batch = write_batch();
    let mut memberships = Vec::new();
    for (offset, row) in rows.iter().enumerate() {
        let memory = 1_000_000 + base.completed + offset as u64;
        let body = row["body"].as_str().ok_or("invalid source body")?;
        let key = row["key"].as_str().ok_or("invalid source identity")?;
        let partition = row["partition"]
            .as_str()
            .ok_or("invalid source partition")?;
        if row["memory"] != memory
            || body.len() > 128 * 1024
            || key.is_empty()
            || key.len() > 256
            || !["p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7"].contains(&partition)
        {
            return Err("non-contiguous or malformed corpus source".into());
        }
        memberships.push((format!("{}:{memory:020}", &partition[1..]), memory));
        let identity = hash(serde_json::to_vec(row)?);
        let properties = vec![
            ("memory", PropertyValue::from(i64::try_from(memory)?)),
            ("seq", PropertyValue::from(0i64)),
            ("partition", PropertyValue::from(partition)),
            ("hash", PropertyValue::from(identity)),
            ("source_key", PropertyValue::from(key)),
        ];
        let mut text = properties.clone();
        text.push(("body", PropertyValue::from(body)));
        batch = batch.var_as(
            &format!("text{offset}"),
            g().add_n(Family::Text.label(id), text),
        );
        if !row["embedding"].is_null() {
            let values: Vec<f32> = serde_json::from_value(row["embedding"].clone())?;
            if values.len() != record.bundle.dimension
                || !values.iter().all(|v| v.is_finite())
                || !values.iter().any(|v| *v != 0.0)
            {
                return Err("invalid corpus embedding".into());
            }
            let mut vector = properties;
            vector.push(("embedding", PropertyValue::from(values)));
            batch = batch.var_as(
                &format!("vector{offset}"),
                g().add_n(Family::Vector.label(id), vector),
            );
            base.copied_vectors += 1;
        }
        base.copied_bytes += body.len() as u64;
    }
    let mut lookup = read_batch();
    let mut names = Vec::new();
    for (i, (key, _)) in memberships.iter().enumerate() {
        let name = format!("member{i}");
        lookup = lookup.var_as(
            &name,
            g().n_with_label_where("ScopeMember", SourcePredicate::eq("key", key.clone()))
                .limit(2)
                .value_map(Some(vec!["key", "memory"])),
        );
        names.push(name);
    }
    let existing = a
        .engine
        .query(QueryRequest::read(lookup.returning(names)))
        .await?;
    for (i, (key, memory)) in memberships.iter().enumerate() {
        let value = &existing[format!("member{i}")];
        if value.is_null() || value == &json!([]) {
            batch = batch.var_as(
                &format!("scope{i}"),
                g().add_n(
                    "ScopeMember",
                    vec![
                        ("key", PropertyValue::from(key.clone())),
                        ("memory", PropertyValue::from(i64::try_from(*memory)?)),
                    ],
                ),
            );
        } else if value != &json!([{"key":key,"memory":memory}]) {
            return Err("original scope membership integrity mismatch".into());
        }
    }
    base.completed += rows.len() as u64;
    base.complete = base.completed == base.documents;
    base.validate(&record.bundle)?;
    if base.complete {
        let outside = a
            .engine
            .query(QueryRequest::read(
                read_batch()
                    .var_as(
                        "outside",
                        g().n_with_label_where(
                            "Document",
                            SourcePredicate::between(
                                "memory",
                                i64::try_from(1_000_000 + base.documents)?,
                                i64::MAX,
                            ),
                        )
                        .limit(1)
                        .value_map(Some(vec!["memory"])),
                    )
                    .returning(["outside"]),
            ))
            .await?;
        if !matches!(&outside["outside"], Value::Null) && outside["outside"] != json!([]) {
            return Err("corpus has records beyond pinned count".into());
        }
    }
    batch = batch.var_as(
        "coverage",
        g().n_with_label_where("Generation", SourcePredicate::eq("key", i64::try_from(id)?))
            .set_property("base", serde_json::to_string(base)?),
    );
    crate::checkpoint(hold, "before-backfill-commit").await;
    a.engine
        .admit_write(serde_json::to_vec(rows)?.len() as u64)?;
    a.engine.query(QueryRequest::write(batch)).await?;
    crate::checkpoint(hold, "after-backfill-commit").await;
    Ok(record)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn cursor_completion_requires_all_declared_bytes_and_vectors() {
        let bundle = Bundle {
            model: "text-embedding-3-large".into(),
            model_fingerprint: "a".repeat(64),
            dimension: 1536,
            chunker: "source-document-v1".into(),
            schema: 1,
        };
        let mut c = Coverage {
            manifest_sha256: "b".repeat(64),
            prepared_sha256: "c".repeat(64),
            model_fingerprint: bundle.model_fingerprint.clone(),
            documents: 8,
            body_bytes: 100,
            vectors: 2,
            completed: 8,
            copied_bytes: 100,
            copied_vectors: 2,
            complete: true,
        };
        assert!(c.validate(&bundle).is_ok());
        c.copied_vectors = 1;
        assert!(c.validate(&bundle).is_err());
        c.copied_vectors = 2;
        c.copied_bytes = 99;
        assert!(c.validate(&bundle).is_err());
        c.copied_bytes = 100;
        c.completed = 7;
        assert!(c.validate(&bundle).is_err());
    }
}
