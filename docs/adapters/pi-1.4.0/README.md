# Pi 1.4.0 adapter

The qualified native package is `@earendil-works/pi-coding-agent@1.4.0` from
[`earendil-works/pi`](https://github.com/earendil-works/pi/tree/v1.4.0).
The existing 0.78.0 installation is outside this adapter's support window.
Use the explicit 1.4.0 executable; installation does not change the default CLI,
native model settings or authentication. Node 22.19 or later is required by Pi.

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

## Evidence limits

Native SDK loading, native SessionManager tree fixtures, four Node bridge
regressions, ten Rust Pi tests and daemon recovery/cancellation checks pass.
The bridge's child cleanup and output bounds are tested with controlled executables.
Two earlier Caller identity tests reported `HISTORY_GAP` during a full run.
A content-free timing probe subsequently passed (cold reconcile 1.136–1.313s);
it did not establish those original failures' runtime cause. Those identity tests
now pre-capture their histories, separating binding assertions from cold ingest.
Cold tree capture and daemon-driven cold binding retain separate integration
coverage. Production timeouts are unchanged.

Authenticated native Caller/lifecycle acceptance, RPC Runner, final review and
publication remain tracked in [the complete checklist](../../plan/todo.md).
