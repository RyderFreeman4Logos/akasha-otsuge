# Required fallback experiment

MVP-003 only, outside the product Cargo workspace. This is the sole fallback
allowed by the source specification after the tested Helix candidate failed
full-data low-memory recovery. No production backend is selected.
See the [decision](../../docs/benchmarks/storage-decision.md) and unchanged
[acceptance contract](../../docs/benchmarks/storage-contract.md).

`prune_versions` is an offline generation request with `generation` and a fixed
absolute `deadline_ms`. It requires the active ready generation and a fully
drained outbox before any cleanup. The single-request native process holds the
SurrealKV exclusive authority lock, excluding generation readers and writers.
It calls public LanceDB `OptimizeAction::Prune`, retains the current table
versions, respects tagged-version errors and reports native removal counts.
It changes no authority revisions, receipts, watermarks or active generation.
The absolute deadline remains in the immutable controller request on resume;
per-table checkpoints retain PID/start identity and measured progress. Pruning
can reclaim space even below the ordinary write-admission floor.

The [small native regression](../../docs/benchmarks/results/fallback-prune-versions/summary.json)
passes133 commands plus a competing-owner check, including interruption before
cleanup/after one table, query-before-resume, repeated cleanup and expired or
oversized deadline refusal. The [full-size run](../../docs/benchmarks/results/fallback-full-prune/summary.json)
removes41,326 obsolete versions in349.791s with native RSS max200,458,240B.
All42 before/after witnesses match; measured free space increases by
40,473,866,240B. Full inactive-generation retirement is measured separately
below; the successful concurrent rebuild peak remains unverified. This is not
online GC.

`retire_generation` accepts an inactive generation and an absolute
`deadline_ms`. The active generation must be ready and drained, and the target
must have no pending delta append wave. An incomplete baseline backfill or index
build can be discarded without completing it first. While holding the native authority lock, it
persists a retirement marker before calling public `Connection.drop_table` for
that generation's baseline and delta tables. Each completed deletion is recorded;
a crash between deletion and that record is reconciled by the same public API.
Retirement permanently prevents reuse of the generation number. `status` keeps
its metadata visible and `indexed_receipt` retains the prior result with
`generation_retired: true`; authority receipts and revisions are not deleted.
Resume retains the original deadline. Expired deadlines fail closed; this probe
does not provide an extension or online retirement path. The
[native regression](../../docs/benchmarks/results/fallback-generation-retirement/summary.json)
passes179 commands, two competing-owner refusals and twelve total SIGKILL
barriers on nine baseline documents with eight real vectors. All four table
families are covered; native RSS max is123,334,656B. The subsequent
[full-size retirement](../../docs/benchmarks/results/fallback-full-retirement/summary.json)
preserves42 before/after witnesses and removes an inactive complete generation
in4.333s with native RSS max50,991,104B. Partial-generation regression and full
cleanup receipts are recorded separately; historical metrics retain their
original executable identities.

The [partial-generation regression](../../docs/benchmarks/results/fallback-partial-retirement/summary.json)
passes223 commands,19 SIGKILL barriers and two competing-owner refusals. It retires
never-created tables, interrupted baseline backfills and failed native index
builds without altering the active generation. RSS max is128,020,480B. Subsequent
[full preparation](../../docs/benchmarks/results/fallback-full-generation5-preparation/summary.json)
retires partial4, prunes active3 and backfills5 with66 before/after witnesses;
fresh full concurrent-build acceptance remains open.

Generation/workload controllers set `MALLOC_MMAP_THRESHOLD_=131072` before
starting the native process. On the measured GNU libc2.36 host this prevents
large freed index buffers accumulating in allocator arenas; the observer records
the setting and any `GLIBC_TUNABLES`. The shared workload admits one heavy search
and four total operations, so waiting searches leave read/write capacity free.
The [allocator controls](../../docs/benchmarks/results/fallback-allocator-diagnosis/summary.json)
pass short-run low-memory bounds but still fail full-corpus search throughput.
Long profiles and concurrent rebuild acceptance remain required.

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
optional `--oracle-verify-text COUNT VERIFICATION_ID` identifies a new historical
check after later index work; replay of the same ID keeps its first deadline.
The controller accepts `verification_id` only for `verify_text`. Existing seed
and verifier identities retain their deadlines, including expired ones. The
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

