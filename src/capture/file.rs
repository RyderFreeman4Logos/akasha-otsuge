use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Read, Seek, SeekFrom};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use super::*;

const MAX_FRAME: usize = 16 * 1024 * 1024;
const ANCHOR: u64 = 4096;

#[derive(Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct FileCursor {
    schema_version: u32,
    device: u64,
    inode: u64,
    generation: u64,
    offset: u64,
    prefix_sha256: String,
    tail_sha256: String,
}

fn anchor_bytes(file: &mut File, offset: u64) -> Result<(Vec<u8>, Vec<u8>)> {
    let size = offset.min(ANCHOR) as usize;
    let mut first = vec![0; size];
    file.seek(SeekFrom::Start(0))?;
    file.read_exact(&mut first)?;
    let mut last = vec![0; size];
    file.seek(SeekFrom::Start(offset - size as u64))?;
    file.read_exact(&mut last)?;
    file.seek(SeekFrom::Start(offset))?;
    Ok((first, last))
}

fn anchor(file: &mut File, offset: u64) -> Result<(String, String)> {
    let (first, last) = anchor_bytes(file, offset)?;
    Ok((hash(&first), hash(&last)))
}

fn next_anchor(file: &mut File, cursor: &FileCursor, line: &[u8]) -> Result<(String, String)> {
    let (mut first, mut last) = anchor_bytes(file, cursor.offset)?;
    if hash(&first) != cursor.prefix_sha256 || hash(&last) != cursor.tail_sha256 {
        return Err(error("CAPTURE_SOURCE_CHANGED"));
    }
    // The new boundary must describe the accepted frame, not a later file generation.
    first.extend_from_slice(&line[..line.len().min(ANCHOR as usize - first.len())]);
    if line.len() >= ANCHOR as usize {
        last.clear();
        last.extend_from_slice(&line[line.len() - ANCHOR as usize..]);
    } else {
        last.extend_from_slice(line);
    }
    Ok((
        hash(&first),
        hash(&last[last.len().saturating_sub(ANCHOR as usize)..]),
    ))
}

