# Host adapter contracts

`akasha_otsuge::adapter` separates authenticated caller identity from execution.
The caller supplies only `ConsultArgs { hint }`; omitted, null and whitespace-only
hints become `None`. A hint is at most 512 Unicode characters before trimming.
Unknown fields, including session IDs, scope and context packages, are rejected.
`CallerAdapter`, `ContextSource`, `AgentRunner` and `RunHandle` are public Rust
traits; an external crate can implement them and construct every structured
refusal. Business rules remain in Rust, not host-specific bridges.

These are the shared library/protocol contracts for MVP-011. Real host adapters,
their `doctor` capability probes and actual authenticated machine-output paths
are MVP-012–015. The consult workflow and final RecallReceipt are MVP-016.

## Identity and diagnostics

The host bridge resolves its authenticated runtime call before constructing a
`RuntimeBinding`. This type intentionally cannot deserialize model tool input.
It records runtime instance, caller session/thread/agent, branch, exact turn/call
when available, explicit root/child registration, scope, principal, workspace,
configuration generation and a capture snapshot/fence. No directory scan, latest
session fallback, inherited root environment variable or hint can supply identity.
Missing binding is `BINDING_UNAVAILABLE`; missing or inconsistent registration is
`CALLER_AMBIGUOUS`; an incapable host is `ADAPTER_UNSUPPORTED`.

`Invocation::bind` validates the retained capture anchor and current read grant,
then durably commits the immutable binding before research. Concurrent calls in
the same directory have distinct invocation records. An exact retry reuses its
receipt; reuse with different identity/fence is `BINDING_CONFLICT`.
`Invocation::load` requires the original principal and checks the live grant and
retained anchor again. Only this storage path deserializes persisted identity.

`Invocation::diagnostic()` serializes the full trusted binding for an authorized
diagnostic consumer. `ContextSource::execute(Describe)` returns a smaller bounded
diagnostic, including **actual invocation precision**. A host capable of exact
turn binding may still produce a session-only invocation; these are not conflated:

```json
{"kind":"description","value":{"invocation_id":"call","source":{"adapter":"codex","profile":"default","session":"s","thread":"t","agent":"a"},"binding_level":"session_only","coverage":"lagging","requested_cursor":10,"available_cursor":9,"read_seq":12}}
```

`CapabilityReport` records the actual installed executable/version, adapter
version, supported binding precision, native-child binding, historical coverage,
machine protocol, cancellation, outer timeout and last explicit self-test. An
installed CLI alone proves none of those optional capabilities. `NotRun` is not
a successful self-test. `CompleteToFence`, `Lagging` and `Unavailable` describe
retained context; unavailable or missing originals must be reported as gaps.

## Consult results and continuation

A trusted Consult submission creates a durable job handle. Pi, Hermes and Claude
native tools submit Consult using their automatically verified identity and optional
hint. Pending results survive the short caller request; use the same daemon socket
or CLI profile for the commands below. The installed Codex hook submits Consult
and transports its follow-up commands through the read-only sandbox.
A binding diagnostic is not a completed Consult.

```sh
akasha-otsuge consult show JOB_ID
akasha-otsuge consult receipt JOB_ID
akasha-otsuge consult receipt JOB_ID --full
akasha-otsuge consult original JOB_ID SOURCE_INDEX
akasha-otsuge consult original JOB_ID SOURCE_INDEX --start BYTE_OFFSET
akasha-otsuge job cancel JOB_ID
```

`consult show` returns only a public result. Pending and failed jobs explicitly
have no final receipt; failures retain their error code. Successful results come
from the immutable RecallReceipt body and exclude the private plan, execution
trace and usage. The same handle works after a daemon restart. `job show` retains
management diagnostics separately.

Research runners return a structured `found`, `no_hit` or `incomplete` conclusion.
Selected source references must match actual reads; selected memory revisions must
have been expanded with their originals. A no-hit conclusion requires a search.
Unresolved conflicts become incomplete results. Conditions and conflicts remain
in the public result, alongside the selected memories' recorded scope, authority,
conditions, assumptions and validity. Delegates use the same contract; their
private execution traces stay separate.

