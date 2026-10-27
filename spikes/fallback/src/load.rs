use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Write},
    os::unix::fs::DirBuilderExt,
    path::Path,
    time::Instant,
};

use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use surrealkv::{Durability, LSMIterator, Transaction};

use crate::{Error, OWNER, open_authority};

const STATE: &[u8] = b"meta:import";
pub(crate) const MANIFEST: &[u8] = b"meta:import-manifest";
pub(crate) const FIRST_MEMORY: u64 = 1_000_000;
pub(crate) const MAX_RECORD: usize = 512 * 1024;
const STORAGE_FORMAT: u32 = 2;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct Document {
    pub key: String,
    pub body: String,
    pub partition: String,
    pub embedding: Option<Vec<f32>>,
}

#[derive(Deserialize)]
struct Manifest {
    corpus: Expected,
}

#[derive(Deserialize)]
struct Expected {
    documents: u64,
    vectors: u64,
    prepared_bytes: u64,
    prepared_sha256: String,
    indexed_body_utf8_bytes: u64,
}

#[derive(Clone, Default, Deserialize, Serialize, PartialEq)]
#[serde(deny_unknown_fields)]
pub(crate) struct State {
    #[serde(default)]
    storage_format: u32,
    pub manifest_sha256: String,
    pub documents: u64,
    content_bytes: u64,
    input_bytes: u64,
    pub vectors: u64,
    prefix: String,
    pub complete: bool,
}

pub(crate) fn hex(digest: &Sha256) -> String {
    let mut value = String::with_capacity(64);
    for byte in digest.clone().finalize() {
        value.push(b"0123456789abcdef"[(byte >> 4) as usize] as char);
        value.push(b"0123456789abcdef"[(byte & 15) as usize] as char);
    }
    value
}

pub(crate) fn document(raw: &[u8]) -> Result<Document, Error> {
    if raw.len() > MAX_RECORD {
        return Err("prepared record exceeds 512 KiB".into());
    }
    let doc: Document = serde_json::from_slice(raw)?;
    if doc.key.is_empty()
        || doc.key.len() > 256
        || doc.body.len() > 128 * 1024
        || !matches!(
            doc.partition.as_str(),
            "p0" | "p1" | "p2" | "p3" | "p4" | "p5" | "p6" | "p7"
        )
    {
        return Err("invalid prepared document".into());
    }
    if let Some(vector) = &doc.embedding
        && (vector.len() != 1536
            || vector.iter().any(|v| !v.is_finite())
            || vector.iter().all(|v| *v == 0.0))
    {
        return Err("invalid embedding".into());
    }
    Ok(doc)
}

fn advance(state: &mut State, digest: &mut Sha256, raw: &[u8], doc: &Document) {
    state.documents += 1;
    state.content_bytes += doc.body.len() as u64;
    state.input_bytes += raw.len() as u64;
    state.vectors += u64::from(doc.embedding.is_some());
    digest.update(raw);
    state.prefix = hex(digest);
}

pub(crate) fn corpus_key(memory: u64) -> String {
    format!("corpus:{memory:020}")
}

pub(crate) fn scope_key(partition: &str, memory: u64) -> String {
    format!("scope:{partition}:{memory:020}")
}

pub(crate) fn decode_record(raw: &[u8]) -> Result<Vec<u8>, Error> {
    Ok(zstd::bulk::decompress(raw, MAX_RECORD)?)
}

pub(crate) fn read_state(tx: &Transaction) -> Result<State, Error> {
    let state: State = serde_json::from_slice(&tx.get(STATE)?.ok_or("missing import state")?)?;
    if state.storage_format != STORAGE_FORMAT {
        return Err("unsupported corpus storage format; explicit reimport required".into());
    }
    Ok(state)
}

