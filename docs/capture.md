# Capture protocol and recovery

`capture` is the shared storage component for [MVP-007](spec/issues/MVP-007.md).
It stores exact payload bytes separately from normalized visible UTF-8 text and
derived compaction events. It uses the sole [authority Storage](storage.md), not
a second database. The installed daemon/CLI integration belongs to #8; real host
identity discovery and native format readers belong to #11–#15.

## Identity and authority

The trusted server constructs `CaptureBinding` from an adapter/profile/session/
thread/agent identity, a Scope, stream ID and coverage level. These are explicit
source identities; cwd and body similarity never establish identity. The binding
constructor validates shape, not authentication. Capture reads, EventView values
and checkpoint mutation are internal trusted APIs, not worker authorization
endpoints. The daemon must authenticate callers and enforce current permissions.

An EventRef combines that source identity with its stable event ID. EventDraft
records source version, originating runtime instance, branch, parent, optional
fork cutoff, source time, observation time, visibility and event kind. IDs are
nonblank and at most 256 bytes. Retrying the same event preserves its first
observation, source version and config generation; a later observation process
must retain the originating runtime identity. Changed payload, scope, ingress or
event semantics under the same identity fails with `CAPTURE_IDENTITY_CONFLICT`.
Two distinct IDs with identical text remain distinct events.

`Ingress` determines provenance. A reported `user` role alone cannot grant human
authority. The interchange file reader always records unverified ingress. Host
adapters must supply independently established ingress to `Capture::ingest`.

## Durable layout and publication

The archive stages an immutable intent, then raw chunks and normalized source
chunks. Raw payloads are at most 4 MiB, split into 32 KiB binary chunks stored as
hex; equal raw chunks can share storage. Normalized text is at most 1 MiB, split
at UTF-8 boundaries into at most 16 KiB chunks using the existing Domain source
schema and provenance checks. Event metadata is limited to 32 KiB; oversized
input is refused, never silently truncated. Every manifest carries exact byte
lengths, SHA-256 hashes and immutable revision references. Reads verify them.

After all chunks are durable, one native transaction publishes the event
manifest, logical-message state and optional checkpoint advancement. Staged
intent/source/blob records do not constitute an accepted capture event. Consumers
such as #17 must follow published `capture-event/` manifests, not treat each
staged source outbox record as a completed message. An interruption can leave
reusable staged records; no garbage collection of these records is claimed.

A stale checkpoint or logical-message conflict cannot publish the new event or
advance its cursor. Retrying reuses staged chunks. Durable request IDs include
the checkpoint's stream key so independent live/backfill streams can advance
over the same events. An accepted replay does not allocate another event.

Message events contain a logical message ID and either `delta { index }` or
`final`. A delta index accepts only one normalized text. The first final is
canonical; identical final deliveries retain their raw event evidence without
creating another logical final. Conflicting final text is refused. Late deltas
never replace the final. All parts retain the same branch, parent, cutoff, scope
and visibility. Consumers use logical message state rather than emitting every
delivery as another final message.

## Branches, compaction and coverage

Lineage follows explicit parents at a fixed commit sequence. Crossing a branch
or source namespace requires a fork cutoff equal to that parent. A child never
inherits later events from its parent's branch. Resume, fork, abandoned,
execution and compaction events preserve their own evidence; a compaction stores
first/last/original references and cannot overwrite those originals.

`lineage` returns at most 64 events plus a continuation or missing-parent marker.
It validates the parent edge even at a one-event page boundary and detects cycles
inside a page. Callers must bound the complete traversal and remember visited
references across pages; this API does not traverse an unlimited history itself.
`missing_references` reports unresolved parent/compaction references. Late
backfill resolves them at a new read point without changing an earlier view.

Coverage is explicit: `available_payloads`, `visible_text`, `metadata_only`, or
`unavailable`. These describe the source surface, not proof of all host internals.
Checkpoints separately report pending `backlog`, `partial_tail`, `invalid_frame`,
`oversized_frame`, `source_missing`, or `conflict`. Rotation/truncation loss flags
persist even after current EOF; offline backfill does not silently clear them.
Only the trusted adapter may clear a loss after proving completeness.

