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
No production dependency has yet been added. The isolated probe now pins the
lockfile and dependency licenses, and passes native transaction/crash/history,
small BM25/IVF, bounded resumable import, authority WAL-corruption and isolated
kernel ENOSPC checks. Initial inline-value WAL replay stalled under the low
profile; native VLog corrects that layout problem on 38,462 real vectors, with
all original bytes verified after direct low reopen. See the
[measured layout comparison](results/fallback-vlog/summary.json), including the
intentional stop and unchanged limits. Compressed full import and low-memory
verification of every original record now pass. Full-corpus 100k history and
all-record native backfill also pass; see the [retained receipts](results/fallback-full-history-backfill/).
Full native text/vector index coverage now passes with bounded scan I/O,
streaming IVF training, private scratch and one build worker. Sustained workloads
and the remaining recovery matrix are still unproven. All failed attempts and
the original native deadline remain retained. Reuse source shards and query oracles, and do not retain
an expanded corpus copy.

The [frozen contract](storage-contract.md) and [remaining checklist](../plan/todo.md)
apply unchanged. If this route also fails a required contract, report the concrete
failure and decision needed; do not choose a third backend or mark dependent
Issues complete.

### Shared workload and real-shard outbox

[Outbox receipts](results/fallback-history100k-catchup/summary.json) verify all
100,000 text-only jobs and activation on the real 38,462-vector shard.
[Shared workload receipts](results/fallback-shared-workload/summary.json) preserve
the original scheduler, thresholds, fixtures and independent Python oracle for
both candidates. Fallback passes burst, a 60-second mixed run (1,698 requests,
22.2 successful writes/s, write P95 219ms, sampled RSS max 299,061,248B), and
12,031-request overload with 0.621s drain. The 16-record mixed fixture additionally
checks unvectorized current/BM25 results in every scope. Helix compatibility is
retained separately; none of these diagnostics replaces full-size acceptance.

The final admission-only correction has a retained failing control and passing
controls for both rebuilt backends: malformed full-scale evidence is rejected
before authority initialization. Runtime measurements remain bound to their
recorded earlier binaries; the correction does not change scheduling or queries.

### Full index training bound

[Training receipts](results/fallback-index-training/summary.json) preserve two
failures. With scan I/O bounded, full FTS completes but vector training OOMs at
640MiB. The existing native streaming API keeps sample_rate=16 while limiting
each batch to four vectors per partition. At one million vectors it completes
training in 17.845s and assigns every vector to a partition; sampled RSS maximum
is 466,124,800B, with no OOM. The separate page-metadata spill finalization then
fails with ENOENT, so this is not full-index acceptance. The bounded syscall
diagnostic and following scratch correction are recorded below.

Release Clippy/build, six units and 48 native search checks pass for this change.
The locked Lance crate archives and the inspected training source hashes match;
no upstream source is modified. Partition count, total sample budget, iteration
limit, resource profile and original absolute deadline are retained. Cumulative
inner k-means iterations are not unique row coverage or a trustworthy total.

### Private native scratch

[File-syscall and cleanup evidence](results/fallback-private-scratch/summary.json)
reproduces the ENOENT: a scratch file is created, but its final rename fails; no
removal of that file or directory appears in the traced process tree. The actor
that removed it is not established. A private observer copy permits only its
exact strace parent for this diagnostic; its timings are not acceptance.

The ordinary observer now places TMPDIR beside its owned receipts and removes
it only after every native descendant exits. Three native controls cover success,
failure and live-descendant cleanup refusal; all four procfs controls still pass.
The same native binary then completes shuffle on full data, but OOMs at partition
584/1000 during merge. After exact process and unit exit checks, 6,150,127,616B of
that attempt's exclusive scratch is removed; authority and committed indexes stay
intact. The single-worker result is recorded below; sustained workload acceptance
remains open.

### Full native index coverage

[The single-worker receipt](results/fallback-full-index/summary.json) completes
all 1,000 IVF partitions in 164.265s. Native index statistics verify every declared
11,428,831 text row and 1,000,000 vector row is indexed, with no unindexed rows.
The generation is ready; pending work and activation are separate. The unchanged
640MiB hard limit, disabled swap and original absolute deadline remain in force.
Sampled RSS maximum/P99 is 610,480,128B, with no OOM; this build measurement does
not satisfy or substitute for the steady 512MiB P99 workload criterion. The
observer confirms private scratch removal after process exit.

The only change from the preceding merge failure is the existing native worker
limit from two to one. Total training sample, partition count and iteration limit
are unchanged. Full exact-ANN queries, ordinary-change catch-up/activation, both
30-minute profiles, cold reads and the remaining failure matrix are still open.

### Full-query memory and scoped dictionaries

[Query diagnosis and layout controls](results/fallback-scoped-fts/summary.json)
show the original full BM25 query cannot meet the low profile: the first query
times out at180s, including with I/O concurrency reduced to1. Native source loads
and retains all435 token dictionaries (861,893,690B of files). The same query,
unchanged low engine caches and I/O1 in a2GiB diagnostic group, returns in7.170s
with RSS1,157,300,224B. This diagnoses residency; it is not low-profile acceptance.

New generations place each source body in exactly one nullable scope column,
with a separate native FTS index for each of the eight frozen scopes. The query
selects that column explicitly and still filters scope. Original body bytes are
not duplicated among columns. Persisted layout identity preserves legacy
generations and the delta-table shape. Seven native units,70 native search and
legacy-layout checks, and103 lifecycle checks pass under the low group. Full
scoped-generation acceptance is pending; source/authority data is reused.

[Full scope-column backfill](results/fallback-scoped-backfill/summary.json) now
covers all11,428,831 text rows and1,000,000 vectors in974.591s; sampled RSS max
is276,017,152B. The first scoped index attempt is interrupted at205.737s by the
controller parsing an incomplete trailing progress line. Its last samples show
no OOM; there is no native terminal exit receipt. Exact process/unit exit and
owned scratch cleanup are verified before explicit abandonment. A deterministic
RED/GREEN check fixes the observer to read only newline-terminated records while
still rejecting malformed complete records. The successor uses the same native
binary and original persisted index deadline; its result remains pending.

The reference-query harness now retains all32 exact-ANN references and supports
an explicit generation. A timeout records the child identity, partial output and
exit before failing. The initial failed run lacked those child receipts; its
cgroup samples and that limitation remain archived. No ANN recall is yet claimed.

### Cold-file preparation and workload reservations

[The real-shard cold case](results/fallback-cold-workload/summary.json) verifies
all2,383 owned database files have zero resident pages immediately before engine
launch, under the same exclusive database lock. This is measured with mincore
after per-file fsync/fadvise; host caches are untouched. Those files were already
nonresident at admission. A separate warm-file test proves eviction and unchanged
bytes, and rejects foreign locks, competing owners and symlinks.

The subsequent60s/100-client case completes1,698 requests,144 synthetic and144
corpus current checks,72 BM25 queries and15 ANN queries. All original rates and
latencies pass; sampled RSS maximum/P99 is324,235,264B. Raw cold-file receipts,
commands, memory and request records are hashed; completed reuse is unchanged.
This covers38,462 real vectors and does not establish full-corpus cold coverage
or either30-minute profile.

Full workload admission previously omitted the45GB rebuild allowance when the
pinned generation was2. Both the native shared scheduler and controller now
retain81,212,254,720B for either generation. Native and controller RED/GREEN
controls verify that boundary before database access. Both candidates' release
units and Clippy pass; no frozen acceptance threshold changed.
