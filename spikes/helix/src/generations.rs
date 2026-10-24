//! Generation and outbox contract probe; corpus backfill is a separate acceptance step.
use std::{
    io::Read,
    num::NonZeroUsize,
    path::{Path, PathBuf},
};

mod corpus;
use corpus::Coverage;

use helix_db::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

use crate::{
    authority::{Authority, Change, params},
    engine::Error,
    indexes,
};

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub(crate) struct Bundle {
    model: String,
    model_fingerprint: String,
    dimension: usize,
    chunker: String,
    schema: u64,
}

impl Bundle {
    fn validate(&self) -> Result<(), Error> {
        if self.model.is_empty()
            || self.model.len() > 128
            || self.model_fingerprint.len() != 64
            || !self
                .model_fingerprint
                .bytes()
                .all(|v| v.is_ascii_digit() || (b'a'..=b'f').contains(&v))
            || self.chunker.is_empty()
            || self.chunker.len() > 128
            || !(1..=4096).contains(&self.dimension)
            || self.schema == 0
        {
            return Err("invalid index bundle".into());
        }
        Ok(())
    }
}

#[derive(Clone, Copy, Deserialize, Serialize)]
#[serde(rename_all = "snake_case")]
enum Family {
    Text,
    Vector,
}
impl Family {
    fn name(self) -> &'static str {
        match self {
            Self::Text => "text",
            Self::Vector => "vector",
        }
    }
    fn label(self, generation: u64) -> String {
        format!(
            "Derived{generation}{}",
            match self {
                Self::Text => "Text",
                Self::Vector => "Vector",
            }
        )
    }
    fn key(self, generation: u64, seq: u64) -> String {
        format!("{generation:020}:{}:{seq:020}", self.name())
    }
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct VectorOutput {
    model: String,
    model_fingerprint: String,
    body_sha256: String,
    values: Vec<f32>,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Begin {
        generation: u64,
        bundle: Bundle,
    },
    BeginCorpus {
        generation: u64,
        bundle: Bundle,
        manifest: PathBuf,
    },
    Backfill {
        generation: u64,
        waves: u64,
        hold: Option<String>,
    },
    CatchUp {
        generation: u64,
        waves: u64,
    },
    Build {
        generation: u64,
        #[serde(default)]
        fail_after_text: bool,
        during_build: Option<Change>,
    },
    Commit {
        change: Change,
    },
    Grant {
        principal: String,
        scope: u8,
        allowed: bool,
    },
    Current {
        principal: String,
        memory: u64,
    },
    History {
        principal: String,
        memory: u64,
        at: u64,
        scope: u8,
    },
    State {
        principal: String,
        scope: u8,
        at: u64,
        cursor: Option<u64>,
        limit: usize,
    },
    Apply {
        generation: u64,
        family: Family,
        seq: u64,
        vector: Option<VectorOutput>,
        hold: Option<String>,
    },
    Advance {
        generation: u64,
        family: Family,
    },
    Activate {
        generation: u64,
        hold: Option<String>,
    },
    Receipt {
        generation: u64,
        request_id: String,
    },
    Query {
        generation: u64,
        bundle: Bundle,
        principal: String,
        scope: u8,
        vector: Option<Vec<f32>>,
        term: Option<String>,
    },
    Status,
}

#[derive(Serialize)]
struct Generation {
    id: u64,
    bundle: Bundle,
    fingerprint: String,
    status: String,
    ready: bool,
    text: u64,
    vector: u64,
    base: Option<Coverage>,
}

fn hash(bytes: impl AsRef<[u8]>) -> String {
    Sha256::digest(bytes.as_ref())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}
fn number(value: &Value) -> Result<u64, Error> {
    value
        .as_u64()
        .ok_or_else(|| "invalid generation integer".into())
}

