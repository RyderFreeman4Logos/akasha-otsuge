use std::{collections::BTreeMap, io::Write, path::Path, time::Instant};

use helix_db::prelude::*;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tokio::sync::Mutex;

use crate::{
    engine::{Engine, Error},
    indexes,
};

pub const ORACLE_MEMORIES: u64 = 1024;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Change {
    pub request_id: String,
    pub memory: u64,
    pub expected: u64,
    pub scope: u8,
    pub active: bool,
    pub body: String,
}

pub struct Authority {
    pub engine: Engine,
    writer: Mutex<()>,
}

impl Authority {
    pub async fn open(engine: Engine) -> Result<Self, Error> {
        for spec in [
            IndexSpec::node_unique_equality("Counter", "key"),
            IndexSpec::node_unique_equality("Head", "key"),
            IndexSpec::node_unique_equality("Receipt", "key"),
            IndexSpec::node_unique_equality("Job", "seq"),
            IndexSpec::node_range_desc("Revision", "order"),
            IndexSpec::node_unique_equality("ScopeMember", "key"),
            IndexSpec::node_range("ScopeMember", "key"),
        ] {
            indexes::create(&engine, spec).await?;
        }
        let counter = engine
            .cypher(
                "MATCH (c:Counter {key:'global'}) RETURN c.seq",
                BTreeMap::new(),
                None,
            )
            .await?;
        if counter.rows.is_empty() {
            engine
                .cypher(
                    "CREATE (:Counter {key:'global',seq:0})",
                    BTreeMap::new(),
                    None,
                )
                .await?;
        }
        Ok(Self {
            engine,
            writer: Mutex::new(()),
        })
    }

    pub async fn apply(&self, change: &Change) -> Result<Value, Error> {
        if change.request_id.is_empty()
            || change.request_id.len() > 128
            || change.body.len() > 128 * 1024
            || change.scope > 7
            || change.memory > i64::MAX as u64
            || change.expected > i64::MAX as u64
        {
            return Err("invalid change".into());
        }
        let digest: String = Sha256::digest(serde_json::to_vec(change)?)
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect();
        // One owner serializes short authority transactions; model work never holds this lock.
        let _writer = self.writer.lock().await;
        let prior = self
            .engine
            .cypher(
                "MATCH (r:Receipt {key:$request}) RETURN r.hash,r.result",
                params(json!({"request":change.request_id})),
                None,
            )
            .await?;
        if let Some(row) = prior.rows.first() {
            return if row[0] == digest {
                Ok(serde_json::from_str(
                    row[1].as_str().ok_or("invalid stored receipt")?,
                )?)
            } else {
                Ok(json!({"conflict":"idempotency"}))
            };
        }
        let head = self
            .engine
            .cypher(
                "MATCH (h:Head {key:$memory}) RETURN h.seq",
                params(json!({"memory": change.memory})),
                None,
            )
            .await?;
        let previous = head
            .rows
            .first()
            .map(|row| row[0].as_u64().ok_or("invalid head sequence"))
            .transpose()?
            .unwrap_or(0);
        if previous != change.expected {
            return Ok(json!({"conflict":"expected_revision", "actual":previous}));
        }
        let counter = self
            .engine
            .cypher(
                "MATCH (c:Counter {key:'global'}) RETURN c.seq",
                BTreeMap::new(),
                None,
            )
            .await?;
        let before = counter
            .rows
            .first()
            .and_then(|row| row[0].as_u64())
            .ok_or("missing commit counter")?;
        let seq = before
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or("commit sequence exhausted")?;
        let order = format!("{:020}:{seq:020}", change.memory);
        let membership = format!("{}:{:020}", change.scope, change.memory);
        let member = self
            .engine
            .cypher(
                "MATCH (m:ScopeMember {key:$key}) RETURN m.key",
                params(json!({"key":membership})),
                None,
            )
            .await?;
        let receipt = json!({"stage":"committed", "request_id":change.request_id,
            "commit_seq":seq, "revision":order, "config_generation":1});
        let head_clause = if head.rows.is_empty() {
            "CREATE (h:Head {key:$memory})"
        } else {
            "MATCH (h:Head {key:$memory}) WHERE h.seq=$expected"
        };
        let member_clause = if member.rows.is_empty() {
            "CREATE (:ScopeMember {key:$membership,memory:$memory})"
        } else {
            ""
        };
        let statement = format!(
            "MATCH (c:Counter {{key:'global'}}) WHERE c.seq=$before \
             {head_clause} SET c.seq=$seq,h.seq=$seq,h.scope=$scope,h.active=$active \
             CREATE (:Revision {{order:$order,memory:$memory,seq:$seq,previous:$expected,scope:$scope,active:$active,body:$body}}), \
                    (:Receipt {{key:$request,hash:$hash,result:$receipt,seq:$seq}}), \
                    (:Job {{seq:$seq,revision:$order,generation:1,status:'pending'}}) \
             {member_clause} RETURN c.seq"
        );
        let result = self
            .engine
            .cypher(
                &statement,
                params(json!({
                    "before":before,"seq":seq,"memory":change.memory,"expected":change.expected,
                    "scope":change.scope,"active":change.active,"body":change.body,"order":order,
                    "request":change.request_id,"hash":digest,"receipt":receipt.to_string(),
                    "membership":membership,
                })),
                None,
            )
            .await?;
        if result.rows != vec![vec![json!(seq)]] {
            return Err("authority compare-and-swap did not commit".into());
        }
        Ok(receipt)
    }

