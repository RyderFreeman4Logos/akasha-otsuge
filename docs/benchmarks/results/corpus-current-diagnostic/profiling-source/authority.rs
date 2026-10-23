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
    pub(crate) writer: Mutex<()>,
    #[cfg(test)]
    history_barrier: Option<(
        std::sync::Arc<tokio::sync::Notify>,
        std::sync::Arc<tokio::sync::Notify>,
    )>,
}

impl Authority {
    pub async fn open(engine: Engine) -> Result<Self, Error> {
        for spec in [
            IndexSpec::node_unique_equality("Document", "memory"),
            IndexSpec::node_unique_equality("Counter", "key"),
            IndexSpec::node_unique_equality("Head", "key"),
            IndexSpec::node_unique_equality("Receipt", "key"),
            IndexSpec::node_unique_equality("Job", "seq"),
            IndexSpec::node_range_desc("Revision", "order"),
            IndexSpec::node_unique_equality("ScopeMember", "key"),
            IndexSpec::node_range("ScopeMember", "key"),
            IndexSpec::node_unique_equality("Permission", "key"),
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
            #[cfg(test)]
            history_barrier: None,
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
                    (:Job {{seq:$seq,revision:$order,config_generation:1,status:'pending'}}) \
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
            Some(Value::Null) => &[][..],
            Some(Value::Array(rows)) => rows,
            _ => return Err(format!("invalid history response: {result}").into()),
        };
        if let Some(row) = rows.first() {
            return Ok(Some(row.clone()));
        }
        if !corpus_memory(memory) {
            return Ok(None);
        }
        let result = self
            .engine
            .query(QueryRequest::read(
                read_batch()
                    .var_as(
                        "source",
                        g().n_with_label_where(
                            "Document",
                            SourcePredicate::eq("memory", i64::try_from(memory)?),
                        )
                        .limit(2)
                        .value_map(Some(vec![
                            "memory",
                            "key",
                            "partition",
                            "body",
                        ])),
                    )
                    .var_as(
                        "corpus",
                        g().n_with_label("ImportState")
                            .limit(2)
                            .value_map(Some(vec!["documents", "complete"])),
                    )
                    .returning(["source", "corpus"]),
            ))
            .await?;
        baseline_entry(memory, &result["source"], corpus_bound(&result["corpus"])?)
    }

    pub async fn read_point(&self) -> Result<u64, Error> {
        let counter = self
            .engine
            .cypher(
                "MATCH (c:Counter {key:'global'}) RETURN c.seq",
                BTreeMap::new(),
                None,
            )
            .await?;
        counter
            .rows
            .first()
            .and_then(|row| row[0].as_u64())
            .ok_or_else(|| "missing current commit point".into())
    }

    async fn permission(&self, principal: &str, scope: u8) -> Result<(bool, u64), Error> {
        let key = permission_key(principal, scope)?;
        let rows = self
            .engine
            .cypher(
                "MATCH (p:Permission {key:$key}) RETURN p.allowed,p.revision",
                params(json!({"key":key})),
                None,
            )
            .await?
            .rows;
        match rows.as_slice() {
            [] => Ok((false, 0)),
            [row] => Ok((
                row[0].as_bool().ok_or("invalid permission flag")?,
                row[1].as_u64().ok_or("invalid permission revision")?,
            )),
            _ => Err("duplicate permission records".into()),
        }
    }

    pub async fn set_permission(
        &self,
        principal: &str,
        scope: u8,
        allowed: bool,
    ) -> Result<u64, Error> {
        let key = permission_key(principal, scope)?;
        let _writer = self.writer.lock().await;
        let (prior, before) = self.permission(principal, scope).await?;
        if before > 0 && prior == allowed {
            return Ok(before);
        }
        let revision = before
            .checked_add(1)
            .filter(|value| *value <= i64::MAX as u64)
            .ok_or("permission revision exhausted")?;
        let clause = if before == 0 {
            "CREATE (p:Permission {key:$key})"
        } else {
            "MATCH (p:Permission {key:$key}) WHERE p.revision=$before"
        };
        let rows = self
            .engine
            .cypher(
                &format!("{clause} SET p.allowed=$allowed,p.revision=$revision RETURN p.revision"),
                params(json!({"key":key,"allowed":allowed,"before":before,"revision":revision})),
                None,
            )
            .await?
            .rows;
        if rows != vec![vec![json!(revision)]] {
            return Err("permission compare-and-swap did not commit".into());
        }
        Ok(revision)
    }