## Offline reconcile command

Build and run the component command against an explicitly selected isolated
store and regular files:

```sh
cargo build --example reconcile --release --locked
target/release/examples/reconcile DATA_DIR BINDING_JSON CAPTURE_JSONL
```

Binding JSON (maximum 16 KiB):

```json
{"source":{"adapter":"fixture-v1","profile":"profile","session":"session","thread":"thread","agent":"agent"},"scope":{"user":"user","project":"project","branch_or_version":null,"session":null},"stream":"live","coverage":"available_payloads"}
```

Each newline-terminated `akasha-capture-v1` interchange frame has this shape:

```json
{"schema_version":1,"event":{"event_id":"entry-1","source_version":"1","runtime_instance":"runtime","branch":"main","parent":null,"fork_cutoff":null,"event_time_ms":10,"observed_time_ms":20,"visibility":"user","kind":{"kind":"message","message_id":"message-1","phase":{"phase":"final"}},"text":"original"},"raw_hex":"6f726967696e616c"}
```

This is an explicit interchange format, not a parser that assumes all four hosts
write the same JSONL. A call processes at most 64 frames; each frame is bounded
to 16 MiB. Partial lines, including partial UTF-8, wait without advancing. The
opaque cursor records device/inode, offset, generation and first/last 4 KiB
anchors at the consumed boundary. Inode changes identify rotation; shortening
or changed anchors identify truncation. Before publishing, the reader rechecks
those anchors and the current line. It does not hash the full prefix on every
frame. The next anchors are assembled from verified prior boundary bytes and
the accepted frame, so a later rewrite cannot silently replace their identity.
It does not claim detection of arbitrary middle-file rewrites outside the anchors.
Writers should append and retain rotated originals for backfill. A concurrently
changing source can fail with `CAPTURE_SOURCE_CHANGED`; retry remains explicit.

Stdout is one JSON ReconcileReport with processed/deduplicated counts and the
durable checkpoint. Exit 0 means caught up with no observed loss for the declared
coverage; inspect that coverage before deciding whether sufficient evidence is
available. Exit 3 means pending work or retained loss, 2 invalid arguments, and
1 another fixed-code error. Exactly 64 processed frames can leave `backlog`
until the next call confirms EOF. Repeating a caught-up call creates no commit.
FIFO and other nonregular input files are refused. The command opens and closes
Storage directly for component acceptance; the installed CLI remains a thin
daemon client when #8 adds the production operation.

## Validation and limits

`tests/capture.rs` exercises stable replay, distinct equal text, binary/UTF-8
originals, concurrent/out-of-order delivery, delta/final conflicts, fixed-point
missing links, branch cutoffs, repeated resume/compaction, real file tails,
rotation/truncation, offline backfill and stale atomic checkpoints. The storage
crash supervisor SIGKILLs a real child after intent, raw chunk, normalized chunk
and event/checkpoint publication, then reopens and verifies exact originals and
idempotent recovery. These are process-crash tests, not power-loss claims.

`scripts/check_capture_command.py` invokes the built executable through its real
exit/stdout/stderr boundary. The canonical gate builds the release example and
runs this acceptance in temporary stores with owned child cleanup. Actual host
authentication, filesystem watchers, native host database readers, production
IPC and model/index integration remain their dependent Issues. Hidden reasoning,
inaccessible payloads, missing pre-install history and all terminal pixels are
not claimed as captured. No actual host or full-corpus acceptance is replaced by
these component fixtures.

Independent R1 review reproduced a same-inode rewrite after frame verification:
the old implementation adopted new-file anchors for an old accepted event, then
silently skipped the replacement on restart. The retained probe is a real
release-command failure. A deterministic regression now covers short/long frames
and consumed boundaries around 4 KiB, plus changed-prefix refusal; the fix derives
anchors from accepted bytes instead of rereading the new frame from the file.