The public body includes its receipt and job IDs within `consult.max_result_chars`
Unicode characters. If the complete answer and references exceed that budget,
the body contains an incomplete result and a `detail_receipt_id`. The full answer,
including its conditions and evidence, remains in a separate immutable receipt;
`consult receipt JOB_ID --full` expands it. JSON and individual source references
are never cut to fit. A budget too small even for the scoped handle fails with
`OUTPUT_LIMIT`; the native result and serialized receipt also retain hard size
bounds.

`consult receipt` includes the recorded and currently validated sequence metadata.
`consult show` additionally checks each selected memory's current adopted revision
and validity, returning `STALE_RECALL` if the completed answer is superseded.
The zero-based `SOURCE_INDEX` addresses only that receipt's sources; the optional
byte offset is relative to the recorded span and must be a UTF-8 boundary. Reads
remain bounded by the accepted original-text budget and return a continuation
endpoint when truncated. Each expansion uses the original verified binding and
live permissions. Code results also retain the original workspace identity, so
read-root revocation or replacement blocks publication and replay, even after
restart. Historical receipt replay does not claim the old answer is current.

## Framing, events and completion

JSONL connections start with `FrameStream::handshake`: sequence zero is an
`Envelope<CapabilityReport>`. Every subsequent envelope has schema version 1,
the same request ID and the next contiguous sequence. The entire frame, including
newline, is bounded to 128 KiB. A missing newline, wrong version/request/sequence,
malformed payload or oversized frame closes the stream. A failed handshake,
including premature EOF, cannot later admit events. Unknown required capabilities
fail with `ADAPTER_UNSUPPORTED`; unknown optional capabilities are returned as
`unsupported_optional`. Both peers must implement a capability, even if an unknown
name was advertised. Ordinary transport EOF is not successful Runner completion.

`RunSpec::new` binds job ID, original invocation, configuration generation, executor
profile, stable instructions version, normalized hint, read capability and shared
budget allocation. Its runner deadline is capped by the original workflow
deadline. `RunSpec::check` preserves the triggering timeout layer. Every host
adapter must check it at admission and event/I/O waits and enforce cancellation
of its owned process or session; an outer timeout does not prove a CLI's inner
provider timeout supports 1700 seconds.

Pi Runner writes only HTTP idle and provider request timeouts into its private
per-run `.pi/settings.json`, using the remaining original RunSpec budget. It
approves that owned directory so Pi reads these settings; built-in tools, other
extensions, MCP, skills and context files remain disabled. The native profile is
unchanged. Akasha's configured Runner deadline governs this subprocess, including
when it is shorter than Pi's profile timeout. Later requests and retries remain
bounded by the original outer deadline; this override cannot restart that clock.

Claude cleanup has a separate one-second grace period, which repeated waits do
not renew. Cancellation reports `acknowledged: false` if exit is still unknown;
the original timeout/error remains authoritative. A background cleanup owner
retains the child, workspace and concurrency lease until exit is observed, even
after the handle is dropped. The version probe uses the same bounded cleanup
after its five-second query timeout. A successful native result still requires
a successful process exit. Spawn failures preserve the OS errno in
`source_code` without exposing executable paths or arguments.

`AgentRunner::start` receives that spec and a separate `ContextSource`; choosing
a Runner does not change the caller's history source. `RunHandle` provides typed
events, cancellation acknowledgment and exit status. `RunCollector` counts all
event bytes against the shared output budget and accepts a public answer only
after `Started`, a nonempty bounded `Completed` event and exit code zero. Partial
stdout, progress text, successful exit without a terminal result, duplicate
terminal events and nonzero/unknown exit are not answers. Structured failures
retain their code. Only the final output is returned; external search traces are
never folded into the main agent response.

## Read authority and tool enforcement

The actual allowlist is `ToolPolicy::authorize`, shared by every host dispatcher:
`context.describe/list/read/search`, `memory.search/get`, `code.search/read` and
`delegate`. Dispatchers must call it **before** running a requested operation and
disable every host-native write, arbitrary shell, approval, deletion, configuration,
messaging and network tool. A reported `Tool` event is checked too, but observing
an event after execution is not permission enforcement. A host that cannot
provide this tool surface must refuse the Runner configuration. Provider network
access is separate from allowing a model an arbitrary network tool.

