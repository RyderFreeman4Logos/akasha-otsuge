//! Offline byte oracle for the benchmark's explicit capture interchange, not host authentication.
use akasha_otsuge::capture::{
    Capture, CaptureBinding, CoverageLevel, EventRecord, EventRef, SourceIdentity,
};
use akasha_otsuge::config::{Paths, resolve};
use akasha_otsuge::domain::Scope;
use akasha_otsuge::storage::{RecordKind, Storage};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use std::io::{BufRead, Read, Write};
use std::path::Path;
use std::process::ExitCode;

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Expected {
    source: SourceIdentity,
    scope: Scope,
    stream: String,
    event_id: String,
    raw_sha256: String,
    text_sha256: String,
}

fn hash(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

async fn verify(data: &Path, mut input: impl BufRead, mut output: impl Write) -> Result<u64> {
    let config = resolve(
        "",
        None,
        "",
        &Paths {
            data_dir: data.into(),
            socket_path: data.join("unused-capture-oracle.sock"),
        },
    )
    .map_err(|e| e.code)?;
    // The native store lock excludes a daemon or another authority owner.
    let store = Storage::open(&config).await.map_err(|e| e.code)?;
    let outcome = async {
        let at = store.sequence().map_err(|e| e.code)?;
        let capture = Capture::new(&store);
        let mut seen = BTreeSet::new();
        let mut line = Vec::new();
        let mut count = 0;
        loop {
            line.clear();
            (&mut input).take(16 * 1024 + 1).read_until(b'\n', &mut line)
                .map_err(|_| "ORACLE_INPUT_IO")?;
            if line.is_empty() { break; }
            if line.len() > 16 * 1024 || line.last() != Some(&b'\n') || count >= 100_000 {
                return Err("INVALID_ORACLE_FRAME");
            }
            let row: Expected = serde_json::from_slice(&line).map_err(|_| "INVALID_ORACLE_FRAME")?;
            if [&row.raw_sha256, &row.text_sha256].iter()
                .any(|s| s.len() != 64 || !s.bytes().all(|b| b.is_ascii_hexdigit())) {
                return Err("INVALID_ORACLE_DIGEST");
            }
            let reference = EventRef { source: row.source.clone(), event_id: row.event_id };
            let identity = hash(&serde_json::to_vec(&reference).map_err(|_| "INVALID_ORACLE_FRAME")?);
            if !seen.insert(identity.clone()) { return Err("DUPLICATE_ORACLE_EVENT"); }
            let binding = CaptureBinding::from_source(row.source, row.scope.clone(), &row.stream, CoverageLevel::AvailablePayloads)
                .map_err(|e| e.code)?;
            let event = capture.event(&binding, &reference, at).map_err(|e| e.code)?
                .ok_or("CAPTURE_EVENT_MISSING")?;
            if event.event.scope != row.scope || event.event.coverage != CoverageLevel::AvailablePayloads
                || event.event.raw_bytes != 4096 || event.event.text_bytes != 4096 {
                return Err("CAPTURE_ORACLE_METADATA_MISMATCH");
            }
            let raw = capture.raw(&event).map_err(|e| e.code)?;
            let text = capture.text(&event).map_err(|e| e.code)?;
            if hash(&raw) != row.raw_sha256 || hash(text.as_bytes()) != row.text_sha256 {
                return Err("CAPTURE_ORACLE_BYTES_MISMATCH");
            }
            let receipt = store.receipt(&event.event.publication_request_id).map_err(|e| e.code)?
                .ok_or("CAPTURE_PUBLICATION_MISSING")?;
            if receipt.commit_seq != event.revision { return Err("CAPTURE_PUBLICATION_MISMATCH"); }
            let refs = receipt.revisions.iter().filter(|r| r.key.kind == RecordKind::Evidence).collect::<Vec<_>>();
            if refs.len() != 1 || refs[0].commit_seq != event.revision { return Err("CAPTURE_PUBLICATION_MISMATCH"); }
            let stored = store.revision(&refs[0].key, event.revision).map_err(|e| e.code)?
                .ok_or("CAPTURE_PUBLICATION_MISSING")?;
            let published: EventRecord = serde_json::from_str(&stored.content).map_err(|_| "CAPTURE_PUBLICATION_MISMATCH")?;
            if stored.previous.is_some() || published != event.event { return Err("CAPTURE_EVENT_REWRITTEN"); }
            count += 1;
            let ack = serde_json::json!({"verified":count,"read_seq":at,"event_sha256":identity,
                "event_revision":event.revision,"raw_sha256":row.raw_sha256,"text_sha256":row.text_sha256});
            serde_json::to_writer(&mut output, &ack).map_err(|_| "ORACLE_OUTPUT_IO")?;
            output.write_all(b"\n").and_then(|_| output.flush()).map_err(|_| "ORACLE_OUTPUT_IO")?;
        }
        if count == 0 { return Err("EMPTY_ORACLE"); }
        if store.sequence().map_err(|e| e.code)? != at { return Err("ORACLE_STORE_CHANGED"); }
        Ok(count)
    }.await;
    let closed = store.close().await.map_err(|e| e.code);
    let count = outcome?;
    closed?;
    Ok(count)
}

fn run() -> Result<u64> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 1 {
        return Err("INVALID_ARGUMENT");
    }
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "RUNTIME_ERROR")?;
    runtime.block_on(verify(
        Path::new(&args[0]),
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    ))
}

fn main() -> ExitCode {
    match run() {
        Ok(count) => {
            eprintln!("verified {count} original capture events");
            ExitCode::SUCCESS
        }
        Err(code) => {
            eprintln!("{code}");
            ExitCode::FAILURE
        }
    }
}
