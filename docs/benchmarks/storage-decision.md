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
effects; the database was not presented as cleanly aborted. It has since been
[retired as an unused experiment](results/retired-helix-database/summary.json),
with original metadata and failure evidence retained separately.

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
binary and original persisted index deadline.

[The subsequent full scoped-index runs](results/fallback-scoped-index/summary.json)
retain two distinct outcomes. R2 completes all eight text indexes, then its unit
is OOM-killed during vector partition merge at812/1000 after2374.023s. Its observer
has no native exit record; systemd's journal establishes the unit OOM. Last sampled
OOM counters precede the failure and do not override the journal. Exact processes
and unit are gone before6,150,127,616 allocated bytes of private scratch are removed.
R3 uses the same native binary and640MiB hard limit in a fresh process, preserves
the original deadline, and completes full native coverage in193.798s without OOM.
However, sampled process-tree RSS reaches691,322,880B, exceeding the671,088,640B
peak criterion. Its functional recovery passes; its resource gate fails. The hard
cgroup limit does not replace the frozen RSS test. Uninterrupted bounded building
and sustained workloads still require acceptance.

The reference-query harness now retains all32 exact-ANN references and supports
an explicit generation. A timeout records the child identity, partial output and
exit before failing. The initial failed run lacked those child receipts; its
cgroup samples and that limitation remain archived.

[The full generation2 query run](results/fallback-scoped-query/summary.json) now
passes64 BM25/ANN queries across all eight scopes and1,292 native invocations.
It verifies1,195 distinct candidates against source authority. All32 ANN queries
are compared with their exact top20 references over the complete million-vector
corpus; mean recall@20 is0.9125. This is a measured value, not a newly invented
acceptance threshold. Elapsed time is334.933s; sampled RSS maximum/P99 is
392,826,880/359,129,088B, including the Python query driver. The group retains
memory.high512MiB, memory.max640MiB, zero swap and no OOM. All commands, identities,
artifact hashes and scratch cleanup verify. BM25 expected rank, full file-cache
coldness, live permission changes and sustained workload are separate tests.

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

### Derived corruption and experiment cleanup

[The corruption fixture](results/fallback-derived-corruption/summary.json)
passes85 native checks. It truncates both text-token and vector-index files only
after their native processes exit. BM25 fails on the zero-sized token object;
ANN fails with UnexpectedEof. Neither returns a success body. The fixture moves
the damaged generation offline, builds a new generation from unchanged authority,
and verifies BM25 and ANN in all eight scopes. Complete authority verification
retains the original source hash before and after rebuild. This is explicit
offline quarantine on1,057 synthetic records, not product automatic quarantine.

The first fixture attempt expected a filename absent from Lance's memory-store
error. Its native failure and missing child-identity limitation are retained;
the corrected driver records command/PID/start before each invocation. The final
run takes6.910s, sampled process-tree RSS max142,204,928B. Native processes, unit,
temporary fixture and scratch are confirmed gone.

The unused rejected Helix database contained20,134 files with38,603,812,864
allocated bytes. Exact ownership, stopped known consumers, exclusive locks and
retained receipt hashes were checked before deletion. Its path is absent and
its import/failure metadata is archived. Btrfs free space rose from168,431,259,648
to206,998,777,856B at later verification; delayed reclaim and concurrent activity
prevent exact attribution of that delta. Source shards and active fallback data
remain available; this retired database cannot be reused by later acceptance.

### Full backlog, budget drain and activation

[The full-data transition](results/fallback-full-profile-drain/summary.json)
first advances256 of100,000 pending jobs in the low profile, then resumes under
the high profile with the same persisted deadline. At4,864 completions the harness
requests512MiB while reporting effective2048MiB and draining. The real exclusive
operation lock refuses new low-profile admission. The high native job finishes
naturally in34.092s, sampled RSS max215,044,096B; low reopen takes2.004s at
24,432,640B and leaves the completed checkpoint bytes unchanged. Both watermarks
reach100,000 and generation2 activates at that sequence. Exact child/unit exit,
artifact hashes and private scratch cleanup verify for every operation.

This verifies the experiment operation controller and controlled reopen. Product
daemon hot reload is still separate. The following historical recheck fails
explicitly because it reused the original verifier's expired durable deadline;
there is no content mismatch or OOM. That failure is retained. A new verification
identity is required to recheck history without extending the old operation.

