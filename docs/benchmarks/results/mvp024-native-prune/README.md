# Native index maintenance

This is a partial result for [#65](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/65).
Full production scale, concurrency, vector coverage and recovery remain pending.

Production sync now attempts native version pruning after 32 authority windows
and at completion. It holds the index cache lock and requires exactly the cache
and builder to own the same index. Existing readers defer reclamation; the lock
prevents new readers entering during cleanup. Each native table operation checks
permission, job cancellation, service shutdown and the original workflow deadline.
The current lexical table and cached vector table retain their latest versions.
Authoritative current/history records and prior vector generations are unchanged.
Recent unverified orphan files remain protected by the native cleanup policy.

Two real-daemon regressions first failed their final retained-version assertions:
idle synchronization retained 6 lexical / 7 vector versions, and synchronization
after a held query retained 4 / 4. They pass unchanged with one version per table.
They also compare all native IDs/revisions, current/history payloads, and prove
that a query holding the index defers reclamation until a later idle sync. Eight
library and 25 daemon retrieval tests pass, with one existing ignored child
helper; all-target Clippy and formatting pass. The normal implementation commit
is `c0b9a969cc151ddc059c464d5dde1d5dfcdd0c0b`.

The third real-corpus pilot uses the same 128 DBpedia and 896 FineWeb records as
the [bounded-batch baseline](../mvp024-production-pilot/README.md). Inputs, policy,
binding, every acknowledgement and the complete 1,042-claim oracle are identical.
Every current claim hash and all eight scoped lexical witnesses pass again.

| Observed value | Bounded batches | Batches with pruning |
| --- | ---: | ---: |
| Daemon source | `b179076` | `c0b9a96` |
| Unique source UTF-8 bytes | 4,449,979 | 4,449,979 |
| Current claim checks | 1,042 | 1,042 |
| Seed seconds | 4.017 | 5.475 |
| Index seconds | 10.559 | 10.491 |
| Search allocated bytes | 86,781,952 | 5,332,992 |
| Search files | 912 | 77 |
| Native versions | 132 | 1 |
| Authority allocated bytes | 12,722,176 | 12,726,272 |
| Daemon RSS sampled peak, bytes | 489,836,544 | 485,277,696 |
| Daemon cgroup charged peak, bytes | 473,677,824 | 461,451,264 |
| Resource samples | 12 | 12 |
| Observed OOM kills | 0 | 0 |

The unchanged seeder SHA-256 is
`3cd4f4f497af240805506bdcdd6ad3c8caf15e413e266d996ae31dc560e65146`;
the new daemon SHA-256 is
`b32485c8d0745be5f7cff8535ccf45994693656293fe0dca847839b59d53aca2`.
These are single observations without independent repetition or cache eviction.
They establish neither a memory improvement nor a full-scale operating envelope.
The pilot has no ANN load or complete native-body oracle; continuous readers can
defer pruning. This change does not compact fragments or reclaim every orphan.

[evidence.zip](evidence.zip) retains 90 members in 513,731 compressed bytes.
[manifest.json](manifest.json) hashes every member and the archive. It includes
the unchanged RED/GREEN tests, related checks, build/commit identities, full pilot
receipts, source preimages and cleanup evidence. Every member was decompressed
and verified. Final canonical checks, independent review and publication remain
separate steps.

The first cleanup guard refused unrelated inaccessible procfs entries before
deleting anything. The subsequent scoped check verified both pilot processes
and controller absent, their owned cgroup removed, and no accessible live handles
to the two disposable roots. It recorded the excluded pre-existing services.
Only the stopped database and socket directory were removed; compact metadata,
shared corpus and build cache remain. Filesystem free space did not immediately
change, so no exact physical-space gain is inferred. No new Daybreak or
adversarial campaign was run.

Both native tables also need a BTree on the merge key. Commit `e36916a` adds the
`key` index beside the existing `scope` index, using the existing initialization
path. A real-daemon regression first proves both key indexes absent and both
lookup plans scanning; the unchanged test then proves both indexes present,
correct single-row results and native `ScalarIndexQuery` plans. This checks the
ordinary lookup plan, not a directly observed merge execution plan. Two earlier
fixture compilation failures are retained separately from the behavioral RED.
Eight library and 26 daemon retrieval tests pass, with one existing ignored child
helper, plus all-target Clippy and formatting. The original pruning assertions
remain unchanged.

Two subsequent pilots use identical 1,024 DBpedia and 7,168 FineWeb source
records: 35,875,218 unique UTF-8 bytes become 8,322 memory claims. The manifest,
policy, binding, every seed acknowledgement and complete expected claim oracle
match byte for byte. **Both pilots fail during indexing** at the public
`daemon status` command's unchanged 10-second IPC timeout. Neither reaches the
complete current-claim checks or partition witnesses.

| Observed value | Pruning, `767b39e` | Pruning and key index, `e36916a` |
| --- | ---: | ---: |
| Last published indexed rows | 3,140 / 8,322 | 3,380 / 8,322 |
| Index elapsed at last progress, seconds | 476.163 | 142.786 |
| Daemon RSS sampled peak, bytes | 691,466,240 | 689,385,472 |
| Samples over the 640 MiB peak limit | 102 / 142 | 32 / 74 |
| Daemon cgroup charged peak, bytes | 566,718,464 | 560,123,904 |
| Observed OOM kills | 0 | 0 |
| Final native store allocated bytes | 114,900,992 | 190,480,384 |

The successor's passive procfs breakdown reaches 562,397,184 anonymous bytes
and 125,710,336 file-backed bytes; its last kernel `memory.high` event counter is
69,779. This establishes sustained memory pressure, not the precise cause of the
IPC timeout. Zero observed OOM kills does not cover the unsampled final interval.
The different partial completion points are not comparable completed-run latency
or disk benchmarks, and the key index did not resolve this scale failure.
The next step is a bounded discriminating memory observation before another
scale attempt; limits, timeouts and acceptance thresholds remain unchanged.

[scale-comparison.zip](scale-comparison.zip) retains 119 members in 2,173,203 bytes;
[scale-comparison-manifest.json](scale-comparison-manifest.json) hashes every
member and the archive. Identical pilot inputs/oracles are stored once. Both
failures, source/binary identities, all metadata, complete stopped-store file
hashes and guarded cleanup receipts remain. Both controllers, daemons, seeders
and owned cgroups exited before their disposable store/socket roots were removed.
No immediate filesystem free-space gain is claimed. The included historical
canonical PASS (638 Rust / 39 ignores, Python 35 / Node 7) belongs to `767b39e`;
the enlarged candidate still requires its own final gate and independent review.
