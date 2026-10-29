# Hermes 0.22.5 integration

MVP-014 is in progress. The native Caller plugin, canonical export decoder and
durable capture bridge are implemented. The independent Runner and authenticated
CLI/TUI acceptance are still pending. `akasha_consult` currently returns its
binding diagnostic; the final consult workflow belongs to MVP-016.

| Surface | Current evidence |
| --- | --- |
| `doctor --host hermes` | Bounded native version probe; no model call or authentication claim |
| Canonical history | Actual `sessions export --format jsonl` fixtures from synthetic SessionDB data |
| Caller | Immutable session/task/turn/call fence, explicit child provenance, current grants |
| Native plugin | Actual loader, hooks, tool dispatcher and daemon; both `in_process` and `host` isolation |
| Interrupted capture | Actual native end hook with `interrupted=true`; durable job completed in each profile |
| Authenticated CLI/TUI and Runner | Pending; controlled native fixtures do not establish these |

The initially examined build was `0.22.5+1914.gfdb04a0`; native plugin integration
ran on `0.22.5+1915.g4960e41` (source `4960e41cec01de499ab99c4251437bc8039e41db`).
The plugin/export interfaces were unchanged between those source commits.
The public plugin interface was also checked against official source
[`6f553435`](https://github.com/NousResearch/hermes-agent/blob/6f553435a827d6ee2542afde1402f129fee091ee/hermes_cli/plugins.py).
Other core versions are refused. An observed version alone does not qualify a
different build's behavior.

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

These receipts cover synthetic public messages and native interoperability. Final
authenticated acceptance, exact-candidate gates and independent review remain
required before closing MVP-014.
