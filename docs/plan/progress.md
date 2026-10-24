# Development checkpoint — 2026-10-24

Repository: https://github.com/RyderFreeman4Logos/akasha-otsuge (public, Apache-2.0).
All 41 source pages and 28 real Issue/dependency mappings are preserved.
The current GitHub audit has 26 open Issues (#3–#28); #1/#2 are closed.

- MVP-001 / #1: complete, PR #29 merged as 9c60f50022a4d5b29338e8603e08153037926856.
  [Clean Linux CI](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/39879774711)
  and the local CLI/plan/format/Clippy/release gate pass.
- MVP-002 / #2: complete, PR #30 merged as 349431a350e9e4ae3364b126c4d2a7133dd37d22.
  [Clean Linux CI](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/39881917776)
  and the exact-commit local gate pass, including 17 configuration and 4 CLI tests.
- MVP-003 / #3 is the sole active implementation Issue; PR #31 remains draft.
  Helix failed three full-data low-budget recoveries and was retired. The specified
  SurrealKV/LanceDB fallback remains under evaluation; no backend is selected.

Use the [complete TODO](todo.md), [storage evidence matrix](../benchmarks/storage-evidence-matrix.md)
and [decision record](../benchmarks/storage-decision.md) for the remaining work and
immutable results. Full import/history, both 1800s resource profiles, burst/overload,
corpus lifecycle and public version/generation reclamation have scoped passes.
The first concurrent full rebuild failed OOM. Its same-native direct-low recovery,
recorded-acknowledgement verification and generation3 activation pass separately.
Ten committed outcomes absent from that failed request log remain unknown.

Shared-session repair 78ceb79 passes 12 release units, release Clippy, 179 lifecycle
commands/12 SIGKILL barriers, a 4,528-request concurrent diagnostic, exact-HEAD root
gate and [CI 40076368543](https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/40076368543).
Generation4 preparation is complete: 11,428,831 texts/1,000,000 vectors, active3,
committed269826; preparation is not full build acceptance. Full r2 failed with
systemd OOM after1762.242s, before its3840s workload completed. Sampled RSS P99/max
was579,792,896/608,862,208B; the P99 budget also failed. Its recorded prefix has
16 search errors and excessive search latency. Disk category caps were not
breached, but a failed run does not establish the completed rebuild peak.

All39,876 complete seed/workload acknowledgements were verified against authority.
One incomplete trailing log line and committed309703 without a complete logged
receipt remain unknown. Eight originals were restored with revision guards;
active3 text/vector watermarks now equal committed309711. Generation4 is partial
and remains inactive; its original deadline is unchanged.

The [derived-admission repair](../benchmarks/results/fallback-derived-admission/summary.json)
now makes baseline builds share the existing delta-index mutex, as spec08 requires.
Its native regression first reproduces the bypass, then verifies queued expiry,
unchanged deadline, ordinary-write progress and a later successful build. All12
release units, release Clippy and179 lifecycle commands/12 SIGKILL barriers pass.
The first160s/4,528-request concurrent check fails write P95 at7.904s, despite
correct results and memory within budget. That failure remains retained. One
bounded same-harness baseline/candidate pair subsequently passes; candidate write
P95 is99ms and RSS P99/max201,355,264/201,494,528B. This does not explain the initial
timing failure or establish full-size performance. Default ANN chunk size and
all resource/performance limits remain unchanged; four bounded allocation/query
controls are archived with their diagnostic limits.

Next: safely retire the failed, inactive generation4 without building it just to
permit deletion, preserve before/after authority/search witnesses, then prepare a
fresh full candidate. The current retirement API unnecessarily rejects unbuilt
candidates. The full concurrent acceptance remains open.

At restart or compaction, first load the WD skill and reconcile the exact managed
task with its completion listener and durable state in `.git/akasha-evidence`.
Keep one writer and one dynamic provider-TTL relay; never replay an ambiguous
case or reset campaign age. A start record is not proof that a process survived.
Read the terminal artifacts before replacing any current in-progress wording.

Issue closure requires its actual acceptance, source review and clean Linux CI.
Run the canonical `bash scripts/check.sh` / `just check` on the final candidate;
avoid competing Cargo work during measured native runs. Continue in dependency
order after #3; all production Storage, daemon, four-host integration, long-path,
migration and release work remains #4–#28. Installed tools and user sessions have
not been changed by this storage experiment.
