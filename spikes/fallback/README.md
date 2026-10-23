# Required fallback experiment

MVP-003 only, outside the product Cargo workspace. This is the sole fallback
allowed by the source specification after the tested Helix candidate failed
full-data low-memory recovery. No production backend is selected.
See the [decision](../../docs/benchmarks/storage-decision.md) and unchanged
[acceptance contract](../../docs/benchmarks/storage-contract.md).

Pinned source:

| Component | Version | Commit | License |
| --- | --- | --- | --- |
| SurrealKV authority | 1.0.0 | `4338a790c8edc56ef5ea1e46157427af4815be3c` | Apache-2.0 |
| LanceDB derived search | 0.40.0 | `b1b080b97c3edd2fd9f2aae8a3b71ff19487b1f8` | Apache-2.0 |
| Lance Rust components | 13.0.0 | registry checksums in Cargo.lock | Apache-2.0 |

`--oracle-seed-text COUNT` and `--oracle-verify-text COUNT` reuse the frozen
version-1 history model in `spikes/oracle_model.rs`. Both run through the existing
exclusive generation controller, publish durable process-identified checkpoints
and preserve the first absolute operation deadline across native replay. The
[history receipts](../../docs/benchmarks/results/fallback-history/summary.json)
cover ten controller cases, 2,048 changes and 5,163 exact historical checks on
eight real source records. Four release units include expired-deadline refusal
before a new commit. The separate 100k real-shard run and full-corpus history are
not claimed by this small receipt. Build with the repository's fixed installed
Rust 1.99.1; a retained `mise rust@stable` update failure preceded compilation.

The initial LanceDB build with no features failed in upstream `job.rs`: it names
`Error::Http` even though that variant is gated behind `remote`. The probe enables
the existing `remote` compilation feature to resolve this tag's feature mismatch;
it supplies only local filesystem database paths. Upstream source is unchanged.
The failed build is separate from native runtime acceptance.

`check_primitives.py` is the first acceptance step. It owns fresh temporary roots,
checks five-key atomicity at three real SIGKILL barriers, rollback, conflicting
transactions, native timestamp history, idempotent primitive replay, real BM25
and IVF Flat queries and separate-process reopen. Its 32 synthetic search rows
are excluded from corpus-scale accounting. A successful compile is not a passing
primitive result; publish the exact binary and raw outcomes before claiming it.

The first native run passed all 20 commands in 1.188 seconds, including reopened
BM25/IVF indexes and rejection of an existing-table overwrite. Its frozen binary
SHA-256 is `11ffd72c9c0fbd44c0bbb8b582338eed0a663423e7f182c2afce2c2d96a57636`.
The dedicated group used `MemoryHigh=512MiB`, `MemoryMax=640MiB` and no swap.
Three one-second samples are insufficient to characterize subsecond native peaks
or full-scale memory. All children exited and all temporary databases were removed.
See [raw receipts](../../docs/benchmarks/results/fallback-primitives/archive.json)
and [summary](../../docs/benchmarks/results/fallback-primitives/summary.json).

The stub failed at the first native read after a successful dependency build.
The passing harness additionally checks the exact error class for negative cases.
The retained initial manifest describes the failed feature combination; it was
reconstructed from that invocation, while the initial lockfile and logs are raw.
The [dependency license inventory](../../docs/benchmarks/results/fallback-primitives/dependency-licenses.json)
records every resolved package, including the unmodified MPL-2.0 `option-ext`
transitive dependency. Project licensing does not replace dependency licenses.

Separate fixture preflights proved isolated kernel ENOSPC in an owned 8MiB
mount namespace and file-cache eviction of one owned 4MiB file. Both fixtures
were removed. Neither is engine disk-full recovery or full-database cold coverage.

Reproduce the primitive test with the pinned Rust toolchain:

```sh
CARGO_TARGET_DIR=target/storage-spike cargo build --manifest-path spikes/fallback/Cargo.toml --release --locked
python3 spikes/fallback/check_primitives.py target/storage-spike/release/akasha-fallback-probe /tmp/akasha-fallback-new-receipts
```

The second command alone does not impose a cgroup. The archived admission records
the exact `systemd-run` and observer command used for the measured run.

Source-grounded controls to exercise:

- SurrealKV defaults to eventual durability. Each confirmed write must explicitly
  use `Durability::Immediate`; `get_for_update` supplies commit-time validation of
  read keys. Ordinary snapshot reads alone are not a serializable commit contract.
- Set finite memtables, SST target and block cache. `memory.rs` documentation is
  not proof of an aggregate RSS bound; measure the complete engine process tree.
  Use `WalRecoveryMode::AbsoluteConsistency` to expose authority corruption.
  In this pin, memtable capacity measures the internal arena; retained values
  live on the heap. Do not interpret its byte setting as a bound on all values.
