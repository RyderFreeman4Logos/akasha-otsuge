# Capture fixture supervision

[Issue #50](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/50) repairs
test supervision and fixture preparation. Production code, operation deadlines,
Storage durability, test parallelism and MVP-024 thresholds are unchanged.

Canonical `just check` on the Editor candidate `3b86282` failed six capture/recovery
waits. One observed replay of the original `cargo test --workspace` vector failed
the crash/reopen capture test: its durable cursor advanced within its accepted
120-second execution budget, but the test stopped at 30 seconds. The Claude and
Codex capture helpers likewise imposed unrelated 15/20-second limits. These
three wait bodies were byte-identical on main `4609349` and the Editor candidate.

The bounded procfs observer sampled only owned processes, without ptrace or source
instrumentation. The failed daemon's main thread was observed 56 times in fsync
on WAL/VLog files, chiefly in `wait_log_commit` and `folio_wait_bit_common` on
the requested SSD mirror's Btrfs filesystem. The maximum sample collection cost
was 0.028 seconds. Samples describe residency, not uninterrupted syscall duration.
They do not identify the underlying filesystem cause or explain older failures.

Capture waits now derive a monotonic supervisor boundary from the job's accepted
absolute queue/execution deadline. They retain the original execution boundary
across retries/restart and allow the existing bounded IPC interval for terminal
publication. Polling and wall-clock reversal cannot renew it. Deterministic checks
cover a partly elapsed execution, queue-to-execution transition (including a
concurrent wall-clock reversal), expiry and changed execution clocks. The mapping
from wall time to monotonic time is captured once, at the first observation.
Existing content, authority, success and restart assertions remain.

The first focused run then exposed two positive Claude fixtures relying on cold
backfill inside the Caller’s two-second bind window. Identity and deduplication
fixtures now complete explicit durable sync before binding, as the Codex fixtures
already do. Missing-tip and wrong-source refusal cases remain. The delayed-flush
case still uses a cold bind with a 500 ms delayed write; one complete event isolates
that clock from unrelated historical replay. Non-capture Consult and cancellation
supervisors retain their existing limits.

`issue050-focused-r2` passed all 102 daemon cases, with five explicit opt-in skips,
and all-target Clippy. It is correctness evidence, not a storage speedup or MVP-024
performance result. Full canonical, independent review and CI identities are
recorded on the publication PR. The [compact archive](results/issue050/manifest.json)
retains failed runs, the observer and source-attribution hashes; compressed files
have both stored and uncompressed SHA-256 values.

The first frozen candidate `d672585` passed the full canonical gate, but independent
review found that a wall-clock reversal at the queue-to-running transition could
extend its supervisor. A new regression failed on that candidate before the mapping
was pinned. All five clock regressions then passed. The
[review-fix archive](results/issue050-review-fix/manifest.json) retains that failure,
the historical passing gate and the focused repair; final verification is separate.