pub(crate) fn manifest(tx: &Transaction) -> Result<Vec<u8>, Error> {
    let bytes = tx
        .get(MANIFEST)?
        .ok_or("missing persisted corpus manifest; bind the original manifest with begin")?;
    if bytes.len() > 1024 * 1024
        || hex(&Sha256::new_with_prefix(&bytes)) != read_state(tx)?.manifest_sha256
    {
        return Err("persisted corpus manifest mismatch".into());
    }
    Ok(bytes)
}

pub(crate) fn atomic_json(path: &Path, value: &Value) -> Result<(), Error> {
    let temp = path.with_extension("json.tmp");
    let mut file = File::create(&temp)?;
    writeln!(file, "{value}")?;
    file.sync_all()?;
    std::fs::rename(&temp, path)?;
    File::open(path.parent().ok_or("missing parent")?)?.sync_all()?;
    Ok(())
}

pub async fn run(root: &Path, input: &Path, manifest: &Path, resume: bool) -> Result<Value, Error> {
    let mut manifest_bytes = Vec::new();
    File::open(manifest)?
        .take(1024 * 1024 + 1)
        .read_to_end(&mut manifest_bytes)?;
    if manifest_bytes.len() > 1024 * 1024 {
        return Err("manifest exceeds 1 MiB".into());
    }
    let expected = serde_json::from_slice::<Manifest>(&manifest_bytes)?.corpus;
    if expected.documents == 0
        || expected.documents > 100_000_000
        || expected.vectors > expected.documents
        || expected.prepared_sha256.len() != 64
        || !expected
            .prepared_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    {
        return Err("invalid corpus manifest".into());
    }
    let manifest_hash = hex(&Sha256::new_with_prefix(&manifest_bytes));
    let hold = std::env::var("AKASHA_SPIKE_IMPORT_HOLD_AFTER")
        .ok()
        .map(|s| s.parse::<u64>())
        .transpose()?;
    if hold.is_some_and(|n| n == 0 || !n.is_multiple_of(128)) {
        return Err("import checkpoint must be a positive multiple of 128".into());
    }
    // The shared stream controller owns the parent before any native database effects.
    if std::fs::read_to_string(root.parent().ok_or("missing root parent")?.join("OWNER"))?
        != "akasha-otsuge MVP-003 owned corpus import and database\n"
    {
        return Err("foreign import parent".into());
    }
    if resume {
        if std::fs::read_to_string(root.join("OWNER"))? != OWNER {
            return Err("foreign database".into());
        }
    } else {
        if root.exists() {
            return Err("database already exists; explicit resume required".into());
        }
        std::fs::DirBuilder::new().mode(0o700).create(root)?;
        let mut marker = File::create(root.join("OWNER"))?;
        marker.write_all(OWNER.as_bytes())?;
        marker.sync_all()?;
        File::open(root)?.sync_all()?;
    }
    let tree = open_authority(root)?;
    let empty = State {
        storage_format: STORAGE_FORMAT,
        manifest_sha256: manifest_hash.clone(),
        prefix: hex(&Sha256::new()),
        ..Default::default()
    };
    if !resume {
        let mut tx = tree.begin()?;
        tx.set_durability(Durability::Immediate);
        tx.set(STATE, serde_json::to_vec(&empty)?)?;
        tx.set(MANIFEST, &manifest_bytes)?;
        tx.commit().await?;
    }
    let stored = read_state(&tree.begin()?)?;
    if stored.manifest_sha256 != manifest_hash || stored.documents > expected.documents {
        return Err("stored manifest does not match resume request".into());
    }
    let started = Instant::now();
    let stat = std::fs::read_to_string("/proc/self/stat")?;
    let start_ticks = stat
        .rsplit_once(')')
        .ok_or("invalid process identity")?
        .1
        .split_whitespace()
        .nth(19)
        .ok_or("missing process start")?;
    let checkpoint = |phase: &str, state: &State| -> Result<(), Error> {
        let elapsed = started.elapsed().as_secs_f64();
        let verifying = phase == "verify_prefix";
        let done = if verifying {
            state.documents
        } else {
            state.documents.saturating_sub(stored.documents)
        };
        let total = if verifying {
            stored.documents
        } else {
            expected.documents
        };
        atomic_json(
            &root.join("import-progress.json"),
            &json!({"phase":phase,"completed":state.documents,
            "total":total,"elapsed_s":elapsed,"rate_per_s":done as f64/elapsed,
            "eta_s":if phase=="import_complete" {Some(0.0)} else if done>0 {Some((total-state.documents) as f64*elapsed/done as f64)} else {None},
            "pid":std::process::id(),"start_ticks":start_ticks,"manifest_sha256":manifest_hash,
            "durable_documents":if verifying {stored.documents} else {state.documents},"content_bytes":state.content_bytes}),
        )
    };
    let mut state = empty;
    let mut digest = Sha256::new();
    checkpoint(
        if stored.documents > 0 {
            "verify_prefix"
        } else {
            "import"
        },
        &state,
    )?;
    let mut reader = BufReader::new(File::open(input)?);
    let mut tx = tree.begin()?;
    tx.set_durability(Durability::Immediate);
    let mut pending = 0;
    let mut compressor = zstd::bulk::Compressor::new(1)?;
    loop {
        let mut raw = Vec::new();
        let bytes = (&mut reader)
            .take((MAX_RECORD + 1) as u64)
            .read_until(b'\n', &mut raw)?;
        if bytes == 0 {
            break;
        }
        let doc = document(&raw)?;
        advance(&mut state, &mut digest, &raw, &doc);
        if state.documents > expected.documents {
            return Err("input exceeds manifest document count".into());
        }
        if state.documents <= stored.documents {
            if state.documents == stored.documents
                && (state.content_bytes != stored.content_bytes
                    || state.input_bytes != stored.input_bytes
                    || state.vectors != stored.vectors
                    || state.prefix != stored.prefix)
            {
                return Err(
                    "input differs from the committed prefix; no new records imported".into(),
                );
            }
            if state.documents.is_multiple_of(1024) || state.documents == stored.documents {
                checkpoint("verify_prefix", &state)?;
            }
            continue;
        }
        let memory = FIRST_MEMORY + state.documents - 1;
        let source_key = format!("source:{}", doc.key);
        if tx.get_for_update(source_key.as_bytes())?.is_some() {
            return Err("duplicate source key".into());
        }
        tx.set(source_key.as_bytes(), &memory.to_be_bytes())?;
        tx.set_at(scope_key(&doc.partition, memory).as_bytes(), b"1", 1)?;
        tx.set_at(corpus_key(memory).as_bytes(), compressor.compress(&raw)?, 1)?;
        pending += bytes;
        if state.documents.is_multiple_of(128) || pending >= 4 * 1024 * 1024 {
            tx.set(STATE, serde_json::to_vec(&state)?)?;
            tx.commit().await?;
            if state.documents.is_multiple_of(1024) || hold == Some(state.documents) {
                checkpoint("import", &state)?;
            }
            if hold == Some(state.documents) {
                eprintln!("IMPORT_CHECKPOINT:{}", state.documents);
                std::future::pending::<()>().await;
            }
            tx = tree.begin()?;
            tx.set_durability(Durability::Immediate);
            pending = 0;
        }
    }
    if state.documents < stored.documents {
        return Err("input ended before committed prefix".into());
    }
    if state.documents != expected.documents
        || state.content_bytes != expected.indexed_body_utf8_bytes
        || state.input_bytes != expected.prepared_bytes
        || state.vectors != expected.vectors
        || state.prefix != expected.prepared_sha256
    {
        return Err("complete input does not match manifest; import is not accepted".into());
    }
    state.complete = true;
    if !stored.complete {
        tx.set(STATE, serde_json::to_vec(&state)?)?;
        tx.commit().await?;
    }
    drop(tx);
    tree.close().await?;
    checkpoint("import_complete", &state)?;
    Ok(serde_json::to_value(state)?)
}

