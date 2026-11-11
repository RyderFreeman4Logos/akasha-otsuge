# MVP-024 controlled storage reopen

Historical candidate: e81402448411b19cf392c9f6f5014a5d6e92d8e7. Independent R1
subsequently found two transition defects despite its passing focused/canonical
tests and CI: native initialization could block busy replies, and a later error
could replace the first failure. This archive records that original candidate,
not a complete responsiveness/error-precedence proof. See the retained report
and causal repairs in [the review-fix archive](../mvp024-storage-reopen-review-fixes/README.md).

This leaf applies a changed storage memory target after running jobs drain. It
keeps the daemon PID/socket, accepted job configuration and original deadlines.
New jobs receive `RESOURCE_BUSY` during drain; existing idempotent submissions
retain their job. New connections receive a bounded busy hello while old
connections, transactions and storage close, and while the next service opens.
Explicit stop and shutdown signals remain authoritative. Reopening retains the
active configuration and its pending/rejected input state.

The parent is d6552b9; `manifest.json` binds the seven original implementation/test
files at the historical candidate above. Its per-attempt receipts preserve command, pre-execution dirty digest,
pinned tool paths, exit, elapsed time, full-log hash and collected process state.
Logs are gzip-compressed without changing their uncompressed bytes. A command
that starts with `cargo fmt` has a pre-format admission digest; the manifest
records the final collected files. This archive is focused candidate evidence;
the committed canonical gate and independent review are separate stages.

| Attempt | Result and interpretation |
| --- | --- |
| red-r1 | Product RED: the real daemon lacked `resources.draining`. |
| green-r1 | One real-daemon PASS: 2048 to 512, running/queued jobs, replay, admission refusal, same PID, original configurations and deadlines. |
| transition-red-r1 | Harness failure: both new fixtures started at default 512 and requested 512, so they did not trigger reopen. The busy test's transition guard failed; the configuration case passed without exercising reopen. |
| transition-red-r2 | After explicitly starting at 2048, both failed: the busy test received exit 1 instead of 4 after the held connection delay; rejected configuration made the reopening daemon unavailable. |
| transition-green-r1 | Both unchanged behavioral assertions passed after serving bounded busy hellos through the transition and retaining the prior configuration readback. |
| controls-r1 | 16 controls passed. The new stop case failed during fixture teardown because it resumed a normal endpoint after the daemon had closed the HTTP response. Later command stages were not reached. |
| controls-r2 | 28 test executions passed: 17 budget-related daemon cases, two retrieval-generation cases and nine scheduler cases. All-target Clippy, plan validation and diff whitespace checks also passed. |

The teardown correction leaves the endpoint paused for its existing `Drop` to
stop and join it; it changes no product behavior or shutdown assertion. Controls
cover watcher reload, repeated/reverted budgets, retained pending paths and
rejected input, explicit stop/SIGTERM/SIGINT, scheduler/accept failures and owned
disconnected writes. The retrieval regression still proves queries use the old
generation during an ordinary index build. When a budget changes, its new search
is now refused at admission instead of being accepted and later failing busy.
After drain, the new search succeeds.

These fixtures use local deterministic endpoints; they are not external model,
production-load or measured-memory evidence. `software_memory_admission` and
`managed_cgroup_limits` remain false. RSS pressure admission, cgroup protection,
continuous measurements, the 50 GB corpus and both 100-client stress profiles
remain unfinished in #24. No Daybreak or new red-team campaign ran.
