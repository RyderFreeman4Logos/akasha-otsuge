# Hermes 0.22.5 integration

MVP-014 is in progress. The native Caller plugin, canonical export decoder and
durable capture bridge and independent Runner are implemented. Authenticated
CLI, interactive TUI and Runner acceptance pass. `akasha_consult` currently returns its
binding diagnostic; the final consult workflow belongs to MVP-016.

| Surface | Current evidence |
| --- | --- |
| `doctor --host hermes` | Bounded native version probe; no model call or authentication claim |
| Canonical history | Actual `sessions export --format jsonl` fixtures from synthetic SessionDB data |
| Caller | Immutable session/task/turn/call fence, explicit child provenance, current grants |
| Native plugin | Actual loader, hooks, tool dispatcher and daemon; both `in_process` and `host` isolation |
| Interrupted capture | Actual native end hook with `interrupted=true`; durable job completed in each profile |
| Authenticated Runner | Two independent profiles, existing native auth, all four context tools, final result and zero exit |
| Authenticated CLI Caller | Real oneshot CLI, native model and canonical call/profile binding |
| Authenticated TUI Caller | Two simultaneous controlling PTYs, distinct profiles (`in_process` and `host`), normal interactive input, successful exit and durable capture |
| Runner restrictions/cancellation | Eight native hook denials; real model's unissued read denied; cancellation after an actual context-tool request reaps the executor |

