# Canonical failure and deterministic incident fixture

Frozen failed candidate: 6e692d6dbb0ac329372166a65d3029c913621049,
tree 9c41c8a7b05037ca52b6fe484c0e7134aad2ea5d. Canonical just check exited 101
after 100.262 s. The lib suite had 84 passes, one failure and ten intentional
ignores; its nested child failure is not a second enclosing-suite test.
The incident child returned from advance_incidents without the admission
checkpoint. Remaining integration suites, release and installed gates were
not reached. The complete original log and custody receipt are retained.

The owner test file was byte-identical at base, HEAD and worktree. Shared
Storage::open admission and the test workload changed, so that fact alone does
not establish a pre-existing failure. The exact selector passed once in 5.322 s;
the same parallel lib suite then passed 85 tests/10 ignores in 7.094 s (6.80 s
test execution), versus 51.48 s test execution in the failed gate. Neither
diagnostic pass clears the canonical failure or identifies its historical
scheduling/I/O trigger. No base execution or alternative serialized suite was
substituted.

Source inspection found that the fixture required a fresh incident to remain
inside its five-second initial-admission window through native close, process
startup, reopen and recovery. The test contract concerns an already admitted
job whose incident link is missing. The repaired fixture now calls the real
submit_incident_editor API to persist that state, verifies its accepted time
and unchanged five-second deadline, then waits for that deadline BEFORE
restarting the child. The child must reconcile the durable job and reaches the
same SIGKILL checkpoint before linking it. Existing history, deadline, terminal
QUEUE_TIMEOUT, attempt-zero and no-extra-receipt assertions remain.

A controlled negative temporarily adds queue-expiry rejection before lookup of
an already admitted job. The unchanged repaired test fails at the actual child
checkpoint assertion. Restoring the exact original incident.rs bytes makes
that same test pass. The restored production SHA-256 is
da5ea261be3b9eabe2e2322b42a1e70292b619a3d6719082837b2e97adae9ae7.
The unchanged test bytes for the negative/positive pair have SHA-256
74c12c4ee445cfc0d996e61777a4914b7f05736a6bfd4b0e079e2f79532cf42b.
Formatting, the final exact selector and Clippy also pass. Only the fixture
setup/wait sequence changes; no production timeout, permission or scheduling
code is retained from the negative control.

The original full-suite trigger remains unclassified. The new exact-HEAD
canonical gate and independent full-range review are still required. This
receipt is not a waiver and does not promote a later isolated pass into
historical causality.

CLASSIFICATION: UNCLASSIFIED