- LanceDB exposes `Session` caches and a `LocalSpillStore` cap through real Rust
  APIs. Cache units are native weighted budgets, not process RSS. FTS construction
  exposes per-worker `memory_limit_mb` and `num_workers`; vector training and
  shuffle still require their own budget/call-chain verification.

`--load INPUT MANIFEST` and `--resume-load INPUT MANIFEST` now consume the same
bounded JSONL stream and manifest as the existing `spikes/helix/import_corpus.py`
controller. The importer stores original record bytes, a unique source-key map
and each committed cursor/hash in one immediate-durability transaction. Batches
contain at most 128 records and stop at 4MiB plus one bounded record. Resume
replays and hashes the committed prefix before adding data. `verify-import`
streams all persisted record bytes in ID order and compares the committed hash;
`corpus-get ID` reads one imported record. Interrupted runs require explicit
resume through the existing exclusive controller. These are probe commands.

The [import/recovery receipts](../../docs/benchmarks/results/fallback-import-recovery/summary.json)
retain a failing unsupported-command baseline, two compile API corrections, 21
native import checks, repeated primitive checks, committed-WAL corruption, and
real kernel ENOSPC after staging an 80KiB transaction. The 8MiB private tmpfs is
filled only after the native barrier. Failure emits no successful receipt or
partial keys; after freeing space, previous history and successful retry verify.
The private mount is disposed. These fixtures do not simulate SSD power loss.

A first real-source stream imports 38,462 actual 1536-dimensional vectors from
the pinned public shard in 28.042 seconds, with no expanded stream retained.
Both producer and consumer exit zero and the stored prefix matches the source
hash. High-profile sampled native RSS reaches 1,503,567,872 bytes. The database
uses 1,278,259,200 allocated bytes after close, including retained WAL; this is
not a compacted-size claim. Low-profile inline reopen was intentionally stopped
after 1,610.563 seconds of memory throttling without reaching verification.
It was neither OOM nor expiry of its original 3,600-second limit. The source pin
retains heap values while replaying all WAL memtables before flushing; the
retained 1.278GB WAL therefore defeats an arena-only memory bound.
The current probe uses 8/64MiB low/high arenas, 32/128MiB caches, 8/64MiB SST
targets and Zstd SST compression; those settings are not an aggregate bound.

The corrected layout enables native VLog for values above 256 bytes, with 16MiB
files and full value checksum verification. It leaves resource thresholds,
transaction durability and strict WAL recovery unchanged. All primitive, import,
WAL-corruption and ENOSPC checks pass again. The same 38,462-vector source then
imports with sampled native RSS of 45,514,752B; direct low-profile reopen verifies
every original byte in 3.223 seconds with sampled native RSS of 78,217,216B.
The four low samples have a maximum 1.002-second gap, no memory.high events or OOM.
The cgroup peak includes page cache and is recorded separately from RSS.
Allocated database size is 1,280,880,640B: VLog payloads are still uncompressed.
See [raw diagnostics and stopped inline run](../../docs/benchmarks/results/fallback-vlog/summary.json).
These are small real-data results, not a full-corpus pass or paired speedup.

Storage format 2 compresses each original JSONL record with the already-resolved
Zstd 0.13.3 library (level 1), with a 512KiB decoded-record limit. The same immediate
transaction also stores source identity and initial scope membership. Verification
reads every original record and checks both mappings; original input hashes are
unchanged. Old format roots are rejected explicitly and remain readable with their
frozen original binary; no implicit migration occurs. The import crash fixture
uses deterministic incompressible bodies so its committed values still exercise
VLog. Oversized/truncated compressed frames and legacy bytes are rejected.

The [compressed-layout receipts](../../docs/benchmarks/results/fallback-compressed/summary.json)
include all six native stages, the unsupported-layout RED, committed VLog payload
CRC failure without silent repair, and original-format compatibility checks. The
same real shard now occupies 500,682,752 allocated bytes (about 61% below the
uncompressed VLog layout). Low reopen verifies original bytes and mappings in
4.242s; five samples show native RSS max 71,561,216B, with no throttling or OOM.
These short samples do not establish sustained P99 or full-size memory behavior.
No new resolved package was added; only the probe gained a direct Zstd edge.

Remaining: verify the completed full import at the low resource profile,
complete full-size indexes, authority/outbox and derived generation
reconciliation, then the unchanged workload and recovery matrix. The original
84GB expanded stream is never materialized. Existing Helix diagnostic passes do
not transfer to this implementation.