`ReadCapability` issues 256 random bits and binds them to the original invocation
and fence. It has redacted Debug output and no Serialize implementation. Transport
code explicitly passes the bearer token out of band and authorizes it against the
same `RunLease`; never put it in model results, source text or ordinary logs.
The declared enforcement level is `tool_policy`. Same-UID arbitrary shell is
outside its isolation guarantee; no prompt or normal file permission is claimed
to be an OS sandbox.

`CapturedContext` is a real implementation over retained Storage/Capture records.
It mints handles only along the bound branch and ancestor cutoffs, at the fixed
capture sequence. Forged, sibling or another invocation's handles fail. List and
search pages hold at most 16 events with at most 4096 retained handles. Search
examines only those authorized originals, exposing a continuation when needed.
Read verifies the full immutable original hash, then returns a UTF-8 boundary-safe
window of at most 16 KiB. Metadata-only/deleted/unavailable originals are gaps,
not fabricated text. The current grant is checked under the storage read guard
for every operation and every original. Search text and tool output remain
untrusted data regardless of any instructions or approval claims inside them.

`BoundWorkspace` pins the bound workspace directory and checks current configured
read roots and the current principal's grant. Relative paths resolve with Linux
`openat2(BENEATH | NO_MAGICLINKS)`; escapes, outside symlinks, directories and FIFOs
are denied. Each file read is limited to 1 MiB before slicing. Size changes during
read return `SOURCE_CHANGED`; content hashes and timestamps describe actual read
bytes, with `changed` and `changed_during_read` diagnostics. Search accepts an
explicit bounded batch of up to 16 project-relative file paths, returning at most
64 line/range/hash observations. It is not a recursive filesystem index. A missing
file is `SOURCE_UNAVAILABLE`. Directory replacement and read-root revocation
prevent further reads. There is no atomic multi-file workspace snapshot claim.

These checks authorize each read. MVP-016 must separately revalidate all selected
content against live grants and current validity immediately before durable
RecallReceipt publication; a research session is not one database snapshot.
Already-sent model context cannot be retroactively erased by revocation.

## Shared budgets and deadlock avoidance

The initial `RunBudget` owns one origin, fence, monotonic deadline, cancellation
state and aggregate counters. `RunLease::delegate` atomically reserves both a
global semaphore slot and another start without waiting. If parent tasks occupy
all slots, delegation returns `DELEGATION_UNAVAILABLE`; it cannot hold a parent
slot while waiting forever for a child. A refused slot reservation consumes no
start. A started worker or retry does consume one; release never refunds starts.
The default is three starts total and depth one. Reads, bytes and outputs share
the same bounded counters across descendants; children cannot refresh a deadline
or reset their depth. An observer's `lease_handle` shares the same lease/permit,
not a new worker admission. Dropping all owners releases the slot.

The default reported-token limit is 128,000 across the run tree. `Usage` events
are cumulative per lease; repeated native/collector reports are charged once,
and decreasing counters are protocol errors. Cached input counts toward the
limit; reasoning already included in output is not added again. Codex uses its
thread totals, Claude reconciles per-message and final usage, and Pi/Hermes send
native usage through the authenticated context broker. Pi includes compaction
usage; Hermes checks cumulative counts before model iterations and tools.
All four adapters charge before exposing the usage event. Exhaustion wakes
blocked native reads and stops the owned process group, descendants and retries.
Native usage arrives after a request, so in-flight requests can overshoot the
limit; observed overshoot remains in the report. This is not pre-send token
reservation. Missing required native usage refuses a successful terminal result.

The workflow decides whether to continue sequentially after a delegation refusal;
it must not silently create a fresh budget. Cancellation denies later admissions,
reads and output. Host cancellation ACK, process termination and resource cleanup
remain separately verified responsibilities of the real host adapter.

## Verification and reference freeze

