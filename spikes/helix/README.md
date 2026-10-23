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
text backfill/query limits are upstream defaults in this small probe; these are
not yet a validated aggregate production budget. Cgroup usage includes the Python
controller and page cache, and is not the full-scale RSS measurement.

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

`--load PREPARED_JSONL` requires a new directory, imports bounded records in
16-record transactions and closes cleanly. Failed imports are deliberately not
resumable. Real precomputed 1536-dimensional input is prepared by
`../data/prepare_vectors.py`. `--build-indexes` builds the Document text and
1536-dimensional vector indexes; its two-hour observation deadline is a harness
limit, not a performance acceptance threshold.

Wrap long runs in `../measure.py NEW_SAMPLES_JSONL COMMAND ...` inside a fresh
dedicated systemd service cgroup with a finite memory limit and swap disabled.
The observer records engine RSS/PSS, its own RSS, cgroup usage/events and the
executed binary hash. Cgroup memory includes file cache; it is reported separately
from process RSS. The observer terminates only its owned child on failure.

[Published diagnostic results](../../docs/benchmarks/results/) include the real
38,462-vector import and its raw memory samples. This first shard is 13,344,347
UTF-8 content bytes, far below the required full corpus. The live long vector
build is not yet an acceptance result. No production backend has been selected.