    pub(crate) async fn checked_heads(
        &self,
        memories: &[u64],
        at: u64,
    ) -> Result<Vec<Option<Value>>, Error> {
        if memories.len() > 20 {
            return Err("head validation exceeds top-20 bound".into());
        }
        let mut batch = read_batch();
        let mut names = Vec::new();
        for (index, memory) in memories.iter().enumerate() {
            let memory_i64 = i64::try_from(*memory)?;
            let head = format!("head{index}");
            let revision = format!("revision{index}");
            batch = batch
                .var_as(
                    &head,
                    g().n_with_label_where("Head", SourcePredicate::eq("key", memory_i64))
                        .limit(2)
                        .value_map(Some(vec!["seq", "scope", "active"])),
                )
                .var_as(
                    &revision,
                    g().n_with_label_where(
                        "Revision",
                        SourcePredicate::between(
                            "order",
                            format!("{memory:020}:00000000000000000000"),
                            format!("{memory:020}:{:020}", i64::MAX),
                        ),
                    )
                    .order_by("order", Order::Desc)
                    .limit(1)
                    .value_map(Some(vec!["memory", "seq", "scope", "active"])),
                );
            names.extend([head, revision]);
            if corpus_memory(*memory) {
                let source = format!("source{index}");
                batch = batch.var_as(
                    &source,
                    g().n_with_label_where("Document", SourcePredicate::eq("memory", memory_i64))
                        .limit(2)
                        .value_map(Some(vec!["memory", "key", "partition"])),
                );
                names.push(source);
            }
        }
        let has_corpus = memories.iter().any(|memory| corpus_memory(*memory));
        if has_corpus {
            batch = batch.var_as(
                "corpus",
                g().n_with_label("ImportState")
                    .limit(2)
                    .value_map(Some(vec!["documents", "complete"])),
            );
            names.push("corpus".into());
        }
        // One read snapshot and one bounded engine call for the whole candidate set.
        let result = self
            .engine
            .query(QueryRequest::read(batch.returning(names)))
            .await?;
        let mut entries = Vec::new();
        let bound = if has_corpus {
            corpus_bound(&result["corpus"])?
        } else {
            None
        };
        for (index, memory) in memories.iter().enumerate() {
            let rows = |name: String| -> Result<&[Value], Error> {
                match result.get(&name) {
                    Some(Value::Null) => Ok(&[]),
                    Some(Value::Array(rows)) => Ok(rows),
                    _ => Err("invalid authority batch response".into()),
                }
            };
            let heads = rows(format!("head{index}"))?;
            let revisions = rows(format!("revision{index}"))?;
            let baseline = if corpus_memory(*memory) {
                baseline_entry(*memory, &result[format!("source{index}")], bound)?
            } else {
                None
            };
            match (heads, revisions) {
                ([], []) => entries.push(baseline),
                ([head], [revision])
                    if head["seq"].as_u64().is_some_and(|seq| seq > 0 && seq <= at)
                        && head["scope"].as_u64().is_some_and(|scope| scope < 8)
                        && head["active"].is_boolean()
                        && revision["memory"] == *memory
                        && ["seq", "scope", "active"]
                            .iter()
                            .all(|key| head[key] == revision[key]) =>
                {
                    entries.push(Some(revision.clone()))
                }
                _ => return Err("authority head/history integrity mismatch".into()),
            }
        }
        Ok(entries)
    }

    pub async fn current(&self, principal: &str, memory: u64) -> Result<Value, Error> {
        permission_key(principal, 0)?;
        let _writer = self.writer.lock().await;
        let at = self.read_point().await?;
        let head = self.checked_heads(&[memory], at).await?.pop().flatten();
        let mut entry = if head.is_some_and(|row| row["active"] == true) {
            let row = self
                .get(memory, at)
                .await?
                .ok_or("current revision disappeared")?;
            if !row["body"].is_string() {
                return Err("invalid authoritative body".into());
            }
            Some(row)
        } else {
            None
        };
        let mut permission_revision = None;
        if let Some(row) = &entry {
            let scope = u8::try_from(row["scope"].as_u64().ok_or("invalid current scope")?)?;
            let (allowed, revision) = self.permission(principal, scope).await?;
            permission_revision = Some(revision);
            if !allowed {
                entry = None;
            }
        }
        Ok(json!({"at":at,"entry":entry,"permission_revision":permission_revision}))
    }