The integration suite exercises durable concurrent bindings/reopen, explicit
precision, missing identity, branch ancestry, gaps, live grant shrink, forged
handles, UTF-8 ranges, path escape/FIFO/symlink rejection, changed files, live
read-root shrink, bounded framing, shared budgets, terminal/exit checks and typed
failures. `tests/support/adapter_matrix.rs` implements all four caller/runner
roles from outside the library and tests all 16 combinations through the real
`CapturedContext`, with unique private caller markers. These contract fakes are
not evidence that real installed host protocols work.

The [reference set with erratum 1](evaluation/reference-v1-errata-1/README.md) freezes 60 sourced tasks,
31 critical tasks and ten paired intents, plus oracles, counting rules, thresholds
and the reference profile. The integrity checker and negative controls run in the
canonical gate. Model quality, cost, four real isolated hosts, the real cross-host
cycle and the full local Runner duration are later acceptance, still unexecuted.

The immutable freeze commit is `7356d17f0a373ade3520e6e1a1249fc52e17ba8d`;
manifest SHA-256 is `6b6dffd7882dc703e393686d327f8aaae6becad1f5566e761b8cae8f4fad6ac7`.
The gate pins this independent baseline, so rewriting both a reference input and
its manifest still fails. Errata/regressions use a separate addressable set.
Erratum 1 preserves that original set and corrects S04's whole-grant revocation
oracle to `error` / `READ_DENIED`, without protected content or a successful
RecallReceipt. A04's individual-memory rescope still expects `no_hit`. Its
independently pinned manifest is
`09bbd47c0080af5b9982438f6e75a1af40c8d57e7eb394d36c93767dab82db16`.
Its freeze commit is `f0dcef99bdc6aaae9635aa77054df525b688f5aa`.
Evaluation consumers use `scripts/check_reference.py::read_reference()` to load
the effective set; reading the uncorrected v1 task file alone is not sufficient.

## Codex native capture component

`adapter::codex::reconcile_transcript` accepts an explicitly authenticated source
identity and an explicit file, using the existing durable bounded file cursor.
It supports the captured **Codex 0.164.0** rollout format. Each native ordinal
has one event identity; native message/call IDs remain in retained visible raw
payloads. Display text comes from response items, so duplicate `item_completed`
notifications do not repeat user messages or tool results. Unknown records,
missing ordinals and incomplete tails remain pending. Unknown native versions
and mismatched session headers are rejected. This parser does not authenticate
a caller or discover the latest session.

The native fork metadata identifies the parent thread and exclusive ordinal
cutoff. A later root resume cannot expand that inherited history. Compaction
records link the already retained ancestor range and its summary, without
replacing originals. An unavailable ancestor stays observable; traversal is
bounded to 65,536 events per compaction. The native rollout does not identify
process resumes as a distinct event, so capture preserves the continuing chain
without inventing a resume marker.

Coverage is `visible_text`, not every native payload. Native system/developer
instructions, environment configuration, private reasoning and binary attachment
bodies are omitted explicitly. Public reasoning summaries and tool inputs/results
are retained; attachments have visible unavailable markers. Retained raw JSON is
a filtered native record, not a byte-identical copy of private configuration.
All captured content is unverified input and conveys no human authority.

[Native fixtures](../tests/fixtures/codex/0.164.0/README.md) bind sanitized examples
to their native versions and source hashes. Controlled local SSE runs prove
protocol and lifecycle behavior. [Native acceptance](adapters/codex-0.164.0/README.md)
records authenticated Caller/Runner, actual tool restrictions and concurrent TUI
results separately from the final source gate and review.

`CodexCaller` accepts a server-owned registration and native hook identity outside
the model's arguments. It verifies the registered session, workspace and root
source, then matches the exact native function-call ID and turn ID in an 8 MiB
tail snapshot. Missing or ambiguous identity is refused. Missing `agent_id` is
insufficient proof: unknown/subagent native sources and explicit child hooks are
currently unsupported. No caller is selected by cwd or by the latest file.

The immutable fence ends at the function-call ordinal, excluding subsequent
usage records, tool results and final output. Catch-up uses the shared capture
cursor, with a 256-frame budget and a two-second checked deadline. A missing
ancestor returns `HISTORY_GAP`; a retry preserves its stored fence even if the
native file is gone. Permission revocation applies to new calls and retries.
Focused tests cover native hook data, concurrent registrations in one workspace,
identity conflicts, unknown versions, duplicate calls, missing ancestors and
revocation. Separate native acceptance proves two simultaneous authenticated TUIs.