pub async fn run(authority: &Authority, path: &Path, root: &Path) -> Result<Value, Error> {
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err("generation request exceeds 1 MiB".into());
    }
    let request: Request = serde_json::from_slice(&bytes)?;
    initialize(authority).await?;
    match request {
        Request::Begin { generation, bundle } => begin(authority, generation, bundle).await,
        Request::BeginCorpus {
            generation,
            bundle,
            manifest,
        } => corpus::begin(authority, generation, bundle, &manifest, root).await,
        Request::Backfill {
            generation,
            waves,
            hold,
        } => corpus::backfill(authority, generation, waves, hold.as_deref(), root).await,
        Request::CatchUp { generation, waves } => {
            if !(1..=1_000_000).contains(&waves) {
                return Err("invalid catch-up wave bound".into());
            }
            let started = std::time::Instant::now();
            let initial = self::generation(authority, generation).await?;
            let before = initial.text.min(initial.vector);
            let mut saved_s = 0.0;
            let mut last = json!(null);
            for wave in 0..waves {
                last = catch_up_one(authority, generation).await?;
                let seconds = started.elapsed().as_secs_f64();
                let completed = number(&last["text"])?.min(number(&last["vector"])?);
                let rate = (completed - before) as f64 / seconds.max(0.000001);
                last["completed"] = json!(completed);
                last["total"] = last["committed"].clone();
                last["elapsed_s"] = json!(seconds);
                last["rate_per_s"] = json!(rate);
                last["eta_s"] = if rate > 0.0 {
                    json!(number(&last["remaining"])? as f64 / rate)
                } else {
                    Value::Null
                };
                last["pid"] = json!(std::process::id());
                if wave == 0
                    || wave + 1 == waves
                    || last["remaining"] == 0
                    || seconds - saved_s >= 1.0
                {
                    crate::workload::atomic_json(
                        &root.join(format!("generation-{generation}-catch-up.json")),
                        &last,
                    )?;
                    saved_s = seconds;
                }
                if last["remaining"] == 0 {
                    break;
                }
                tokio::task::yield_now().await;
            }
            Ok(last)
        }
        Request::Build {
            generation,
            fail_after_text,
            during_build,
        } => build(authority, generation, fail_after_text, during_build).await,
        Request::Commit { change } => authority.apply(&change).await,
        Request::Grant {
            principal,
            scope,
            allowed,
        } => Ok(json!({"revision":authority.set_permission(&principal,scope,allowed).await?})),
        Request::Current { principal, memory } => authority.current(&principal, memory).await,
        Request::History {
            principal,
            memory,
            at,
            scope,
        } => authority.history_for(&principal, memory, at, scope).await,
        Request::State {
            principal,
            scope,
            at,
            cursor,
            limit,
        } => {
            authority
                .state_for(&principal, scope, at, cursor, limit)
                .await
        }
        Request::Apply {
            generation,
            family,
            seq,
            vector,
            hold,
        } => apply(authority, generation, family, seq, vector, hold.as_deref()).await,
        Request::Advance { generation, family } => {
            let _writer = authority.writer.lock().await;
            let count = advance(authority, generation, family).await?;
            Ok(json!({"advanced": count, "next_wave_may_be_needed": count == 256}))
        }
        Request::Activate { generation, hold } => {
            activate(authority, generation, hold.as_deref()).await
        }
        Request::Receipt {
            generation,
            request_id,
        } => receipt(authority, generation, &request_id).await,
        Request::Query {
            generation,
            bundle,
            principal,
            scope,
            vector,
            term,
        } => {
            query(
                authority, generation, bundle, &principal, scope, vector, term,
            )
            .await
        }
        Request::Status => status(authority).await,
    }
}

