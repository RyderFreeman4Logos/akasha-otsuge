# Helix embedded diagnostic probe

Temporary MVP-003 experiment, outside the production Cargo workspace. This is
an initial primitive test, not the full storage harness or backend selection.
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
corpus preparer. `--build-indexes` builds the Document text and
1536-dimensional vector indexes; its two-hour observation deadline is a harness
limit, not a performance acceptance threshold.

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