    pub async fn get(&self, memory: u64, at: u64) -> Result<Option<Value>, Error> {
        let result = self
            .engine
            .query(QueryRequest::read(
                read_batch()
                    .var_as(
                        "revision",
                        g().n_with_label_where(
                            "Revision",
                            SourcePredicate::between(
                                "order",
                                format!("{memory:020}:00000000000000000000"),
                                format!("{memory:020}:{at:020}"),
                            ),
                        )
                        .order_by("order", Order::Desc)
                        .limit(1)
                        .value_map(Some(vec!["memory", "seq", "scope", "active", "body"])),
                    )
                    .returning(["revision"]),
            ))
            .await?;
        // A bounded single-object traversal represents an empty result as null.
        let rows = match result.get("revision") {
            Some(Value::Null) => return Ok(None),
            Some(Value::Array(rows)) => rows,
            _ => return Err(format!("invalid history response: {result}").into()),
        };
        Ok(rows.first().cloned())
    }

    pub async fn current(&self, memory: u64) -> Result<Value, Error> {
        let _writer = self.writer.lock().await;
        let counter = self
            .engine
            .cypher(
                "MATCH (c:Counter {key:'global'}) RETURN c.seq",
                BTreeMap::new(),
                None,
            )
            .await?;
        let at = counter
            .rows
            .first()
            .and_then(|row| row[0].as_u64())
            .ok_or("missing current commit point")?;
        Ok(json!({"at":at,"entry":self.get(memory,at).await?.filter(|row|row["active"]==true)}))
    }