async fn initialize(a: &Authority) -> Result<(), Error> {
    for spec in [
        IndexSpec::node_unique_equality("Generation", "key"),
        IndexSpec::node_unique_equality("IndexState", "key"),
        IndexSpec::node_unique_equality("IndexedJob", "key"),
        IndexSpec::node_range("IndexedJob", "key"),
    ] {
        indexes::create(&a.engine, spec).await?;
    }
    let _writer = a.writer.lock().await;
    let rows = a
        .engine
        .cypher(
            "MATCH (p:IndexState {key:'active'}) RETURN p.generation",
            Default::default(),
            None,
        )
        .await?
        .rows;
    if rows.is_empty() {
        let count = a
            .engine
            .cypher(
                "MATCH (g:Generation) RETURN count(g)",
                Default::default(),
                None,
            )
            .await?
            .rows;
        if count != vec![vec![json!(0)]] {
            return Err("missing active generation pointer".into());
        }
        a.engine.admit_write(0)?;
        a.engine
            .cypher(
                "CREATE (:IndexState {key:'active',generation:0,fingerprint:''})",
                Default::default(),
                None,
            )
            .await?;
    }
    Ok(())
}

async fn generation(a: &Authority, id: u64) -> Result<Generation, Error> {
    if !(1..=2).contains(&id) {
        return Err("generation outside lifetime bound".into());
    }
    let rows=a.engine.cypher("MATCH (g:Generation {key:$id}) RETURN g.bundle,g.fingerprint,g.status,g.ready,g.text,g.vector,g.base",params(json!({"id":id})),None).await?.rows;
    let [row] = rows.as_slice() else {
        return Err("unknown or duplicate generation".into());
    };
    let bundle: Bundle = serde_json::from_str(row[0].as_str().ok_or("invalid bundle")?)?;
    bundle.validate()?;
    let fingerprint = hash(serde_json::to_vec(&bundle)?);
    if row[1] != fingerprint {
        return Err("bundle fingerprint mismatch".into());
    }
    let record = Generation {
        id,
        bundle,
        fingerprint,
        status: row[2].as_str().ok_or("invalid generation status")?.into(),
        ready: row[3].as_bool().ok_or("invalid generation readiness")?,
        text: number(&row[4])?,
        vector: number(&row[5])?,
        base: match &row[6] {
            Value::Null => None,
            Value::String(value) => Some(serde_json::from_str(value)?),
            _ => return Err("invalid corpus coverage".into()),
        },
    };
    if !matches!(
        record.status.as_str(),
        "building" | "active" | "retired" | "failed"
    ) || (record.status == "failed" && record.ready)
        || (matches!(record.status.as_str(), "active" | "retired") && !record.ready)
    {
        return Err("generation readiness/status integrity mismatch".into());
    }
    if let Some(base) = &record.base {
        base.validate(&record.bundle)?;
        if record.ready && !base.complete {
            return Err("ready generation has incomplete corpus coverage".into());
        }
    }
    Ok(record)
}

async fn begin(a: &Authority, id: u64, bundle: Bundle) -> Result<Value, Error> {
    bundle.validate()?;
    let _writer = a.writer.lock().await;
    if !a
        .engine
        .cypher(
            "MATCH (d:Document) RETURN d.key LIMIT 1",
            Default::default(),
            None,
        )
        .await?
        .rows
        .is_empty()
    {
        return Err(
            "generation diagnostic excludes imported corpus; full backfill is required".into(),
        );
    }
    begin_record(a, id, bundle, None).await
}

// Caller holds the shared authority writer.
async fn begin_record(
    a: &Authority,
    id: u64,
    bundle: Bundle,
    base: Option<Coverage>,
) -> Result<Value, Error> {
    let rows = a
        .engine
        .cypher(
            "MATCH (g:Generation) RETURN g.key ORDER BY g.key",
            Default::default(),
            None,
        )
        .await?
        .rows;
    if rows.iter().any(|row| row[0] == id) {
        let prior = generation(a, id).await?;
        let same_source = match (&prior.base, &base) {
            (None, None) => true,
            (Some(a), Some(b)) => a.same_source(b),
            _ => false,
        };
        if prior.bundle != bundle || !same_source {
            return Err("generation identity reuse".into());
        }
        return Ok(serde_json::to_value(prior)?);
    }
    // shortcut: two builds per database until native text-blob reclamation is proven.
    if rows.len() >= 2 || id != rows.len() as u64 + 1 {
        return Err("generation lifetime limit: two retained builds".into());
    }
    let encoded = serde_json::to_string(&bundle)?;
    let fingerprint = hash(encoded.as_bytes());
    a.engine.admit_write(0)?;
    a.engine.cypher("CREATE (:Generation {key:$id,bundle:$bundle,fingerprint:$fingerprint,status:'building',ready:false,text:0,vector:0,base:$base})",params(json!({"id":id,"bundle":encoded,"fingerprint":fingerprint,"base":base.as_ref().map(serde_json::to_string).transpose()?})),None).await?;
    Ok(serde_json::to_value(generation(a, id).await?)?)
}