    pub async fn history_for(
        &self,
        principal: &str,
        memory: u64,
        at: u64,
        scope: u8,
    ) -> Result<Value, Error> {
        let _writer = self.writer.lock().await;
        let (allowed, revision) = self.permission(principal, scope).await?;
        let validated = self.read_point().await?;
        if at > validated {
            return Err("historical read point is not committed".into());
        }
        let entry = if allowed {
            self.get(memory, at)
                .await?
                .filter(|row| row["active"] == true && row["scope"] == scope)
        } else {
            None
        };
        Ok(
            json!({"entry":entry,"read_seq":at,"validated_at_seq":validated,
            "permission_revision":revision,"retrieval_mode":"historical_state"}),
        )
    }

    pub async fn state_for(
        &self,
        principal: &str,
        scope: u8,
        at: u64,
        cursor: Option<u64>,
        limit: usize,
    ) -> Result<Value, Error> {
        {
            let _writer = self.writer.lock().await;
            let (allowed, revision) = self.permission(principal, scope).await?;
            let validated = self.read_point().await?;
            if at > validated {
                return Err("historical read point is not committed".into());
            }
            if !allowed {
                return Ok(
                    json!({"entries":[],"next":null,"at":at,"permission_revision":revision,"validated_at_seq":validated,"retrieval_mode":"historical_state"}),
                );
            }
        }
        #[cfg(test)]
        if let Some((started, resume)) = &self.history_barrier {
            started.notify_one();
            resume.notified().await;
        }
        // Committed history is immutable; only final live authorization needs the writer.
        let mut page = self.state(scope, at, cursor, limit).await?;
        let _writer = self.writer.lock().await;
        let (allowed, revision) = self.permission(principal, scope).await?;
        let validated = self.read_point().await?;
        if at > validated {
            return Err("historical read point is no longer committed".into());
        }
        if !allowed {
            page = json!({"entries":[],"next":null,"at":at});
        }
        page["permission_revision"] = json!(revision);
        page["validated_at_seq"] = json!(validated);
        page["retrieval_mode"] = json!("historical_state");
        Ok(page)
    }

