# Frozen candidate gate investigation

Candidate 77a78c620a8a303b8d98d591f2f9ea57232673bb, tree
b96d227063086bc82bba800529f6ca3f9c408bda. No product code, assertion or
timeout was changed during this investigation.

- Canonical r1: 89 daemon tests passed, 17 failed, 6 ignored. Failures were
  capture-fixture waits, before their intended behavior assertions. The gate
  remains failed; later release/installed stages were not reached.
- Clean base 4609349: 98 daemon tests passed, 5 ignored, under default
  parallelism. This does not reproduce the candidate failure and therefore
  does not establish a pre-existing defect.
- Initial candidate/four-thread comparison is INVALID: it ran the same 98 tests
  and omitted the nine Editor tests because the shared target retained the base
  test executable. This is not candidate evidence.
- After cargo clean --package akasha-otsuge (dependencies retained), discovery
  verified all 112 candidate tests, including nine Editor cases. Four-thread
  execution passed 97, failed 9 and ignored 6. Remaining failures were capture
  or IPC waits. Restricting parallelism did not solve the problem.
- First fsync/exact-selector probe had an instrumentation setup error: nesting
  TMPDIR lengthened the Unix socket beyond its supported path length, so the
  daemon returned IO_ERROR before readiness. It is not the original failure.
- Corrected probe preserved the original test environment. File/directory fsync
  took about 0.3 ms each; the one selected fork/history test passed in 0.87 s.
  This is current isolated evidence, not proof about the earlier suite timeout.

The initial gate failure is still UNCLASSIFIED. A single unchanged-tree canonical
r2 is now collecting read-only process wait states and I/O pressure. It retains
default test parallelism and the literal just check recipe. No additional
unchanged-tree canonical rerun is authorized by this investigation if r2 fails.
Any PASS must retain the previous failures and their attribution limits.

The captured environment uses the one canonical SSD target and private sccache
socket. No global cache/server settings, shared services, source deadlines,
assertions or test selection were changed. Future worktree switches must verify
the actual test inventory before treating reused artifacts as candidate proof.

CLASSIFICATION: UNCLASSIFIED