async fn build(
    a: &Authority,
    id: u64,
    fail_after_text: bool,
    during: Option<Change>,
) -> Result<Value, Error> {
    let g = {
        let _writer = a.writer.lock().await;
        let g = generation(a, id).await?;
        if !matches!(g.status.as_str(), "building" | "failed") {
            return Err("cannot rebuild active or retained generation".into());
        }
        if g.base.as_ref().is_some_and(|base| !base.complete) {
            return Err("corpus backfill is incomplete".into());
        }
        a.engine.admit_write(0)?;
        a.engine
            .cypher(
                "MATCH (g:Generation {key:$id}) SET g.status='building',g.ready=false",
                params(json!({"id":id})),
                None,
            )
            .await?;
        g
    };
    let result=async {
        for family in [Family::Text,Family::Vector] {indexes::create(&a.engine,IndexSpec::node_unique_equality(family.label(id),"memory")).await?;}
        let text=indexes::create(&a.engine,IndexSpec::node_text(Family::Text.label(id),"body",Some("partition"))).await?;
        if fail_after_text {return Err::<Value,Error>("injected build validation failure after native text activation".into());}
        // The authority writer stays available while the candidate bundle is building.
        let concurrent_commit=if let Some(change)=during {Some(a.apply(&change).await?)} else {None};
        let vector=indexes::create(&a.engine,IndexSpec::node_vector(Family::Vector.label(id),"embedding",NonZeroUsize::new(g.bundle.dimension).ok_or("zero dimension")?,VectorDistanceMetric::Cosine,Some("partition"))).await?;
        Ok(json!({"generation":id,"text":text,"vector":vector,"concurrent_commit":concurrent_commit}))
    }.await;
    let _writer = a.writer.lock().await;
    a.engine.cypher("MATCH (g:Generation {key:$id}) SET g.ready=$ready,g.status=$status",params(json!({"id":id,"ready":result.is_ok(),"status":if result.is_ok(){"building"}else{"failed"}})),None).await?;
    result
}

