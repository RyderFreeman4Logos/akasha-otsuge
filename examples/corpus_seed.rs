//! Offline benchmark seeding through the production Domain, never a daemon write bypass.
use akasha_otsuge::config::{Paths, ReviewMode, resolve};
use akasha_otsuge::domain::{
    Domain, Ingress, MemoryDraft, MemoryKind, MemoryWrite, Policy, Scope, SourceDraft, SourceSpan,
};
use akasha_otsuge::storage::{Change, CommitRequest, RecordKey, RecordKind, Storage};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::io::{BufRead, Read, Write};
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::ExitCode;

type Result<T> = std::result::Result<T, &'static str>;

#[derive(Clone, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Binding {
    source_manifest_sha256: String,
    observed_time_ms: i64,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Record {
    key: String,
    body: String,
    partition: String,
}

#[derive(Debug, Serialize, Deserialize, PartialEq)]
struct Ack {
    key_sha256: String,
    body_sha256: String,
    source_id: String,
    source_revision: u64,
    memories: Vec<(String, u64)>,
    utf8_bytes: usize,
}

async fn seed_stream(
    data: &Path,
    binding: &Binding,
    mut input: impl BufRead,
    mut output: impl Write,
) -> Result<u64> {
    if binding.source_manifest_sha256.len() != 64
        || !binding
            .source_manifest_sha256
            .bytes()
            .all(|b| b.is_ascii_hexdigit())
        || binding.observed_time_ms <= 0
    {
        return Err("INVALID_CORPUS_BINDING");
    }
    let config = resolve(
        "",
        None,
        "",
        &Paths {
            data_dir: data.into(),
            socket_path: data.join("unused-corpus.sock"),
        },
    )
    .map_err(|e| e.code)?;
    let store = Storage::open(&config).await.map_err(|e| e.code)?;
    let result = async {
        let binding_json = serde_json::to_string(binding).map_err(|_| "INVALID_CORPUS_BINDING")?;
        let binding_key = RecordKey {
            kind: RecordKind::State,
            id: "benchmark/corpus-seed-v1".into(),
        };
        match store.current(&binding_key).map_err(|e| e.code)? {
            Some(old) if old.content == binding_json => (),
            Some(_) => return Err("CORPUS_BINDING_MISMATCH"),
            None if store.sequence().map_err(|e| e.code)? != 0 => {
                return Err("CORPUS_STORE_NOT_EMPTY");
            }
            None => {
                store
                    .commit(&CommitRequest {
                        request_id: "benchmark/corpus-binding".into(),
                        config_generation: 1,
                        policy_revision: None,
                        changes: vec![Change {
                            key: binding_key,
                            expected_revision: None,
                            content: binding_json.clone(),
                        }],
                    })
                    .await
                    .map_err(|e| e.code)?;
            }
        }
        let domain = Domain::new(&store);
        let mut count = 0;
        let mut line = Vec::new();
        loop {
            line.clear();
            (&mut input)
                .take(512 * 1024 + 1)
                .read_until(b'\n', &mut line)
                .map_err(|_| "CORPUS_INPUT_IO")?;
            if line.is_empty() {
                break;
            }
            if line.len() > 512 * 1024 || line.last() != Some(&b'\n') {
                return Err("INVALID_CORPUS_FRAME");
            }
            let row: Record = serde_json::from_slice(&line).map_err(|_| "INVALID_CORPUS_FRAME")?;
            if row.key.trim().is_empty()
                || row.key.len() > 256
                || row.body.trim().is_empty()
                || row.body.len() > 65536
                || !matches!(
                    row.partition.as_str(),
                    "p0" | "p1" | "p2" | "p3" | "p4" | "p5" | "p6" | "p7"
                )
            {
                return Err("INVALID_CORPUS_RECORD");
            }
            let key_sha256 = digest(row.key.as_bytes());
            let identity = digest(
                &serde_json::to_vec(&(&binding_json, &row.key))
                    .map_err(|_| "INVALID_CORPUS_RECORD")?,
            );
            let source_id = format!("corpus/{identity}");
            let scope = Scope {
                user: format!("uid:{}", rustix::process::geteuid().as_raw()),
                project: Some(format!("corpus/{}", row.partition)),
                branch_or_version: None,
                session: None,
            };
            let ingress = Ingress::tool(
                "licensed-corpus-seed",
                &binding.source_manifest_sha256,
                scope.clone(),
            )
            .map_err(|e| e.code)?;
            let source = domain
                .record_source(
                    &format!("source/{source_id}"),
                    &SourceDraft {
                        event_id: source_id.clone(),
                        scope: scope.clone(),
                        text: row.body.clone(),
                        reported_role: None,
                        event_time_ms: None,
                        observed_time_ms: binding.observed_time_ms,
                    },
                    &ingress,
                    1,
                )
                .await
                .map_err(|e| e.code)?;
            let body_sha256 = domain
                .source(&source_id, source.commit_seq)
                .map_err(|e| e.code)?
                .content_sha256;
            let policy = Policy {
                revision: "corpus-seed-v1".into(),
                config_generation: 1,
                mode: ReviewMode::Automatic,
                scope: scope.clone(),
                adopt_explicit_decisions: false,
            };
            let mut ack = Ack {
                key_sha256,
                body_sha256: body_sha256.clone(),
                source_id: source_id.clone(),
                source_revision: source.commit_seq,
                memories: Vec::new(),
                utf8_bytes: row.body.len(),
            };
            let mut start = 0;
            while start < row.body.len() {
                let mut end = (start + 32768).min(row.body.len());
                while !row.body.is_char_boundary(end) {
                    end -= 1;
                }
                let id = format!("{source_id}/{start}");
                let memory = domain
                    .propose(
                        &MemoryWrite {
                            request_id: format!("memory/{id}"),
                            memory_id: id.clone(),
                            expected_revision: None,
                            draft: MemoryDraft {
                                kind: MemoryKind::Fact,
                                scope: scope.clone(),
                                claim: row.body[start..end].into(),
                                rationale: String::new(),
                                conditions: vec![],
                                assumptions: vec![],
                                source_spans: vec![SourceSpan {
                                    event_id: source_id.clone(),
                                    revision: source.commit_seq,
                                    content_sha256: body_sha256.clone(),
                                    start,
                                    end,
                                }],
                                valid_from_ms: None,
                                valid_until_ms: None,
                            },
                        },
                        &ingress,
                        &policy,
                    )
                    .await
                    .map_err(|e| e.code)?;
                ack.memories.push((id, memory.commit_seq));
                start = end;
            }
            serde_json::to_writer(&mut output, &ack).map_err(|_| "CORPUS_OUTPUT_IO")?;
            output.write_all(b"\n").map_err(|_| "CORPUS_OUTPUT_IO")?;
            output.flush().map_err(|_| "CORPUS_OUTPUT_IO")?;
            count += 1;
        }
        if count == 0 {
            return Err("CORPUS_EMPTY");
        }
        Ok(count)
    }
    .await;
    let closed = store.close().await.map_err(|e| e.code);
    let count = result?;
    closed?;
    Ok(count)
}

fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

fn run() -> Result<()> {
    let args = std::env::args_os().skip(1).collect::<Vec<_>>();
    if args.len() != 2 {
        return Err("INVALID_ARGUMENT");
    }
    let file = std::fs::OpenOptions::new()
        .read(true)
        .custom_flags(0o4000)
        .open(&args[1])
        .map_err(|_| "CORPUS_BINDING_IO")?;
    if !file.metadata().map_err(|_| "CORPUS_BINDING_IO")?.is_file() {
        return Err("INVALID_CORPUS_BINDING");
    }
    let mut bytes = Vec::new();
    file.take(16385)
        .read_to_end(&mut bytes)
        .map_err(|_| "CORPUS_BINDING_IO")?;
    if bytes.len() > 16384 {
        return Err("INVALID_CORPUS_BINDING");
    }
    let binding: Binding = serde_json::from_slice(&bytes).map_err(|_| "INVALID_CORPUS_BINDING")?;
    let runtime = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
        .map_err(|_| "RUNTIME_ERROR")?;
    runtime.block_on(seed_stream(
        Path::new(&args[0]),
        &binding,
        std::io::stdin().lock(),
        std::io::stdout().lock(),
    ))?;
    Ok(())
}

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => {
            let _ = writeln!(
                std::io::stderr().lock(),
                "{{\"schema_version\":1,\"error\":{{\"code\":\"{code}\"}}}}"
            );
            ExitCode::from(if code == "INVALID_ARGUMENT" { 2 } else { 1 })
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use akasha_otsuge::config::{Paths, resolve};
    use akasha_otsuge::domain::{Actor, Authority, Domain, Lifecycle};
    use akasha_otsuge::storage::Storage;
    use std::path::PathBuf;

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "akasha-corpus-seed-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed)
            ));
            std::fs::create_dir(&root).unwrap();
            Self(root)
        }
    }
    impl Drop for Fixture {
        fn drop(&mut self) {
            std::fs::remove_dir_all(&self.0).unwrap();
        }
    }
    fn binding() -> Binding {
        Binding {
            source_manifest_sha256: "a".repeat(64),
            observed_time_ms: 1_793_000_000_000,
        }
    }
    fn line(body: &str) -> Vec<u8> {
        let mut bytes = serde_json::to_vec(&serde_json::json!({
            "key": "public-document", "body": body, "partition": "p3"
        }))
        .unwrap();
        bytes.push(b'\n');
        bytes
    }

    #[tokio::test]
    async fn seed_preserves_utf8_evidence_delegated_authority_and_exact_replay() {
        let root = Fixture::new();
        let data = root.0.join("data");
        // The first split falls inside a three-byte character before adjustment.
        let body = format!("{}终章", "a".repeat(32767));
        let bytes = line(&body);
        let mut output = Vec::new();
        assert_eq!(
            seed_stream(&data, &binding(), bytes.as_slice(), &mut output)
                .await
                .expect("the offline component must persist one actual source"),
            1
        );
        let ack: Ack = serde_json::from_slice(&output).unwrap();
        assert_eq!(
            ack.body_sha256,
            "232fa5517d5899d8c8d25f6989a0b3cb61a21413859feefced9ff39aa90f917f"
        );
        assert_eq!(ack.utf8_bytes, body.len());
        assert_eq!(ack.memories.len(), 2);
        let config = resolve(
            "",
            None,
            "",
            &Paths {
                data_dir: data.clone(),
                socket_path: root.0.join("unused.sock"),
            },
        )
        .unwrap();
        let store = Storage::open(&config).await.unwrap();
        let before = store.sequence().unwrap();
        let domain = Domain::new(&store);
        let source = domain.source(&ack.source_id, ack.source_revision).unwrap();
        assert_eq!(source.payload.text, body);
        assert_eq!(source.payload.observed_time_ms, binding().observed_time_ms);
        assert!(matches!(source.provenance.actor, Actor::Tool(_)));
        let mut joined = String::new();
        for (id, seq) in &ack.memories {
            let memory = domain.active(id).unwrap().unwrap();
            assert_eq!(memory.revision.commit_seq, *seq);
            assert_eq!(memory.record.authority, Authority::DelegatedApproved);
            assert_eq!(memory.record.lifecycle, Lifecycle::Active);
            assert_eq!(
                memory.record.draft.scope.project.as_deref(),
                Some("corpus/p3")
            );
            assert!(memory.record.approval.is_none());
            assert!(memory.record.draft.claim.len() <= 32768);
            let span = &memory.record.draft.source_spans[0];
            assert_eq!(span.event_id, ack.source_id);
            assert_eq!(span.revision, ack.source_revision);
            assert_eq!(&body[span.start..span.end], memory.record.draft.claim);
            joined.push_str(&memory.record.draft.claim);
        }
        assert_eq!(joined, body);
        store.close().await.unwrap();
        let mut replay = Vec::new();
        seed_stream(&data, &binding(), bytes.as_slice(), &mut replay)
            .await
            .unwrap();
        assert_eq!(replay, output);
        let store = Storage::open(&config).await.unwrap();
        assert_eq!(store.sequence().unwrap(), before);
        store.close().await.unwrap();
    }

    #[tokio::test]
    async fn partial_stream_keeps_acknowledged_rows_and_refuses_changed_replay() {
        let root = Fixture::new();
        let data = root.0.join("data");
        let original = line("A retained public document");
        let mut partial = original.clone();
        partial.extend_from_slice(b"{\"key\":");
        let mut output = Vec::new();
        assert_eq!(
            seed_stream(&data, &binding(), partial.as_slice(), &mut output).await,
            Err("INVALID_CORPUS_FRAME")
        );
        let first: Ack = serde_json::from_slice(&output).unwrap();
        let mut replay = Vec::new();
        assert_eq!(
            seed_stream(&data, &binding(), original.as_slice(), &mut replay).await,
            Ok(1)
        );
        assert_eq!(serde_json::from_slice::<Ack>(&replay).unwrap(), first);
        let mut changed_binding = binding();
        changed_binding.source_manifest_sha256 = "b".repeat(64);
        assert_eq!(
            seed_stream(&data, &changed_binding, original.as_slice(), Vec::new()).await,
            Err("CORPUS_BINDING_MISMATCH")
        );
        assert_eq!(
            seed_stream(
                &data,
                &binding(),
                line("Changed payload").as_slice(),
                Vec::new()
            )
            .await,
            Err("IDEMPOTENCY_CONFLICT")
        );
        let mut unchanged = Vec::new();
        seed_stream(&data, &binding(), original.as_slice(), &mut unchanged)
            .await
            .unwrap();
        assert_eq!(unchanged, output);
    }

    #[tokio::test]
    async fn oversized_frames_are_bounded_and_invalid_input_has_no_ack() {
        let root = Fixture::new();
        let data = root.0.join("data");
        let mut oversized = std::io::Cursor::new(vec![b'x'; 1024 * 1024]);
        let mut output = Vec::new();
        assert_eq!(
            seed_stream(&data, &binding(), &mut oversized, &mut output).await,
            Err("INVALID_CORPUS_FRAME")
        );
        assert_eq!(oversized.position(), 512 * 1024 + 1);
        assert!(output.is_empty());
        let bad = line(&"x".repeat(65537));
        assert_eq!(
            seed_stream(&data, &binding(), bad.as_slice(), &mut output).await,
            Err("INVALID_CORPUS_RECORD")
        );
        assert!(output.is_empty());
        let valid = line("Ordinary valid input after the refused frames");
        assert_eq!(
            seed_stream(&data, &binding(), valid.as_slice(), &mut output).await,
            Ok(1)
        );
        let _: Ack = serde_json::from_slice(&output).unwrap();
    }
}
