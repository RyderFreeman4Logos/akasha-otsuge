//! Bounded reconciliation of complete native session snapshots.
use crate::{
    capture::*,
    domain::{Ingress, Scope},
    storage::{Result, Storage, StorageError, hash},
};
use serde::{Deserialize, Serialize};

pub const EXPORT_STREAM: &str = "hermes-export-v1";

pub struct ExportReport {
    pub capture: ReconcileReport,
    pub tip: Option<EventRef>,
}

#[derive(Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Cursor {
    version: u32,
    snapshot: String,
    next: usize,
    tip: Option<EventRef>,
}

pub async fn reconcile_export(
    store: &Storage,
    source: &SourceIdentity,
    scope: Scope,
    profile: &str,
    bytes: &[u8],
    limit: usize,
    config: u64,
) -> Result<ExportReport> {
    if limit == 0 || limit > 64 || config == 0 {
        return Err(StorageError::new("INVALID_CAPTURE_PAGE"));
    }
    // Decode the complete bounded snapshot before writing anything: malformed or foreign
    // snapshots never advance a successful checkpoint or turn into empty history.
    let frames = super::export::decode_export(source, profile, bytes)?;
    reconcile_frames(store, source, scope, &frames, limit, config).await
}

pub(super) async fn reconcile_frames(
    store: &Storage,
    source: &SourceIdentity,
    scope: Scope,
    frames: &[CaptureFrame],
    limit: usize,
    config: u64,
) -> Result<ExportReport> {
    let snapshot = hash(&serde_json::to_vec(&frames)?);
    let binding = CaptureBinding::from_source(
        source.clone(),
        scope.clone(),
        EXPORT_STREAM,
        CoverageLevel::VisibleText,
    )?;
    let capture = Capture::new(store);
    let mut saved = capture.checkpoint(&binding)?;
    let previous = saved
        .as_ref()
        .map(|v| {
            serde_json::from_str::<Cursor>(&v.checkpoint.cursor)
                .map_err(|_| StorageError::new("INVALID_CAPTURE_CHECKPOINT"))
        })
        .transpose()?;
    if previous.as_ref().is_some_and(|c| c.version != 1) {
        return Err(StorageError::new("INVALID_CAPTURE_CHECKPOINT"));
    }
    let mut cursor = previous
        .filter(|c| c.snapshot == snapshot)
        .unwrap_or(Cursor {
            version: 1,
            snapshot,
            next: 0,
            tip: None,
        });
    let reference = |frame: &CaptureFrame| EventRef {
        source: source.clone(),
        event_id: frame.event.event_id.clone(),
    };
    if cursor.next > frames.len()
        || cursor.tip != cursor.next.checked_sub(1).map(|i| reference(&frames[i]))
    {
        return Err(StorageError::new("INVALID_CAPTURE_CHECKPOINT"));
    }
    let ingress = Ingress::unverified(
        // Both the immediate fence and durable interchange backfill publish the
        // same source event, so their unverified ingress identity must agree.
        "capture-interchange",
        &hash(&serde_json::to_vec(source)?),
        scope,
    )?;
    let mut processed = 0;
    let mut deduplicated = 0;
    for frame in frames.iter().skip(cursor.next).take(limit) {
        let event_ref = reference(frame);
        let duplicate = capture
            .event(&binding, &event_ref, store.sequence()?)?
            .is_some();
        cursor.next += 1;
        cursor.tip = Some(event_ref);
        let checkpoint = Checkpoint {
            cursor: serde_json::to_string(&cursor)?,
            coverage: CoverageLevel::VisibleText,
            loss: Vec::new(),
            pending: (cursor.next < frames.len()).then_some(PendingReason::Backlog),
        };
        let raw = crate::capture::decode_hex(&frame.raw_hex, 4 * 1024 * 1024)?;
        capture
            .ingest(
                &binding,
                &frame.event,
                &raw,
                &ingress,
                Some(&Advance {
                    expected_revision: saved.as_ref().map(|s| s.revision),
                    checkpoint: checkpoint.clone(),
                }),
                config,
            )
            .await?;
        saved = capture.checkpoint(&binding)?;
        if saved.as_ref().is_none_or(|v| v.checkpoint != checkpoint) {
            return Err(StorageError::new("REVISION_CONFLICT"));
        }
        processed += 1;
        deduplicated += usize::from(duplicate);
    }
    Ok(ExportReport {
        capture: ReconcileReport {
            processed,
            deduplicated,
            checkpoint: saved.ok_or_else(|| StorageError::new("INVALID_CAPTURE_CHECKPOINT"))?,
        },
        tip: cursor.tip,
    })
}
