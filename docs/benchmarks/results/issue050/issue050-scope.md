Capture integration tests can fail before the accepted job deadline under concurrent durable storage writes.

The Claude capture helper waits 15 seconds, the Codex fixture 20 seconds, and the crash/reopen capture fixture 30 seconds. The admitted capture execution budget is 120 seconds. These cases verify captured identity/lineage/recovery and configuration pinning; the MVP-007/008 specifications do not impose those fixture-specific whole-backfill limits. The separate MVP-024 latency thresholds remain unchanged.

On local frozen candidate `3b862820b62f189c34f51f45d67b90714aed4ee9`, canonical `just check` failed with 109 daemon cases passing, six capture/recovery wait failures, and six opt-in skips. Log SHA-256: `1eb86387e5d3a9fc15167e0649f252b8f4f0020f930e4d8be4061e85f184933d`.

One unchanged `cargo test --workspace` diagnostic reproduced the recovery failure (114 daemon passes, one failure, six skips). An observer traversed only owned processes via procfs, with no ptrace, source instrumentation, parallelism changes or durability changes. The failing daemon PID 2024083 was observed 56 times in fsync on its WAL/VLog, predominantly `wait_log_commit` and `folio_wait_bit_common`, while its persisted cursor advanced. It was still inside its original 120-second budget when the 30-second supervisor failed. Device: /dev/sdb1, Btrfs, under the requested SSD mirror. Sampled residency is not proof of one uninterrupted syscall or of the cause of earlier historical failures. The diagnostic did not establish why Btrfs was slow.

- [ ] Make these capture fixture waits follow the accepted absolute execution/queue deadline, without resetting it across polls, retries or restart.
- [ ] Keep a bounded terminal/transport grace; retain structured errors and all existing success/content/authority/recovery assertions.
- [ ] Add deterministic checks for original deadline preservation, queued-to-running transition, expiry and rejection of a changed execution deadline.
- [ ] Do not change production timeouts, test parallelism, Storage durability or MVP-024 performance thresholds.
- [ ] Verify the original workspace vector, canonical gate, independent review and CI; preserve the failed evidence and publish the focused harness change separately from #20.

This blocks local promotion of PR #49. It is a test-supervision issue, not a claim that Editor caused the storage delay or that production performance acceptance has passed.

Caller inventory: the Claude terminal helper also serves one controlled Consult case. Preserve that non-capture case's existing 15-second supervisor; only capture jobs use the new admitted-clock helper. Codex sync callers all submit capture jobs.