async fn apply(
    a: &Authority,
    id: u64,
    family: Family,
    seq: u64,
    vector: Option<VectorOutput>,
    hold: Option<&str>,
) -> Result<Value, Error> {
    if hold.is_some_and(|h| !["after-index-application", "before-watermark"].contains(&h)) {
        return Err("invalid application checkpoint".into());
    }
    if seq == 0 || seq > i64::MAX as u64 {
        return Err("invalid derived sequence".into());
    }
    let _writer = a.writer.lock().await;
    let g = generation(a, id).await?;
    if !g.ready || !matches!(g.status.as_str(), "building" | "active" | "retired") {
        return Err("generation not ready for derived writes".into());
    }
    let request_hash = hash(serde_json::to_vec(
        &json!({"generation":id,"bundle":g.fingerprint,"family":family,"seq":seq,"vector":vector}),
    )?);
    let key = family.key(id, seq);
    let prior = a
        .engine
        .cypher(
            "MATCH (j:IndexedJob {key:$key}) RETURN j.hash,j.reason",
            params(json!({"key":key})),
            None,
        )
        .await?
        .rows;
    if let [row] = prior.as_slice() {
        if row[0] != request_hash {
            return Err("indexed job idempotency conflict".into());
        }
        advance(a, id, family).await?;
        return Ok(
            json!({"generation":id,"family":family,"seq":seq,"reason":row[1],"reused":true}),
        );
    } else if !prior.is_empty() {
        return Err("duplicate indexed job".into());
    }
    let jobs = a
        .engine
        .cypher(
            "MATCH (j:Job {seq:$seq}) RETURN j.revision",
            params(json!({"seq":seq})),
            None,
        )
        .await?
        .rows;
    let [job] = jobs.as_slice() else {
        return Err("missing or duplicate durable outbox job".into());
    };
    let revisions = a
        .engine
        .cypher(
            "MATCH (r:Revision {order:$order}) RETURN r.memory,r.seq,r.scope,r.active,r.body,r.text_only",
            params(json!({"order":job[0]})),
            None,
        )
        .await?
        .rows;
    let [revision] = revisions.as_slice() else {
        return Err("missing or duplicate immutable source revision".into());
    };
    let memory = number(&revision[0])?;
    let scope = number(&revision[2])?;
    if revision[1] != seq || scope > 7 {
        return Err("outbox revision integrity mismatch".into());
    }
    let active = revision[3].as_bool().ok_or("invalid active flag")?;
    let body = revision[4].as_str().ok_or("invalid immutable body")?;
    let text_only = match revision[5] {
        Value::Null | Value::Bool(false) => false,
        Value::Bool(true) => true,
        _ => return Err("invalid immutable vector policy".into()),
    };
    if text_only && vector.is_some() {
        return Err("text-only source cannot consume vector output".into());
    }
    let at = a.read_point().await?;
    let head = a
        .checked_heads(&[memory], at)
        .await?
        .pop()
        .flatten()
        .ok_or("missing authoritative head")?;
    let label = family.label(id);
    let head_seq = number(&head["seq"])?;
    if head_seq < seq {
        return Err("outbox source revision is ahead of its head".into());
    }
    a.engine
        .admit_write(body.len() as u64 + (g.bundle.dimension * 4) as u64)?;
    let reason = if head_seq > seq {
        "superseded"
    } else if !active || (text_only && matches!(family, Family::Vector)) {
        a.engine
            .cypher(
                &format!("MATCH (d:{label} {{memory:$memory}}) DELETE d"),
                params(json!({"memory":memory})),
                None,
            )
            .await?;
        if active { "not_required" } else { "retracted" }
    } else {
        let payload = match family {
            Family::Text => {
                if vector.is_some() {
                    return Err("text job cannot consume vector output".into());
                }
                json!(body)
            }
            Family::Vector => {
                let output = vector.as_ref().ok_or("embedding output required")?;
                if output.model != g.bundle.model
                    || output.model_fingerprint != g.bundle.model_fingerprint
                    || output.body_sha256 != hash(body.as_bytes())
                    || output.values.len() != g.bundle.dimension
                    || !output.values.iter().all(|v| v.is_finite())
                    || !output.values.iter().any(|v| *v != 0.0)
                {
                    return Err("embedding bundle or source identity mismatch".into());
                }
                json!(output.values)
            }
        };
        let existing = a
            .engine
            .cypher(
                &format!("MATCH (d:{label} {{memory:$memory}}) RETURN d.seq,d.hash"),
                params(json!({"memory":memory})),
                None,
            )
            .await?
            .rows;
        let clause = match existing.as_slice() {
            [] => format!("CREATE (d:{label} {{memory:$memory}})"),
            [row] if number(&row[0])? <= seq => {
                if row[0] == seq && row[1] != request_hash {
                    return Err("derived application identity conflict".into());
                }
                format!("MATCH (d:{label} {{memory:$memory}})")
            }
            _ => return Err("derived entry is ahead of authority or duplicated".into()),
        };
        let property = match family {
            Family::Text => "body",
            Family::Vector => "embedding",
        };
        a.engine.cypher(&format!("{clause} SET d.seq=$seq,d.partition=$partition,d.hash=$hash,d.{property}=$payload RETURN d.seq"),params(json!({"memory":memory,"seq":seq,"partition":format!("p{scope}"),"hash":request_hash,"payload":payload})),None).await?;
        "applied"
    };
    crate::checkpoint(hold, "after-index-application").await;
    a.engine.cypher("CREATE (:IndexedJob {key:$key,generation:$id,family:$family,seq:$seq,reason:$reason,hash:$hash})",params(json!({"key":key,"id":id,"family":family.name(),"seq":seq,"reason":reason,"hash":request_hash})),None).await?;
    crate::checkpoint(hold, "before-watermark").await;
    advance(a, id, family).await?;
    Ok(
        json!({"generation":id,"family":family,"seq":seq,"reason":reason,"reused":false,"visibility":"strong_with_bounded_pending_overlay"}),
    )
}