    pub async fn state(
        &self,
        scope: u8,
        at: u64,
        cursor: Option<u64>,
        limit: usize,
    ) -> Result<Value, Error> {
        if scope > 7 || limit == 0 || limit > 100 {
            return Err("invalid history page".into());
        }
        let mut result = Vec::new();
        let mut cursor = cursor;
        loop {
            let lower = match cursor {
                Some(u64::MAX) => return Ok(json!({"entries":result,"next":null,"at":at})),
                Some(id) => format!("{scope}:{:020}", id + 1),
                None => format!("{scope}:00000000000000000000"),
            };
            let members = self
                .engine
                .query(QueryRequest::read(
                    read_batch()
                        .var_as(
                            "members",
                            g().n_with_label_where(
                                "ScopeMember",
                                SourcePredicate::between(
                                    "key",
                                    lower,
                                    format!("{scope}:{:020}", u64::MAX),
                                ),
                            )
                            .order_by("key", Order::Asc)
                            .limit(100)
                            .value_map(Some(vec!["memory"])),
                        )
                        .returning(["members"]),
                ))
                .await?;
            let members = members["members"]
                .as_array()
                .ok_or("invalid membership page")?;
            if members.is_empty() {
                return Ok(json!({"entries":result,"next":null,"at":at}));
            }
            for member in members {
                let id = member["memory"].as_u64().ok_or("invalid membership ID")?;
                if cursor.is_some_and(|previous| id <= previous) {
                    return Err(format!("membership range did not advance: scope={scope}, cursor={cursor:?}, returned={id}").into());
                }
                cursor = Some(id);
                if let Some(revision) = self.get(id, at).await?
                    && revision["active"] == true
                    && revision["scope"] == scope
                {
                    result.push(revision);
                }
                if result.len() == limit {
                    return Ok(json!({"entries":result,"next":cursor,"at":at}));
                }
            }
        }
    }
}

fn params(value: Value) -> BTreeMap<String, Value> {
    value
        .as_object()
        .expect("literal parameter object")
        .iter()
        .map(|(key, value)| (key.clone(), value.clone()))
        .collect()
}

pub fn oracle_change(seq: u64) -> Change {
    let memory = (seq - 1) % ORACLE_MEMORIES;
    let round = (seq - 1) / ORACLE_MEMORIES;
    Change {
        request_id: format!("oracle-v1:{seq}"),
        memory,
        expected: seq.saturating_sub(ORACLE_MEMORIES),
        scope: ((memory + round) % 8) as u8,
        active: round % 4 != 1,
        body: format!(
            "Synthetic oracle only; excluded from corpus bytes. Memory {memory}, revision round {round}."
        ),
    }
}

pub fn oracle_state(memory: u64, at: u64) -> Option<Value> {
    if at <= memory {
        return None;
    }
    let seq = memory + 1 + ((at - memory - 1) / ORACLE_MEMORIES) * ORACLE_MEMORIES;
    let change = oracle_change(seq);
    Some(
        json!({"memory":memory,"seq":seq,"scope":change.scope,"active":change.active,"body":change.body}),
    )
}

fn checkpoint(
    path: &Path,
    phase: &str,
    completed: u64,
    total: u64,
    started: Instant,
) -> Result<(), Error> {
    let elapsed = started.elapsed().as_secs_f64();
    let progress = json!({"oracle_version":1,"phase":phase,"completed":completed,"total":total,
        "elapsed_s":elapsed,"rate_per_s":completed as f64/elapsed,
        "eta_s":if completed>0 {Some((total-completed) as f64*elapsed/completed as f64)} else {None},
        "pid":std::process::id(),"config_epoch":1});
    let temporary = path.with_extension("json.tmp");
    let mut file = std::fs::File::create(&temporary)?;
    writeln!(file, "{progress}")?;
    file.sync_all()?;
    std::fs::rename(temporary, path)?;
    std::fs::File::open(path.parent().ok_or("checkpoint needs parent")?)?.sync_all()?;
    Ok(())
}

pub async fn seed(authority: &Authority, count: u64, progress: &Path) -> Result<Value, Error> {
    if count == 0 || count > 1_000_000 {
        return Err("oracle count must be in 1..=1000000".into());
    }
    let started = std::time::Instant::now();
    checkpoint(progress, "oracle_seed", 0, count, started)?;
    for seq in 1..=count {
        let change = oracle_change(seq);
        let receipt = authority.apply(&change).await?;
        if receipt["commit_seq"] != seq {
            return Err(format!("unexpected oracle receipt at {seq}: {receipt}").into());
        }
        if seq.is_multiple_of(1000) {
            checkpoint(progress, "oracle_seed", seq, count, started)?;
            eprintln!(
                "{}",
                json!({"phase":"oracle_seed","commits":seq,"seconds":started.elapsed().as_secs_f64()})
            );
        }
    }
    checkpoint(progress, "oracle_seeded", count, count, started)?;
    Ok(
        json!({"phase":"oracle_seeded","changes":count,"version":1,"seconds":started.elapsed().as_secs_f64()}),
    )
}

