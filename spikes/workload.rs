use std::{
    collections::BTreeMap,
    fs::File,
    io::{BufWriter, Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::{sync::Semaphore, task::JoinSet, time::Instant};

use crate::{
    Error,
    authority::{self, Authority, Change},
    query_fixture as search,
    workload_backend::Bundle,
};

const KINDS: [&str; 6] = ["write", "current", "bm25", "ann", "history", "correction"];
const MIN_RATES: [f64; 6] = [10.0, 2.0, 1.0, 0.2, 0.2, 1.0 / 6.0];
const P95_MS: [u64; 6] = [5000, 5000, 60000, 60000, 10000, 60000];

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Config {
    run_id: String,
    phase: String,
    seconds: u64,
    fixture: PathBuf,
    output: PathBuf,
    corpus_manifest: Option<PathBuf>,
    diagnostic: bool,
    generation: Option<u64>,
}

struct Tally {
    attempted: u64,
    successful: u64,
    within_window: u64,
    conflicts: u64,
    errors: u64,
    latency_ms: Vec<u64>,
}

impl Default for Tally {
    fn default() -> Self {
        Self {
            attempted: 0,
            successful: 0,
            within_window: 0,
            conflicts: 0,
            errors: 0,
            latency_ms: vec![0; 60_002],
        }
    }
}

struct Metrics {
    tallies: [Tally; 6],
    clients: [u64; 100],
    peak_queued: usize,
}

impl Default for Metrics {
    fn default() -> Self {
        Self {
            tallies: Default::default(),
            clients: [0; 100],
            peak_queued: 0,
        }
    }
}

struct Shared {
    authority: Arc<Authority>,
    fixture: search::Fixture,
    config: Config,
    base: u64,
    start: Instant,
    admission: Semaphore,
    search_admission: Semaphore,
    queued: AtomicUsize,
    active: AtomicUsize,
    metrics: Mutex<Metrics>,
    log: Mutex<BufWriter<File>>,
    samples: Mutex<BufWriter<File>>,
    bundle: Option<Bundle>,
    derived: Mutex<Value>,
}

pub(crate) fn atomic_json(path: &Path, value: &Value) -> Result<(), Error> {
    let temporary = path.with_extension("tmp");
    let mut file = File::create(&temporary)?;
    writeln!(file, "{value}")?;
    file.sync_all()?;
    std::fs::rename(temporary, path)?;
    File::open(path.parent().ok_or("missing output parent")?)?.sync_all()?;
    Ok(())
}

pub(crate) fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn body(run: &str, client: usize, ordinal: u64) -> String {
    let prefix = format!(
        "Synthetic workload; excluded from corpus. Run {run}; client {client}; request {ordinal}. "
    );
    let remaining = 4096 - prefix.len();
    prefix + &"x".repeat(remaining)
}

fn role(client: usize) -> (usize, f64, usize, usize) {
    match client {
        0..60 => (0, 2.5, client / 2, 30),
        60..72 => (1, 5.0, client - 60, 12),
        72..84 => (2, 10.0, client - 72, 12),
        84..88 => (3, 16.0, client - 84, 4),
        88..92 => (4, 16.0, client - 88, 4),
        _ => (5, 40.0, client - 92, 8),
    }
}

fn planned_requests(phase: &str, seconds: u64) -> Option<u64> {
    match phase {
        "burst" => Some(100),
        "overload" => None,
        _ => Some(
            (0..100)
                .map(|id| {
                    let (_, period, offset, groups) = role(id);
                    ((seconds as f64 - period * offset as f64 / groups as f64) / period)
                        .max(0.0)
                        .ceil() as u64
                })
                .sum(),
        ),
    }
}

impl Shared {
    async fn operation(&self, kind: usize, client: usize, ordinal: u64) -> Result<Value, Error> {
        let memory = self.base + (if kind == 0 { client / 2 } else { client % 30 }) as u64;
        match kind {
            0 | 5 => {
                let prior = self
                    .authority
                    .work_get(memory, i64::MAX as u64)
                    .await?
                    .ok_or("missing workload seed")?;
                let change = Change {
                    request_id: format!("{}:{client}:{ordinal}", self.config.run_id),
                    memory,
                    expected: prior["seq"].as_u64().ok_or("invalid current sequence")?,
                    scope: ((client as u64 + ordinal) % 8) as u8,
                    active: kind == 0 || ordinal.is_multiple_of(2),
                    body: body(&self.config.run_id, client, ordinal),
                    text_only: true,
                };
                let mut receipt = self.authority.work_apply(&change).await?;
                if receipt.get("conflict").is_some() {
                    if receipt["conflict"] != "expected_revision" {
                        return Err("unexpected idempotency conflict".into());
                    }
                    return Ok(receipt);
                }
                let seq = receipt["commit_seq"]
                    .as_u64()
                    .ok_or("missing durable receipt")?;
                let actual = self.authority.work_get(memory, seq).await?;
                let expected = json!({"memory":memory,"seq":seq,"scope":change.scope,"active":change.active,"body":change.body});
                if actual != Some(expected) {
                    return Err("committed revision content mismatch".into());
                }
                receipt["oracle"] = json!({"memory":memory,"seq":seq,"scope":change.scope,"active":change.active,"body_sha256":hash(change.body.as_bytes())});
                Ok(receipt)
            }
            1 => {
                let mut current = self
                    .authority
                    .work_current(&self.config.run_id, memory)
                    .await?;
                if !current["entry"].is_null() {
                    let value = &mut current["entry"];
                    let body = value["body"].as_str().ok_or("missing current body")?;
                    if body.len() != 4096 || value["active"] != true || value["memory"] != memory {
                        return Err("current content oracle mismatch".into());
                    }
                    value["body_sha256"] = json!(hash(body.as_bytes()));
                    value
                        .as_object_mut()
                        .ok_or("invalid current entry")?
                        .remove("body");
                }
                current["memory"] = json!(memory);
                let index = (client * 101 + ordinal as usize * 97) % self.fixture.queries.len();
                let query = &self.fixture.queries[index];
                let source_memory = query.memory.ok_or("missing current corpus identity")?;
                let mut source = self
                    .authority
                    .work_current(&self.config.run_id, source_memory)
                    .await?;
                let entry = &mut source["entry"];
                let body = entry["body"]
                    .as_str()
                    .ok_or("missing current corpus body")?;
                let body_hash = hash(body.as_bytes());
                if entry["memory"] != source_memory
                    || entry["seq"] != 0
                    || entry["scope"] != query.partition.as_bytes()[1] - b'0'
                    || entry["active"] != true
                    || query.body_sha256.as_deref() != Some(&body_hash)
                {
                    return Err("current corpus differs from frozen source".into());
                }
                entry["body_sha256"] = json!(body_hash);
                entry
                    .as_object_mut()
                    .ok_or("invalid corpus entry")?
                    .remove("body");
                source["key"] = json!(query.key);
                source["query_index"] = json!(index);
                current["corpus"] = source;
                Ok(current)
            }
            2 | 3 => {
                let index = self
                    .fixture
                    .select_index(client * 101 + ordinal as usize * 97, kind == 3);
                let query = self.fixture.query(index)?;
                let validated = self
                    .authority
                    .work_query(
                        &query,
                        kind == 3,
                        &self.config.run_id,
                        self.config.generation,
                        self.bundle.as_ref(),
                    )
                    .await?;
                let hits = validated["hits"]
                    .as_array()
                    .ok_or("invalid search response")?;
                let self_at_20 = hits.iter().any(|hit| hit["key"] == query.key);
                if hits.is_empty()
                    || (!self.fixture.population() && !self_at_20)
                    || hits.iter().any(|hit| hit["partition"] != query.partition)
                {
                    return Err("real search oracle mismatch".into());
                }
                Ok(
                    json!({"hits":hits,"partition":query.partition,"query_key":query.key,"query_index":index,"self_at_20":self_at_20,"validation":validated}),
                )
            }
            4 => {
                let scope = ((client as u64 + ordinal) % 8) as u8;
                let page = self
                    .authority
                    .work_history(&self.config.run_id, scope, 4096, None, 100)
                    .await?;
                let expected: Vec<_> = (0..authority::ORACLE_MEMORIES)
                    .filter_map(|id| authority::oracle_state(id, 4096))
                    .filter(|row| row["scope"] == scope && row["active"] == true)
                    .take(100)
                    .collect();
                if expected.len() != 100 || page["entries"] != json!(expected) {
                    return Err("100-entry historical page oracle mismatch".into());
                }
                Ok(json!({"entries":100,"scope":scope,"at":4096}))
            }
            _ => unreachable!(),
        }
    }

    fn snapshot(&self, complete: bool) -> Result<Value, Error> {
        let metrics = self.metrics.lock().map_err(|_| "metrics poisoned")?;
        let operations: BTreeMap<_,_> = metrics.tallies.iter().enumerate().map(|(i,t)| {
            let rank = (t.attempted * 95).div_ceil(100);
            let mut sum = 0;
            let p95 = t.latency_ms.iter().position(|count| {sum += count; sum >= rank && rank > 0});
            (KINDS[i],json!({"attempted":t.attempted,"successful":t.successful,"within_window":t.within_window,
                "conflicts":t.conflicts,"errors":t.errors,"p95_upper_ms":p95,
                "successful_per_s":t.within_window as f64/self.config.seconds as f64}))
        }).collect();
        let seconds = self.start.elapsed().as_secs_f64();
        let completed: u64 = metrics.tallies.iter().map(|t| t.attempted).sum();
        Ok(
            json!({"phase":if complete {"completed"} else {"running"},"run_id":self.config.run_id,
            "config_epoch":1,"completed":completed,"total":planned_requests(&self.config.phase,self.config.seconds),
            "rate_per_s":completed as f64/seconds.max(0.000001),"eta_s":(self.config.seconds as f64-seconds).max(0.0),"elapsed_s":seconds,
            "completed_waves":(seconds as u64).min(self.config.seconds),
            "pid":std::process::id(),"start_ticks":std::fs::read_to_string("/proc/self/stat")?.rsplit_once(')').ok_or("invalid proc stat")?.1.split_whitespace().nth(19),
            "clients":metrics.clients.to_vec(),"operations":operations,"queued":self.queued.load(Ordering::Relaxed),
            "queued_bytes_upper_bound":self.queued.load(Ordering::Relaxed)*8192,"peak_queued":metrics.peak_queued,
            "active":self.active.load(Ordering::Relaxed),"admission_slots":4,"search_admission_slots":1,"queue_entry_bound":100,
            "queue_bytes_bound":100*8192,"wave_seconds":1,"derived":*self.derived.lock().map_err(|_|"derived progress poisoned")?}),
        )
    }

    fn checkpoint(&self, complete: bool) -> Result<Value, Error> {
        let value = self.snapshot(complete)?;
        let mut log = self.log.lock().map_err(|_| "latency log poisoned")?;
        log.flush()?;
        log.get_ref().sync_all()?;
        let mut samples = self.samples.lock().map_err(|_| "metric samples poisoned")?;
        writeln!(samples, "{value}")?;
        samples.flush()?;
        samples.get_ref().sync_all()?;
        atomic_json(&self.config.output.join("progress.json"), &value)?;
        Ok(value)
    }
}

async fn admit<'a>(
    admission: &'a Semaphore,
    search: &'a Semaphore,
    kind: usize,
) -> Result<
    (
        tokio::sync::SemaphorePermit<'a>,
        Option<tokio::sync::SemaphorePermit<'a>>,
    ),
    tokio::sync::AcquireError,
> {
    // Waiting searches must leave ordinary read/write capacity available.
    let search = if matches!(kind, 2 | 3) {
        Some(search.acquire().await?)
    } else {
        None
    };
    Ok((admission.acquire().await?, search))
}

async fn client(shared: Arc<Shared>, id: usize) -> Result<(), Error> {
    let (mut kind, period, offset, groups) = role(id);
    let phase = shared.config.phase.as_str();
    if phase != "steady" {
        kind = 0;
    }
    let end = shared.start + Duration::from_secs(shared.config.seconds);
    let mut planned = shared.start
        + if phase == "steady" {
            Duration::from_secs_f64(period * offset as f64 / groups as f64)
        } else {
            Duration::ZERO
        };
    let mut ordinal = 0;
    while planned < end {
        tokio::time::sleep_until(planned).await;
        if Instant::now() >= end {
            break;
        }
        ordinal += 1;
        let queued = shared.queued.fetch_add(1, Ordering::Relaxed) + 1;
        {
            let mut metrics = shared.metrics.lock().map_err(|_| "metrics poisoned")?;
            metrics.peak_queued = metrics.peak_queued.max(queued);
        }
        let deadline = Instant::now() + Duration::from_secs(60);
        let acquired = tokio::time::timeout_at(
            deadline,
            admit(&shared.admission, &shared.search_admission, kind),
        )
        .await;
        shared.queued.fetch_sub(1, Ordering::Relaxed);
        let result = match acquired {
            Ok(Ok(permit)) => {
                shared.active.fetch_add(1, Ordering::Relaxed);
                // Allocate the large embedded-engine future only after bounded admission.
                let result = tokio::time::timeout_at(
                    deadline,
                    Box::pin(shared.operation(kind, id, ordinal)),
                )
                .await;
                shared.active.fetch_sub(1, Ordering::Relaxed);
                drop(permit);
                match result {
                    Ok(value) => value,
                    Err(_) => Err(
                        "request deadline exceeded; commit status may require reconciliation"
                            .into(),
                    ),
                }
            }
            _ => Err("admission deadline exceeded".into()),
        };
        let completed = Instant::now();
        let latency = completed.duration_since(planned).as_secs_f64();
        let (status, receipt) = match result {
            Ok(value) if value.get("conflict").is_some() => ("conflict", value),
            Ok(value) => ("success", value),
            Err(error) => ("error", json!({"error":error.to_string()})),
        };
        let record = json!({"client":id,"ordinal":ordinal,"operation":KINDS[kind],"status":status,
            "scheduled_seconds":planned.duration_since(shared.start).as_secs_f64(),
            "completed_seconds":completed.duration_since(shared.start).as_secs_f64(),"seconds":latency,"receipt":receipt});
        {
            let mut log = shared.log.lock().map_err(|_| "latency log poisoned")?;
            writeln!(log, "{record}")?;
        }
        {
            let mut metrics = shared.metrics.lock().map_err(|_| "metrics poisoned")?;
            metrics.clients[id] += 1;
            let tally = &mut metrics.tallies[kind];
            tally.attempted += 1;
            tally.latency_ms[((latency * 1000.0).ceil() as usize).min(60001)] += 1;
            match status {
                "success" => {
                    tally.successful += 1;
                    tally.within_window += u64::from(completed <= end);
                }
                "conflict" => tally.conflicts += 1,
                _ => tally.errors += 1,
            }
        }
        if phase == "burst" {
            break;
        }
        planned = if phase == "overload" {
            Instant::now()
        } else {
            planned + Duration::from_secs_f64(period)
        };
    }
    Ok(())
}

pub async fn run(root: PathBuf, request: &Path) -> Result<(), Error> {
    let mut bytes = Vec::new();
    File::open(request)?.take(65537).read_to_end(&mut bytes)?;
    if bytes.len() > 65536 {
        return Err("workload config exceeds 64 KiB".into());
    }
    let config: Config = serde_json::from_slice(&bytes)?;
    if config.run_id.is_empty()
        || config.run_id.len() > 64
        || !config
            .run_id
            .bytes()
            .all(|c| c.is_ascii_alphanumeric() || c == b'-')
        || !(1..=7200).contains(&config.seconds)
        || !matches!(config.phase.as_str(), "steady" | "burst" | "overload")
        || (!config.diagnostic && config.phase == "steady" && config.seconds < 1800)
    {
        return Err("invalid workload identity, phase or duration".into());
    }
    let (fixture, fixture_hash) = search::load(&config.fixture)?;
    if fixture
        .queries
        .iter()
        .any(|query| query.memory.is_none() || query.body_sha256.is_none())
    {
        return Err("workload requires pinned corpus current-read identities".into());
    }
    let corpus = corpus_admission(&config, &fixture)?;
    std::fs::create_dir(&config.output)?;
    atomic_json(
        &config.output.join("admission.json"),
        &json!({"state":"admitted","config":config,"config_sha256":hash(&bytes),"fixture_sha256":fixture_hash,"config_epoch":1,
        "pid":std::process::id(),"start_ticks":std::fs::read_to_string("/proc/self/stat")?.rsplit_once(')').ok_or("invalid proc stat")?.1.split_whitespace().nth(19),
        "resume":"completed units are reusable only by the outer controller; an interrupted unit requires explicit abandonment"}),
    )?;
    let authority = Arc::new(Authority::work_open(root, corpus).await?);
    let outcome = run_open(authority.clone(), config, fixture).await;
    authority.work_close().await?;
    println!("{}", outcome?);
    Ok(())
}

fn corpus_admission(
    config: &Config,
    fixture: &search::Fixture,
) -> Result<Option<(String, u64)>, Error> {
    if !config.diagnostic {
        if config.generation.is_none() {
            return Err("full workload requires a pinned active generation".into());
        }
        let reserve = 30 * 1024u64.pow(3) + 4_000_000_000 + 45_000_000_000;
        let path = config
            .corpus_manifest
            .as_ref()
            .ok_or("full workload requires corpus manifest")?;
        let mut bytes = Vec::new();
        File::open(path)?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 1024 * 1024 {
            return Err("corpus manifest too large".into());
        }
        let manifest: Value = serde_json::from_slice(&bytes)?;
        if manifest["status"] != "full_corpus_prepared"
            || manifest["corpus"]["unique_content_utf8_bytes"]
                .as_u64()
                .unwrap_or(0)
                < 50_000_000_000
            || manifest["corpus"]["unique_vectors"].as_u64().unwrap_or(0) < 1_000_000
        {
            return Err("corpus does not meet frozen scale".into());
        }
        if !fixture.population()
            || fixture.queries.iter().filter(|q| !q.text_only).count() < 1024
            || fixture.queries.iter().filter(|q| q.text_only).count() < 512
            || fixture
                .queries
                .iter()
                .filter(|q| q.text_only)
                .map(|q| &q.partition)
                .collect::<std::collections::BTreeSet<_>>()
                .len()
                != 8
            || fixture.corpus_manifest_sha256.as_deref() != Some(hash(&bytes).as_str())
        {
            return Err(
                "full workload requires a distributed query population bound to its corpus".into(),
            );
        }
        return Ok(Some((hash(&bytes), reserve)));
    }
    Ok(None)
}

async fn run_open(
    authority: Arc<Authority>,
    config: Config,
    fixture: search::Fixture,
) -> Result<Value, Error> {
    let bundle = if let Some(id) = config.generation {
        Some(
            authority
                .work_pin(
                    id,
                    fixture
                        .corpus_manifest_sha256
                        .as_deref()
                        .ok_or("generation workload requires corpus identity")?,
                )
                .await?,
        )
    } else {
        None
    };
    for scope in 0..8 {
        authority
            .set_permission(&config.run_id, scope, true)
            .await?;
    }
    if authority.work_get(0, 4096).await? != authority::oracle_state(0, 4096) {
        return Err("historical oracle must be seeded before workload".into());
    }
    let digest = Sha256::digest(config.run_id.as_bytes());
    let base = 1_000_000_000 + u32::from_le_bytes(digest[..4].try_into()?) as u64 * 64;
    let mut baseline = Vec::new();
    for id in 0..50 {
        if authority
            .work_get(base + id, i64::MAX as u64)
            .await?
            .is_some()
        {
            return Err("workload run identity already has effects; do not replay".into());
        }
        let change = Change {
            request_id: format!("{}-seed-{id}", config.run_id),
            memory: base + id,
            expected: 0,
            scope: (id % 8) as u8,
            active: true,
            body: body(&config.run_id, id as usize, 0),
            text_only: true,
        };
        let receipt = authority.work_apply(&change).await?;
        if receipt.get("conflict").is_some() {
            return Err("workload seed conflicted".into());
        }
        baseline.push(json!({"memory":base+id,"seq":receipt["commit_seq"],"scope":id%8,"active":true,"body_sha256":hash(change.body.as_bytes())}));
    }
    atomic_json(
        &config.output.join("baseline.json"),
        &json!({"base":base,"run_id":config.run_id,"heads":baseline}),
    )?;
    let derived = if let Some(id) = config.generation {
        let deadline = Instant::now() + Duration::from_secs(300);
        loop {
            let progress =
                tokio::time::timeout_at(deadline, Box::pin(authority.work_catch_up(id))).await??;
            atomic_json(&config.output.join("index-preparation.json"), &progress)?;
            if progress["remaining"] == 0 {
                break progress;
            }
        }
    } else {
        Value::Null
    };
    let log = BufWriter::new(File::create_new(config.output.join("requests.jsonl"))?);
    let samples = BufWriter::new(File::create_new(config.output.join("metrics.jsonl"))?);
    let shared = Arc::new(Shared {
        authority,
        fixture,
        config,
        base,
        start: Instant::now(),
        admission: Semaphore::new(4),
        search_admission: Semaphore::new(1),
        queued: AtomicUsize::new(0),
        active: AtomicUsize::new(0),
        metrics: Mutex::new(Metrics::default()),
        log: Mutex::new(log),
        samples: Mutex::new(samples),
        bundle,
        derived: Mutex::new(derived),
    });
    let mut tasks = JoinSet::new();
    for id in 0..100 {
        tasks.spawn(client(shared.clone(), id));
    }
    let mut tick = tokio::time::interval(Duration::from_secs(1));
    let mut failures = Vec::new();
    let indexing_done = Arc::new(std::sync::atomic::AtomicBool::new(false));
    let indexer = shared.config.generation.map(|id| {
        let shared = shared.clone();
        let done = indexing_done.clone();
        tokio::spawn(async move {
            loop {
                let progress = Box::pin(shared.authority.work_catch_up(id)).await?;
                let caught_up = progress["remaining"] == 0;
                *shared
                    .derived
                    .lock()
                    .map_err(|_| "derived progress poisoned")? = progress;
                if caught_up {
                    if done.load(Ordering::Acquire) {
                        return Ok::<_, Error>(());
                    }
                    tokio::time::sleep(Duration::from_millis(50)).await;
                } else {
                    tokio::task::yield_now().await;
                }
            }
        })
    });
    while !tasks.is_empty() {
        tokio::select! {
            _=tick.tick()=>{shared.checkpoint(false)?;},
            result=tasks.join_next()=>{
                match result {Some(Ok(Ok(())))=>{},Some(Ok(Err(error)))=>failures.push(error.to_string()),Some(Err(error))=>failures.push(error.to_string()),None=>{}}
            }
        }
    }
    if shared.config.phase == "steady" {
        tokio::time::sleep_until(shared.start + Duration::from_secs(shared.config.seconds)).await;
    }
    indexing_done.store(true, Ordering::Release);
    if let Some(mut indexer) = indexer {
        let deadline = shared.start + Duration::from_secs(shared.config.seconds + 300);
        match tokio::time::timeout_at(deadline, &mut indexer).await {
            Ok(Ok(Ok(()))) => {}
            Ok(Ok(Err(error))) => failures.push(format!("indexer failed: {error}")),
            Ok(Err(error)) => failures.push(format!("indexer task failed: {error}")),
            Err(_) => {
                indexer.abort();
                let _ = indexer.await;
                failures.push(
                    "indexing drain exceeded 300 seconds; persisted work remains pending".into(),
                );
            }
        }
    }
    let snapshot = shared.checkpoint(true)?;
    let drain = (shared.start.elapsed().as_secs_f64() - shared.config.seconds as f64).max(0.0);
    {
        let metrics = shared.metrics.lock().map_err(|_| "metrics poisoned")?;
        if metrics.clients.contains(&0) {
            failures.push("some of the 100 clients completed no request".into());
        }
        for (i, tally) in metrics.tallies.iter().enumerate() {
            if tally.errors > 0 {
                failures.push(format!("{}: {} errors", KINDS[i], tally.errors));
            }
            if shared.config.phase == "steady" {
                if tally.within_window as f64 / (shared.config.seconds as f64) < MIN_RATES[i] {
                    failures.push(format!("{} minimum successful rate missed", KINDS[i]));
                }
                if snapshot["operations"][KINDS[i]]["p95_upper_ms"]
                    .as_u64()
                    .is_none_or(|ms| ms > P95_MS[i])
                {
                    failures.push(format!("{} P95 exceeded", KINDS[i]));
                }
            }
        }
        if drain > 300.0 {
            failures.push("overload drain exceeded 300 seconds".into());
        }
    }
    if let Some(id) = shared.config.generation {
        let progress = shared
            .derived
            .lock()
            .map_err(|_| "derived progress poisoned")?
            .clone();
        atomic_json(
            &shared.config.output.join("derived-oracle.json"),
            &Box::pin(
                shared
                    .authority
                    .work_derived_oracle(id, shared.base, progress),
            )
            .await?,
        )?;
    }
    let result = json!({"status":if failures.is_empty(){"passed"}else{"failed"},"diagnostic":shared.config.diagnostic,"phase":shared.config.phase,
        "snapshot":snapshot,"drain_s":drain,"failures":failures,
        "not_tested":["live_permission_revocation","correction_of_search_candidates","RSS_thresholds_checked_by_outer_observer"],
        "generation_workload":shared.config.generation});
    atomic_json(&shared.config.output.join("result.json"), &result)?;
    if !failures.is_empty() {
        return Err(format!("workload failed: {result}").into());
    }
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn heavy_search_waits_without_occupying_ordinary_admission() {
        use std::{
            future::Future,
            task::{Context, Waker},
        };
        let general = Semaphore::new(4);
        let search = Semaphore::new(1);
        let first = admit(&general, &search, 2).await.unwrap();
        let mut second = Box::pin(admit(&general, &search, 3));
        let mut cx = Context::from_waker(Waker::noop());
        assert!(second.as_mut().poll(&mut cx).is_pending());
        assert_eq!(general.available_permits(), 3);
        let ordinary = admit(&general, &search, 0).await.unwrap();
        assert_eq!(general.available_permits(), 2);
        drop(first);
        let resumed = match second.as_mut().poll(&mut cx) {
            std::task::Poll::Ready(Ok(permits)) => permits,
            _ => panic!("waiting search did not resume"),
        };
        drop((ordinary, resumed));
        assert_eq!(general.available_permits(), 4);
        assert_eq!(search.available_permits(), 1);
    }

    #[test]
    fn every_full_generation_reserves_rebuild_space() {
        let path = std::env::temp_dir().join(format!(
            "akasha-reserve-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let bytes = serde_json::to_vec(&json!({"status":"full_corpus_prepared",
            "corpus":{"unique_content_utf8_bytes":50_000_000_000u64,"unique_vectors":1_000_000}}))
        .unwrap();
        std::fs::write(&path, &bytes).unwrap();
        let fixture: search::Fixture = serde_json::from_value(json!({
            "source_sha256":"0".repeat(64), "model":"text-embedding-3-large", "dimension":1536,
            "corpus_manifest_sha256":hash(&bytes),
            "vector_file":{"name":"vectors.f32","bytes":1024*1536*4,"sha256":"0".repeat(64)},
            "queries":(0..1536).map(|i|json!({"key":i.to_string(),"partition":format!("p{}",i%8),
                "term":"source", "text_only":i>=1024})).collect::<Vec<_>>()
        }))
        .unwrap();
        let mut config = Config {
            run_id: "reserve".into(),
            phase: "steady".into(),
            seconds: 1800,
            fixture: PathBuf::new(),
            output: PathBuf::new(),
            corpus_manifest: Some(path.clone()),
            diagnostic: false,
            generation: Some(1),
        };
        let first = corpus_admission(&config, &fixture);
        config.generation = Some(2);
        let next = corpus_admission(&config, &fixture);
        std::fs::remove_file(path).unwrap();
        for result in [first, next] {
            assert_eq!(
                result.unwrap().unwrap().1,
                30 * 1024u64.pow(3) + 49_000_000_000
            );
        }
    }

    #[test]
    fn full_window_has_all_clients_and_the_frozen_offered_load() {
        assert_eq!(planned_requests("steady", 1800), Some(50_940));
        assert_eq!(planned_requests("burst", 1), Some(100));
        assert_eq!(planned_requests("overload", 60), None);
        let mut roles = [0; 6];
        for id in 0..100 {
            roles[role(id).0] += 1;
            assert_eq!(body("test", id, u64::MAX).len(), 4096);
        }
        assert_eq!(roles, [60, 12, 12, 4, 4, 8]);
    }
}
