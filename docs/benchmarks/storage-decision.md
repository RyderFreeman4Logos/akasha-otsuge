# Storage candidate decision record

2026-10-23. MVP-003 remains open; no production backend is selected.

## Helix candidate: failed full-data low-memory recovery

The pinned Helix v3.5.1 / SDK 3.0.0 candidate imported the complete manifest:
11,428,831 documents, 50,000,002,783 eligible unique UTF-8 bytes and one million
real 1536-dimensional vectors. Its smaller transaction/history/search/concurrency
diagnostics passed. Full-size indexes and sustained workloads did not pass.

The original unique ordinal-index preparation exhausted its 7,200-second
observation window. That timeout alone did not eliminate the backend. After
implementing ordinary immutable-source lookup with duplicate rejection and an
exact-operation abort, the full cleanup was deliberately interrupted under 2GiB
and reopened under the low profile, with its original absolute deadline retained.

All three low-profile runs failed with systemd `oom-kill` under a 640MiB resource
group and swap disabled:

| Run | Native binary SHA-256 prefix | Changed setting | Seconds | Sampled native RSS max |
| --- | --- | --- | ---: | ---: |
| low-v1 | 4e50aeb7 | baseline | 175.399 | 595,562,496B |
| low-v2 | 2dea2798 | compaction ranges 4 to 1 | 44.399 | 674,660,352B |
| low-v3 | 006fbe3c | encoded prefetch 2MiB to 256KiB | 16.142 | 676,163,584B |

These are successive recovery states, not paired timing measurements. The
resource group includes the observer and page cache; RSS sampling may miss the
terminal peak. The first failure does not establish a sustained RSS violation.
All three do establish failed completion under the required recovery budget.
Observer exit events are absent after OOM; journal records supply the terminal
classification. Exact processes are gone, units inactive and retained artifact
hashes verified. Explicit abandonment preserves failed outcomes and unknown
effects; the retained database is not presented as cleanly aborted.

[Raw evidence and per-file hashes](results/compaction-range-recovery/) preserve
the controlled crash, three failed runs, exact settings, journal messages,
memory samples and single-setting RED/GREEN checks. Neither tuning change solved
the failure. This rejects the tested candidate for the frozen MVP contract; it
does not prove that every possible Helix configuration or future version fails.
No memory/latency threshold was weakened and no deadline was reset.

## Required fallback: validation in progress

Proceed only with the fallback already named in the specification: Apache-2.0
[SurrealKV 1.0.0](https://github.com/surrealdb/surrealkv/releases/tag/v1.0.0),
commit `4338a790c8edc56ef5ea1e46157427af4815be3c`, for authority, and
[LanceDB 0.40.0](https://github.com/lancedb/lancedb/releases/tag/v0.40.0),
commit `b1b080b97c3edd2fd9f2aae8a3b71ff19487b1f8`, for derived search.
No production dependency has yet been added. Rust API availability, exact
dependency licenses, lockfile, primitive durability and resource controls must be
verified before the same full-data acceptance. Reuse source shards and query
oracles; do not retain an expanded corpus copy.

The [frozen contract](storage-contract.md) and [remaining checklist](../plan/todo.md)
apply unchanged. If this route also fails a required contract, report the concrete
failure and decision needed; do not choose a third backend or mark dependent
Issues complete.