[Explicit verifier identities](results/fallback-verification-identity/summary.json)
now provide that boundary. The old CLI rejects the new form in the retained RED
run. The new native/controller path passes22 cases, including exact same-identity
deadline reuse and malformed-ID refusal. A release unit proves a new identity
leaves the original expired deadline unchanged; the controller observes only the
matching checkpoint. Nine release units, three controller units and Clippy pass.
The independently identified post-activation check verifies all100,000 changes
and5,163 historical invariants in4.015s observed elapsed time, sampled RSS max
72,450,048B. This adds no seed changes and does not relax expired-job handling.

The [first full cold mixed workload](results/fallback-full-cold-workload-r1/summary.json)
fails with a journal-confirmed group OOM after2.749s. Before launch, the exclusive
owner verifies all47,108 files have zero resident pages, covering138,284,851,200
allocated bytes. This proves cold preparation only. The last durable checkpoint
has29/1,698 requests and commit100070; the native terminal receipt is absent.
Exact native/controller termination, artifact hashes and private scratch cleanup
are reconciled, then the run is explicitly abandoned without replaying writes.
Concurrent heavy searches share four general permits; bounding their combined
working set is the next controlled change. Single-query success did not prove
concurrent workload acceptance, and the separate build peak remains unresolved.

[Allocator isolation](results/fallback-allocator-diagnosis/summary.json) narrows
this failure. One heavy-search permit still OOMs. Keeping the low engine settings
under a diagnostic2GiB cgroup reveals about1.06GB RSS. A child-only malloc_info
sampler then observes29 glibc arenas retaining729,096,192B at shutdown, of which
728,283,099B is free; the largest sampled live-allocation estimate is244,664,286B.
These allocator samples are not simultaneous RSS measurements and do not account
for every direct mapping. They support retained free arenas as the dominant
cause, rather than a claim that every retained byte is live application data.

