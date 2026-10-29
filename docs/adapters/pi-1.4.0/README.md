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
fail closed. Native tool-call IDs are opaque: Responses IDs such as
`call_...|fc_...` are preserved exactly, with the shared256-byte/no-control
boundary. Interactive input is machine-observed provenance, not human approval.
The tool submits a durable Consult job. Its public result contains the job status
and, after successful research, a bounded conclusion with evidence and a receipt.
Use `consult show JOB_ID` and `consult receipt JOB_ID` to continue after a pending response.

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
and exactly nine research tools: `context_describe/list/read/search`,
`memory_search/get`, `code_search/read` and `delegate`. Built-in tools, other
extensions, MCP, skills, prompt templates,
themes and context files are disabled. The native tool inventory must match
before the Runner sends its prompt. Each run has a private read capability and
an abstract Unix socket with peer-UID checks; model arguments cannot choose the
source session or expand its scope.

Responses are correlated to the submitted prompt. Completion requires a final
JSON answer, native `agent_settled` and zero process exit. `agent_end` can precede
automatic retries and is not completion. The original deadline and shared
budgets cover tools, events and cleanup. Owned run-local settings propagate its
remaining time to Pi HTTP/provider timeouts. Only that private directory is
approved; native user settings remain unchanged. Private callback greetings may
wait behind delegated research and retain the same original deadline. Cancellation kills the owned process
group, including bridge children, and reaps it before releasing run resources.
Native retry context edits and compaction/summarization events are accepted as
metadata; they cannot complete a run or expand its tool authority.
Missing credentials and an expired OAuth refresh token return `AUTH_REQUIRED`.
Reauthenticate through Pi's native `/login`; the adapter does not copy credentials
from another host. Provider error text is not returned to the model.

## Evidence limits

Native SDK loading, native SessionManager tree fixtures, seven Node bridge
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
the actual1.4.0 SDK schema validator. A subsequent [protocol receipt](../../../tests/fixtures/pi/1.4.0/protocol-controlled/manifest.json)
binds the corrected source and both Chat Completions and Responses protocols.
Each passes five Runner cases, six RPC Caller bindings/19 capture jobs, and four
interactive Caller bindings/11 jobs in real foreground controlling terminals.
The TUI cases include overlapping sessions, fresh-process resume and compact.
Responses-native IDs retain their pipe separator; five unchanged native files
retain69,059bytes. The previous binary fails the same Responses Caller with
`CALLER_AMBIGUOUS`; the corrected bridge passes without normalizing its ID.
These synthetic providers do not establish authenticated model execution.
All owned processes and scratch are removed.
The bridge's child cleanup and output bounds are tested with controlled executables.
Two earlier Caller identity tests reported `HISTORY_GAP` during a full run.
A content-free timing probe subsequently passed (cold reconcile 1.136–1.313s);
it did not establish those original failures' runtime cause. Those identity tests
now pre-capture their histories, separating binding assertions from cold ingest.
Cold tree capture and daemon-driven cold binding retain separate integration
coverage. Production timeouts are unchanged.

Real native Caller, Runner and foreground-TUI acceptance subsequently passed
on unchanged source `a11130f` through the owner's configured localrouter and
`mp-cpi/grok-4.7`. Three Runner cases, six RPC bindings/19 jobs and four TUI
bindings/11 jobs pass, including native model compaction, fork, reload, resume
and interruption. The first real compaction fixture was too small for the native
20,000-token keep window; a private profile with keepRecentTokens256 exercises
real summarization without changing user defaults. Earlier failed receipts remain.
All owned processes/scratch ended and original native settings/models stayed
unchanged. [The final acceptance receipt](https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/44#issuecomment-6497475368)
records hashes and scope. The exact candidate also passed the canonical gate,
independent R4 review and CI40860874178. PR #44 merged and #15 closed. The subsequent durable Consult, delegation and
long-request receipts are in the [MVP-016 evidence](../../benchmarks/results/mvp016/README.md).