The initially examined build was `0.22.5+1914.gfdb04a0`; native plugin integration
ran on `0.22.5+1915.g4960e41` (source `4960e41cec01de499ab99c4251437bc8039e41db`).
The plugin/export interfaces were unchanged between those source commits.
The public plugin interface was also checked against official source
[`6f553435`](https://github.com/NousResearch/hermes-agent/blob/6f553435a827d6ee2542afde1402f129fee091ee/hermes_cli/plugins.py).
Other core versions are refused. An observed version alone does not qualify a
different build's behavior. At the same qualified source commit, the native
version resolver's three-second git-tag probes can fall back to
`0.21.5+18146.g4960e41` or `git.4960e41`. These two observed labels are explicitly
recognized; other0.21.5/git builds remain unsupported. Doctor preserves the
observed label and lists the full qualified source commit. Its bounded probe
allows30seconds for the native resolver's multiple git commands; it still makes
no model call. A read-only diagnostic observed7.008s and3.795s version commands
at the same clean source commit. Native update checks can add time and may be
disabled through the owning profile's `updates.check` setting.

## Runner execution boundary

Hermes0.22.5 routes ordinary `chat --format stream-json` and `-z` through its
Gateway. Killing that CLI detaches accepted work; it does not cancel the native
executor. `--run-budget` is also rejected on that route. The Akasha Runner uses
the public plugin CLI extension `hermes akasha-runner` and Hermes' native
`AIAgent` in its owned process instead. Native configuration, runtime credential
resolution, provider fallback and reasoning settings remain owned by Hermes.
The Python bridge registers tools and translates native execution results;
Rust owns read authorization, budgets and context retrieval.

Runner requires the selected profile to enable the installed plugin with
`plugins.isolation: in_process` and expose the four context tools directly
(`tools.tool_search.enabled: off`). Hermes explicitly skips plugin CLI commands
under `host` isolation; that mode is supported for Caller capture, not Runner.
The installer never changes these settings. An unexpected actual native tool
surface is refused before a model request. Context files, native memory,
background review and checkpoints are disabled for the independent run.

An authenticated private UDS supplies context progressively. Its per-run bearer
is never placed in the prompt or tool schema. Native pre-tool guards and the
Rust broker reject other tools. This is tool-policy enforcement, not an OS
sandbox. The Runner retains its lease and private scratch until its process group
has exited; a final answer also requires an explicit successful native result
and exit status zero. Controlled tests cover partial output, malformed frames,
forged identity, extra tools, cancelled descendants and live processes after EOF.

## Install and undo

The daemon's configured `access.read_roots` must contain the owning native profile
home and the session workspace. The bridge resolves and checks those paths before
loading history; adding a plugin does not grant access to other profiles.

```sh
akasha-otsuge --socket /absolute/akasha.sock adapter install hermes \
  --binary /absolute/hermes --home /absolute/native-profile
HERMES_HOME=/absolute/native-profile hermes plugins enable akasha-otsuge

HERMES_HOME=/absolute/native-profile hermes plugins disable akasha-otsuge
akasha-otsuge adapter uninstall hermes --home /absolute/native-profile
```

Install creates a private `plugins/akasha-otsuge` directory with the plugin,
manifest and content hashes. It records the selected executable and socket in
the installed plugin. Repeating the same install is harmless; edited files,
symlinks and conflicting installations are refused. Undo verifies the content
hashes before removing these three files. Extra files and Python caches remain.
The Rust installer never edits native settings or changes `plugins.isolation`.
Hermes' own enable/disable commands control activation for the selected profile.
Optional native plugin settings `socket` and `hermes_binary` override the installed
defaults and are read for every call.

The native tool schema contains only optional `hint` (at most 512 characters).
The hook supplies runtime IDs privately; `ctx.session_cwd()` supplies the native
workspace. A handler without a matching hook binding fails. Native0.22.5 handlers
do not receive turn/call IDs, so overlapping calls with the same profile,
session, task and arguments are rejected as `CALLER_AMBIGUOUS`. The plugin does
not select a latest call. A pre-existing tool with the same name rejects plugin
registration before hooks are installed.

## Capture and recovery

`on_session_start`, `on_session_end` and `post_compaction` request canonical exports.
The end hook covers interrupted and failed turns as well as successful ones.
Restarting/resuming a native session backfills its persisted history. A process
that dies before Hermes persists a message cannot supply that message later.

The bridge reads one session per JSONL export, bounded to 8 MiB and 16,384 message
rows. It retains stable native message UIDs, public text/tool records, compaction
originals and explicit parent metadata. It omits private reasoning/configuration
and binary bodies, reporting `visible_text` coverage. Missing compaction originals,
unknown records, native export limits and read failures remain errors.

Immediate Caller capture stops at the invoking tool call. Background export writes
one normalized private spool per native session and uses the existing durable job
scheduler. Retry/deduplication preserves old fences. `job list` / `job show ID`
report completion or failure; the plugin's native state records the last submitted
job or error. A `queued` status does not mean the history is already captured.
Source files may be rewritten by Hermes; an unavailable old event remains a
history gap rather than falling back to the parent's session.

## Controlled native evidence

The native plugin probe used two fresh profiles, actual plugin isolation, native
SessionDB and CLI export, real Rust daemon IPC, background job completion and
install/undo. Both profiles bound distinct invocation IDs at cursor3 and recovered
the interrupted tail. The fixture allocated 962,560 bytes across42 files; its
profiles, daemon data and scratch were removed after all consumers ended.
The native launchers had identical hashes before and after the successful probe.

An earlier probe incorrectly assumed the native state filename. A second fixture
mistake shared PM state through temporary dependency symlinks, causing Hermes to
publish two launchers with a temporary interpreter path. Those exact path changes
were repaired and `hermes --version` reverified. The successful fixture used small
real native profile directories and kept daemon scratch on SSD; it did not copy
dependencies or modify existing profiles.

These controlled receipts cover synthetic public messages and native interoperability.
The authenticated receipts below are separate. Exact-candidate gates and independent
review remain required before closing MVP-014.

The [authenticated Runner receipt](runner-acceptance.json) separately records two
native `gpt-6.1-sol` / `openai-codex` runs. Each invoked all four scoped context
tools exactly once, returned the expected structured final result and exited
successfully. Claude, Codex and Pi executables were absent from the child PATH.
This uses real model calls over synthetic captured context; it does not establish
1700-second execution. The fixture allocated229,376
bytes across27 files, and both native profiles and all scratch were removed.

The [CLI receipt](cli-acceptance.json) records an actual
`hermes chat --oneshot --query-file REQUEST --format stream-json --toolsets akasha
--ignore-rules --in WORKSPACE` invocation. The [TUI receipt](tui-acceptance.json)
records two simultaneous `hermes chat --tui --toolsets akasha --ignore-rules --in
WORKSPACE` processes, each with a controlling PTY and its own native profile.
Both profiles retain distinct session, invocation and profile hashes, bind at
cursor3, produce a final answer, exit zero through Ctrl+D and finish their capture
jobs. The fixture allocated23,113,728 bytes and removed both profiles and all SSD
scratch after every owned process ended. Native dependency installations and
launcher hashes were preserved. Existing native auth supplied the same
`gpt-6.1-sol` / `openai-codex` route as the Runner checks.

Two earlier `host` TUI startup-query probes timed out. The bounded follow-up
observed an existing session and zero persisted messages. Their `-q` path remains unqualified; ordinary interactive input
passes. A subsequent interactive probe obtained both correct bindings but failed
its scripted `/exit` delivery. The successful probe uses the native empty-composer
Ctrl+D exit. Those failed receipts remain failures, not successful TUI runs.
The isolated Gateway fixture uses `gateway.standalone: true`; the older
`multiplex_profiles: false` setting is ignored by this native build. No existing
user Gateway was stopped or reconfigured.

The [control receipt](control-acceptance.json) distinguishes actual native plugin
hook tests from authenticated requests. The native guard blocks terminal, file
write/patch/read, web extraction/search, delegation and foreign MCP tool names.
An authenticated Runner asking `context_read` for an unissued handle receives
`READ_DENIED`. Another run is cancelled after the model calls `context_describe`;
acknowledgement requires executor exit, and its private run directory is removed.
This does not claim an OS network/filesystem sandbox.

Explicit native tests require an isolated, enabled `in_process` profile with
existing lawful authentication and `tools.tool_search.enabled: off`. Set
`AKASHA_HERMES_BINARY` and `AKASHA_HERMES_HOME` to that installation/profile, then run:

```sh
cargo test --test hermes --locked runner::installed_native_runner_roundtrip -- --ignored --exact
cargo test --test hermes --locked runner::installed_native_rejects_unissued_context -- --ignored --exact
cargo test --test hermes --locked runner::installed_native_cancellation_ends_executor_after_context_tool -- --ignored --exact
```

Ordinary `just check` leaves these opt-in authenticated tests ignored. Compact
[acceptance originals](acceptance-evidence.zip) include the actual supervised
harnesses, command/terminal receipts and sanitized parent checkpoints; no native
private reasoning, credentials or dependency copies are archived.