// Called with the authority writer held; each batch advances only an exact contiguous prefix.
async fn advance(a: &Authority, id: u64, family: Family) -> Result<u64, Error> {
    let record = generation(a, id).await?;
    let before = match family {
        Family::Text => record.text,
        Family::Vector => record.vector,
    };
    if before == i64::MAX as u64 {
        return Ok(0);
    }
    let response = a
        .engine
        .query(QueryRequest::read(
            read_batch()
                .var_as(
                    "jobs",
                    g().n_with_label_where(
                        "IndexedJob",
                        SourcePredicate::between(
                            "key",
                            family.key(id, before + 1),
                            family.key(id, i64::MAX as u64),
                        ),
                    )
                    .order_by("key", Order::Asc)
                    .limit(256)
                    .value_map(Some(vec![
                        "key",
                        "seq",
                        "generation",
                        "family",
                        "reason",
                    ])),
                )
                .returning(["jobs"]),
        ))
        .await?;
    let rows = match &response["jobs"] {
        Value::Null => return Ok(0),
        Value::Array(rows) => rows,
        _ => return Err("invalid completion prefix".into()),
    };
    let next = contiguous_prefix(id, family, before, rows)?;
    if next == before {
        return Ok(0);
    }
    a.engine.admit_write(0)?;
    let field = family.name();
    let rows = a.engine.cypher(&format!("MATCH (g:Generation {{key:$id}}),(c:Counter {{key:'global'}}) WHERE g.{field}=$before AND c.seq>=$next SET g.{field}=$next RETURN g.{field}"),
        params(json!({"id":id,"before":before,"next":next})),None).await?.rows;
    if rows != vec![vec![json!(next)]] {
        return Err("watermark compare-and-swap or commit-prefix validation failed".into());
    }
    Ok(next - before)
}

fn contiguous_prefix(id: u64, family: Family, before: u64, rows: &[Value]) -> Result<u64, Error> {
    if rows.len() > 256 {
        return Err("watermark wave exceeds 256 entries".into());
    }
    let mut next = before;
    for row in rows {
        let seq = number(&row["seq"])?;
        if row["key"] != family.key(id, seq)
            || row["generation"] != id
            || row["family"] != family.name()
            || (row["reason"] == "not_required" && !matches!(family, Family::Vector))
            || !matches!(
                row["reason"].as_str(),
                Some("applied" | "superseded" | "retracted" | "not_required")
            )
        {
            return Err("derived completion integrity mismatch".into());
        }
        if next.checked_add(1) != Some(seq) {
            break;
        }
        next = seq;
    }
    Ok(next)
}

