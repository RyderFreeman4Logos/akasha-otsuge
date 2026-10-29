# Configuration contract (MVP-002)

The configuration library is implemented and tested independently of the daemon.
Production config commands, automatic file watching and durable generation/audit
storage are integrated by [MVP-008's daemon](daemon.md). This document describes
the shared contract; `config validate/reload/status/show --effective` use that
same manager in the running service.

See [example TOML](../examples/config.toml) and the complete
[built-in defaults](../src/config/defaults.toml). MVP-003 selected the single
[production backend](benchmarks/storage-decision.md); there is no backend switch.

The daemon supplies absolute data/socket paths at startup. The library never reads
project configuration, environment overrides, credentials or installed-host state
implicitly. The daemon's setup layer resolves the documented XDG paths.
Credential references accept only `env:NAME`; values are resolved by prepared
consumers, never stored in the configuration audit.

Precedence is explicit management overrides, selected `profiles.NAME`, user TOML,
then built-in defaults. All base/profile documents must be valid; an override cannot
hide an invalid base or unknown key. `ResolvedConfig::sources()` records the winning
layer per effective field, including defaulted profile members. `resolve` returns an immutable, validated configuration,
parsed timeouts, an effective-content SHA-256 and a retrieval fingerprint.
One-shot management overrides create a separate resolved plan; `reload` has no
override argument and cannot publish a per-command override to the whole daemon.

Configuration is limited to 1 MiB and 64 profiles per executor/model/notifier or
named-profile category. Unknown fields, types, malformed/zero durations, absent
profile references, cyclic fallback, invalid URL/credential forms and contradictory
budgets are rejected. Durations are positive integer `ms`, `s`, `m` or `h` values;
only idle accepts `disabled`, and only workflow accepts `auto`.

`models.NAME` requires `kind` (`text`, `embedding`, `reranker`), `endpoint`, `model`
and explicit `local`. Embedding requires `dimension`. An optional `timeout`
overrides the common request budget. Endpoints accept HTTP(S), with no userinfo,
query or fragment; use `credential_ref` for authentication. Unconfigured model
services are valid. `retrieval.embedding/reranker` reference profiles of the
corresponding kind. `retrieval.extraction` accepts a text profile or
`executor:NAME` to use one enabled CLI executor with its existing authentication.
The latter needs no extra text, embedding or reranker endpoint. Nothing is selected
or invoked by default.

`grill.interviewer`, `grill.expert` and `grill.reviewer` select enabled executor
profiles. Configure all three together; they may share a single CLI/profile.
Auto-Grill is unconfigured by default. Its finite limits are `max_rounds`
(1–8, default2), `max_runner_starts` (1–32, default8), `max_questions`
(1–8, default4) and `max_tokens` (1–10,000,000, default128000).
Each role uses its executor's model, thinking arguments and timeout. New jobs
pin these settings; reload does not rewrite already accepted plans. An automatic
workflow deadline includes the full start budget and the slowest selected role.
An explicit workflow deadline must allow at least one selected role plus overhead.
Token usage includes cached input and stops the workflow when native usage crosses
the shared limit; it is not a provider-specific currency/cost estimate.

`executors.NAME` requires `kind` (`claude_code`, `codex`, `hermes`, `pi`) and `binary`.
Optional fields are `args`, `enabled`, a text `model`, `timeout`, an absolute
`native_home` and an ordered `fallback` list. No other host, fallback or paid API
is enabled by default. Resolving config does not spawn a binary. The selected
host's prepare consumer validates its capabilities. `notifiers.NAME` accepts
`local` or `stdio`; stdio requires `command`.

Consult pins the selected profiles and native-home locations before queuing. An
omitted home uses the bound caller profile for the same host; another host uses
its daemon environment override (`CODEX_HOME`, `CLAUDE_CONFIG_DIR`, `HERMES_HOME`,
`PI_CODING_AGENT_DIR`) or its standard directory under `HOME`. Configure
`native_home` for a different profile, and include that directory in `read_roots`.
Live path restrictions and executor disablement are checked before startup.
Extraction has no bound caller: an omitted native home uses the daemon environment
or standard host directory. It pins that location at job admission and uses the
configured profile once, with no implicit fallback. The source plan supplies all
evidence; extraction cannot use research tools to expand its access.
Fallback follows only the configured ordered lists, depth first, without repeating
a profile. Every attempt and descendant shares the original invocation, workflow
clock and start budget. Authority, cancellation and budget failures stop fallback.
A delegate starts from its parent's active executor. Profile-specific timeouts
remain bounded by the original workflow. An explicitly selected model's `timeout`
also caps the entire native attempt; it cannot extend the executor deadline.

Native model overrides apply only to the isolated worker. Codex uses a Responses
endpoint and requires the model in its native catalog. Hermes uses a Chat
Completions endpoint. Pi retains the matching native model's capabilities and
supports its OpenAI Completions/Responses or Anthropic Messages protocol; unknown,
ambiguous or provider-specific OAuth models return `ADAPTER_UNSUPPORTED`. Claude
uses an Anthropic endpoint (`/v1` is removed before passing its API base URL).
An explicit override requires an available `credential_ref = "env:NAME"`, except
Codex permits an anonymous endpoint when that field is omitted. No credential or
fallback is borrowed from another provider. Default-self execution retains the
native profile; overrides never write its defaults.

`args` accepts up to eight flag/value pairs: Codex `-c`/`--config` for only
`model_reasoning_effort` or `model_verbosity`; Claude `--effort`; Pi `--thinking`;
Hermes `--max-iterations`. Values must be supported by the pinned host. Tool,
profile, prompt, resume and extension options return `CONFIG_INVALID`.

| Settings | Activation | Accepted/queued jobs |
| --- | --- | --- |
| Executor/model routes, prompts, timeouts, retrieval, review | Complete resource prepare, then one snapshot swap | Retain accepted generation/resources |
| Embedding model/dimension | Prepare complete matching encoder/index resources before swap | Retain old matching bundle |
| Concurrency, pause, read policy, executor permission | Consumers check current snapshot at admission/read/delegation/return | Execution plan stays pinned; limits stay live |
| Notifier enable/route/credentials | `delivery_route()` reads current generation for each claim/retry/send | Never reuse event-generation route permission |
| Data directory or socket path | Entire candidate pending; explicit drain/restart required | No fields partially applied |
| Logging | New active diagnostic setting; retrieval fingerprint unaffected | No retrieval cache invalidation for log-only changes |

`ConfigManager::pin()` fixes the accepted task generation. Generic resource owner
`R` is prepared once for that complete configuration and drops only after its last
snapshot reference. A watcher and explicit reload submit complete bytes to the same
`reload` pipeline; the watcher must debounce stable bytes/atomic replacement.
Preparation is serialized. It must be bounded, must not re-enter the manager, and
must build the entire candidate resource set without changing active consumers.
Failure preserves the old snapshot. Clone watch payloads before awaiting; never
hold a watch borrow across reload.

`status()` supplies active/pending generations, redacted effective/desired config,
parsed timeouts, field sources and the last 64 revision results. Reload returns
`applied`, `rejected` or `restart_required`, including field/reason and candidate
digest. A mixed path/model change remains entirely pending. Rejection does not
discard a previous valid pending candidate. JSON error locations omit untrusted
values; credentials, endpoints and command arguments are redacted in diagnostics.

The in-memory revision ring is a diagnostic window, not durable audit authority.
MVP-008 persists complete checkpoints before publication, including returned
reports and accepted task snapshots, restores the positive generation, and records
separate live revocations. OS permissions and actual read/dispatch operations belong to their
consumers; an old task must not authorize itself using its pinned policy.

Timeout defaults and calculation:

- Each text/embedding/reranker provider request: 1700 seconds.
- Each runner execution: 1700 seconds, starting after admission/startup.
- Queue: 300 seconds on a separate clock; queue deadlines cannot parent execution.
- Startup 30s, cleanup 10s, bounded tool budget 120s, summary budget 60s.
- Workflow auto: `starts × (slowest selected/fallback runner + startup + cleanup) + tools + summary`. Defaults yield 5400s, covering all three 1700s starts.
- Explicit workflow must cover at least one selected runner plus all declared overhead. An explicit smaller total can limit later starts; the triggering layer stays visible. Idle is disabled unless explicitly configured.

`Deadline` uses monotonic Tokio time and never resets on output/heartbeat.
Its trace records configured/effective/elapsed/remaining time and the limiting and
triggered layer. A provider started late inside a runner receives the remaining
runner budget and reports the runner as the limiting layer. Reload does not mutate
existing deadlines. These controlled-clock tests do not certify a real CLI/model
path: `local_long_timeout_verified` remains false. MVP-009 and host integration must
supply the required real 1600–1690s long tests.

Reproduce the configuration acceptance and a redacted revision log:

```sh
cargo test --test config --locked
cargo test --test config rejected_or_mixed_restart_changes_never_partially_apply -- --exact --nocapture
bash scripts/check.sh
```

Dependency contracts were checked against current primary documentation:
[TOML document deserialization](https://docs.rs/toml/latest/toml/fn.from_str.html)
and [Tokio watch snapshots](https://docs.rs/tokio/latest/tokio/sync/watch/index.html).
Versions are recorded in `Cargo.lock`.
