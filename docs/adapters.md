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
protocol and lifecycle behavior. Authenticated Caller/Runner, actual tool restriction and concurrent TUI
acceptance remain MVP-013 work.

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
revocation. These tests do not yet prove live TUI acceptance.

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
recognizes exactly `akasha-otsuge consult` or that command followed by one
JSON-quoted hint; shell suffixes and identity arguments are rejected. At this
stage the command returns a binding diagnostic; the consult workflow is MVP-016.
The trusted callback binds the native identity and stores an atomic per-call
result, then rewrites the command to an absolute read-only result reader. This
is necessary because the native Linux read-only sandbox rejects Unix socket
connections. Neither registration credentials nor a daemon connection enter the
sandbox command. Concurrent sessions share no current-context file or global
lock. Binding errors return the native explicit deny response.

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
Only `context_describe`, `context_list`, `context_read` and `context_search` are
registered. Their requests pass the existing invocation, permission and shared
budget checks; model arguments cannot supply the operation or caller identity.
Memory/code tools and workflow composition are integrated in MVP-016.

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
that transport only, not authenticated model quality or two concurrent TUIs.
The interface follows the official [hook protocol](https://learn.chatgpt.com/docs/hooks)
and [configuration reference](https://learn.chatgpt.com/docs/config-file/config-reference),
checked against the installed native binary rather than the older source clone.