Each authority now owns one Lance Session: its16MiB index cache,8MiB metadata
cache and64MiB spill cap are shared by build, query and delta connections. The
native regression verifies shared dataset-session identity and fresh rows/indexes
after another connection writes. Aggregate RSS still requires full measurement.

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

The [full import/reopen and history receipts](../../docs/benchmarks/results/fallback-full-import-history/summary.json)
verify all 11,428,831 original records and source/scope mappings at the low profile
in 3,570.278s. The original full import uses 37,799,542,784 allocated database bytes;
after low verification/close it uses 37,805,289,472B. Across 3,565 samples, RSS max
is 133,865,472B and nearest-rank P99 is 133,861,376B, with no OOM and a maximum
1.022s sample gap. Native page-cache pressure causes memory.high throttling;
this single sequential verification is not mixed-workload memory acceptance.
The retained first low open fails at the service's default soft1024 file limit
for 2,202 eagerly opened VLogs. The second uses explicit service-local8192 and
the unchanged original deadline/memory/swap limits. Its older observer did not
record the effective nofile limits; the admitted command is retained.

On the 38,462-record real shard, the same frozen history model also passes
100,000 changes in 311.838s and 5,163 reopened checks in 4.007s. Sampled native
RSS maxima are 183,025,664B and 81,698,816B, without OOM. Full-corpus history and
backfill now run separately; none of these receipts proves full native indexes.

Remaining: complete full-size indexes, authority/outbox and derived generation
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

Import now retains the original manifest in authority metadata. Every generation
read and creation checks its model, fingerprint, dimension and chunker against
that hash-bound source; a schema-only generation upgrade remains valid. The
pinned publisher manifest derives the same model fingerprint as the original
Helix corpus reader. Diagnostic synthetic manifests declare `embedding_bundle`
explicitly and cannot mix it with publisher metadata. These labels do not claim
that synthetic vectors came from the real model.

For a legacy import without persisted manifest bytes, repeat `begin` with the
existing generation, unchanged bundle and original `manifest` path. Only bytes
matching the original import SHA can be bound; missing, changed or contradictory
metadata is refused. Binding does not rewrite originals or refresh deadlines.
Previously mislabeled generation state is rejected at the common read boundary,
including query, workload pinning and delta replay. Request failures await
database close before returning, retaining the original operation error.

First-time legacy binding checks the configured native disk reservation before
writing metadata. Already-bound Begin reuse and status remain readable under an
exhausted reserve. `check_legacy_binding.py CURRENT_BINARY LEGACY_BINARY OUTPUT`
verifies both refusals without metadata effects and successful state/deadline
preservation using a real import created by the supplied pre-binding binary.

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
`verify_acknowledgements` checks1..=256 recorded workload receipts per request
against durable receipts, request hashes, immutable revision content hashes and
outbox identity.
It reads historical revisions after reopen without changing current heads or
commit sequence. This bounded diagnostic supports failed-workload reconciliation;
the caller must verify its complete log identity, unique acknowledgements and
batch coverage before claiming that every logged success survived.
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
These initial authority receipts are superseded by the additional lifecycle and
full-history evidence below; the sustained workload matrix remains unfinished.

[Full history and backfill receipts](../../docs/benchmarks/results/fallback-full-history-backfill/summary.json)
verify 100,000 changes and 5,163 historical assertions on the complete corpus.
All 11,428,831 text records and 1,000,000 vectors backfill in 830.918s under the
640MiB/swap=0 group; sampled RSS max is 334,905,344B. Native row counts, cursor,
versions, empty pending wave and original manifest agree. Allocated storage is
68,256,649,216B after backfill. Native indexes remain separate acceptance.