async fn activate(a: &Authority, id: u64, hold: Option<&str>) -> Result<Value, Error> {
    if hold.is_some_and(|h| !["before-activation", "after-activation"].contains(&h)) {
        return Err("invalid activation checkpoint".into());
    }
    let _writer = a.writer.lock().await;
    let g = generation(a, id).await?;
    let at = a.read_point().await?;
    if !g.ready
        || !matches!(g.status.as_str(), "building" | "active")
        || g.text < at
        || g.vector < at
    {
        return Err("candidate generation has incomplete coverage".into());
    }
    let rows = a
        .engine
        .cypher(
            "MATCH (p:IndexState {key:'active'}) RETURN p.generation",
            Default::default(),
            None,
        )
        .await?
        .rows;
    let [row] = rows.as_slice() else {
        return Err("invalid active generation pointer".into());
    };
    let previous = number(&row[0])?;
    if previous == id {
        return Ok(json!({"active":id,"at":at,"reused":true}));
    }
    a.engine.admit_write(0)?;
    crate::checkpoint(hold, "before-activation").await;
    let old = if previous == 0 {
        String::new()
    } else {
        "MATCH (old:Generation {key:$previous}) SET old.status='retired'".into()
    };
    let rows=a.engine.cypher(&format!("MATCH (p:IndexState {{key:'active'}}),(g:Generation {{key:$id}}) WHERE p.generation=$previous SET p.generation=$id,p.fingerprint=$fingerprint,g.status='active' {old} RETURN p.generation"),params(json!({"id":id,"previous":previous,"fingerprint":g.fingerprint})),None).await?.rows;
    if rows != vec![vec![json!(id)]] {
        return Err("activation compare-and-swap failed".into());
    }
    crate::checkpoint(hold, "after-activation").await;
    Ok(json!({"active":id,"at":at,"bundle":g.bundle,"reused":false}))
}

async fn receipt(a: &Authority, id: u64, request: &str) -> Result<Value, Error> {
    let _writer = a.writer.lock().await;
    let g = generation(a, id).await?;
    let rows = a
        .engine
        .cypher(
            "MATCH (r:Receipt {key:$request}) RETURN r.seq,r.result",
            params(json!({"request":request})),
            None,
        )
        .await?
        .rows;
    let [row] = rows.as_slice() else {
        return Err("unknown durable receipt".into());
    };
    let seq = number(&row[0])?;
    let mut reasons = serde_json::Map::new();
    for family in [Family::Text, Family::Vector] {
        let rows = a
            .engine
            .cypher(
                "MATCH (j:IndexedJob {key:$key}) RETURN j.reason",
                params(json!({"key":family.key(id,seq)})),
                None,
            )
            .await?
            .rows;
        match rows.as_slice() {
            [] => {}
            [row] => {
                reasons.insert(family.name().into(), row[0].clone());
            }
            _ => return Err("duplicate completion receipt".into()),
        }
    }
    Ok(
        json!({"stage":if reasons.len()==2 && g.ready {"indexed"} else {"committed"},"commit_seq":seq,"generation":id,"bundle":g.bundle,"reasons":reasons,"watermarks":{"text":g.text,"vector":g.vector},"corpus":g.base,"visibility":"strong_with_bounded_pending_overlay"}),
    )
}

pub(crate) async fn query(
    a: &Authority,
    id: u64,
    bundle: Bundle,
    principal: &str,
    scope: u8,
    vector: Option<Vec<f32>>,
    term: Option<String>,
) -> Result<Value, Error> {
    let record = generation(a, id).await?;
    if !record.ready || !matches!(record.status.as_str(), "active" | "retired") {
        return Err("query generation unavailable".into());
    }
    if record.bundle != bundle || scope > 7 {
        return Err("query encoder and index bundle mismatch".into());
    }
    let partition = Some(PropertyValue::from(format!("p{scope}")));
    let traversal = match (vector, term) {
        (Some(vector), None)
            if vector.len() == bundle.dimension
                && vector.iter().all(|v| v.is_finite())
                && vector.iter().any(|v| *v != 0.0) =>
        {
            g().vector_search_nodes(Family::Vector.label(id), "embedding", vector, 20, partition)
        }
        (None, Some(term)) if !term.trim().is_empty() && term.len() <= 128 => {
            g().text_search_nodes(Family::Text.label(id), "body", term, 20, partition)
        }
        _ => return Err("invalid generation query".into()),
    };
    let at = a.read_point().await?;
    let response = a
        .engine
        .query(QueryRequest::read(
            read_batch()
                .var_as(
                    "hits",
                    traversal.value_map(Some(vec!["memory", "seq", "partition", "source_key"])),
                )
                .returning(["hits"]),
        ))
        .await?;
    let hits = response["hits"]
        .as_array()
        .ok_or("invalid generation candidates")?;
    let mut validated = a.revalidate(principal, scope, at, at, hits).await?;
    validated["generation"] = json!(id);
    validated["bundle"] = json!(record.bundle);
    validated["watermarks"] = json!({"text":record.text,"vector":record.vector});
    Ok(validated)
}

