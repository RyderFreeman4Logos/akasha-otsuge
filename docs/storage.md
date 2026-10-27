# Authority storage format 1

MVP-004 introduces the sole product authority store, using Apache-2.0 SurrealKV
1.0.0 at `4338a790c8edc56ef5ea1e46157427af4815be3c`. Its source and transitive
dependencies are pinned by the root `Cargo.lock`. LanceDB remains the selected
derived backend; its product integration belongs to MVP-010. No SQLite metadata
store or alternative authority backend is used.

`storage::Storage` is a library consumed by the following product modules.
`open(&ResolvedConfig)` uses `paths.data_dir/authority`, containing `format.json`
and the native `kv/` directory. Configuration owns the resource profile. Budgets
below 2048 MiB use 8/32/8 MiB memtable/cache/SST settings; larger budgets use
64/128/64 MiB. Both use a two-memtable stall threshold, 256-byte value-log
threshold, 16 MiB value-log segments, full value-log checksums, Zstd compression,
unlimited native version retention and absolute-consistency WAL recovery.
These settings reproduce the authority part of the [selected ADR](benchmarks/storage-decision.md).
The library does not claim the final daemon's RSS, admission or full-corpus result.

## Records and transactions

The store provides four namespaces: `Evidence`, `Memory`, `State` and `Job`.
Each record holds exact UTF-8 content. Domain modules serialize their versioned
payloads into that content; MVP-005 owns actor/provenance, scope and adoption
validation. The raw storage API is not an authorization API or a user-facing
daemon command. Evidence records cannot be changed after their first commit.
Memory, state and job changes always create immutable revisions.

`CommitRequest` contains a stable request ID, positive configuration generation,
optional policy revision and a nonempty batch of `Change`s. Every change names
a record key, exact expected previous commit sequence and new content. `None`
means the record must not yet exist. Retract, supersede and rollback are new
domain payloads/revisions, never a physical delete in this API.

One Immediate-durability transaction checks all previous heads, writes all new
revisions and heads, stores the `CommitReceipt` and an immutable `OutboxEntry`,
and advances the application `commit_seq`. No external work runs in it. An error
before native commit rolls back the entire batch, including an already staged
new evidence row. The native database lock excludes a second store owner.

Requests are normalized by sorting the unique record keys before hashing the
serialized request. An identical retry returns the original persisted receipt,
including after restart or later corrections. Reusing the request ID with changed
content/context returns `IDEMPOTENCY_CONFLICT`; an outdated expected revision
returns `REVISION_CONFLICT`. Cancellation or loss of a response does not establish
that a native commit failed: reconcile by the same request ID and retry the same
request. Admitted native commits retain the tree and writer lease in an owned
task even if their caller drops the future; retries, checkpoints and `close()`
wait behind that work. Await `close()` before shutting down the Tokio runtime.
A returned receipt means authority committed, not indexed or externally
delivered. Outbox entries describe a committed batch; later workers own durable
processing state in the job/state namespaces.

Identifiers are 1–256 UTF-8 bytes. A batch contains at most 64 unique keys, each
content is at most 128 KiB, and total content is at most 1 MiB. Validation precedes
cloning, normalization or transaction creation. The bounds are fixed format/API
limits, not automatic truncation. Large source streams must be chunked by their
ingestion module while retaining source identity. Empty content is allowed.

| Native key | Value |
| --- | --- |
| `meta/schema_version` | Single byte `1` |
| `meta/commit_seq` | Big-endian u64; initialized to zero |
| `revision/{kind}/{id_byte_length:04x}/{id}/{seq:020}` | Zstd level-1 JSON revision with key, sequence, predecessor, content and SHA-256 |
| `head/{kind}/{id_byte_length:04x}/{id}` | Big-endian revision sequence, versioned at native timestamp `seq + 1` |
| `receipt/{request_id}` | Normalized request hash, committed sequence, revision references and configuration/policy context |
| `outbox/{seq:020}` | Immutable references to every revision in the batch |

`current`, `revision`, `receipt`, `outbox` and `sequence` read committed native
snapshots. Revision reads decompress at most 1 MiB and check content hash, record
identity, sequence and predecessor ordering. Missing commit metadata is
corruption, not sequence zero. Full historical pagination and RecallReceipt are
separate MVP-006 requirements. No hard-delete or history-pruning API is exposed.

## Format admission and migration

New authority directories are private (0700 on Unix); the format marker is 0600.
Initialization durably commits schema/sequence before publishing the marker.
Existing authority and native roots must be real directories, not symlinks.
Configured parent-directory aliases are allowed. A missing native directory,
missing/unknown marker, or incompatible internal schema fails closed. Native
files are not opened when the external format marker is missing or unknown.

Format 1 is the first product format. The spike layout is not a product format
and is not silently imported. No in-place migration is currently defined.
A future migration must recognize the old version explicitly, preserve an
independent verified backup, write/validate a new directory, and publish the new
format only after durable completion. Never hand an unknown directory to a
native automatic migration. Failed initialization/restoration leaves an
unpublished directory for inspection; it cannot be reopened as an empty store.

## Checkpoint, integrity and restore

`checkpoint(new_destination)` holds the application writer and calls the native
checkpoint API. It never copies a live database directory directly or overwrites
a destination. Destinations cannot overlap the live authority directory. It
detaches native hard links before publishing `backup.json` with the application
sequence and every file's relative path, byte length and SHA-256. This is an
integrity manifest, not an authenticated signature. Backup directories must be
kept immutable while verification/restoration runs.

`restore(config, backup)` first checks the format and exact file/hash set,
rejecting missing/extra files, symlinks and malformed manifests. It only creates
a fresh authority directory. The native restore API installs the checkpoint;
the engine is closed, shared file links are detached, and the tree is reopened
so caches cannot retain backup-owned inodes. Source hashes, restored sequence
and internal schema are checked before publishing the product format marker.
Existing user data is never overwritten. An incomplete backup has no valid
manifest; an incomplete restore has no valid product marker.

Files are hashed in 64 KiB blocks. Hard-link detachment copies one file at a time
and syncs it before replacement, adding at most one native file's temporary
space rather than a second complete database. A backup still needs capacity for
one independent snapshot and a restore for one new database. Manifests are
bounded to 32 MiB/100,000 files, paths to 1024 bytes, and directory depth to eight.
No whole database or source corpus is loaded into memory for copying/hashing.

## Verification boundary

`cargo test --test storage` covers native reopen, immutable evidence/history,
atomic optimistic conflicts, concurrent and restarted idempotence, request bounds,
record checksums, format refusal, missing data/metadata, exclusive ownership,
backup integrity, existing destinations and independent backup/restore inodes.
The fixture inventory checks file names and magic bytes for SQLite artifacts.

`cargo test --lib storage::crash_tests::killed_transactions_recover_whole_batches_and_retry_exactly_once -- --exact --nocapture`
launches six isolated real child processes and SIGKILLs them before staging,
after the first staged revision, before native commit, after polling the native
commit future to Pending, after durable commit, and after the returned receipt.
Every recovery verifies the complete evidence/memory/state/job/head/receipt/outbox
set and sequence, preserves the baseline, and retries exactly once. The Pending
case permits either whole outcome; it does not claim interruption at a specific
filesystem syscall. The ignored child test is executed by this supervisor.
Crash hooks compile only into the unit-test executable, never the product.

The root `bash scripts/check.sh` remains the canonical gate. These are real
native persistence fixtures, not a rerun of the 50 GB spike or a claim of hardware
power-loss, encrypted-at-rest, daemon/worker, four-host or release acceptance.
