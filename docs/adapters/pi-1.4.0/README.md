# Pi 1.4.0 adapter

The qualified native package is `@earendil-works/pi-coding-agent@1.4.0` from
[`earendil-works/pi`](https://github.com/earendil-works/pi/tree/v1.4.0).
The existing 0.78.0 installation is outside this adapter's support window.
Use the explicit 1.4.0 executable; installation does not change the default CLI,
native model settings or authentication. Node 22.19 or later is required by Pi.
Optional hints accept up to512 Unicode characters, including supplementary
characters such as emoji, consistently with the shared Rust contract.

## Opt-in Caller extension

Start the local Akasha daemon, grant the intended scope, and allow its read roots
to cover the chosen native agent directory, session file and workspace. Then run:

```sh
akasha-otsuge doctor --host pi --binary /path/to/pi-1.4.0
akasha-otsuge --socket /path/to/service.sock adapter install pi \
  --binary /path/to/pi-1.4.0 --agent-dir /path/to/pi-agent-dir
```

The selected agent directory must already exist. Run Pi with that same native
`PI_CODING_AGENT_DIR`, then reload or restart it. The installer creates only
`extensions/akasha-otsuge/{index.ts,bridge.cjs,install.json}`. It records the
current Akasha executable and socket; keep the executable at that path.
The extension registers `akasha_consult` with an optional short `hint`. Session,
branch, workspace and tool identity come from the native runtime and are checked
against persisted native entries. Nested calls without their own persisted entry
fail closed. Interactive input is machine-observed provenance, not human approval.
The current tool returns a verified binding; the consult workflow is tracked in #16.

Input and message callbacks mark capture pending. Turn completion, settled,
session start/tree/compact and shutdown queue durable reconciliation from the
native file. This preserves other extensions' final transformations and native
parent links. It does not interpret `agent_end` as final settlement. The daemon
preserves original text, fork metadata, edits and compaction originals; private
thinking, private custom data and external image/output files are omitted.
Capture failures are shown as error codes in the native status/notification UI;
resume or reload retries from durable checkpoints. A queued capture is not a
claim that the entire history is already available.

## Removal and recovery

```sh
akasha-otsuge adapter uninstall pi --agent-dir /path/to/pi-agent-dir
```

Reload or restart Pi after removal. Installation and removal refuse modified
owned files. Preserve edits separately before restoring the recorded originals.
Removal records its intent before unlinking files, so rerunning uninstall after
an interruption completes the same removal. Extra user files remain in place;
native settings and credentials are never edited. Changing the executable or
socket requires uninstalling the unchanged extension and installing again.

## RPC Runner

The Runner uses the same selected Pi profile and its native authentication. It
starts a separate `--mode rpc --no-session` process with the installed extension
and exactly four tools: `context_describe`, `context_list`, `context_read` and
`context_search`. Built-in tools, other extensions, MCP, skills, prompt templates,
themes and context files are disabled. The native tool inventory must match
before the Runner sends its prompt. Each run has a private read capability and
an abstract Unix socket with peer-UID checks; model arguments cannot choose the
source session or expand its scope.

Responses are correlated to the submitted prompt. Completion requires a final
JSON answer, native `agent_settled` and zero process exit. `agent_end` can precede
automatic retries and is not completion. The original deadline and shared
budgets cover tools, events and cleanup. Cancellation kills the owned process
group, including bridge children, and reaps it before releasing run resources.
Native retry context edits and compaction/summarization events are accepted as
metadata; they cannot complete a run or expand its tool authority.
Missing credentials and an expired OAuth refresh token return `AUTH_REQUIRED`.
Reauthenticate through Pi's native `/login`; the adapter does not copy credentials
from another host. Provider error text is not returned to the model.

## Evidence limits

Native SDK loading, native SessionManager tree fixtures, six Node bridge
behaviors, three controlled Runner regressions and daemon recovery/cancellation
checks pass. The Runner cases include 23 negative scenarios and exercise the
actual Rust context bridge, including recovered authentication retries.
The maintained [native CLI fixtures](../../../tests/fixtures/pi/1.4.0/README.md)
also pass five Runner cases: all four context tools, actual automatic recovery
after a local HTTP503, unissued-context refusal, cancellation and an unadvertised
Bash call with no side effect. The native Caller binds six calls across two
overlapping sessions, reload, tree selection, fork and resume. Other-extension
transforms, context edits, native compaction and interrupted settlement pass;
all19 capture jobs finish. The three retained native files total42,065bytes.
These fixtures use an isolated profile and synthetic loopback SSE, with the other
three host CLIs absent. Their manifests pin the source before the Unicode hint
correction; its separate regression accepts512 emoji and refuses513, matching
the actual1.4.0 SDK schema validator. They do not prove authenticated model execution or a
controlling-terminal interactive UI. All owned processes and scratch are removed.
The bridge's child cleanup and output bounds are tested with controlled executables.
Two earlier Caller identity tests reported `HISTORY_GAP` during a full run.
A content-free timing probe subsequently passed (cold reconcile 1.136–1.313s);
it did not establish those original failures' runtime cause. Those identity tests
now pre-capture their histories, separating binding assertions from cold ingest.
Cold tree capture and daemon-driven cold binding retain separate integration
coverage. Production timeouts are unchanged.

Authenticated native Caller/Runner acceptance, final review and
publication remain tracked in [the complete checklist](../../plan/todo.md).
The 2026-10-29 native attempt accepted the prompt and then ended with an OAuth
refresh error (`401`, `invalid_refresh_token`) on `openai-codex/gpt-6.1-sol`.
The selected profile needs a renewed native login. No authenticated success is
claimed; the temporary integration was removed and native settings were unchanged.
