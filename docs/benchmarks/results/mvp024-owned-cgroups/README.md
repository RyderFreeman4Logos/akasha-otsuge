# Owned cgroup functional leaf — PR66

PR [#66](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/66) merged at
2026-11-11T09:06:30Z. Reviewed head `84428714a378bc849895c8f3ff4af6ed6af6e1c9`
and merge `9253a8b799421db3893e0e67dd2cfb55ca8c4b79` share tree
`6e1e6d2f84c9ac99c9e1e45e181f39dad1b91849`.

The functional leaf owns daemon/runner/service cgroup-v2 limits, worker-group
entry before execution, reservations through cleanup, positive kernel peaks,
and drain-aware live budgets with truthful requested/effective/pending state.
It also retries an entirely fresh bounded procfs census across normal child exit;
persistent unknown data remains unavailable. Initial charges do not migrate.

- Exact `just check`: 626 Rust executions / 39 intentional opt-in ignores,
  including the installed repeat; Python 34 and Node 7. Installed lifecycle and
  UID controls pass, zero TCP sockets; release SHA-256
  `9b058b37553f9cf590d7f0062ad2baaec29e4a18b5f35e1a429b753c8df868f1`.
- Two separate committed normal real-kernel tests pass with actual membership,
  limit/peak readbacks, total-only convergence and normal collection.
- Independent whole-range conventional review: PASS, zero actionable findings,
  44 fresh tests. The [complete immutable original report](mvp024-owned-cgroup-review-r1.md)
  was [published and read back](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/66#issuecomment-6791715702).
  Parent checked 80 source coordinates; no corrections were needed.
- [CI42989102629](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/42989102629)
  passed the same head; the complete decoded job log and actual cache restoration
  and primary-key hit are retained.

`attempts.json` retains all 19 capability, harness, RED/GREEN, diagnostic, commit,
gate and review attempts. Logs are losslessly gzip-compressed. Admission `head`
fields on dirty runs identify the committed parent, not the uncommitted source;
their diff/untracked hashes and retained custody records are separate identities.
Intermediate dirty trees are not all reconstructable from those hashes alone.
The final historical source hashes in `manifest.json` bind to reviewed 8442871,
not to whatever source accompanies this archive in a future checkout.

The original one-editor failure and the diagnostic topology's 18 other IPC
failures remain [UNCLASSIFIED](mvp024-cgroup-editor-failure-classification-r1.md).
The normal-exit RED/GREEN establishes its own mechanism, not either historical
cause. Both probe preimages and exact restoration evidence are retained.
Initial capability failures were setup/readiness failures, not product RED;
the original first attempt lacks child stderr. The preparation JSON is an early
proposal; the final exec helper reuses the guardian entry point. Later passes do
not rewrite any of these limitations or the older CI42949516550 HistoryGap.

This is not full production 50 GB/100-client/RSS/recovery acceptance. Daybreak,
new adversarial/OOM/supervisor-death experiments were owner-deferred. Normal
collection is distinct from those unexecuted lifecycle conditions. The false
completed state of #24 after PR64 is preserved as history, and open
[#65](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/65) retains the
unchanged remaining MVP-024 criteria. No partial leaf is a scale completion.

Only exact owned raw review transport (610,827 bytes) was removed after terminal
collection, full report publication and readback. The complete report, tests,
usage/exit and publication records remain. The SSD worktree and shared target
were reused; no new corpus copy was created and no cache capacity was changed.