pub async fn verify(authority: &Authority, count: u64, progress: &Path) -> Result<Value, Error> {
    if count == 0 || count > 1_000_000 {
        return Err("invalid oracle count".into());
    }
    let started = std::time::Instant::now();
    let first = oracle_change(1);
    let receipt = authority.apply(&first).await?;
    assert_eq!(
        receipt["commit_seq"], 1,
        "durable idempotency must survive reopening"
    );
    let mut changed = first.clone();
    changed.body.push_str(" changed");
    assert_eq!(
        authority.apply(&changed).await?,
        json!({"conflict":"idempotency"})
    );
    changed.request_id = "stale-expectation".into();
    assert_eq!(
        authority.apply(&changed).await?["conflict"],
        "expected_revision"
    );
    let points: std::collections::BTreeSet<_> =
        [0, 1, count / 2, count.saturating_sub(1), count].into();
    let total = (points.len() as u64) * (ORACLE_MEMORIES + 8) + 3;
    checkpoint(progress, "oracle_verify", 0, total, started)?;
    let mut checks = 0;
    for at in points {
        eprintln!(
            "{}",
            json!({"phase":"oracle_get","at":at,"checks":checks,"seconds":started.elapsed().as_secs_f64()})
        );
        for memory in 0..ORACLE_MEMORIES {
            assert_eq!(
                authority.get(memory, at).await?,
                oracle_state(memory, at),
                "memory={memory}, at={at}"
            );
            checks += 1;
            if memory.is_multiple_of(128) {
                checkpoint(progress, "oracle_verify", checks, total, started)?;
                eprintln!(
                    "{}",
                    json!({"phase":"oracle_get","at":at,"memory":memory,"checks":checks,"seconds":started.elapsed().as_secs_f64()})
                );
            }
        }
        for scope in 0..8 {
            eprintln!(
                "{}",
                json!({"phase":"oracle_state","at":at,"scope":scope,"checks":checks,"seconds":started.elapsed().as_secs_f64()})
            );
            let expected: Vec<_> = (0..ORACLE_MEMORIES)
                .filter_map(|memory| oracle_state(memory, at))
                .filter(|row| row["active"] == true && row["scope"] == scope)
                .collect();
            let mut actual = Vec::new();
            let mut cursor = None;
            loop {
                let page = authority.state(scope, at, cursor, 100).await?;
                actual.extend(
                    page["entries"]
                        .as_array()
                        .ok_or("invalid page")?
                        .iter()
                        .cloned(),
                );
                let next = page["next"].as_u64();
                if let Some(next) = next {
                    assert!(
                        cursor.is_none_or(|before| next > before),
                        "cursor must progress"
                    );
                    cursor = Some(next);
                } else {
                    break;
                }
            }
            assert_eq!(actual, expected, "scope={scope}, at={at}");
            checks += 1;
            checkpoint(progress, "oracle_verify", checks, total, started)?;
        }
    }
    for label in ["Revision", "Receipt", "Job"] {
        let rows = authority
            .engine
            .cypher(
                &format!("MATCH (n:{label}) RETURN count(n)"),
                BTreeMap::new(),
                None,
            )
            .await?
            .rows;
        assert_eq!(
            rows,
            vec![vec![json!(count)]],
            "no partial or duplicate {label}"
        );
        checks += 1;
    }
    checkpoint(progress, "oracle_verified", checks, total, started)?;
    Ok(
        json!({"phase":"oracle_verified","changes":count,"checks":checks,"seconds":started.elapsed().as_secs_f64(),
        "not_tested":["50GB","100_clients","derived_watermark","live_permission_revocation"]}),
    )
}