[Lifecycle receipts](../../docs/benchmarks/results/fallback-lifecycle/summary.json)
cover generation-specific delta tables. A durable pending-wave identity and
native append transaction tag reconcile interrupted writes before indexed
completion. Separate text/vector watermarks advance only through contiguous
completed jobs. `indexed_receipt` distinguishes durable authority commits from
both-family index completion. No cross-store atomic transaction is claimed.

`apply`, `advance`, `catch_up`, `activate`, `indexed_receipt`, `lifecycle_status`
and `query_current` are native `--generation` requests. Vector output must match
model, fingerprint, source body hash and dimension. Retracted/superseded jobs and
explicit text-only vectors record their reason. Catch-up takes bounded 256-job
waves and persists its original target/deadline. These probe operations have one
controller owner; persistent multi-client execution remains a later acceptance.

Activation checks complete native baseline/delta coverage, then holds the
ordinary writer briefly for the authority sequence and active-pointer commit.
Old activated generations remain readable with their own bundle. Queries bound
and combine baseline/delta candidates, then recheck live authority and permissions;
coverage explicitly says `candidate_set_only`, with no global BM25 rank claim.
A minimum commit sequence fails when the relevant watermark lags.

Delta text updates use Lance's incremental FTS optimization, merging into one
index and tokenizing only new fragments. Native coverage still must include every
row before publishing the indexed receipt. Both delta families also maintain the
same scope bitmap used by baseline tables, so prefiltering uses the index instead
of scanning every appended fragment. Activation checks both scope and search
index coverage. Initial text construction and vector
indexing retain their existing builders. The pinned Lance API does not persist
FTS build-time limits, so incremental operations require child startup settings
`LANCE_FTS_NUM_SHARDS=1 LANCE_FTS_PARTITION_SIZE=32`. Generation/workload
controllers supply them and the observer records them; direct probe callers must
also supply them. A mismatched budget refuses the index operation and leaves its
original pending wave recoverable with the correct settings.

`check_corpus_lifecycle.py BINARY DATABASE FIXTURE OUTPUT --generation N`
selects returned corpus candidates and checks correction, rescoping, retraction,
restoration, historical bodies and live permission changes. Run it through
`measure.py` in a dedicated640MiB/swap=0 service with the startup settings above
and `MALLOC_MMAP_THRESHOLD_=131072`. Use `--exclude-fixture` to protect the frozen
mixed-workload population and `--resume` only after the previous owner exits.
Resume verifies source/reply hashes and uses stable idempotency keys for interrupted
steps; completed reuse makes no native calls. The small fixture covers a killed
commit/reply boundary and tamper refusals. These are serial checks; simultaneous
query/build behavior uses the separate workload controller.

The workload controller optionally accepts `[paths].corrections`: a pinned JSON
array of eight original unvectorized corpus candidates, one per scope, each with
`memory`, `scope`, `key`, `body` and `query_index`. Native admission verifies
original authority content and a current BM25 hit before starting. The existing
correction clients cycle through correction, rescoping, retraction and original
restoration; duration must be a multiple of160seconds. Fixed query IDs are
protected. The controller snapshots and hashes this file, independently checks
acknowledged revisions and returned candidates, and refuses changed-source reuse.
`check_generation_workload.py ... fallback --rebuild` exercises this on24
records/eight real vectors for160seconds. This does not cover vectorized candidate
corrections or replace the full concurrent build measurement.

The 103-command native fixture covers six process-crash boundaries, out-of-order
completion/gaps, source validation, retraction/supersession/text-only behavior,
failed build/old reads, activation/model isolation and revocation. Five release
units include a deterministic concurrent write during paused index publication.
48 baseline-search and 53 authority regressions pass on the preceding lifecycle
candidate. Read-only status ignores free-space admission while every mutation
retains its reservation check. Catch-up rate/ETA exclude work from earlier runs.
Full 100k outbox application, full activation, concurrent workloads, sustained
P99, power loss and physical derived corruption remain unproven.