    pub async fn revalidate(
        &self,
        principal: &str,
        scope: u8,
        read_seq: u64,
        min_commit_seq: u64,
        hits: &[Value],
    ) -> Result<Value, Error> {
        if hits.len() > 20 {
            return Err("candidate count exceeds top-20 bound".into());
        }
        let _writer = self.writer.lock().await;
        let (allowed, permission_revision) = self.permission(principal, scope).await?;
        let at = self.read_point().await?;
        if at < read_seq || at < min_commit_seq {
            return Err("min_commit_seq_not_reached".into());
        }
        let mut visible = Vec::new();
        if allowed && !hits.is_empty() {
            let memories: Vec<_> = hits
                .iter()
                .map(|hit| {
                    hit["memory"]
                        .as_u64()
                        .filter(|value| *value <= i64::MAX as u64)
                        .ok_or("invalid candidate memory")
                })
                .collect::<Result<_, _>>()?;
            let heads = self.checked_heads(&memories, at).await?;
            for (hit, head) in hits.iter().zip(heads) {
                let indexed = match hit.get("seq") {
                    None | Some(Value::Null) => 0,
                    Some(value) => value.as_u64().ok_or("invalid candidate revision")?,
                };
                if hit["partition"] != format!("p{scope}") {
                    return Err("candidate scope mismatch".into());
                }
                // Imported immutable documents are revision zero until a correction creates a head.
                let valid = match head {
                    None => false,
                    Some(row) => {
                        row["seq"] == indexed && row["scope"] == scope && row["active"] == true
                    }
                };
                if valid {
                    visible.push(hit.clone());
                }
            }
        }
        Ok(
            json!({"hits":visible,"read_seq":read_seq,"validated_at_seq":at,
            "permission_revision":permission_revision,"retrieval_mode":"current_candidates",
            "coverage":"candidate_set_only","filtered":hits.len()-visible.len()}),
        )
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

fn corpus_memory(memory: u64) -> bool {
    (1_000_000..101_000_000).contains(&memory)
}

fn corpus_bound(value: &Value) -> Result<Option<u64>, Error> {
    match value {
        Value::Null => Ok(None),
        Value::Array(rows) if rows.is_empty() => Ok(None),
        Value::Array(rows) if rows.len() == 1 && rows[0]["complete"] == true => {
            let count = rows[0]["documents"]
                .as_u64()
                .filter(|v| (1..=100_000_000).contains(v))
                .ok_or("invalid authoritative corpus count")?;
            Ok(Some(count))
        }
        _ => Err("incomplete or malformed authoritative corpus".into()),
    }
}

fn baseline_entry(memory: u64, value: &Value, count: Option<u64>) -> Result<Option<Value>, Error> {
    let rows = match value {
        Value::Null => &[][..],
        Value::Array(rows) => rows,
        _ => return Err("invalid source lookup".into()),
    };
    let expected = count.is_some_and(|count| memory < 1_000_000 + count);
    let row = match rows {
        [] if !expected => return Ok(None),
        [row] => row,
        _ => return Err("authoritative corpus record missing or duplicated".into()),
    };
    let scope = match row["partition"].as_str() {
        Some("p0") => 0,
        Some("p1") => 1,
        Some("p2") => 2,
        Some("p3") => 3,
        Some("p4") => 4,
        Some("p5") => 5,
        Some("p6") => 6,
        Some("p7") => 7,
        _ => return Err("invalid source scope".into()),
    };
    if row["memory"] != memory
        || (count.is_some() && !expected)
        || !row["key"]
            .as_str()
            .is_some_and(|key| !key.is_empty() && key.len() <= 256)
    {
        return Err("authoritative source identity mismatch".into());
    }
    let mut entry = json!({"memory":memory,"seq":0,"scope":scope,"active":true});
    if let Some(body) = row.get("body") {
        if !body.as_str().is_some_and(|body| body.len() <= 128 * 1024) {
            return Err("invalid source body".into());
        }
        entry["body"] = body.clone();
    }
    Ok(Some(entry))
}

fn permission_key(principal: &str, scope: u8) -> Result<String, Error> {
    if scope > 7
        || principal.is_empty()
        || principal.len() > 64
        || !principal
            .bytes()
            .all(|value| value.is_ascii_alphanumeric() || b"-_".contains(&value))
    {
        return Err("invalid permission principal or scope".into());
    }
    Ok(format!("{principal}/{scope}"))
}

pub(crate) fn params(value: Value) -> BTreeMap<String, Value> {
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
                &format!("MATCH (n:{label}) WHERE n.seq <= $at RETURN count(n)"),
                params(json!({"at":count})),
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

#[cfg(test)]
mod history_concurrency_tests {
    use super::*;
    use std::{
        sync::Arc,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    #[tokio::test]
    async fn historical_scan_releases_writer_and_rechecks_revocation_before_return() {
        let path = std::env::temp_dir().join(format!(
            "akasha-history-race-{}-{}",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir(&path).unwrap();
        std::fs::write(
            path.join("OWNER"),
            "akasha-otsuge MVP-003 history concurrency unit fixture\n",
        )
        .unwrap();
        let engine = Engine::open_with_config(
            helix_db::HelixDbSource::Disk {
                root: path.clone(),
                database: "akasha-history-race".into(),
            },
            crate::config().unwrap(),
        )
        .await
        .unwrap();
        let mut authority = Authority::open(engine).await.unwrap();
        for memory in 1..=4 {
            authority
                .apply(&Change {
                    request_id: format!("seed-{memory}"),
                    memory,
                    expected: 0,
                    scope: 0,
                    active: true,
                    body: format!("Synthetic history race {memory}"),
                })
                .await
                .unwrap();
        }
        authority.set_permission("reader", 0, true).await.unwrap();
        let started = Arc::new(tokio::sync::Notify::new());
        let resume = Arc::new(tokio::sync::Notify::new());
        authority.history_barrier = Some((started.clone(), resume.clone()));
        let a = Arc::new(authority);
        let reader = a.clone();
        let page =
            tokio::spawn(
                async move { Box::pin(reader.state_for("reader", 0, 4, None, 100)).await },
            );
        tokio::time::timeout(Duration::from_secs(5), started.notified())
            .await
            .unwrap();
        let writer = a.clone();
        let mut change = tokio::spawn(async move {
            writer.set_permission("reader", 0, false).await?;
            writer
                .apply(&Change {
                    request_id: "during-page".into(),
                    memory: 99,
                    expected: 0,
                    scope: 0,
                    active: true,
                    body: "Synthetic change during historical page".into(),
                })
                .await
        });
        let completed = tokio::time::timeout(Duration::from_secs(1), &mut change).await;
        let before_resume = completed.is_ok();
        resume.notify_one();
        let changed = match completed {
            Ok(result) => result,
            Err(_) => change.await,
        }
        .unwrap()
        .unwrap();
        let page = page.await.unwrap().unwrap();
        a.engine.close().await.unwrap();
        drop(a);
        std::fs::remove_dir_all(&path).unwrap();
        assert!(
            before_resume,
            "historical scan held the shared writer across immutable reads"
        );
        assert_eq!(changed["commit_seq"], 5);
        assert_eq!(page["entries"], json!([]));
        assert_eq!(page["permission_revision"], 2);
        assert_eq!(page["validated_at_seq"], 5);
    }
}