pub async fn inspect(root: &Path, operation: &str, memory: Option<u64>) -> Result<Value, Error> {
    let tree = open_authority(root)?;
    let result = {
        let tx = tree.begin()?;
        let expected = read_state(&tx)?;
        if operation == "corpus-get" {
            let id = memory.ok_or("missing corpus ID")?;
            if id < FIRST_MEMORY {
                return Err("invalid corpus ID".into());
            }
            tx.get(corpus_key(id).as_bytes())?
                .map(|v| -> Result<Value, Error> {
                    Ok(serde_json::from_slice(&decode_record(&v)?)?)
                })
                .transpose()?
                .unwrap_or(Value::Null)
        } else {
            if operation == "verify-import" {
                let mut iter = tx.range(b"corpus:".to_vec(), b"corpus;".to_vec())?;
                iter.seek_first()?;
                let mut state = State {
                    storage_format: STORAGE_FORMAT,
                    manifest_sha256: expected.manifest_sha256.clone(),
                    prefix: hex(&Sha256::new()),
                    ..Default::default()
                };
                let mut digest = Sha256::new();
                let started = Instant::now();
                let stat = std::fs::read_to_string("/proc/self/stat")?;
                let start_ticks = stat
                    .rsplit_once(')')
                    .ok_or("invalid process identity")?
                    .1
                    .split_whitespace()
                    .nth(19);
                let checkpoint = |state: &State, complete: bool| -> Result<(), Error> {
                    let elapsed = started.elapsed().as_secs_f64();
                    atomic_json(
                        &root.join("verify-import-progress.json"),
                        &json!({
                            "phase":if complete {"verified"} else {"verify_import"},
                            "completed":state.documents,"total":expected.documents,"elapsed_s":elapsed,
                            "rate_per_s":state.documents as f64/elapsed,
                            "eta_s":if complete {Some(0.0)} else if state.documents>0 {
                                Some(expected.documents.saturating_sub(state.documents) as f64*elapsed/state.documents as f64)
                            } else {None}, "pid":std::process::id(),"start_ticks":start_ticks
                        }),
                    )
                };
                checkpoint(&state, false)?;
                while iter.valid() {
                    if iter.key().user_key()
                        != corpus_key(FIRST_MEMORY + state.documents).as_bytes()
                    {
                        return Err("noncontiguous stored corpus".into());
                    }
                    let memory = FIRST_MEMORY + state.documents;
                    let raw = decode_record(&iter.value()?)?;
                    let doc = document(&raw)?;
                    if tx.get(format!("source:{}", doc.key).as_bytes())?.as_deref()
                        != Some(memory.to_be_bytes().as_slice())
                        || tx
                            .get_at(scope_key(&doc.partition, memory).as_bytes(), 1)?
                            .as_deref()
                            != Some(b"1".as_slice())
                    {
                        return Err("corpus source or initial scope index mismatch".into());
                    }
                    advance(&mut state, &mut digest, &raw, &doc);
                    iter.next()?;
                    if state.documents.is_multiple_of(1024) {
                        checkpoint(&state, false)?;
                    }
                }
                state.complete = expected.complete;
                if state != expected {
                    return Err("stored corpus differs from committed checkpoint".into());
                }
                checkpoint(&state, true)?;
            }
            serde_json::to_value(expected)?
        }
    };
    tree.close().await?;
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compressed_records_reject_oversized_frames_and_legacy_bytes() {
        let original = b"{\"key\":\"test\"}\n";
        let encoded = zstd::bulk::compress(original, 1).unwrap();
        assert_eq!(decode_record(&encoded).unwrap(), original);
        assert!(decode_record(original).is_err());
        assert!(decode_record(&encoded[..encoded.len() - 1]).is_err());
        let oversized = zstd::bulk::compress(&vec![b'x'; MAX_RECORD + 1], 1).unwrap();
        assert!(decode_record(&oversized).is_err());
    }
}
