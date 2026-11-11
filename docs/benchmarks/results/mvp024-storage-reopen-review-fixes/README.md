# Controlled storage reopen: R1 repairs

Independent conventional review of e814024 returned FAIL with two actionable
findings. The complete report and exact GitHub readback are retained here. Its
477.171-second canonical PASS (611 Rust executions/35 ignores) and successful
CI42946605310 are historical prerequisites; they do not override the findings
or validate the successor. No report was rewritten into a passing verdict.

R1: the current-thread daemon could block in native initialization despite
polling open and accept in one select. The successor owns and collects the whole
service-opening operation in `spawn_blocking`, including its filesystem work
and async initialization through the existing runtime handle. The listener
continues returning busy while opening is held; the old storage is still closed
and dropped first.

R2: an accept failure was discarded when awaited transition cleanup later failed.
The successor retains the first observed serve/scheduler/accept error while still
collecting every owner. A successfully opened next service is closed before an
accept failure returns. An earlier scheduler error remains ahead of later accept
and owner errors.

The new maintained library regressions exercise the actual serve/open/close
paths. A test-only gate, keyed by the fixture's canonical authority directory,
holds the native initialization handoff. An independent thread asks the real
Unix listener for status with a two-second bound, then releases the gate even on
failure. No environment variable or process-global runtime setting is changed;
the map lock is released before waiting, and the gate unregisters/releases on
drop. The error test releases a controlled failing owned task only after accept
fails, checks the returned first error and verifies storage is closed. Its second
case preserves an already observed scheduler error.

| Attempt | Evidence |
| --- | --- |
| review-fixes-red-r1 | Two product REDs: the native-open request timed out; error precedence returned JOB_ERROR instead of IO_ERROR. |
| review-fixes-green-r1 | Both regressions passed, including the added prior-scheduler-error control. The printed controlled owner panics are collected test inputs, not failed tests. |
| review-fixes-focused-r1 | 92 library tests passed/11 intentional ignores, 17 daemon budget cases and both retrieval-generation controls passed: 111 executions total. All-target Clippy, plan and whitespace checks passed. |

After the first GREEN, the test-only gate was made indifferent to unrelated
invalid/unlinked paths; the broader focused run covers the final helper. Only
the blocking-open test uses the directory-keyed hold; other fixtures retain
ordinary initialization. `manifest.json` pins the complete current affected
source/test set. Per-attempt admissions precede any formatter in the command;
the final source pins and subsequent committed gate are distinct authorities.

This is functional lifecycle evidence. Measured-memory admission, cgroup limits,
continuous peak measurement and #24's full production stress acceptance remain
unfinished. No Daybreak, new red-team campaign, private corpus mutation or new
provider/authentication test was run.
