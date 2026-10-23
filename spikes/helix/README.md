# Helix embedded diagnostic probe

Temporary MVP-003 experiment, outside the production Cargo workspace. Full-size
acceptance and the backend decision are still pending.
[Frozen acceptance criteria](../../docs/benchmarks/storage-contract.md).

Pinned upstream: [Helix v3.5.1 / c72e64f0](https://github.com/HelixDB/helix-db/tree/c72e64f0c7e480f3d125f071659d5e3ad7b8da15),
Rust SDK `3.0.0`, engine `db 0.1.0`, Apache-2.0. Its SlateDB fork is pinned to
`2fb25fdaa61d42cc3756f979a8a7b37dbfc483fd` (`0.16.0`). The independent Cargo.lock
records all resolved dependencies. No upstream files were modified.

From the repository root:

```sh
CARGO_TARGET_DIR="$PWD/target/storage-spike" cargo build --release --locked \
  --manifest-path spikes/helix/Cargo.toml
systemd-run --user --wait --pipe --collect --unit=akasha-helix-crash-probe \
  -p MemoryHigh=536870912 -p MemoryMax=671088640 -p MemorySwapMax=0 \
  -p RuntimeMaxSec=300 \
  /usr/bin/python3 "$PWD/spikes/helix/check_crash.py" \
  "$PWD/target/storage-spike/release/akasha-helix-probe"
```

The controller creates and removes its own fresh temporary directories. It
kills only its own child process at explicit barriers; then separate processes
reopen the database and verify revision/head/receipt/job/sequence counts and
contents. A runtime type error after staging a revision must roll the whole
statement back. The engine's commit is observed before `close()`, so a clean
shutdown cannot conceal a missing acknowledgement durability boundary.

The probe disables embedded telemetry. Explicit budgets are vector cache 32 MiB,
SimHasher 2 MiB, Slate block/metadata 32/8 MiB, FTS 8 MiB, unflushed data 32 MiB,
pending queue 8 MiB/4096 items per index, strong vector pending 8 MiB, strong text
analysis 16 MiB, vector build cache 32 MiB and one lifecycle build slot. Remaining
text backfill limits are explicit pinned values: 512 entities/8 MiB input and
output, 32,768 output operations, 512 artifacts/4 MiB, compaction fan-in 32,
64 MiB compaction input/output, 128 MiB temporary disk and a 4 MiB manifest.
Cypher uses the
public embedded request boundary with 32 MiB/query, 2 MiB results and a 60-second
deadline; native queries also receive that deadline. These controls are
not yet a validated aggregate production budget. Cgroup usage includes the Python
controller and page cache, and is not the full-scale RSS measurement.

The default remains `AKASHA_SPIKE_PROFILE=low`. The explicit `high` profile uses
128 MiB vector cache, 64/16 MiB Slate caches, 32 MiB FTS, 512 MiB vector build
cache and 64 MiB unflushed data; request, staging and concurrency limits stay the
same. Every open logs the selected profile. An unknown or non-UTF-8 profile fails
before opening the database. Both configurations pass the native search fixture;
neither configuration alone proves the full RSS/rate contract.

Verified source chain: [Helix mutation commit](https://github.com/HelixDB/helix-db/blob/c72e64f0c7e480f3d125f071659d5e3ad7b8da15/crates/db/src/execution/interpreter/mutation/tx.rs)
uses its owned serializable transaction; [SlateDB transaction](https://github.com/HelixDB/slatedb/blob/2fb25fdaa61d42cc3756f979a8a7b37dbfc483fd/slatedb/src/db_transaction.rs)
commits with default write options whose `await_durable` is true. The runtime
probe verifies process-crash recovery. It does not certify arbitrary power loss,
historical pagination, idempotency, indexing, 100 clients or 50 GB operation.

## Additional diagnostics

`--indexes` creates 100 small fixtures, builds partitioned BM25 and vector
indexes, and checks matching, excluded scope, nearest self and reuse of an
already active definition. Its vectors have only three dimensions; this checks
API behavior, not production-size performance.

`--load PREPARED_JSONL [MANIFEST]` requires a new directory and imports bounded
records in 16-record transactions. With a `prepare_corpus.py` manifest, each batch
atomically stores its document count and input-prefix hash. `--resume-load INPUT
MANIFEST` revalidates that prefix before any new writes and rejects changed
manifests. It checks complete stream size/hash, body bytes, vector and document
counts before marking the import complete. Progress is fsynced to the owned root.
The manifest-free diagnostic form is not resumable. Real precomputed
1536-dimensional input is prepared by `../data/prepare_vectors.py` or the full
corpus preparer. `--build-indexes [SECONDS_PER_INDEX]` builds the Document text
and 1536-dimensional vector indexes. The default two-hour observation deadline
is a harness limit, not a performance acceptance threshold. Full-size runs pass
an explicit budget (up to seven days per index). Every five seconds and at
completion, each family's progress is fsynced with operation identity, actual
stage and counters. Entity counters include validation visits; total unique
coverage and ETA are not inferred from them. Reopening resumes the same native
operation. `check_index_progress.py PROBE REAL_FIXTURE` exercises both native
builds, terminal checkpoints, reuse and invalid deadlines before database open.

Wrap long runs in `../measure.py NEW_SAMPLES_JSONL COMMAND ...` inside a fresh
dedicated systemd service cgroup with a finite memory limit and swap disabled.
The observer records engine RSS/PSS, its own RSS, cgroup usage/events and the
executed binary hash. Cgroup memory includes file cache; it is reported separately
from process RSS. The observer terminates only its owned child on failure.

[Published diagnostic results](../../docs/benchmarks/results/) include the real
38,462-vector import and its raw memory samples. This first shard is 13,344,347
indexed UTF-8 body bytes, including 38,462 added title/text separators, far below
the required full corpus. The first long vector build exhausted its observation
deadline without completing. Reopening the persistent operation under a 2 GiB
cgroup completed it in a further 1,201 seconds; the engine retained its original
32 MiB build cache. The 76,932 reported entities include validation passes over
38,462 inputs. No production backend has been selected.

`../data/prepare_search_fixture.py FIRST_VECTOR_PARQUET NEW_JSON` verifies the
pinned shard and selects one real document with a rare term in each partition.
`--search-corpus FIXTURE_JSON` reopens the database and checks BM25/self-vector
top-20 inclusion and partition exclusion. All 24 queries passed in 2.32 seconds
under a 640 MiB cgroup maximum. This is a process reopen, without dropping the
OS page cache, and not complete recall coverage or a sustained load result.

## Immutable history and idempotency oracle

`--oracle-seed COUNT` appends deterministic revisions to 1,024 logical memories,
cycling scope, retraction and restoration. One authority transaction checks the
head and global counter, appends the immutable revision, switches the head,
stores the request hash/receipt and appends pending derived work. Repeating the
same seed reconciles each durable receipt; changed input with the same ID conflicts.
Synthetic oracle bodies are excluded from all eligible public-corpus byte counts.

`--oracle-verify COUNT` runs in a separate process after close/reopen. It compares
exact identities/content at five commit points, paginates all eight scopes,
checks stale-head and idempotency conflicts and counts revisions/receipts/jobs.
Each operation publishes fsynced progress in the owned database directory.
The active database has one writer; do not run seed/verify against the same root
concurrently. Both commands take the existing database root before the mode.

Historical lookups use one `between` predicate on a composite ordered revision
key. In this pinned engine, expressing the same range as two separate inequalities
made the diagnostic scan much slower. Membership cursors must strictly advance.
This oracle proves the measured history/transaction leaf only: derived watermarks,
live permission revocation, 100 clients and the full-scale profiles remain separate
required acceptance work.

`check_import.py ABSOLUTE_PROBE_BINARY` force-kills its own child after 32 durable
documents, rejects a changed prefix, resumes 1,057 documents including a partial
last batch, repeats resume without duplicates and rejects a changed manifest.
`AKASHA_SPIKE_IMPORT_HOLD_AFTER` is only an explicit crash-fixture barrier; normal
imports leave it unset. Test directories are owned and removed after children exit.

`import_corpus.py INPUT_DIR MANIFEST NEW_RUN_DIR PROBE DATA_PYTHON` pipes the
reproduced corpus directly to the importer. It retains the exact probe binary,
admits unique producer/consumer units durably and separates the 8 GiB Python
preparation limit from the 2 GiB engine limit. Both processes and the complete
stream hash must succeed. No second 84 GB JSONL file is retained. The controller
reserves space for authority, two index generations, scratch and 30 GiB free;
these are planning envelopes, not measured index sizes. It stops its own units
before import consumes the later-index reservation. `--resume` validates run
ownership, source/input/binary identities and dead prior units, then reconciles
the import prefix. Completed runs validate their artifact hashes without writes.
Use the virtual environment's Python path without resolving its symlink.

The streaming controller passes a 38,462-record end-to-end check, completed-run
reuse and negative identity/artifact checks. Full corpus import, indexing and
acceptance remain separate steps.

## Bounded workload cases

Copy `workload.example.toml` outside Git and fill its absolute paths. Run
`python3 spikes/helix/run_case.py CASE.toml`. Use a fresh run ID and output path
for each `steady`, `burst` and `overload` case. The runner reads configuration only
between cases and freezes it, the executable, input manifest and fixture hashes
before effects. Epoch 1 is local to that case's runtime ID; do not aggregate
different case configuration snapshots. Full steady cases require at least 1,800
seconds and a completed import bound to the full manifest. Seed the historical
oracle before starting workload cases in a new full database.

The Rust harness starts 100 independent clients, one operation in flight per
client, with four fair admission slots and at most 100 waiting requests. It
records every result/latency, including real conflicts between writers sharing
identities. Schedules include approximately 4 KiB writes, structured current
reads, real BM25/ANN and exact 100-entry historical pages. Success rates count
only operations completed inside the measurement window. Latency includes client
scheduling delay and queue time. Corrections have their own success count.

Every second the harness syncs request records, appends queue/progress samples and
atomically checkpoints progress. The separate observer measures the entire
engine/client process RSS/PSS and I/O; its Python memory is reported separately.
If cgroup I/O accounting is unavailable, `/proc` counters are explicitly labelled
as currently sampled processes. Derived lag remains unavailable until that
separate acceptance leaf is implemented.

An independent controller oracle reconstructs committed contents from request
identities and compares every current read at its recorded commit point, including
retractions. It rejects wrong content, duplicate/gapped sequences, executable or
resource-profile drift and mismatched raw counts. `--resume` reuses a completed
case only after validating its identity and artifact hashes. A still-live owner
is refused; an ambiguous interrupted case requires `--abandon EXACT_RUN_ID`
before another case may use the database. No measured case is automatically
replayed. A TOML change is a new case, not an in-place change to old work.

The 60-second, 38,462-vector diagnostic completed 1,698 requests from all 100
clients; burst and ten-second overload diagnostics also passed. These are
controller checks, not 50 GB or 30-minute profile acceptance. Current synthetic
corrections do not yet modify the real search candidates; generation watermarks,
live permissions, broad cold-query coverage and recovery remain required.

`../data/prepare_query_population.py INPUT_DIR CORPUS_MANIFEST NEW_OUTPUT`
freezes 128 real queries from each vector shard (3,328 for the full corpus).
Per-shard preparation checkpoints support explicit `--resume`; completed results
are reused after hash validation. Query metadata stays bounded and vectors live
in a checked float32 sidecar, read by offset only for the admitted request.
Full workload cases require this distributed population bound to their corpus.
Each raw search receipt records query identity, returned identities and self@20.
Self inclusion is a ranking measurement for this population, separate from the
exact scope oracle and from recall against an exhaustive top-20 reference.
Neither disjoint partition names nor process restart proves file-cache coldness.

`check_query_population.py PROBE DATABASE QUERY_JSON NEW_RESULT` checks real
BM25/ANN/excluded-scope queries and rejects bad offsets, traversal paths,
dimensions, hashes, zero vectors and NaNs before workload admission.
`check_case_recovery.py PROBE DATABASE QUERY_JSON NEW_EVIDENCE` runs a 60-second
case, kills its exact child after a fsynced successful write, verifies ordinary
resume refuses ambiguity, explicitly abandons that run without modifying raw
artifacts, then executes a successor case and revalidates the prior completion.
The tested abandonment record keeps effects unknown; it never invents success.

`../data/ann_oracle.rs` is a standalone standard-library Rust exact-cosine
scanner. Compile with `rustc --edition 2024 -O -C target-cpu=native` and run
`../data/prepare_ann_oracle.py INPUT MANIFEST QUERY_JSON ORACLE_BINARY NEW_OUTPUT`.
The preparation selects four queries per partition by a frozen hash order,
verifies every source shard and scans all one million float32 vectors. It uses
float64 accumulation, filters by the same partition and resolves ties by key.
Per-shard conversion and exact-scan checkpoints support explicit reconciliation;
temporary vector streams are deleted after the complete result is durable.
The resulting `queries.json` can be passed to `--search-corpus`, which reports
recall@20 separately from exact scope checks. On the 38,462-vector diagnostic,
32 exact-reference ANN queries achieved recall@20 of 1.0; full-corpus measured
recall remains pending even though its reference has been prepared.
