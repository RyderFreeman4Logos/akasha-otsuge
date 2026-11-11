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
```

`daemon status` includes `resources`:

- `requested` and `config_generation` identify one pinned configuration.
- `storage_open_budget_mib` identifies the configuration used to open the
  authority engine. A changed target sets `storage_reopen_required` and
  `draining`; the daemon pauses new job admission and claims while running jobs
  finish with their original deadlines. It then drains owned transactions and
  connections, closes storage and its index, and reopens with the latest active
  budget in the same process. `index.open_budget_mib` reports the opened index separately;
  `inspected=false` means an index operation currently owns that lock.
- `measurement` reports an on-demand Linux procfs sample. `daemon_group` is this
  process, including its in-process storage/index threads. `runner_group`
  includes every observed descendant: native probes, guardians, external CLIs,
  notifier plugins and their children, including children in separate sessions.
  `service_total` sums both. Independently managed model servers and calling
  clients are outside this process tree.
- Each group supplies PIDs, RSS bytes and PSS bytes. RSS sums include shared-page
  double counting; PSS apportions shared pages. `sampled_at_ms` timestamps the
  observation. This is neither peak tracking nor the #24 P99 acceptance report.
- `complete=false` with `RESOURCE_MEASUREMENT_UNAVAILABLE` and null groups means
  a procfs read, identity validation or bounded census failed. It never converts
  missing processes or malformed counters into a successful zero measurement.

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

The current `enforcement` flags explicitly report
`software_memory_admission=false` and `managed_cgroup_limits=false`. Existing
bounded job queues/concurrency, budget-change draining and engine cache settings
operate, but measured-memory admission and dedicated cgroup limits are not yet
installed. Reopening caches does not prove RSS reaches the requested target.
Parent/system cgroup restrictions may still apply; these flags make no claim
about those externally managed limits. That enforcement,
continuous group/peak monitoring and both full production stress profiles remain
required work in MVP-024 before its issue can close.
