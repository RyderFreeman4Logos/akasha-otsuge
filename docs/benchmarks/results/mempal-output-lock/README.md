# Mempal output-lock lifetime

Commit `1ebec1478cf6b896b349827ad131cc630d67d7b5` explicitly unlocks the
conversion directory and pinned lock file when their output owner is dropped.
The `Writer` still drops its buffered sinks before that owner. Full and
incremental conversion use this same path. No archive format, identity check,
conflict policy or active-writer exclusion changes.

Closing a file alone can leave its lock held while another descriptor references
the same open file description, including between a concurrent fork and exec.
The existing storage admission guard already explicitly unlocks at owner release.
A new ordinary regression preserves a duplicate of the directory descriptor and
lock-file descriptor separately. It checks that an active writer excludes a
successor and that dropping the writer admits one while the duplicate remains.
Both cases genuinely failed with `MEMPAL_CONVERSION_BUSY` before the change;
the exact same test passes afterward. Five Mempal library tests and 17 public
full/incremental migration tests pass, with one intentional library child helper
ignored. All-target Clippy and formatting pass.

This repair was investigated after the pruning branch's canonical gate failed
at `2e320a4`: the unchanged moved-snapshot test expected
`UNSAFE_MEMPAL_SNAPSHOT` for `manifest.json.tmp`, but got
`MEMPAL_CONVERSION_BUSY`. That exact test passed alone on the unchanged candidate.
The failed invocation did not identify which lock or process retained ownership,
so its cause remains **UNCLASSIFIED**. The deterministic duplicate-descriptor
test proves a separate existing lifetime defect; it does not establish the cause
of that historical occurrence or turn the original failed gate into a pass.

[evidence.zip](evidence.zip) retains 50 members in 77,272 compressed bytes.
[manifest.json](manifest.json) records every member's size and SHA-256, plus the
ZIP digest. All members were decompressed and verified. The archive includes the
original failed gate and isolated classification, the unchanged RED/GREEN oracle,
related checks, normal commit receipt, source preimages and the separate pruning
context. That pruning candidate remains intact on its branch and must be
qualified again after integrating this repair.

PR [#68](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/68) is merged as
`9f2ba152529a796383204a8a98dd89a5c6af6616`, with exactly the reviewed `e7cef25`
tree. Its exact-HEAD canonical gate passes 634 Rust executions (39 intentional
ignores), Python 35 and Node 7. Independent conventional review passes with zero
findings and 23 fresh Rust tests plus all-target Clippy. The full
[public review](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/68#issuecomment-6795824581)
was read back exactly before merge. CI43020682782 passes on a synthetic merge
checkout with the same tree and actually restores the cache saved by main's
CI43015543965. These are separate gate, review, CI and merge receipts.

[finalization.zip](finalization.zip) retains 39 members in 97,102 bytes;
[finalization-manifest.json](finalization-manifest.json) records their hashes and
the archive SHA-256 `4a70d7a75640899bd338b30606d0302f36b3eff141a54999f8ebb2117cc463d9`.
Every member was decompressed and verified. Disposable reviewer transport was
removed after terminal collection and public readback; its digests, report and
test logs remain. The original unexplained canonical failure remains a failure.
This is not a repeat of full real-snapshot acceptance or completion of
[#65](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/65). No new Daybreak
or adversarial campaign was run.
