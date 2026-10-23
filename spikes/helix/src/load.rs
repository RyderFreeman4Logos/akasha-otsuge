use std::{
    fs::File,
    io::{BufRead, BufReader, Read, Write},
    path::PathBuf,
    time::Instant,
};

use crate::engine::{Engine, Error};
use helix_db::{HelixDbSource, prelude::*};
use serde::Deserialize;
use serde_json::json;
use sha2::{Digest, Sha256};

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Document {
    key: String,
    body: String,
    partition: String,
    embedding: Option<Vec<f32>>,
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

fn hex(digest: &Sha256) -> String {
    digest
        .clone()
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

#[derive(Default)]
struct Progress {
    documents: u64,
    bytes: u64,
    input_bytes: u64,
    vectors: u64,
    digest: Sha256,
}

pub async fn run(
    root: PathBuf,
    input: PathBuf,
    manifest: Option<PathBuf>,
    resume: bool,
) -> Result<(), Error> {
    let hold_after: Option<u64> = std::env::var("AKASHA_SPIKE_IMPORT_HOLD_AFTER")
        .ok()
        .map(|value| value.parse())
        .transpose()?;
    if hold_after.is_some_and(|count| count == 0 || !count.is_multiple_of(16)) {
        return Err("import test checkpoint must be a positive multiple of 16".into());
    }
    let expected = if let Some(path) = manifest {
        let mut bytes = Vec::new();
        File::open(path)?
            .take(1024 * 1024 + 1)
            .read_to_end(&mut bytes)?;
        if bytes.len() > 1024 * 1024 {
            return Err("manifest exceeds 1 MiB".into());
        }
        let expected = serde_json::from_slice::<Manifest>(&bytes)?.corpus;
        if expected.documents == 0
            || expected.documents > 100_000_000
            || expected.prepared_sha256.len() != 64
            || !expected
                .prepared_sha256
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err("invalid corpus manifest".into());
        }
        let mut digest = Sha256::new();
        digest.update(bytes);
        Some((expected, hex(&digest)))
    } else {
        None
    };
    if resume && expected.is_none() {
        return Err("resume requires a pinned manifest".into());
    }
    if !resume {
        std::fs::create_dir(&root)?;
    } else if !root.is_dir() {
        return Err("resume requires an existing database".into());
    }
    let client = Engine::open_with_config(
        HelixDbSource::Disk {
            root: root.clone(),
            database: "akasha-storage-spike".into(),
        },
        super::config()?,
    )
    .await?;
    let stored = if let Some((_, manifest_hash)) = &expected {
        if !resume {
            client.cypher("CREATE (:ImportState {manifest:$manifest,documents:0,content_bytes:0,input_bytes:0,vectors:0,prefix:$empty,complete:false})",
                [("manifest".into(),json!(manifest_hash)),("empty".into(),json!(hex(&Sha256::new())))].into(),None).await?;
        }
        let rows=client.cypher("MATCH (s:ImportState) RETURN s.manifest,s.documents,s.content_bytes,s.input_bytes,s.vectors,s.prefix",Default::default(),None).await?.rows;
        if rows.len() != 1 || rows[0].len() != 6 || rows[0].first() != Some(&json!(manifest_hash)) {
            return Err("stored manifest does not match resume request".into());
        }
        Some(rows.into_iter().next().ok_or("missing import state")?)
    } else {
        None
    };
    let completed = stored
        .as_ref()
        .map(|row| row[1].as_u64().ok_or("invalid stored import position"))
        .transpose()?
        .unwrap_or(0);
    let start = Instant::now();
    let process_stat = std::fs::read_to_string("/proc/self/stat")?;
    let start_ticks = process_stat
        .rsplit_once(')')
        .and_then(|(_, fields)| fields.split_whitespace().nth(19))
        .ok_or("invalid process identity")?;
    let checkpoint = |phase: &str, progress: &Progress| -> Result<(), Error> {
        let elapsed = start.elapsed().as_secs_f64();
        let total = expected.as_ref().map(|(expected, _)| expected.documents);
        let completed_now = progress.documents - completed;
        let receipt = json!({"phase":phase,"completed":progress.documents,"total":total,
        "elapsed_s":elapsed,"rate_per_s":completed_now as f64/elapsed,
        "eta_s":if phase=="import_complete" {Some(0.0)} else {total.filter(|_| completed_now>0).map(|total|(total-progress.documents) as f64*elapsed/completed_now as f64)},
        "pid":std::process::id(),"start_ticks":start_ticks,
        "manifest_sha256":expected.as_ref().map(|(_,hash)|hash),"content_bytes":progress.bytes});
        let temporary = root.join("import-progress.json.tmp");
        let mut file = File::create(&temporary)?;
        writeln!(file, "{receipt}")?;
        file.sync_all()?;
        std::fs::rename(temporary, root.join("import-progress.json"))?;
        File::open(&root)?.sync_all()?;
        Ok(())
    };
    let mut source = BufReader::new(File::open(input)?);
    let mut progress = Progress::default();
    let mut batch = write_batch();
    loop {
        let mut line = String::new();
        let size = (&mut source).take(512 * 1024 + 1).read_line(&mut line)?;
        if size == 0 {
            break;
        }
        if size > 512 * 1024 {
            return Err("prepared record exceeds 512 KiB".into());
        }
        let doc: Document = serde_json::from_str(&line)?;
        if doc.key.is_empty()
            || doc.key.len() > 256
            || doc.body.len() > 128 * 1024
            || !["p0", "p1", "p2", "p3", "p4", "p5", "p6", "p7"].contains(&doc.partition.as_str())
        {
            return Err("invalid prepared document".into());
        }
        let body_bytes = doc.body.len() as u64;
        if let Some(vector) = &doc.embedding {
            if vector.len() != 1536
                || vector.iter().any(|v| !v.is_finite())
                || vector.iter().all(|v| *v == 0.0)
            {
                return Err("invalid 1536-dimensional embedding".into());
            }
            progress.vectors += 1;
        }
        progress.documents += 1;
        progress.bytes += body_bytes;
        progress.input_bytes += size as u64;
        progress.digest.update(line.as_bytes());
        if expected
            .as_ref()
            .is_some_and(|(expected, _)| progress.documents > expected.documents)
        {
            return Err("input exceeds manifest document count".into());
        }
        if progress.documents <= completed {
            if progress.documents == completed {
                let row = stored.as_ref().ok_or("missing resume checkpoint")?;
                if row[2] != progress.bytes
                    || row[3] != progress.input_bytes
                    || row[4] != progress.vectors
                    || row[5] != hex(&progress.digest)
                {
                    return Err(
                        "input differs from the committed prefix; no new records imported".into(),
                    );
                }
            }
            continue;
        }
        let mut properties = vec![
            ("key", PropertyValue::from(doc.key)),
            ("body", PropertyValue::from(doc.body)),
            ("partition", PropertyValue::from(doc.partition)),
            // Corpus IDs do not overlap the synthetic historical oracle's 0..1024 IDs.
            (
                "memory",
                PropertyValue::from(i64::try_from(1_000_000 + progress.documents - 1)?),
            ),
        ];
        if let Some(vector) = doc.embedding {
            properties.push(("embedding", PropertyValue::from(vector)));
        }
        batch = batch.var_as(
            &format!("doc{}", progress.documents % 16),
            g().add_n("Document", properties),
        );
        if progress.documents.is_multiple_of(16) {
            commit(
                &client,
                std::mem::take(&mut batch),
                &progress,
                expected.is_some(),
            )
            .await?;
            if hold_after == Some(progress.documents) {
                eprintln!("IMPORT_CHECKPOINT:{}", progress.documents);
                std::future::pending::<()>().await;
            }
        }
        if progress.documents.is_multiple_of(1024) {
            checkpoint("import", &progress)?;
            eprintln!(
                "{}",
                json!({"phase":"import","documents":progress.documents,"content_bytes":progress.bytes,"elapsed_seconds":start.elapsed().as_secs_f64()})
            );
        }
    }
    if progress.documents < completed {
        return Err("input ended before committed prefix".into());
    }
    if let Some((expected, _)) = &expected
        && (progress.documents != expected.documents
            || progress.bytes != expected.indexed_body_utf8_bytes
            || progress.vectors != expected.vectors
            || progress.input_bytes != expected.prepared_bytes
            || hex(&progress.digest) != expected.prepared_sha256)
    {
        return Err("complete input does not match manifest; import is not accepted".into());
    }
    if !batch.entries.is_empty() {
        commit(&client, batch, &progress, expected.is_some()).await?;
    }
    if expected.is_some() {
        client
            .query(QueryRequest::write(
                write_batch().var_as(
                    "complete",
                    g().n_with_label("ImportState")
                        .set_property("complete", true),
                ),
            ))
            .await?;
    }
    client.close().await?;
    checkpoint("import_complete", &progress)?;
    println!(
        "{}",
        json!({"phase":"import_complete","documents":progress.documents,"content_bytes":progress.bytes,"prepared_sha256":hex(&progress.digest),"resumed_documents":completed,"elapsed_seconds":start.elapsed().as_secs_f64()})
    );
    std::io::stdout().flush()?;
    Ok(())
}

async fn commit(
    client: &Engine,
    mut batch: WriteBatch,
    progress: &Progress,
    pinned: bool,
) -> Result<(), Error> {
    if pinned {
        batch = batch.var_as(
            "checkpoint",
            g().n_with_label("ImportState")
                .set_property("documents", i64::try_from(progress.documents)?)
                .set_property("content_bytes", i64::try_from(progress.bytes)?)
                .set_property("input_bytes", i64::try_from(progress.input_bytes)?)
                .set_property("vectors", i64::try_from(progress.vectors)?)
                .set_property("prefix", hex(&progress.digest)),
        );
    }
    client.query(QueryRequest::write(batch)).await?;
    Ok(())
}
