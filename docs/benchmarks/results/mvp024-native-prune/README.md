# Exclusive native-version reclamation

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
