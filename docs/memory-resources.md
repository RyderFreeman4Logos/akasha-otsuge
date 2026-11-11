# Memory budgets and observations

`config show --effective` includes `memory_limits`. The existing
`runtime.memory_budget_mib` remains the daemon/storage/index steady-state target.
Default target/peak pairs are 512/640, 1024/1280 and 2048/2048 MiB. Other existing
targets retain compatibility; the default peak is 125% rounded down, capped at
2048 MiB. These are requested limits, not measured acceptance results.

Optional `runtime.daemon_peak_limit_mib` must be at least the target and at most
2048. Optional `runtime.service_total_budget_mib` defaults to 2048 and must cover
the daemon peak. Worker capacity must fit inside the remaining total budget.
Omitted options stay omitted in archived configuration serialization, preserving
old configuration digests and accepted jobs. Invalid cross-budget combinations
reject the entire reload; pinned jobs retain their configuration generation.

```toml
[runtime]
memory_budget_mib = 512
daemon_peak_limit_mib = 640
service_total_budget_mib = 2048
# Estimate for a native worker, including its descendants; this is not a cap.
runner_memory_estimate_mib = 1024
```

Native worker admission reserves the daemon peak as headroom and the greatest of
the configured worker estimate, that profile's previously sampled RSS peak, or
its retained kernel charged-memory peak when available. Set
`executors.<name>.memory_estimate_mib` to override the runtime estimate for a named
executor. Both estimates must be positive; the runtime default is 1024 MiB.
Each active worker keeps the unused part of its own reservation: one worker
using more than estimated cannot consume another worker's reserved capacity.
Delegates, explicit fallbacks, extraction and grill use the same pool.

An estimate that does not fit returns `RESOURCE_BUSY` before worker preparation
or spawn; a refused delegate returns `DELEGATION_UNAVAILABLE`. Memory refusal
does not select a fallback. Estimates remain pinned to the accepted job while
global capacity is rechecked under admission immediately before each native
start. The reservation remains owned until process cleanup releases the run
lease. Version probes performed for an admitted worker share its reservation;
standalone registration/export probes and notification plugins use the daemon
headroom. Those helpers are checked before starting and are included in its RSS.

For example, a 512/640 MiB daemon plus one 1024 MiB worker estimate fits the
default 2048 MiB total. A 2048/2048 MiB daemon reserves that entire total; raise
the total budget to admit an additional worker. These calculations do not prove
that an arbitrary Node/Python CLI stays within an estimate.

`daemon status` includes `resources`:

- `requested` and `config_generation` identify one pinned configuration.
- `storage_open_budget_mib` identifies the configuration used to open the
  authority engine. A changed target sets `storage_reopen_required` and
  `draining`; the daemon pauses new job admission and claims while running jobs
  finish with their original deadlines. It then drains owned transactions and
  connections, closes storage and its index, and reopens with the latest active
  budget in the same process. `index.open_budget_mib` reports the opened index separately;
  `inspected=false` means an index operation currently owns that lock.
- `measurement` reports an on-demand Linux procfs sample. `daemon_group` includes
  this process, its storage/index threads and core helpers such as standalone
  registration/export probes and notification plugins. `runner_group` includes
  registered worker roots and all observed descendants, including guardians,
  worker preparation probes and children in separate sessions.
  `service_total` sums both. Independently managed model servers and calling
  clients are outside this process tree.
- A raced or incomplete procfs scan is discarded and retried up to three total
  scans. Persistent failure remains unavailable and prevents admission; no partial
  values or previous sample are substituted.
- Each group supplies PIDs, RSS bytes and PSS bytes. RSS sums include shared-page
  double counting; PSS apportions shared pages. `sampled_at_ms` timestamps the
  observation. This is not the #24 P99 acceptance report.
- `workers` reports active reservations, reserved bytes and sampled RSS peaks
  for each group. Completed worker peaks inform later admission in this daemon.
  The profile table is bounded at 256 entries; excess profiles share a conservative
  maximum. Sampling can miss short-lived peaks; absent profile entries mean no
  positive sample was retained. These counters survive controlled storage reopen
  and reset on a new daemon process. They are not kernel peak measurements.
  `completed_runner_kernel_peaks` separately records each settled worker's
  cgroup peak, including charged cache/kernel memory. These values also inform
  later admission and have the same bounded profile table and process lifetime.
