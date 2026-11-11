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

Final exact-HEAD checks, independent review and publication are separate steps.
This is not a repeat of full real-snapshot acceptance or completion of
[#65](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/65). No new Daybreak
or adversarial campaign was run.