`derive REQUEST.json` builds a corpus baseline in separate generation-specific
Lance text/vector tables. It uses bounded 16MiB/8,192-record waves, Zstd text
columns, finite file/group sizes and a durable SurrealKV pending-wave record.
Native Lance transaction properties identify each committed append. Resume
checks count, version, identity and every row of an ambiguously completed append
before advancing the authority cursor; it never blindly repeats an append.
Full-text and cosine IVF Flat indexes plus scope bitmaps must report complete
native coverage before the baseline is marked ready. This does not activate a
production generation or reconcile ordinary writes. Native index progress
callbacks write durable stage counters.

[39 native search checks](../../docs/benchmarks/results/fallback-search/summary.json)
pass four SIGKILL barriers (before text, after text, after vector, after cursor),
separate-process resume, all-row coverage, reopened queries, scope filters,
model-bundle mismatch and invalid-input rejection. The 1,057 synthetic records
are excluded from scale accounting. Under 640MiB/swap=0, eleven samples show
harness-plus-child RSS max 190,279,680B and no throttling/OOM. Small native import
and primitive regressions also pass. The raw evidence retains both the missing
operation RED and the first append-mode failure: explicit native WriteParams
defaulted to Create, so subsequent appends must set Append explicitly. No
upstream code was changed. The three new direct dependency edges reuse already
resolved packages; no new package was added.

`--generation` now uses the existing exclusive `run_generation.py` controller.
Backfill and index construction persist separate absolute deadlines before work;
resume cannot extend them. Native progress includes PID/start ticks, and disk
reservations are checked before effects and at wave/index boundaries. The
[47-command extension](../../docs/benchmarks/results/fallback-owned-search/summary.json)
proves deadline refusal after SIGKILL and continued reads of the ready generation.
It also binds the requested corpus manifest to the imported source hash.

The real 38,462-row shard backfills in 10.311s. Its first low-profile index build
OOMs during partition merging, retained with the systemd journal. Lance defaults
to 128MiB shuffle batches and preloaded partition windows. The controller now
uses the existing `LANCE_SHUFFLE_BATCH_BYTES=16777216` and
`LANCE_SHUFFLE_MAX_PRELOADED_OFFSETS_BYTES=0` controls, selecting bounded singleton
partition reads, with two compute threads. After explicit failed-run abandonment,
the same binary completes the remaining index build in 19.730s, preserving its
original native deadline. Seventeen samples show native RSS max 464,904,192B;
the 640MiB/swap=0 group reclaims cache but records no OOM. This is not sustained
P99 evidence. The observer records the effective native environment and file limit.

Eight source-bound query vectors exercise BM25 and ANN in every scope: 16 queries,
297 native commands and 272 independently checked authority candidates pass.
All eight ANN queries return their source at top20; six BM25 queries do, reported
without claiming an expected BM25 rank or full-corpus ANN recall. The query group
uses at most 178,450,432B sampled harness-plus-child RSS without throttling/OOM.
The checker retains hashes instead of duplicating complete source records.

Full corpus import has completed at 37,799,542,784 allocated database bytes.
The default systemd soft file limit of 1,024 cannot open its 2,202 native VLogs;
generation services therefore explicitly use `LimitNOFILE=8192`. This is local
to the measured services, with no global configuration change. Full-data low
verification, full indexes and the unchanged workload matrix remain separate.

`--authority REQUEST.json` now exercises ordinary changes on imported roots.
One immediate transaction commits the compressed immutable revision, versioned
head and scope membership, request-hash receipt, pending job and global sequence.
The revision key orders by memory and sequence; historical heads must agree with
the latest immutable revision at that read point. Source revision zero remains
readable after corrections, scope changes, retractions and restores. The encoded
revision bound matches the 512KiB decoder bound, including JSON escape expansion.

Current reads and candidate publication check live permissions and latest heads.
Candidates return canonical authority metadata, excluding supplied body/provenance.
Historical pages preserve their commit point, release the writer while scanning,
and check permission again before publication. A deterministic in-process pause
allows a concurrent revocation to complete and verifies an empty final page.
Raw diagnostic get/state operations deliberately bypass authorization; they are
internal probe operations, not a public product interface.

[53 native authority checks](../../docs/benchmarks/results/fallback-authority/summary.json)
pass three commit SIGKILL barriers, receipt reuse/conflicts, historical membership
and pagination, live authorization, restoration, forged derived metadata and
input validation. Three release units cover codec bounds, corrupted head/sequence
rejection and the revocation race. The prior candidate also passes all 47 search
regressions; the final change adds the encoded authority-record bound after its
retained native RED. Short resource samples do not establish sustained P99.
Pending outbox jobs are durable; applying them to derived indexes, generation
activation, full 100k history and the workload matrix remain unfinished.
