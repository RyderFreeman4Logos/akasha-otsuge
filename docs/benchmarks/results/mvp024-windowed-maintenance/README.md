# Windowed native index maintenance

The production prefix contains 65,536 real sources and 65,531 eligible vectors.
All original and current reads passed before indexing. The first two index attempts
ended incomplete. R3 subsequently indexed the entire prefix. [Issue #65](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/65)
retains the full corpus, workload and recovery requirements.

R1 stopped because the disk observer encountered a file removed by native pruning.
R2 reused the closed store and the completed read checks. At 09:37 UTC on
2026-11-17, R2 reported 61,834 vectors and 61,839 lexical rows. Its recent rate
was 0.856 vectors/s. Physical reads increased by 727,467,528,192 bytes in
1,570.937 seconds, with sustained reclaim. The parent requested native cancellation.

The job ended as `cancelled/CANCELLED`. The daemon and lookup service exited 0.
The controller exited 1. All owned processes exited and the socket was removed.
The original absolute deadline and resource limits stayed unchanged. A closed-store
digest inventory covers 14,079 files and 2,354,728,960 allocated bytes.
The last progress counters are observations before shutdown, not final offline counts.

The 2,787 index-period samples recorded no OOM events. Sampled RSS P99 was
524,890,112 bytes. Sampled maximum was 540,213,248 bytes. Process high-water RSS
was 551,141,376 bytes. These samples do not cover the required workload or shutdown tail.
The partial lookup audit remains available, but no complete pair reconciliation is claimed.

## Change and focused checks

The daemon maintains native tables after 32 publication windows or at completion.
Each publication window still covers at most 32 authority transactions. Maintenance
compacts unindexed fragments before index optimization. It skips unchanged native
versions and retains the existing authorization and exclusive pruning checks.
Native queries include new rows before the next maintenance window.

The regression checks new-row search, denied maintenance, fewer fragments,
unchanged authority progress, old-version readers, repeated maintenance and reopen.
The initial focused run passed ten retrieval tests with one intentional ignore.
The first formatter launch lacked the established toolchain environment and failed.
Formatting passed with that environment. The subsequent gate and review are recorded next.

## Review correction

The exact `c861c26` canonical gate passed 653 Rust executions, 35 Python tests
and seven Node tests. Independent R1 review then found one actionable defect.
A query pinned to the old vector generation could replace the shared cache
while the new generation's writer awaited its batch. Maintenance then selected
the old cached table instead of the writer's table.

The daemon now retains the writer's vector object through its batch and passes
that object to maintenance. Queries can still select the previous generation.
The deterministic regression switches the real cache between two native tables.
Old code left 48 writer rows unindexed. The repaired code indexes all 64 rows,
preserves the reader's native version and returns all rows from both tables.
The authority sequence stays unchanged. Eleven retrieval tests pass with one ignore.
The fixture data and assertions are unchanged. GREEN adapts the call to the new
explicit argument, so raw test bytes and selected command scopes differ.

The first fixture attempt failed compilation because it lacked a required trait
object cast. That setup failure remains separate from behavioral RED.
The reviewer exited 0 with a FAIL report. Its wrapper exited 3 because two
runtime cache directories remained. Parent cleanup checked consumers and removed
only that private scratch directory. The failed report remains unchanged.
The corrected candidate still requires its own canonical gate and independent review.

`review-correction.zip` contains 49 members and 94,600 bytes, SHA-256
`5a98ddd07162bb83af3be8f71d9ee01f7099560303f94bbda710b1d830f4aa5c`.
Its manifest records each member hash. The archive preserves the full R1 report,
historical gate, cleanup receipts, fixture compilation failure, native RED and GREEN.

## Bounded native diagnostics

Separate 2,048-row native diagnostics retained exact row digests, scoped queries
and reopen behavior. Compaction alone left 128 vector fragments unchanged because
their index coverage differed. Compaction before windowed optimization left five
fragments per table. Six queries found newly appended unindexed rows.

A valid-wide-row diagnostic used 384 rows with 119,537,664 logical bytes.
A 64-MiB source cap made no progress. A 256-MiB cap reduced 24 fragments to three.
Both used an 8-MiB scan buffer. The successful case reached 626,792 kB process
high-water RSS, close to the 640-MiB peak limit. This is diagnostic evidence,
not a steady-state memory result. All disposable diagnostic binaries and stores were removed.

The retained prefix rebuild at `6bc6efd` reached 62,975 lexical rows and
62,970 of 65,531 eligible vectors before daemon status timed out. Both the
graceful shutdown and the subsequent SIGTERM wait timed out. After the original
deadline expired, the daemon was still reclaiming memory. Its cgroup had
recorded over 18 million soft-limit events and zero OOM kills. The daemon
exited after the soft limit was raised for cleanup. The closed store contains
732 files using 1,795,784,704 allocated bytes.

The last completed fragment held 512 more vectors. In the existing index,
the largest of the five partitions contained 16,300 rows. Those additions
raised the partition to 16,432 rows under cosine assignment ([partition counts](ivf-partition-threshold.json)).
This crossed Lance's 16,384-row split threshold, where the default optimization
can merge every existing delta. The later R3 run and growth measurement follow.

## R3 real-prefix index and search recovery

At commit `4b9545216449d0ea6202785ae1e748f0711f8ffa`, R3 rebuilt 65,536 lexical rows
and 65,531 vectors in 6,812.94 seconds. Both watermarks reached 225,057. The
provider's first 65,531 ordered input/vector pairs exactly matched the source
oracle. Eight scoped keyword witnesses passed. During indexing, 1,350 samples
gave a daemon-group RSS P99 of 385,900,544 bytes and a maximum of 386,342,912
bytes. The daemon read 987,680,768 and wrote 7,064,563,712 physical bytes
between the first and last samples.

| Observation | Recorded outcome |
| --- | --- |
| First hybrid request on the warm daemon | The lookup prepared one query vector. Job observation returned `IPC_TIMEOUT`. Shutdown advanced through SIGTERM to SIGKILL. Kernel samples end before this interval. The cause is unresolved. |
| Reopened daemon | The original job recovered as `failed/PROVIDER_INTERRUPTED`. All eight fresh scoped hybrid queries returned the expected memory and revision using the same lexical and vector generations. Responses retained the `rule_scan_budget` and `index_lag` coverage gaps. |
| Independent native ANN diagnostic | The raw vector leg at Lance table version 4,491 returned all 640 exact top-20 neighbors across 32 queries and eight scopes. The read-only probe used 1,792/2,048 MiB cgroup soft/hard limits. |
| 130-delta synthetic control | At 512/640 MiB, the staged 8,192-row fixture returned 100/100 in 1.961 s. With 488 MiB of touched anonymous memory in the same cgroup, it obtained a native result stream but had no completed rows at the 65 s deadline. The paired runs gave 100/100 in 0.685 s and a censored 105 s respectively. Neither pressured run recorded an OOM kill. |

The closed failure inventory contained 1,150 files and 2,196,033,536 allocated
bytes. The recovery inventory contained 1,151 files and 2,196,242,432 bytes.
Three authority files changed. The search files retained their identities.
A separately admitted import then doubled the authority to 131,072 records in
32 units, with 131,063 eligible vectors and 2,569,203,712 allocated bytes.
Its seal retains the identical search inventory. Incremental index, full-corpus
and 100-client workload acceptance remained open at that measurement.

The [R3 evidence and replay bundle](prefix-r3.zip) is 60,203 bytes,
SHA-256 `de753368e6b89333af8c7fb096698cfa84c283c3c3dc54a094ec00cb30a1a442`. Its manifest covers every payload. To
recompute the sample percentiles, physical I/O and per-query recall, extract
the archive into an empty directory and run `python3 verify.py` there. The
included instructions also reproduce the exact cosine reference from pinned
public Parquet rows and run the controlled native query in a disposable checkout.
The closed native store, live-query measurements and full receipt hashes
remain independently auditable on the original host.

## Evidence

`evidence.zip` contains 129 members and 5,714,889 compressed bytes, SHA-256
`c5f5f6a56b77ec69560ccb79b1479c34720b7b076cbb468f237ef288f282c028`.
`manifest.json` records each member's size and digest. The archive preserves
both baseline attempts, raw resource samples, partial lookup audits, control
scripts, stopped-store inventories, diagnostic failures and focused tests.
The nested diagnostic archive has its own inventory and checksums.
No native database, raw corpus copy or temporary executable is included.