The GNU libc2.36 [allocation tunable documentation](https://sourceware.org/glibc/manual/2.36/html_node/Memory-Allocation-Tunables.html)
explains that a fixed mmap threshold disables its dynamic adjustment and allows
large allocations to be returned on free. A control changes only the launch-time
MALLOC_MMAP_THRESHOLD_=131072 setting, restores the original640MiB cgroup and
one-second observer, and removes the diagnostic preload. Full cold workload RSS
max/P99 falls to363,409,408B with no OOM. Content and derived-state checks pass,
but BM25 reaches only49 in-window successes in60s, below the unchanged1/s gate.
The result remains failed. A [two-search control](results/fallback-heavy-two-control/summary.json)
also meets short-run memory (max/P99 530,878,464B) but only44 BM25 and10 ANN
requests finish inside60s. It is reverted to the verified single-search policy;
the subsequent query-stage profiling below precedes another performance change.
Ten fallback and13 Helix release units plus both Clippy checks cover the shared
admission behavior. Waiting search requests leave general permits available to
ordinary reads/writes. Generation and case controllers now apply the fixed GNU
allocator threshold at child startup, and observer receipts record it. No
long-duration or build-memory acceptance is inferred from these controls.

[Isolated query-stage profiling](results/fallback-query-stages/summary.json)
measures the full cold60s case with one heavy-search permit. It completes all1,698
requests and content checks, with sampled RSS maximum360,960,000B. Mean baseline
result-stream time is0.391s for BM25 and1.108s for ANN; connection/table setup
takes milliseconds, and delta-text queries average0.035s total. The opened table
retains the shared Lance session; no early-connection-drop defect is demonstrated.
Instrumentation changes timing, so this pass does not erase earlier canonical
throughput failures. Their run-to-run difference remains unattributed because
host contention was not recorded. Subsequent observer samples now retain host
load and CPU/memory/I/O pressure; a dedicated-service smoke verifies those fields.
The next required measurement is the first canonical30-minute low case, using
the unchanged native binary and frozen thresholds, not another short diagnostic.


[The first canonical1800s low workload](results/fallback-full-sustained-low-r1/summary.json)
completes50,536 recorded requests in1811.135s. All current/history/source and
final derived-content oracles pass, including581 unvectorized current reads and
198 unvectorized BM25 queries across all eight scopes. Both watermarks reach
147083 and drain takes7.912s. RSS P99/max is430,141,440/453,545,984B with no OOM;
low sustained memory passes. BM25 reaches0.968889/s and BM25/ANN P95 exceed60s,
so the complete case fails. High and full burst/overload remain unrun.

The unchanged delta-text refresh replaces its entire FTS index on each batch.
The retained native callbacks show691 builds,8,286,832 repeated tokenizations,
1652.439 aggregate seconds in tokenization stages and467,116 progress events.
Each event currently persists a checkpoint. Host I/O pressure averages30.36%
for the sampled ten-second `some` values; this is pressure evidence, not device
bandwidth or attribution to another process. Source and measured repetition
justify investigating the pinned Lance incremental FTS path before repeating
full performance acceptance. Original thresholds and this failed result remain.

[Concurrent rebuild entrypoint](results/fallback-concurrent-rebuild/summary.json)
keeps the old generation active while the same authority builds its candidate.
The small60s/100-client fixture completes1,698 requests; nine ordinary commits
and one completed query of each search type fall within the build's sequence
interval. Eight corrupted receipts are rejected. All103 lifecycle commands and
six crash barriers pass separately; those direct subprocess checks are functional
evidence, not cgroup memory acceptance. Ten fallback and13 Helix release units,
both release Clippy checks and six controller units cover the shared plumbing.
The controller now accepts positive u64 generation identities so a fresh third
generation can exercise the full build. Full concurrent rebuild and bounded
uninterrupted full-build acceptance remain pending.

[Incremental delta FTS](results/fallback-incremental-text/summary.json) changes
only the text update path: merge the current index with new fragments, retaining
the original pending-wave deadline and complete native coverage before receipt
publication. A native regression observes three tokenized documents for one new
document before the fix and exactly one afterward. The104-command lifecycle
fixture covers six SIGKILL barriers and rejection/recovery of an incorrect FTS
budget; the same-authority workload again passes1,698 requests. Ten release units,
release all-targets Clippy and six controller tests pass. Child launch explicitly
supplies one FTS worker and32MiB because Lance omits build-time limits from stored
index metadata. Full-data measurements follow below.

[The subsequent full cold60s diagnosis](results/fallback-full-incremental-text-r1/summary.json)
passes all1,698 requests and unchanged short-run thresholds. BM25 achieves1.2/s
with P95 1.474s; ANN achieves0.25/s with P95 1.603s. RSS P99/max is367,230,976B,
and both watermarks reach148183 with0.044s drain. The observer verifies56,261
owned files have no resident pages after eviction. Delta FTS now tokenizes775
new documents across592 updates, taking4.776s aggregate in tokenization stages.
This short result does not establish sustained performance.

[The subsequent1800s low run](results/fallback-full-sustained-low-r2/summary.json)
fails sustained search performance after1,818.988s: BM25 achieves0.638889/s and
ANN0.2/s. Both histogram P95 values exceed60s; raw scheduled-latency P95 is
764.259s for BM25 and315.032s for ANN. These include scheduling debt before
native admission. All49,856 recorded operations have zero errors, current-content
and scoped oracles pass, RSS P99/max is441,647,104/452,685,824B with no OOM,
and both watermarks reach182001 with15.167s drain. Delta tokenization falls to
21,854 new documents across9,480 updates and97.313s of tokenizer stages.
An inference from request completion/scheduling times estimates search service
cost rising from0.85s to1.77s across successive five-minute windows. This includes
admission overhead and is not direct phase timing; inspect growing fragment and
metadata work before raising search concurrency. High, burst and overload remain
unrun. All exact processes, units, scratch removal and archived hashes are checked.

### Actual corpus-candidate lifecycle driver

[The small native fixture](results/fallback-corpus-lifecycle/summary.json) passes
16 cases across eight scopes and both search modes,64 logical revisions and756
native commands. It corrects, rescopes, retracts and restores real returned
candidates, checks stale-hit rejection and exact current/history bodies, restores
original vectors, and revokes/restores live permissions. A SIGKILL after a durable
commit but before reply-cache publication resumes with the identical receipt and
no duplicate revision. Completed reuse leaves all case files unchanged; changed
fixture/reply bytes and seven forged current-entry fields are rejected.

The initial collector incorrectly expected a `reused` field, and its first
continuation lacked an import path. Both failures are retained separately; the
continuation verifies the existing native result and runs only remaining controls.
All ten observer services and native children exit, and the owned fixture is
removed. This serial driver establishes neither full-data behavior nor simultaneous
query/build scheduling. No production code changed in this driver leaf.
