# Mempal reader final acceptance

PR [#60](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/60) merged
as fe30c78169fbf87bfa47cf06912b7c64d3a418b3. Its tree exactly equals reviewed
head b98eec93cd2a2d6ca525eaa56d681864280bed05,
tree 6e1aeea391de14b0a46d4e9fcab7abe41877c6de.

- Canonical just check: exit 0 in 444.337 s, 578 Rust passes and 34 ignores
  including installed repeat; formatting, Clippy, Python/Node checks, release
  build and installed lifecycle/UID isolation passed.
- Independent full-range R4: **PASS, zero actionable findings**; 67 fresh Rust
  passes, five actual SIGKILL recovery phases, 11 forced public-CLI interleavings
  and eight schema cases (69 CLI exits). The complete report and focused log
  remain unchanged. R1/R2/R3 failures remain in the preceding archives.
- Candidate CI [42813077335](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/42813077335)
  passed at that exact head and actually restored its cache. The separate
  post-merge main run was still pending when this archive was written.
- The first push succeeded, but its immediate remote-readback assertion failed.
  The original nonzero publication helper is retained. The continuation
  independently verified the remote head and exact PR body without repushing.
- Review publication and merge/readback receipts are separate from test results.
  Native-child raw output was hashed and removed after known child/wrapper
  completion. The cleanup receipt discloses 29 inaccessible same-UID processes;
  it does not claim a complete global process census.

The immutable manifest hashes every member and pins all production files changed
in the reviewed leaf to the historical reviewed commit. Those hashes do not claim
that later incremental-import changes were tested by these runs. Full private-data
conversion and roundtrip evidence remains bound to 4193301 in the
[reader archive](../mvp025-mempal-reader/README.md); it was not rerun for this
filesystem publication repair. No private snapshot, source fingerprint, archive
content or native-child transcript is included here.

Issue #25 remains open for incremental histories and changed legacy snapshots.
Production resource stress, packaging, fixed-oracle evaluation and final release
remain separate (#24, #26, #27, #28); actual Claude authentication (#12) is deferred.
