# Mempal source preservation repair

The whole-range R1 review at `5e10a62ea75d7908e00fb8f8568f1bf351c2f35e`
failed with one HIGH introduced source-loss defect. The unchanged full report
and focused log are retained here and published on
[PR #60](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/60#issuecomment-6765037118).
The earlier exact canonical PASS is historical evidence, not a waiver.

The public CLI regression exposed a destructive `conversion.json.tmp` alias
and an accepted source-as-lock sibling across eight output names. A second
regression moved the pinned source after reader admission and observed writer
adoption before the inode guard existed. Both causal RED receipts remain.
The first broader run passed 63 integration tests and five SIGKILL recovery
points but failed a new fixture's error assertion: an unowned manifest stage
was rejected before the intended file opener. Its original test, log and
classification remain. The corrected fixture prepares legitimate owned output
before moving the source; its five variants preserve bytes and refuse the inode.

The final focused continuation passed two mempal unit tests (one supervisor
child intentionally ignored), five actual SIGKILL phases, formatting and
all-target Clippy. Together with the unchanged production integration pass,
this covers 65 distinct Rust tests. The compatibility probe used the retained
old release executable to create a small archive, then replayed it with the
new debug executable: all four conversion/archive files were byte-identical.
An independent typed-row/source-span oracle passed and rejected corrupt data.
All probe scratch was removed. No actual private snapshot was modified or copied.

`manifest.json` pins this repair's five source/test files; exact canonical R2,
independent R2 and hosted CI are separate subsequent gates. The earlier real
3,525,645-row acceptance remains bound to source `4193301` and its recorded
binary. The repair changes path/inode admission, not mapping or archive format;
no new full private-data execution is claimed here. Different-source incremental
migration remains unfinished in #25.