- `complete=false` with `RESOURCE_MEASUREMENT_UNAVAILABLE` and null groups means
  a procfs read, identity validation or bounded census failed. It never converts
  missing processes or malformed counters into a successful zero measurement.

An incomplete measurement or measured/projected pressure refuses new jobs and
claims while retaining health, job/receipt reads and queued deadline handling.
A total-only decrease also sets `draining` when existing reservations no longer
fit; it does not interrupt an already running worker or release its reservation.
New starts resume when usage and reservations fit again.

During job drain, existing idempotent submissions still return their admitted
job; new submissions receive `RESOURCE_BUSY`. During close/open, new connections
receive the same retriable error promptly. Retry with the original request or
idempotency key. Queued jobs keep their accepted configuration, queue deadline
and execution budget across the reopen; running jobs are not cancelled merely
because the budget changed. The socket and PID stay unchanged. Explicit stop,
SIGTERM and SIGINT still stop the daemon. Pending path changes remain pending,
and rejected configuration input does not replace the active snapshot.

The census is bounded at 4096 processes, 32768 threads and 64 KiB per procfs
file, and reads every task's children. It checks PID start identity and parentage
before accepting a sample. `atomic=false` remains explicit: processes can fork,
exit or change memory during observation. Linux documents these limitations of
[children and smaps_rollup](https://docs.kernel.org/filesystems/proc.html).
Do not treat a successful read as an exact instantaneous census or OOM protection.

On Linux with an available systemd user manager, `/usr/bin/busctl`, delegated
cgroup v2 memory control and `cgroup.kill`, the daemon creates its own transient
scope after acquiring its socket and before opening storage. It confirms actual
scope membership and ownership before creating `daemon` and `runner` groups.
The daemon and core helpers use the daemon group. A native worker and its
preparation probes enter one reservation-owned runner leaf before executing the
external CLI; guardians and descendants inherit that leaf. Each reservation
retains capacity through the existing reaper and a kernel empty-subtree check.
Only that owned leaf can be killed or removed by this cleanup.

`resources.cgroup` reports the scope path, group paths, current charged bytes,
kernel peaks (null when the kernel lacks the counter), `memory.high` and
`memory.max` readbacks, and OOM-kill counters. The daemon uses target/peak as
high/max; the runner group uses total minus daemon peak for both; the scope uses
the total budget for both. The requested budget is separate from the read-back
`effective` budget. `pending` and top-level `draining` remain true while a new
limit has not become effective. An incomplete read never means zero memory or
confirmed enforcement.

Budget increases that do not shrink another group's limit can apply immediately.
Decreases wait for running jobs, notifications, owned operations and native
cleanup to drain and for observed usage to fit. A target change uses the existing
controlled storage reopen; a total-only change keeps storage open. Original
worker deadlines remain in force. Failed or partial limit updates remain visible
and block new resource admission until reconciled. Scope identity and both kinds
of peak history survive a storage reopen. Normal daemon exit lets systemd collect
the empty owned scope.

`enforcement.software_memory_admission` remains true. The
`managed_cgroup_limits` flag is true only when the installed owned limits can be
read back; pending lowering can still have the older limits enforced. If initial
delegation is unavailable, `resources.cgroup.available=false` reports the error
and software admission/procfs measurement continue. Parent/system restrictions
may still apply; these flags do not describe those externally managed limits.

Kernel charged bytes include page cache and kernel allocations, unlike RSS/PSS.
Moving a process does not transfer its previous memory charges. Status records
the RSS observed at enrollment and explicitly reports
`preexisting_charges_migrated=false`; complete procfs measurement still includes
that memory. Neither kernel counters nor reopening caches prove the required
RSS P99 operating envelope. See the kernel's
[memory-controller contract](https://docs.kernel.org/admin-guide/cgroup-v2.html).

Normal real-kernel lifecycle and running-worker budget-lowering tests are opt-in
because ordinary CI may lack user delegation:

```sh
cargo test --locked --test daemon owned_cgroup_ -- --ignored
```

These tests verify kernel readbacks, worker/guardian membership, retained peaks,
safe lowering, same-PID reopen and normal cleanup. Full production corpus/load
and recovery acceptance remain required in MVP-024. No new Daybreak or red-team
campaign is represented by these functional tests.
