# MVP-023 R2 recovery repairs

Independent R2 rejected candidate `03acdccb102daf3d6556ed21d19c125b3648b8bb`
after verifying its canonical gate, hosted CI and both R1 repairs. The unchanged
FAIL report was published and read back on PR #55. Those historical green gates
do not waive the recovery findings.

The behavioral RED has three executed failures: missing executables become
terminal immediately, a failed destination blocks a new one, and returning to a
previous destination after source advancement replaces its accepted envelope.
The repair normalizes reused native execution errors at the shared failure
boundary and selects persistent state by channel plus destination. Five-failure
budgets, exact accepted input, keys and provider IDs survive A/B/A routing and
restart. Successful current generations still do not resend. Displaced history
is bounded at 64 entries and the existing record-capacity guard; refusal keeps
all old entries instead of evicting them to renew a budget.

Four standalone Python cases, four notifier/policy units and two config cases
passed. The first aggregate notifier run failed: 10 passes and 12 socket-read or
IPC timeouts. Its log and exact admission remain. On unchanged source, the three
causal regressions passed in isolation, then the original complete notifier
vector passed all 22 tests in 40.58 seconds, followed by all-target Clippy (total
50.202 seconds). The observer records only owned process metadata and PSI;
there were no timeout, concurrency or source changes between these runs. The
historical IPC failure cause remains unproven. A later passing observation does
not identify it or substitute for the fresh canonical gate.

The matrix covers native failure mappings versus terminal output/protocol errors,
legacy recovery, policy cancellation, exhausted destination return, ambiguous
pending return with a newer generation, successful-generation suppression and
bounded history. Tests exercise real executable restore, live reload and restart.
The archive keeps prior canonical/CI evidence, immutable R2 FAIL, causal RED,
failed focused run, isolation and complete observed verification. Source hashes
bind the repair tree, not the older candidates. Exact clean-head canonical,
whole-range R3 review and new-head hosted CI remain separate publication stages.
