# Controlled storage budget reopen: completed leaf

PR #63 merged as 8e14ee61c63b4dafe486e98ce5ca1f251f213001. Its tree
498c2d9564cc34358b5a76b39422b5d74fe18017 is identical to reviewed source
0de2704f5030102291ea492baeda89a3eb183818. The local main checkout and reused SSD
worktree were synchronized and clean. Issue #24 remains open.

The daemon retains its PID/socket through budget changes, pauses new admission
and claims, drains accepted owners with their existing job identities and
deadlines, closes storage and opens the requested budget. Busy replies remain
responsive through native initialization. Pending/rejected configuration,
repeated or reverted budgets, watcher updates and terminal Stop/signals retain
their contracts. First-error recording covers accept, scheduler and owner-first
ordering without abandoning cleanup.

| Final evidence | Result |
| --- | --- |
| Exact-source just check | PASS, 497.610 seconds; 613 Rust executions/35 intentional opt-in ignores, including the installed repeat; 34 Python and 7 Node tests. |
| Installed release | Lifecycle and private/peer UID controls PASS, zero TCP sockets; SHA-256 601aba781f7668c9d6409969dcb652f994c6442177d7a7e070e0708e97875d7a. |
| Independent conventional R2 | PASS, zero actionable findings; all four commits/95 paths assessed, 30 fresh functional executions passed. |
| CI42951891062 | SUCCESS on exact 0de2704; full log and actual Cargo cache restore/hit readback retained. |
| Merge/readback | Identical reviewed tree; complete report and issue-progress bodies read back exactly. |

The complete immutable R2 report is included. Parent postflight found five
inaccurate line coordinates and checked the named symbols/control flow in the
frozen Git blobs. The separate citation-corrections receipt and superseding
verified-r2 receipt record those corrections; the original report and earlier
verification receipt are preserved. No semantic source contradiction was found,
no product bytes changed, and tests were not repeated to fix line references.
The public body contains that explicit parent note and the complete original
report as an exact suffix; its whole-body hash is distinct from the report hash.
The report's post-test attestation precedes its final write; the wrapper and
parent re-attested the final report and frozen product tree after completion.

[Full public review](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/63#issuecomment-6786312238)
and [issue progress](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/24#issuecomment-6786357770)
retain the completion boundary. The review's current-CI disclaimer is retained
verbatim; the parent separately collected the successful current CI.

Prior failures remain in the original, review-fix and owner-order archives.
The R1 FAIL is not rewritten. Historical CI42949516550 failed in an unchanged
Codex caller bind with HistoryGap; its cause is still UNCLASSIFIED despite local
passes and the fresh promoted-candidate CI success. No blind same-tree retry was
used. Those historical receipts do not become current-commit evidence.

Raw completed reviewer transport files (461,534 bytes) were hashed and removed
after publication; full report, tests and compact receipts remain. No live build
artifacts, reusable corpus, user files or shared cache were reclaimed.

Remaining #24 work includes actual measured-memory admission, daemon/runner/
service limits, production corpus/index coverage, required 50 GB/100-client load
and recovery acceptance. Functional passes do not establish that acceptance.
No Daybreak or new red-team campaign was run.
