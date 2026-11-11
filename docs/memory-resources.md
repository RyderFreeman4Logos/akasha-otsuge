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

Native worker admission reserves the daemon peak as headroom and the greater of
the configured worker estimate or that profile's previously sampled peak. Set
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
- Each group supplies PIDs, RSS bytes and PSS bytes. RSS sums include shared-page
  double counting; PSS apportions shared pages. `sampled_at_ms` timestamps the
  observation. This is not the #24 P99 acceptance report.
- `workers` reports active reservations, reserved bytes and sampled RSS peaks
  for each group. Completed worker peaks inform later admission in this daemon.
  The profile table is bounded at 256 entries; excess profiles share a conservative
  maximum. Sampling can miss short-lived peaks; absent profile entries mean no
  positive sample was retained. These counters survive controlled storage reopen
  and reset on a new daemon process. They are not kernel peak measurements.
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

The current `enforcement` flags report `software_memory_admission=true` and
`managed_cgroup_limits=false`. Measured admission, bounded job queues/concurrency,
worker reservations, budget-change draining and engine cache settings operate.
Dedicated cgroup limits are not yet installed. Reopening caches does not prove
RSS reaches the requested target, and admission cannot prevent an existing
process from growing past its estimate.
Parent/system cgroup restrictions may still apply; these flags make no claim
about those externally managed limits. Hard enforcement, kernel group/peak
measurements and both full production stress profiles remain
required work in MVP-024 before its issue can close.
