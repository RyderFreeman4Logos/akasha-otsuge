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
- LanceDB exposes `Session` caches and a `LocalSpillStore` cap through real Rust
  APIs. Cache units are native weighted budgets, not process RSS. FTS construction
  exposes per-worker `memory_limit_mb` and `num_workers`; vector training and
  shuffle still require their own budget/call-chain verification.

After primitives: bound/resume streaming import using the existing manifest and
source shards, implement authority/outbox and derived generation reconciliation,
then run the complete unchanged workload and recovery matrix. The original
84GB expanded stream is never materialized. Existing Helix diagnostic passes do
not transfer to this implementation.
