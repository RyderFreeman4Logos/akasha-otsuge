//! Immediate authority transactions and immutable revision-time views.
use std::{
    fs::File,
    io::{Read, Write},
    path::{Path, PathBuf},
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use surrealkv::{Durability, LSMIterator, Mode, Transaction, Tree};
use tokio::sync::Mutex;

use crate::{Error, checkpoint, load, open_authority, search::Settings};

const SEQ: &[u8] = b"authority:seq";

mod oracle;
pub(crate) use oracle::{ORACLE_MEMORIES, oracle_state, run as run_oracle};

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Change {
    pub request_id: String,
    pub memory: u64,
    pub expected: u64,
    pub scope: u8,
    pub active: bool,
    pub body: String,
    #[serde(default)]
    pub text_only: bool,
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Entry {
    pub memory: u64,
    pub seq: u64,
    pub previous: u64,
    pub scope: u8,
    pub active: bool,
    pub body: String,
    pub text_only: bool,
    pub source_key: Option<String>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Receipt {
    hash: String,
    result: Value,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Permission {
    allowed: bool,
    revision: u64,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "snake_case", deny_unknown_fields)]
enum Request {
    Apply {
        change: Change,
        hold: Option<String>,
    },
    Get {
        memory: u64,
        at: u64,
    },
    Receipt {
        request_id: String,
    },
    VerifyAcknowledgements {
        receipts: Vec<Value>,
    },
    Job {
        seq: u64,
    },
    Status,
    State {
        scope: u8,
        at: u64,
        cursor: Option<u64>,
        limit: usize,
    },
    Permission {
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
        scope: u8,
        at: u64,
        cursor: Option<u64>,
        limit: usize,
    },
    Revalidate {
        principal: String,
        scope: u8,
        read_seq: u64,
        min_commit_seq: u64,
        hits: Vec<Value>,
    },
}

pub(crate) struct Authority {
    pub tree: Tree,
    pub(crate) writer: Mutex<()>,
    pub(crate) derived: Mutex<()>,
    pub(crate) root: PathBuf,
    pub(crate) settings: Settings,
    pub(crate) search_session: std::sync::Arc<lancedb::Session>,
    documents: u64,
    #[cfg(test)]
    pub(crate) derived_barrier: Option<(
        std::sync::Arc<tokio::sync::Notify>,
        std::sync::Arc<tokio::sync::Notify>,
    )>,
    #[cfg(test)]
    history_barrier: Option<(
        std::sync::Arc<tokio::sync::Notify>,
        std::sync::Arc<tokio::sync::Notify>,
    )>,
}

fn number(value: Option<Vec<u8>>) -> Result<u64, Error> {
    let number = value
        .map(|v| Ok(u64::from_be_bytes(v.as_slice().try_into()?)))
        .unwrap_or(Ok::<_, Error>(0))?;
    if number > i64::MAX as u64 {
        return Err("invalid authority sequence".into());
    }
    Ok(number)
}
pub(crate) fn point(tx: &Transaction) -> Result<u64, Error> {
    number(tx.get(SEQ)?)
}
fn head(memory: u64) -> String {
    format!("head:{memory:020}")
}
fn revision(memory: u64, seq: u64) -> String {
    format!("revision:{memory:020}:{seq:020}")
}
fn job(seq: u64) -> String {
    format!("job:{seq:020}")
}
fn receipt_key(request: &str) -> Result<String, Error> {
    if request.is_empty() || request.len() > 128 {
        return Err("invalid request identity".into());
    }
    Ok(format!(
        "receipt:{}",
        load::hex(&Sha256::new_with_prefix(request.as_bytes()))
    ))
}
fn permission_key(principal: &str, scope: u8) -> Result<String, Error> {
    if scope > 7
        || principal.is_empty()
        || principal.len() > 64
        || !principal
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_".contains(&b))
    {
        return Err("invalid permission principal or scope".into());
    }
    Ok(format!("permission:{principal}:{scope}"))
}
fn permission(tx: &Transaction, principal: &str, scope: u8) -> Result<Permission, Error> {
    let value = tx.get(permission_key(principal, scope)?.as_bytes())?;
    let result: Permission = value
        .map(|v| serde_json::from_slice(&v))
        .transpose()?
        .unwrap_or(Permission {
            allowed: false,
            revision: 0,
        });
    if result.allowed && result.revision == 0 {
        return Err("invalid permission revision".into());
    }
    Ok(result)
}
fn validate_page(scope: u8, at: u64, cursor: Option<u64>, limit: usize) -> Result<(), Error> {
    if scope > 7
        || limit == 0
        || limit > 100
        || at > i64::MAX as u64
        || cursor.is_some_and(|v| v > i64::MAX as u64)
    {
        return Err("invalid history page".into());
    }
    Ok(())
}

impl Authority {
    pub fn open(root: &Path) -> Result<Self, Error> {
        let settings = Settings::read(root)?;
        let tree = open_authority(root)?;
        let imported = load::read_state(&tree.begin_with_mode(Mode::ReadOnly)?)?;
        if !imported.complete {
            return Err("corpus import is incomplete".into());
        }
        Ok(Self {
            tree,
            writer: Mutex::new(()),
            derived: Mutex::new(()),
            root: root.into(),
            settings,
            search_session: crate::search::session()?,
            documents: imported.documents,
            #[cfg(test)]
            history_barrier: None,
            #[cfg(test)]
            derived_barrier: None,
        })
    }

    pub(crate) fn get(
        &self,
        tx: &Transaction,
        memory: u64,
        at: u64,
    ) -> Result<Option<Entry>, Error> {
        if memory > i64::MAX as u64 || at > point(tx)? {
            return Err("historical read point is not committed".into());
        }
        let sequence = number(tx.get_at(head(memory).as_bytes(), at + 1)?)?;
        let mut revisions = tx.range(
            revision(memory, 0).into_bytes(),
            revision(memory, at + 1).into_bytes(),
        )?;
        revisions.seek_last()?;
        let latest = if revisions.valid() {
            std::str::from_utf8(revisions.key().user_key())?
                .rsplit(':')
                .next()
                .ok_or("invalid revision key")?
                .parse::<u64>()?
        } else {
            0
        };
        if sequence != latest {
            return Err("head and immutable revision disagree".into());
        }
        if sequence > 0 {
            if sequence > at {
                return Err("head points beyond read point".into());
            }
            let bytes = tx
                .get(revision(memory, sequence).as_bytes())?
                .ok_or("head revision missing")?;
            let entry: Entry = serde_json::from_slice(&load::decode_record(&bytes)?)?;
            if entry.memory != memory
                || entry.seq != sequence
                || entry.previous >= entry.seq
                || entry.scope > 7
                || entry.body.len() > 128 * 1024
                || entry
                    .source_key
                    .as_ref()
                    .is_some_and(|v| v.is_empty() || v.len() > 256)
            {
                return Err("invalid authoritative revision".into());
            }
            return Ok(Some(entry));
        }
        if !(load::FIRST_MEMORY..load::FIRST_MEMORY + self.documents).contains(&memory) {
            return Ok(None);
        }
        let bytes = tx
            .get(load::corpus_key(memory).as_bytes())?
            .ok_or("authoritative corpus record missing")?;
        let doc = load::document(&load::decode_record(&bytes)?)?;
        if tx.get(format!("source:{}", doc.key).as_bytes())?.as_deref()
            != Some(memory.to_be_bytes().as_slice())
        {
            return Err("authoritative source identity mismatch".into());
        }
        Ok(Some(Entry {
            memory,
            seq: 0,
            previous: 0,
            scope: doc.partition.as_bytes()[1] - b'0',
            active: true,
            body: doc.body,
            text_only: doc.embedding.is_none(),
            source_key: Some(doc.key),
        }))
    }

    pub(crate) fn job_entry(&self, tx: &Transaction, seq: u64) -> Result<Entry, Error> {
        if seq == 0 || seq > point(tx)? {
            return Err("invalid outbox sequence".into());
        }
        let value: Value = serde_json::from_slice(
            &tx.get(job(seq).as_bytes())?
                .ok_or("missing durable outbox job")?,
        )?;
        let memory = value["memory"].as_u64().ok_or("invalid outbox memory")?;
        let entry = self
            .get(tx, memory, seq)?
            .ok_or("missing immutable outbox revision")?;
        if entry.seq != seq
            || value["seq"] != seq
            || value["revision"] != format!("{memory:020}:{seq:020}")
        {
            return Err("outbox revision integrity mismatch".into());
        }
        Ok(entry)
    }

    pub(crate) fn stored_receipt(&self, request: &str) -> Result<Value, Error> {
        let raw = self
            .tree
            .begin_with_mode(Mode::ReadOnly)?
            .get(receipt_key(request)?.as_bytes())?
            .ok_or("unknown durable receipt")?;
        Ok(serde_json::from_slice::<Receipt>(&raw)?.result)
    }

    fn verify_acknowledgements(&self, receipts: &[Value]) -> Result<Value, Error> {
        if receipts.is_empty() || receipts.len() > 256 {
            return Err("acknowledgement count must be in 1..=256".into());
        }
        let tx = self.tree.begin_with_mode(Mode::ReadOnly)?;
        for receipt in receipts {
            let request = receipt["request_id"]
                .as_str()
                .ok_or("missing acknowledged request identity")?;
            let seq = receipt["commit_seq"]
                .as_u64()
                .ok_or("missing acknowledged commit sequence")?;
            let mut expected = receipt.clone();
            let oracle = expected
                .as_object_mut()
                .and_then(|object| object.remove("oracle"))
                .ok_or("missing acknowledgement content oracle")?;
            let stored: Receipt = serde_json::from_slice(
                &tx.get(receipt_key(request)?.as_bytes())?
                    .ok_or("unknown durable receipt")?,
            )?;
            if stored.result != expected {
                return Err("acknowledged receipt differs from durable receipt".into());
            }
            let entry = self.job_entry(&tx, seq)?;
            let hash = load::hex(&Sha256::new_with_prefix(entry.body.as_bytes()));
            if oracle
                != json!({"memory":entry.memory,"seq":entry.seq,"scope":entry.scope,
                "active":entry.active,"body_sha256":hash})
            {
                return Err("acknowledged content differs from immutable revision".into());
            }
            let change = Change {
                request_id: request.into(),
                memory: entry.memory,
                expected: entry.previous,
                scope: entry.scope,
                active: entry.active,
                body: entry.body,
                text_only: entry.text_only,
            };
            if stored.hash != load::hex(&Sha256::new_with_prefix(serde_json::to_vec(&change)?)) {
                return Err("acknowledged request hash differs from durable receipt".into());
            }
        }
        Ok(json!({"verified":receipts.len(),"commit_seq":point(&tx)?}))
    }

    pub async fn apply(&self, change: &Change, hold: Option<&str>) -> Result<Value, Error> {
        if change.request_id.is_empty()
            || change.request_id.len() > 128
            || change.body.len() > 128 * 1024
            || change.scope > 7
            || change.memory > i64::MAX as u64
            || change.expected > i64::MAX as u64
        {
            return Err("invalid change".into());
        }
        if hold.is_some_and(|h| !matches!(h, "before-commit" | "after-commit" | "after-reply")) {
            return Err("invalid authority checkpoint".into());
        }
        let hash = load::hex(&Sha256::new_with_prefix(serde_json::to_vec(change)?));
        let _writer = self.writer.lock().await;
        let mut tx = self.tree.begin()?;
        let key = receipt_key(&change.request_id)?;
        if let Some(bytes) = tx.get_for_update(key.as_bytes())? {
            let receipt: Receipt = serde_json::from_slice(&bytes)?;
            return Ok(if receipt.hash == hash {
                receipt.result
            } else {
                json!({"conflict":"idempotency"})
            });
        }
        let before = number(tx.get_for_update(SEQ)?)?;
        let current = self.get(&tx, change.memory, before)?;
        let previous = current.as_ref().map(|e| e.seq).unwrap_or(0);
        if previous != change.expected {
            return Ok(json!({"conflict":"expected_revision","actual":previous}));
        }
        let seq = before
            .checked_add(1)
            .filter(|s| *s <= i64::MAX as u64)
            .ok_or("commit sequence exhausted")?;
        let entry = Entry {
            memory: change.memory,
            seq,
            previous,
            scope: change.scope,
            active: change.active,
            body: change.body.clone(),
            text_only: change.text_only,
            source_key: current.as_ref().and_then(|e| e.source_key.clone()),
        };
        let order = format!("{:020}:{seq:020}", change.memory);
        let result = json!({"stage":"committed","request_id":change.request_id,"commit_seq":seq,"revision":order,"config_generation":1});
        let encoded = serde_json::to_vec(&entry)?;
        if encoded.len() > load::MAX_RECORD {
            return Err("authority record exceeds 512 KiB".into());
        }
        self.settings.check_disk(&self.root)?;
        tx.set_durability(Durability::Immediate);
        tx.set(
            revision(change.memory, seq).as_bytes(),
            zstd::bulk::compress(&encoded, 1)?,
        )?;
        tx.set_at(
            head(change.memory).as_bytes(),
            seq.to_be_bytes().as_slice(),
            seq + 1,
        )?;
        tx.set(
            key.as_bytes(),
            serde_json::to_vec(&Receipt {
                hash,
                result: result.clone(),
            })?,
        )?;
        tx.set(job(seq).as_bytes(),serde_json::to_vec(&json!({"seq":seq,"memory":change.memory,"revision":order,"config_generation":1,"status":"pending"}))?)?;
        tx.set(SEQ, seq.to_be_bytes().as_slice())?;
        if let Some(old) = current
            && old.scope != change.scope
        {
            tx.set_at(
                load::scope_key(&format!("p{}", old.scope), change.memory).as_bytes(),
                b"0",
                seq + 1,
            )?;
        }
        tx.set_at(
            load::scope_key(&format!("p{}", change.scope), change.memory).as_bytes(),
            if change.active { b"1" } else { b"0" },
            seq + 1,
        )?;
        checkpoint(hold, "before-commit").await;
        tx.commit().await?;
        checkpoint(hold, "after-commit").await;
        if hold == Some("after-reply") {
            println!("{result}");
            std::io::stdout().flush()?;
            checkpoint(hold, "after-reply").await;
        }
        Ok(result)
    }

    fn page(&self, scope: u8, at: u64, cursor: Option<u64>, limit: usize) -> Result<Value, Error> {
        validate_page(scope, at, cursor, limit)?;
        let tx = self.tree.begin_with_mode(Mode::ReadOnly)?;
        if at > point(&tx)? {
            return Err("historical read point is not committed".into());
        }
        let start = cursor.map(|v| v + 1).unwrap_or(0);
        let mut iter = tx.range(
            load::scope_key(&format!("p{scope}"), start).into_bytes(),
            format!("scope:p{scope};").into_bytes(),
        )?;
        iter.seek_first()?;
        let mut entries = Vec::new();
        while iter.valid() {
            let key = iter.key().user_key();
            let memory = std::str::from_utf8(key)?
                .rsplit(':')
                .next()
                .ok_or("invalid membership key")?
                .parse::<u64>()?;
            let member = tx.get_at(key, at + 1)?;
            if member.as_deref().is_some_and(|v| v != b"0" && v != b"1") {
                return Err("invalid historical membership".into());
            }
            if member.as_deref() == Some(b"1".as_slice()) {
                let entry = self
                    .get(&tx, memory, at)?
                    .ok_or("membership revision missing")?;
                if !entry.active || entry.scope != scope {
                    return Err("membership and revision disagree".into());
                }
                entries.push(entry);
                if entries.len() == limit {
                    return Ok(json!({"entries":entries,"next":memory,"at":at}));
                }
            }
            iter.next()?;
        }
        Ok(json!({"entries":entries,"next":null,"at":at}))
    }

    pub async fn current(&self, principal: &str, memory: u64) -> Result<Value, Error> {
        permission_key(principal, 0)?;
        let _writer = self.writer.lock().await;
        let tx = self.tree.begin_with_mode(Mode::ReadOnly)?;
        let at = point(&tx)?;
        let mut entry = self.get(&tx, memory, at)?.filter(|e| e.active);
        let mut revision = None;
        if let Some(value) = &entry {
            let permit = permission(&tx, principal, value.scope)?;
            revision = Some(permit.revision);
            if !permit.allowed {
                entry = None;
            }
        }
        Ok(json!({"at":at,"entry":entry,"permission_revision":revision}))
    }

    pub async fn set_permission(
        &self,
        principal: &str,
        scope: u8,
        allowed: bool,
    ) -> Result<Value, Error> {
        let key = permission_key(principal, scope)?;
        let _writer = self.writer.lock().await;
        let mut tx = self.tree.begin()?;
        tx.get_for_update(key.as_bytes())?;
        let prior = permission(&tx, principal, scope)?;
        if prior.revision > 0 && prior.allowed == allowed {
            return Ok(json!({"revision":prior.revision}));
        }
        let revision = prior
            .revision
            .checked_add(1)
            .ok_or("permission revision exhausted")?;
        self.settings.check_disk(&self.root)?;
        tx.set_durability(Durability::Immediate);
        tx.set(
            key.as_bytes(),
            serde_json::to_vec(&Permission { allowed, revision })?,
        )?;
        tx.commit().await?;
        Ok(json!({"revision":revision}))
    }

    pub async fn history(
        &self,
        principal: &str,
        scope: u8,
        at: u64,
        cursor: Option<u64>,
        limit: usize,
    ) -> Result<Value, Error> {
        validate_page(scope, at, cursor, limit)?;
        permission_key(principal, scope)?;
        {
            let tx = self.tree.begin_with_mode(Mode::ReadOnly)?;
            if at > point(&tx)? {
                return Err("historical read point is not committed".into());
            }
            let permit = permission(&tx, principal, scope)?;
            if !permit.allowed {
                return Ok(
                    json!({"entries":[],"next":null,"at":at,"permission_revision":permit.revision,"validated_at_seq":point(&tx)?,"retrieval_mode":"historical_state"}),
                );
            }
        }
        // Immutable history can scan without blocking writes; authorize again at publication.
        #[cfg(test)]
        if let Some((entered, resume)) = &self.history_barrier {
            entered.notify_one();
            resume.notified().await;
        }
        let mut page = self.page(scope, at, cursor, limit)?;
        let _writer = self.writer.lock().await;
        let tx = self.tree.begin_with_mode(Mode::ReadOnly)?;
        let permit = permission(&tx, principal, scope)?;
        if !permit.allowed {
            page = json!({"entries":[],"next":null,"at":at});
        }
        page["permission_revision"] = json!(permit.revision);
        page["validated_at_seq"] = json!(point(&tx)?);
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
        let tx = self.tree.begin_with_mode(Mode::ReadOnly)?;
        let permit = permission(&tx, principal, scope)?;
        let at = point(&tx)?;
        if at < read_seq || at < min_commit_seq {
            return Err("min_commit_seq_not_reached".into());
        }
        let mut visible = Vec::new();
        if permit.allowed {
            for hit in hits {
                let memory = hit["memory"].as_u64().ok_or("invalid candidate memory")?;
                let seq = hit["seq"].as_u64().ok_or("invalid candidate revision")?;
                if hit["partition"] != format!("p{scope}") {
                    return Err("candidate scope mismatch".into());
                }
                if let Some(entry) = self.get(&tx, memory, at)?
                    && entry.active
                    && entry.scope == scope
                    && entry.seq == seq
                {
                    visible.push(json!({"memory":entry.memory,"seq":entry.seq,
                        "partition":format!("p{}",entry.scope),"source_key":entry.source_key}));
                }
            }
        }
        Ok(
            json!({"hits":visible,"read_seq":read_seq,"validated_at_seq":at,"permission_revision":permit.revision,
            "retrieval_mode":"current_candidates","coverage":"candidate_set_only","filtered":hits.len()-visible.len()}),
        )
    }
}

pub async fn run(root: &Path, path: &Path) -> Result<Value, Error> {
    let mut bytes = Vec::new();
    File::open(path)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut bytes)?;
    if bytes.len() > 1024 * 1024 {
        return Err("authority request exceeds 1 MiB".into());
    }
    let request: Request = serde_json::from_slice(&bytes)?;
    let a = Authority::open(root)?;
    let result = match request {
        Request::Apply { change, hold } => a.apply(&change, hold.as_deref()).await?,
        Request::Get { memory, at } => {
            serde_json::to_value(a.get(&a.tree.begin_with_mode(Mode::ReadOnly)?, memory, at)?)?
        }
        Request::Receipt { request_id } => a
            .tree
            .begin_with_mode(Mode::ReadOnly)?
            .get(receipt_key(&request_id)?.as_bytes())?
            .map(|v| serde_json::from_slice::<Receipt>(&v).map(|v| v.result))
            .transpose()?
            .unwrap_or(Value::Null),
        Request::VerifyAcknowledgements { receipts } => a.verify_acknowledgements(&receipts)?,
        Request::Job { seq } => a
            .tree
            .begin_with_mode(Mode::ReadOnly)?
            .get(job(seq).as_bytes())?
            .map(|v| serde_json::from_slice::<Value>(&v))
            .transpose()?
            .unwrap_or(Value::Null),
        Request::Status => json!({"commit_seq":point(&a.tree.begin_with_mode(Mode::ReadOnly)?)?}),
        Request::State {
            scope,
            at,
            cursor,
            limit,
        } => a.page(scope, at, cursor, limit)?,
        Request::Permission {
            principal,
            scope,
            allowed,
        } => a.set_permission(&principal, scope, allowed).await?,
        Request::Current { principal, memory } => a.current(&principal, memory).await?,
        Request::History {
            principal,
            scope,
            at,
            cursor,
            limit,
        } => a.history(&principal, scope, at, cursor, limit).await?,
        Request::Revalidate {
            principal,
            scope,
            read_seq,
            min_commit_seq,
            hits,
        } => {
            a.revalidate(&principal, scope, read_seq, min_commit_seq, &hits)
                .await?
        }
    };
    a.tree.close().await?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::{
        sync::Arc,
        time::{Duration, SystemTime, UNIX_EPOCH},
    };
    use tokio::sync::Notify;

    pub(super) fn fixture() -> Result<(PathBuf, Authority), Error> {
        let path = std::env::temp_dir().join(format!(
            "akasha-authority-unit-{}-{}",
            std::process::id(),
            SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos()
        ));
        std::fs::create_dir(&path)?;
        let a = Authority {
            tree: open_authority(&path)?,
            writer: Mutex::new(()),
            derived: Mutex::new(()),
            root: path.clone(),
            settings: Settings::read(&path)?,
            search_session: crate::search::session()?,
            documents: 0,
            history_barrier: None,
            #[cfg(test)]
            derived_barrier: None,
        };
        Ok((path, a))
    }

    fn change() -> Change {
        Change {
            request_id: "unit".into(),
            memory: 42,
            expected: 0,
            scope: 0,
            active: true,
            body: "immutable".into(),
            text_only: true,
        }
    }

    #[tokio::test]
    async fn acknowledged_receipts_retain_exact_historical_content() -> Result<(), Error> {
        let (path, a) = fixture()?;
        let mut change = change();
        let mut receipts = Vec::new();
        for seq in 1..=2 {
            change.request_id = format!("ack-{seq}");
            change.expected = seq - 1;
            change.body = format!("revision {seq}");
            change.scope = seq as u8;
            change.active = seq == 1;
            let mut receipt = a.apply(&change, None).await?;
            receipt["oracle"] = json!({"memory":42,"seq":seq,"scope":change.scope,
                "active":change.active,"body_sha256":load::hex(&Sha256::new_with_prefix(change.body.as_bytes()))});
            receipts.push(receipt);
        }
        let outcome = async {
            assert_eq!(
                a.verify_acknowledgements(&receipts)?,
                json!({"verified":2,"commit_seq":2})
            );
            for field in ["memory", "seq", "scope", "active", "body_sha256"] {
                let mut tampered = receipts.clone();
                tampered[0]["oracle"][field] = Value::Null;
                assert!(a.verify_acknowledgements(&tampered).is_err(), "{field}");
            }
            for field in [
                "request_id",
                "commit_seq",
                "revision",
                "config_generation",
                "stage",
                "oracle",
            ] {
                let mut tampered = receipts.clone();
                tampered[0][field] = Value::Null;
                assert!(a.verify_acknowledgements(&tampered).is_err(), "{field}");
            }
            let mut unknown = receipts[0].clone();
            unknown["request_id"] = json!("unknown");
            assert!(a.verify_acknowledgements(&[unknown]).is_err());
            assert!(a.verify_acknowledgements(&[]).is_err());
            assert!(
                a.verify_acknowledgements(&vec![receipts[0].clone(); 257])
                    .is_err()
            );
            let key = receipt_key("ack-1")?;
            let raw = a.tree.begin()?.get(key.as_bytes())?.unwrap();
            let mut corrupt: Receipt = serde_json::from_slice(&raw)?;
            corrupt.hash = "0".repeat(64);
            let mut tx = a.tree.begin()?;
            tx.set_durability(Durability::Immediate);
            tx.set(key.as_bytes(), serde_json::to_vec(&corrupt)?)?;
            tx.commit().await?;
            assert!(a.verify_acknowledgements(&receipts).is_err());
            let mut tx = a.tree.begin()?;
            tx.set_durability(Durability::Immediate);
            tx.set(key.as_bytes(), raw)?;
            tx.commit().await?;
            assert_eq!(a.verify_acknowledgements(&receipts)?["verified"], 2);
            assert_eq!(point(&a.tree.begin_with_mode(Mode::ReadOnly)?)?, 2);
            Ok::<_, Error>(())
        }
        .await;
        a.tree.close().await?;
        std::fs::remove_dir_all(path)?;
        outcome
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn missing_head_cannot_hide_an_immutable_revision() -> Result<(), Error> {
        let (path, a) = fixture()?;
        let outcome = async {
            a.apply(&change(), None).await?;
            let mut tx = a.tree.begin()?;
            tx.set_durability(Durability::Immediate);
            tx.set_at(head(42).as_bytes(), 0u64.to_be_bytes().as_slice(), 2)?;
            tx.commit().await?;
            let tx = a.tree.begin_with_mode(Mode::ReadOnly)?;
            let bad_head = a.get(&tx, 42, 1).err().is_some_and(|e| {
                e.to_string()
                    .contains("head and immutable revision disagree")
            });
            drop(tx);
            let mut tx = a.tree.begin()?;
            tx.set_durability(Durability::Immediate);
            tx.set(SEQ, u64::MAX.to_be_bytes().as_slice())?;
            tx.commit().await?;
            let bad_sequence = a
                .current("reader", 42)
                .await
                .err()
                .is_some_and(|e| e.to_string().contains("invalid authority sequence"));
            Ok::<_, Error>(bad_head && bad_sequence)
        }
        .await;
        a.tree.close().await?;
        std::fs::remove_dir_all(path)?;
        assert!(outcome?);
        Ok(())
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn paused_history_releases_writer_and_rechecks_revocation() -> Result<(), Error> {
        let (path, mut a) = fixture()?;
        a.apply(&change(), None).await?;
        a.set_permission("reader", 0, true).await?;
        let entered = Arc::new(Notify::new());
        let resume = Arc::new(Notify::new());
        a.history_barrier = Some((entered.clone(), resume.clone()));
        let a = Arc::new(a);
        let reader = a.clone();
        let task = tokio::spawn(async move { reader.history("reader", 0, 1, None, 100).await });
        let reached = tokio::time::timeout(Duration::from_secs(3), entered.notified()).await;
        let revoked =
            tokio::time::timeout(Duration::from_secs(3), a.set_permission("reader", 0, false))
                .await;
        resume.notify_one();
        let page = task.await;
        let a = Arc::try_unwrap(a).map_err(|_| "authority reader retained ownership")?;
        a.tree.close().await?;
        std::fs::remove_dir_all(path)?;
        reached?;
        revoked??;
        let page = page??;
        assert_eq!(page["entries"], json!([]));
        assert_eq!(page["permission_revision"], 2);
        assert_eq!(page["validated_at_seq"], 1);
        Ok(())
    }
}
