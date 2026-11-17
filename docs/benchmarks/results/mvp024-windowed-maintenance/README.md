# Windowed native index maintenance

The production prefix contains 65,536 real sources and 65,531 eligible vectors.
All original and current reads passed before indexing. The two index attempts
remain incomplete. [Issue #65](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/65)
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
Ten related retrieval tests pass. One crash child is intentionally ignored.
The first formatter launch lacked the established toolchain environment and failed.
Formatting passed with that environment. No full gate or review result is claimed here.

Separate 2,048-row native diagnostics retained exact row digests, scoped queries
and reopen behavior. Compaction alone left 128 vector fragments unchanged because
their index coverage differed. Compaction before windowed optimization left five
fragments per table. Six queries found newly appended unindexed rows.

A valid-wide-row diagnostic used 384 rows with 119,537,664 logical bytes.
A 64-MiB source cap made no progress. A 256-MiB cap reduced 24 fragments to three.
Both used an 8-MiB scan buffer. The successful case reached 626,792 kB process
high-water RSS, close to the 640-MiB peak limit. This is diagnostic evidence,
not a steady-state memory result. All disposable diagnostic binaries and stores were removed.

The old fragmented production store needs an explicit native rebuild.
Real-prefix rebuild, source-transition qualification and measured growth remain pending.
The small diagnostics do not establish full-scale speed, memory or ANN recall.

## Evidence

`evidence.zip` contains 129 members and 5,714,889 compressed bytes, SHA-256
`c5f5f6a56b77ec69560ccb79b1479c34720b7b076cbb468f237ef288f282c028`.
`manifest.json` records each member's size and digest. The archive preserves
both baseline attempts, raw resource samples, partial lookup audits, control
scripts, stopped-store inventories, diagnostic failures and focused tests.
The nested diagnostic archive has its own inventory and checksums.
No native database, raw corpus copy or temporary executable is included.