The daemon's private `codex` transport persists root registrations with opaque
credentials, binds native calls and queues native capture through the existing
durable scheduler. Registration credentials travel over the owner-only local
socket and stdin bridge; binding responses contain no credential. Access roots
and current read grants are rechecked for subsequent calls and capture batches.
The `adapter codex-call` stdin bridge is internal host transport, not a model
tool. `capture codex` selects the pinned format for explicit backfill input;
the binding must use stream `codex-native-v1` and `visible_text` coverage.

`doctor --host codex [--binary PATH]` observes the executable/version with bounded
output and a five-second probe deadline. It reports authentication and hook trust
as unchecked. The process owner signals its group before releasing the leader
PID, and deferred reaping retains ownership after the one-second cleanup grace.
The daemon, CLI, Codex and library regression suites pass 75 tests (five existing
ignored helpers/acceptance cases), including registration/replay across daemon
restart and native background backfill. Full acceptance and review remain open.

## Codex opt-in launcher and native hooks

Install a separate launcher for the pinned Codex version:

```sh
akasha-otsuge --config /absolute/akasha.toml adapter install codex \
  --binary /absolute/codex --launcher /absolute/akasha-codex \
  --state-dir /absolute/private-call-state
/absolute/akasha-codex
akasha-otsuge adapter uninstall codex --launcher /absolute/akasha-codex
```

Parent directories must exist. Installation atomically publishes one owner-only
script. Repeating an identical install is harmless; replacement, symlinks and
edited scripts are refused. Undo verifies its content hash before removing that
single file. Native settings and credentials are not edited. The native hook
trust review still applies: installation never automatically approves hooks.
Call-state files are separate from the launcher and are removed by their owning
PostToolUse/PostToolUseFailure callback, so uninstall cannot delete active calls.
Interrupted callbacks can leave small results; 258 directory entries stop further
admission rather than trigger an age-based deletion sweep.

The launcher selects `features.code_mode={direct_only_tool_namespaces=["functions"]}`
for that launch. This exposes native function tools directly even for models that
default to Code Mode, replacing the corresponding per-launch namespace setting.
Codex0.164.0 assigns random IDs to nested Code Mode calls without a matching
rollout entry; those calls cannot establish an exact fence. Other namespaces can
still use Code Mode. Installation reports this setting, leaves native settings
files untouched, and never infers a parent call from JavaScript text.

The hook configuration uses the official CLI override
mechanism. SessionStart/Stop schedule incremental native backfill. PreToolUse
recognizes `akasha-otsuge consult` with an optional JSON-quoted hint, and the
`consult show/receipt/original` and `job cancel` commands above. Follow-up IDs
must be Consult job IDs; shell suffixes and identity arguments are rejected.
The trusted callback binds and submits the initial call, then returns public
status/result through an atomic per-call file. Follow-ups recheck current
permissions before publishing their response. The command is rewritten to an
absolute read-only result reader. This
is necessary because the native Linux read-only sandbox rejects Unix socket
connections. Neither registration credentials nor a daemon connection enter the
sandbox command. Concurrent sessions share no current-context file or global
lock. Repeated native calls reuse the same job and per-call response; a new show
call retrieves updated status. Result files remain bounded by the IPC size cap,
including explicit full receipts. Binding errors return the native deny response.

## Codex standalone Runner

`CodexRunner::new(binary, native_home, workspace, instructions)` starts the pinned
native `codex app-server --stdio` with the selected native home and its existing
authentication/provider configuration. No other agent CLI is used. The native
home must contain a valid bounded `models_cache.json` from Codex0.164.0; missing or
unsupported metadata returns `ADAPTER_UNSUPPORTED`. This versioned dependency is
necessary because model metadata can enable tools even when feature flags are off.