impl Capture<'_> {
    /// Reconcile only the explicit akasha-capture-v1 JSONL interchange format.
    /// The path is supplied by a trusted server; this does not discover a host session.
    pub async fn reconcile_jsonl(
        &self,
        binding: &CaptureBinding,
        path: &Path,
        limit: usize,
        config: u64,
    ) -> Result<ReconcileReport> {
        self.reconcile_file(binding, path, limit, config, |_| {})
            .await
    }

    async fn reconcile_file(
        &self,
        binding: &CaptureBinding,
        path: &Path,
        limit: usize,
        config: u64,
        mut boundary: impl FnMut(&'static str),
    ) -> Result<ReconcileReport> {
        if limit == 0 || limit > 64 || config == 0 {
            return Err(error("INVALID_CAPTURE_PAGE"));
        }
        let mut saved = self.checkpoint(binding)?;
        let mut checkpoint = saved
            .as_ref()
            .map(|v| v.checkpoint.clone())
            .unwrap_or(Checkpoint {
                cursor: String::new(),
                coverage: binding.coverage,
                loss: Vec::new(),
                pending: Some(PendingReason::Backlog),
            });
        // Linux O_NONBLOCK prevents a FIFO from hanging before the regular-file check.
        let mut file = match OpenOptions::new()
            .read(true)
            .custom_flags(0o4000)
            .open(path)
        {
            Ok(file) => file,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => {
                checkpoint.pending = Some(PendingReason::SourceMissing);
                return Ok(ReconcileReport {
                    processed: 0,
                    deduplicated: 0,
                    checkpoint: self
                        .advance_checkpoint(
                            binding,
                            &Advance {
                                expected_revision: saved.map(|v| v.revision),
                                checkpoint,
                            },
                            config,
                        )
                        .await?,
                });
            }
            Err(e) => return Err(e.into()),
        };
        let metadata = file.metadata()?;
        if !metadata.is_file() {
            return Err(error("INVALID_CAPTURE_FILE"));
        }
        let empty = hash(b"");
        let mut cursor = if checkpoint.cursor.is_empty() {
            FileCursor {
                schema_version: 1,
                device: metadata.dev(),
                inode: metadata.ino(),
                generation: 0,
                offset: 0,
                prefix_sha256: empty.clone(),
                tail_sha256: empty,
            }
        } else {
            serde_json::from_str::<FileCursor>(&checkpoint.cursor)
                .map_err(|_| error("INVALID_CAPTURE_CHECKPOINT"))?
        };
        if cursor.schema_version != 1 {
            return Err(error("INVALID_CAPTURE_CHECKPOINT"));
        }
        let changed = if cursor.device != metadata.dev() || cursor.inode != metadata.ino() {
            Some(LossReason::Rotation)
        } else if metadata.len() < cursor.offset {
            Some(LossReason::Truncation)
        } else {
            let (first, last) = anchor(&mut file, cursor.offset)?;
            if first != cursor.prefix_sha256 || last != cursor.tail_sha256 {
                Some(LossReason::Truncation)
            } else {
                None
            }
        };
        boundary("resume-validated");
        if let Some(loss) = changed {
            if !checkpoint.loss.contains(&loss) {
                checkpoint.loss.push(loss);
            }
            cursor.generation = cursor
                .generation
                .checked_add(1)
                .ok_or_else(|| error("CAPTURE_LIMIT"))?;
            cursor.device = metadata.dev();
            cursor.inode = metadata.ino();
            cursor.offset = 0;
            cursor.prefix_sha256 = hash(b"");
            cursor.tail_sha256 = cursor.prefix_sha256.clone();
        }
        checkpoint.cursor = serde_json::to_string(&cursor)?;
        checkpoint.pending = Some(PendingReason::Backlog);
        let ingress = Ingress::unverified(
            "capture-interchange",
            &digest(&binding.source)?,
            binding.scope.clone(),
        )?;
        let mut report = ReconcileReport {
            processed: 0,
            deduplicated: 0,
            checkpoint: CheckpointView {
                revision: 0,
                checkpoint: checkpoint.clone(),
            },
        };
        while report.processed < limit {
            file.seek(SeekFrom::Start(cursor.offset))?;
            let mut line = Vec::new();
            BufReader::new((&mut file).take((MAX_FRAME + 1) as u64))
                .read_until(b'\n', &mut line)?;
            if line.is_empty() {
                checkpoint.pending = None;
                break;
            }
            if line.len() > MAX_FRAME {
                checkpoint.pending = Some(PendingReason::OversizedFrame);
                break;
            }
            if line.last() != Some(&b'\n') {
                checkpoint.pending = Some(PendingReason::PartialTail);
                break;
            }
            let parsed = serde_json::from_slice::<CaptureFrame>(&line)
                .map_err(|_| error("INVALID_CAPTURE_FRAME"));
            let frame = match parsed {
                Ok(v) if v.schema_version == 1 => v,
                _ => {
                    checkpoint.pending = Some(PendingReason::InvalidFrame);
                    break;
                }
            };
            let raw = match decode_hex(&frame.raw_hex, MAX_RAW) {
                Ok(v) => v,
                Err(_) => {
                    checkpoint.pending = Some(PendingReason::InvalidFrame);
                    break;
                }
            };
            if validate(&frame.event, &raw, config).is_err() {
                checkpoint.pending = Some(PendingReason::InvalidFrame);
                break;
            }
            // Detect concurrent changes to the consumed boundary before publishing a cursor.
            let current = file.metadata()?;
            if current.len() < cursor.offset + line.len() as u64
                || anchor(&mut file, cursor.offset)?
                    != (cursor.prefix_sha256.clone(), cursor.tail_sha256.clone())
            {
                return Err(error("CAPTURE_SOURCE_CHANGED"));
            }
            file.seek(SeekFrom::Start(cursor.offset))?;
            let mut verify = vec![0; line.len()];
            file.read_exact(&mut verify)?;
            if verify != line {
                return Err(error("CAPTURE_SOURCE_CHANGED"));
            }
            boundary("frame-validated");
            let mut next = cursor.clone();
            next.offset = next
                .offset
                .checked_add(line.len() as u64)
                .ok_or_else(|| error("CAPTURE_LIMIT"))?;
            let (first, last) = next_anchor(&mut file, &cursor, &line)?;
            next.prefix_sha256 = first;
            next.tail_sha256 = last;
            let mut next_checkpoint = checkpoint.clone();
            next_checkpoint.cursor = serde_json::to_string(&next)?;
            let duplicate = self
                .event(
                    binding,
                    &EventRef {
                        source: binding.source.clone(),
                        event_id: frame.event.event_id.clone(),
                    },
                    self.storage.sequence()?,
                )?
                .is_some();
            let result = self
                .ingest(
                    binding,
                    &frame.event,
                    &raw,
                    &ingress,
                    Some(&Advance {
                        expected_revision: saved.as_ref().map(|v| v.revision),
                        checkpoint: next_checkpoint.clone(),
                    }),
                    config,
                )
                .await;
            match result {
                Ok(_) => {}
                Err(e)
                    if matches!(
                        e.code,
                        "CAPTURE_IDENTITY_CONFLICT" | "CAPTURE_MESSAGE_CONFLICT"
                    ) =>
                {
                    checkpoint.pending = Some(PendingReason::Conflict);
                    break;
                }
                Err(e) => return Err(e),
            }
            saved = self.checkpoint(binding)?;
            if saved
                .as_ref()
                .is_none_or(|v| v.checkpoint != next_checkpoint)
            {
                return Err(error("REVISION_CONFLICT"));
            }
            checkpoint = next_checkpoint;
            cursor = next;
            report.processed += 1;
            report.deduplicated += usize::from(duplicate);
        }
        report.checkpoint = self
            .advance_checkpoint(
                binding,
                &Advance {
                    expected_revision: saved.map(|v| v.revision),
                    checkpoint,
                },
                config,
            )
            .await?;
        Ok(report)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;
    use std::path::PathBuf;
    use std::sync::atomic::{AtomicU64, Ordering};

    struct Fixture(PathBuf);
    impl Fixture {
        fn new() -> Self {
            static NEXT: AtomicU64 = AtomicU64::new(0);
            let root = std::env::temp_dir().join(format!(
                "akasha-anchor-{}-{}",
                std::process::id(),
                NEXT.fetch_add(1, Ordering::Relaxed)
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

    #[test]
    fn checkpoint_anchors_use_the_accepted_frame_after_a_concurrent_rewrite() {
        let fixture = Fixture::new();
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(fixture.0.join("source"))
            .unwrap();
        for (before, size) in [
            (0, 1),
            (0, 4096),
            (1, 1),
            (1, 9000),
            (4095, 200),
            (4096, 1),
            (7000, 9000),
        ] {
            let prefix = vec![b'p'; before];
            let line = vec![b'a'; size];
            file.set_len(0).unwrap();
            file.rewind().unwrap();
            file.write_all(&prefix).unwrap();
            // The line was validated as A; a same-inode writer now replaces it with B.
            file.write_all(&vec![b'b'; size]).unwrap();
            let cursor = FileCursor {
                schema_version: 1,
                device: 0,
                inode: 0,
                generation: 0,
                offset: before as u64,
                prefix_sha256: hash(&prefix[..before.min(ANCHOR as usize)]),
                tail_sha256: hash(&prefix[before.saturating_sub(ANCHOR as usize)..]),
            };
            let accepted = [prefix, line.clone()].concat();
            let expected = (
                hash(&accepted[..accepted.len().min(ANCHOR as usize)]),
                hash(&accepted[accepted.len().saturating_sub(ANCHOR as usize)..]),
            );
            assert_eq!(next_anchor(&mut file, &cursor, &line).unwrap(), expected);
            if before > 0 {
                file.rewind().unwrap();
                file.write_all(b"changed prefix").unwrap();
                assert_eq!(
                    next_anchor(&mut file, &cursor, &line).unwrap_err().code,
                    "CAPTURE_SOURCE_CHANGED"
                );
            }
        }
    }

    #[tokio::test]
    async fn reconcile_never_adopts_replacement_anchors_at_resume_or_frame_boundaries() {
        use crate::config::{Paths, resolve};
        fn frame(id: &str) -> Vec<u8> {
            let mut bytes = serde_json::to_vec(&serde_json::json!({
                "schema_version": 1,
                "event": {
                    "event_id": id, "source_version": "1", "runtime_instance": "runtime",
                    "branch": "main", "parent": null, "fork_cutoff": null,
                    "event_time_ms": 10, "observed_time_ms": 20, "visibility": "user",
                    "kind": {"kind": "message", "message_id": id, "phase": {"phase": "final"}},
                    "text": id
                },
                "raw_hex": "00ff"
            }))
            .unwrap();
            bytes.push(b'\n');
            bytes
        }
        for stage in ["resume-validated", "frame-validated", "append"] {
            let f = Fixture::new();
            let store = Storage::open(
                &resolve(
                    "",
                    None,
                    "",
                    &Paths {
                        data_dir: f.0.join("data"),
                        socket_path: f.0.join("unused.sock"),
                    },
                )
                .unwrap(),
            )
            .await
            .unwrap();
            let binding = CaptureBinding::from_source(
                SourceIdentity {
                    adapter: "fixture".into(),
                    profile: "profile".into(),
                    session: "session".into(),
                    thread: "thread".into(),
                    agent: "agent".into(),
                },
                Scope {
                    user: "user".into(),
                    project: None,
                    branch_or_version: None,
                    session: None,
                },
                "live",
                CoverageLevel::AvailablePayloads,
            )
            .unwrap();
            let path = f.0.join("events.jsonl");
            let old = frame("old");
            let new = frame("new");
            assert_eq!(old.len(), new.len());
            std::fs::write(&path, &old).unwrap();
            let capture = Capture::new(&store);
            if stage == "resume-validated" {
                assert_eq!(
                    capture
                        .reconcile_jsonl(&binding, &path, 64, 1)
                        .await
                        .unwrap()
                        .processed,
                    1
                );
            }
            let mut changed = false;
            let report = capture
                .reconcile_file(&binding, &path, 64, 1, |at| {
                    if !changed && (at == stage || stage == "append" && at == "frame-validated") {
                        if stage == "append" {
                            OpenOptions::new()
                                .append(true)
                                .open(&path)
                                .unwrap()
                                .write_all(&new)
                                .unwrap();
                        } else {
                            std::fs::write(&path, &new).unwrap();
                        }
                        changed = true;
                    }
                })
                .await
                .unwrap();
            assert!(changed);
            let original = capture
                .event(
                    &binding,
                    &EventRef {
                        source: binding.source.clone(),
                        event_id: "old".into(),
                    },
                    store.sequence().unwrap(),
                )
                .unwrap()
                .unwrap();
            assert_eq!(capture.text(&original).unwrap(), "old");
            if stage != "append" {
                let cursor: FileCursor =
                    serde_json::from_str(&report.checkpoint.checkpoint.cursor).unwrap();
                assert_eq!(cursor.prefix_sha256, hash(&old), "{stage}");
                assert_eq!(cursor.tail_sha256, hash(&old), "{stage}");
            }
            let recovered = capture
                .reconcile_jsonl(&binding, &path, 64, 1)
                .await
                .unwrap();
            if stage == "append" {
                assert_eq!(report.processed, 2);
                assert_eq!(recovered.processed, 0);
                assert!(recovered.checkpoint.checkpoint.loss.is_empty());
            } else {
                assert_eq!(recovered.processed, 1, "{stage}");
                assert_eq!(
                    recovered.checkpoint.checkpoint.loss,
                    [LossReason::Truncation]
                );
            }
            assert!(
                capture
                    .message(&binding, "new")
                    .unwrap()
                    .unwrap()
                    .final_event
                    .is_some()
            );
            store.close().await.unwrap();
        }
    }
}