pub(crate) async fn pin(a: &Authority, id: u64, manifest: &str) -> Result<Bundle, Error> {
    initialize(a).await?;
    let state = status(a).await?;
    let record = generation(a, id).await?;
    if state["active"] != id
        || record.status != "active"
        || record
            .base
            .as_ref()
            .is_none_or(|base| !base.complete || base.manifest_sha256 != manifest)
    {
        return Err("workload needs the active generation for its exact corpus".into());
    }
    Ok(record.bundle)
}

pub(crate) async fn catch_up_one(a: &Authority, id: u64) -> Result<Value, Error> {
    let before = generation(a, id).await?;
    if !before.ready || !matches!(before.status.as_str(), "building" | "active" | "retired") {
        return Err("generation not ready for catch-up".into());
    }
    let at = a.read_point().await?;
    if before.text == at && before.vector == at {
        return Ok(json!({"generation":id,"committed":at,"text":at,"vector":at,"remaining":0}));
    }
    for (family, watermark) in [(Family::Text, before.text), (Family::Vector, before.vector)] {
        if watermark < at {
            apply(a, id, family, watermark + 1, None, None).await?;
        } else if watermark > at {
            return Err("generation watermark ahead of committed authority".into());
        }
    }
    let after = generation(a, id).await?;
    let at = a.read_point().await?;
    Ok(
        json!({"generation":id,"committed":at,"text":after.text,"vector":after.vector,
        "remaining":at.saturating_sub(after.text.min(after.vector))}),
    )
}

async fn status(a: &Authority) -> Result<Value, Error> {
    let _writer = a.writer.lock().await;
    let rows = a
        .engine
        .cypher(
            "MATCH (p:IndexState {key:'active'}) RETURN p.generation,p.fingerprint",
            Default::default(),
            None,
        )
        .await?
        .rows;
    let [pointer] = rows.as_slice() else {
        return Err("invalid active pointer".into());
    };
    let rows = a
        .engine
        .cypher(
            "MATCH (g:Generation) RETURN g.key ORDER BY g.key",
            Default::default(),
            None,
        )
        .await?
        .rows;
    let mut generations = Vec::new();
    for row in rows {
        generations.push(generation(a, number(&row[0])?).await?);
    }
    if generations.iter().filter(|g| g.status == "active").count() != usize::from(pointer[0] != 0)
        || (pointer[0] != 0
            && !generations.iter().any(|g| {
                json!(g.id) == pointer[0]
                    && g.status == "active"
                    && json!(g.fingerprint) == pointer[1]
            }))
    {
        return Err("active pointer/bundle integrity mismatch".into());
    }
    Ok(
        json!({"active":pointer[0],"generations":generations,"retention":"two builds lifetime bound; no native text blob GC"}),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn completion_prefix_is_bounded_and_cannot_cross_a_gap_or_a_foreign_family() {
        let row = |seq| {
            json!({"key":format!("00000000000000000001:text:{seq:020}"),"seq":seq,
            "generation":1,"family":"text","reason":"applied"})
        };
        assert_eq!(
            contiguous_prefix(1, Family::Text, 0, &[row(1), row(2), row(4)]).unwrap(),
            2
        );
        assert_eq!(
            contiguous_prefix(1, Family::Text, 0, &(1..=256).map(row).collect::<Vec<_>>()).unwrap(),
            256
        );
        assert!(
            contiguous_prefix(1, Family::Text, 0, &(1..=257).map(row).collect::<Vec<_>>()).is_err()
        );
        assert!(contiguous_prefix(1, Family::Vector, 0, &[row(1)]).is_err());
    }
}