Each run creates one private temporary workspace and a private catalog that
disables native shell, patch, search, image, Code Mode and unrelated tools while
retaining the model's remaining metadata. Before starting an ephemeral thread,
the Runner disables every effective MCP server, native hooks, plugins, agent
delegation and host skill discovery. It requests the native read-only sandbox,
disabled tool networking and no approvals, and verifies the returned policy.
The registered inventory contains `context_describe/list/read/search`,
`memory_search/get`, `code_search/read` and `delegate`. Strict shared request
decoding rejects model-supplied caller identity. CapturedContext serves only the
context operations; the consult workflow supplies the remaining research tools
under the same invocation, permissions and budget.
Both code tools take individual workspace-relative file paths; they do not expand
directories, globs or absolute paths. Read a named file directly with `code_read`.

The Runner accepts one structured nonempty `answer`, then closes stdin and waits
for the native process to exit successfully. EOF, an answer followed by a failing
exit, duplicate calls/finals, unexpected identities, malformed requests, excessive
output and expiry cannot become success. All waits use the original deadline.
Cancellation and drop retain process-group, lease and directory ownership until
the shared reaper terminates and reaps the child. Native unauthorized/HTTP401
errors become `AUTH_REQUIRED`; provider error prose is never returned as a
diagnostic. Native configuration/account/moderation notifications are metadata,
not tool requests or completion evidence. `doctor` remains a free version probe;
its `NotRun` self-test status does not claim authentication or model acceptance.

The native0.164.0 controlled-endpoint fixture passed the installed launcher,
real hook, actual read-only shell sandbox, exact call fence, result cleanup and
uninstall round trip. A prior failed socket bridge and a native EPERM diagnostic
are retained; no sandbox or network permission was relaxed. The result proves
that transport. Separate authenticated Runner and concurrent-TUI receipts are
linked above; they do not measure model quality.
The interface follows the official [hook protocol](https://learn.chatgpt.com/docs/hooks)
and [configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference),
checked against the installed native binary rather than the older source clone.

## Hermes native Caller and capture

The Hermes0.22.5 plugin, opt-in install/undo, immutable call fences and canonical
export backfill are described in the [Hermes support and recovery notes](adapters/hermes-0.22.5/README.md).
Controlled native plugin tests cover two profiles and both plugin isolation modes.
Independent Runner and authenticated CLI/TUI acceptance remain pending in MVP-014.

## Claude native bridge under development

`adapter claude-call` accepts a bounded host-only JSON envelope on stdin and uses
the existing private daemon IPC. It is not a model tool schema. Registration
checks the installed2.1.311 executable and configured read roots, then retains
only a hash of its random bearer token with the session, source and workspace.
The token response is for the native bridge; it must never become a tool result.
The same-UID local management boundary remains the declared trust boundary.

Root binding fixes the native turn, tool call, event UUID and observed event
cursor before research. It reuses the existing incremental file cursor and checks
for the requested tip between at most256 single-frame batches, stopping new
batches after two seconds. This bounds catch-up work, not an individual storage
operation's latency. A missing tip returns `HISTORY_GAP`; it never selects the
latest session or substitutes another caller. Long backlogs can be imported with
the existing capture command before retrying. The saved invocation and capture
sequence are immutable across retries and daemon restarts; live permissions and
read roots are checked again. Changed identity or fence data conflicts, and an
unregistered child is refused. Re-registering the same source preserves call
identity. Brief native transcript flush delays are retried within the same
catch-up deadline; malformed records remain explicit gaps.

The native `caller.js` module captures session, turn and append events, registers
the optional-hint `consult` tool and submits it as a durable job from the verified
native call. It returns only public status/results and queues durable backfill on
turn completion and session end. Caller binding waits until the native transcript
directory exists. The registered native home is inherited by children and checked
against current read roots before use; diagnostic Bind requests remain supported.
`scripts/check-claude-caller.py` verifies startup/resume bindings and completed
backfill with the actual CLI, production hook and daemon under local synthetic
SSE. That probe replaces Consult with diagnostic Bind and explicitly does not
qualify Runner execution or authenticated model use. Claude2.1.311 requires at
least one native builtin (such as `ToolSearch`) to register plugin tools;
`--tools ""` refuses Consult registration. Native children are identified by append/turn events and the host's agent
registry. Only a known registered parent can admit a child. The pinned2.1.311
transcript layout and each frame's session/agent/sidechain identity are checked;
an unknown or mismatched child cannot fall back to the root. Parent identity is
immutable across registration retries. Ordinary child transcripts have their
own ancestry; registering the parent does not invent inherited originals.
The hook checks the live cwd against the session's registered workspace before
binding either a root or descendant call. A changed cwd returns
`BINDING_UNAVAILABLE` before the tool body executes; start a new native session in
the new directory. It does not rewrite cached parent/child registrations.
The canonical gate exercises the production hook with root, child and grandchild
workspace transitions, including a root call before a descendant call.

Native sync jobs persist `until_event` with their capture input and wait for
that event within the original job deadline, including delayed file creation
or a partial final record. Restart preserves the deadline and cancellation
remains available. Malformed frames before the target fail with `HISTORY_GAP`.
Manual imports without a target retain their existing partial/gap reports.
Native startup/resume/clear/compact/fork reasons are retained in registration
evidence. A CLI fork copies its retained UUID chain into the new session; those
originals remain readable within that branch. The2.1.311 SessionStart contract
does not expose a fork's parent session ID, so that relationship stays unknown.
No later parent or sibling events are imported into the branch.

### Claude session plugin installation

The supported native version is2.1.311. Install into a new directory whose parent
already exists, using the same executable/configuration as the running daemon:

```sh
akasha-otsuge --config /absolute/path/akasha.toml adapter install claude \
  --binary /absolute/path/claude --directory /absolute/path/akasha-claude-v1
akasha-otsuge doctor --host claude --binary /absolute/path/claude \
  --directory /absolute/path/akasha-claude-v1
claude --plugin-dir /absolute/path/akasha-claude-v1
```

The returned `launch_args` identify the exact native executable and plugin.
This uses the native [session plugin option](https://code.claude.com/docs/en/plugins);
loading is explicit on each launch. Start the daemon first and include the
native transcript directory in its configured read roots. Explicit `--config`,
`--profile` and `--socket` options are retained in the bridge arguments; omitted
configuration uses normal defaults. Installation does not start a daemon or add
a consult model tool; that workflow is tracked in #16.

The installer publishes a verified private directory atomically and never edits
native user/project settings or other hooks. An identical reinstall succeeds;
changed owned files, symlinks and unexpected content are refused. Upgrade into a new
directory, validate it with `claude plugin validate DIR`, then switch the launch
path. Keep the old directory to roll back by selecting its launch path again.
The executable paths in the old plugin must remain available for rollback.

```sh
akasha-otsuge adapter uninstall claude --directory /absolute/path/akasha-claude-v1
```

Uninstall removes only the receipt's verified files and empty owned directories;
user additions or edits outside the native-generated paths prevent removal.
The native TUI [generates type support](https://code.claude.com/docs/en/plugins/mods/create#get-type-definitions-for-your-version)
under `.claude-plugin/types/` and at `tsconfig.json`. These paths, including user
edits, are preserved and listed in `preserved_paths`; their parent directories
remain after uninstall. The installer neither validates nor deletes their contents.
Removal first detaches the verified plugin directory and retains a recovery receipt.
If removal fails, `doctor --directory <original-path>` reports
`uninstall_pending` and `recovery_dir`. Resolve the reported filesystem problem
and repeat the same uninstall command; reinstall is refused while removal is
pending. Changed files and replacements at the original path are retained.
`doctor` reports the observed CLI
version, supported-version match, optional installation integrity and explicit
coverage limits. Its version probe does not execute a caller/model self-test,
check authentication or claim that a running session loaded the plugin. Fork
parent IDs remain unavailable. The local native protocol fixture exercises the
unmodified generated plugin in headless and controlling-PTY sessions, durable
capture, doctor and removal. UUIDs and cursor-prefix hashes bind the captured
input/output to native files. Local command/queue notifications that may not be
persisted cannot advance the file fence. Native permission metadata is skipped;
turn-duration events retain their identity and session visibility. Authenticated
model and the remaining TUI lifecycle acceptance remain separate.
