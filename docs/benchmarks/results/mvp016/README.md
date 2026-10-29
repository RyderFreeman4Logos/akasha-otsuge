# MVP-016 Consult evidence

These samples exercise trusted native Caller → durable Consult → native Runner,
progressive research, bounded public results and immutable receipts. The disposable
workspace contains a197-byte policy file; its SHA256 is recorded in
[native-consult.json](native-consult.json). They are functional acceptance samples,
not the frozen60-task quality evaluation assigned to MVP-027.

The owner has no Claude subscription and explicitly authorized Codex or the
existing localhost:48317 Pi/Hermes route as a temporary substitute. Claude-native
authentication remains deferred in #12. Controlled Claude plugin/bridge tests do
not establish authenticated Claude acceptance.

| Actual native route | Recorded evidence |
| --- | --- |
| Codex0.164.0 → Codex, empty hint | Native Caller, policy source SHA, public/receipt equality and cleanup |
| Hermes4960e41 → Hermes, empty hint | Existing installed dependencies and native authentication, same source/receipt checks |
| Pi1.4.0 → Pi, empty hint | Same source/receipt checks through grok-4.7 on the existing local CPI |
| Pi → Pi descendant | Shared run tree and usage, parent/child evidence and bounded final result |
| Pi → explicitly named Codex | Actual cross-host executor with the isolated verified Codex0.164.0 binary |

The final Pi empty-hint/delegation and Hermes empty-hint samples all pass on
`ec95da5` after the queued-greeting repair (16.705s,30.974s,31.715s).
Each case retains its own source commit/tree and product binary hash. Historical
passes are not silently reassigned to the final commit. Effective models are
recorded only when observed; Codex's effective model was not separately captured.
Incomplete public results preserve gaps from extra failed searches. A successful
policy lookup does not mean every search or memory lookup succeeded.

## Original deadline

The actual Pi1.4.0 CLI also ran against a deterministic loopback SSE provider,
without a paid model response. The provider delays the first response1605seconds;
Akasha retains its original1700-second run budget. A separate10-second deadline
case deliberately expects `RunnerTimeout` and verifies complete process cleanup.
Its Rust test exits101 as the expected negative control, not as a passing Rust test.

R1 stopped in its fast control because the old provider fixture expected four
tools instead of nine. Its stale running status is superseded by the retained
terminal failure receipt. R2 used the corrected fixture and exposed the native
300-second HTTP/provider ceiling: long execution failed at300.746seconds while
both its short control and timeout cleanup passed. The fix sets both native
request timeouts from remaining RunSpec time in the owned run directory only.
Native user profile settings are unchanged. R3 passed: the long case completed1612.327485s after a1605s delay; the short
control took1.320521s and the10s deadline case cleaned up in11.200486s. Its
[qualification summary](qualification.json) and original receipts retain exact
source/binary identity. HTTP timeout and native fixture source remained unchanged
through the later bridge repair; [reuse.json](reuse.json) records those hashes.

## Queued private callbacks

Pi and Hermes serial brokers can hold later callback greetings behind delegated
research. Deterministic controls reproduce the old premature10-second timeout
with a30-second run budget and an11-second delayed greeting. They also expose
incorrect `IPC_TIMEOUT` classification when the original2-second deadline expires.
The repair keeps connect/peer checks bounded and uses the same original deadline
for the queued greeting, request and response. Both causal controls and final
regressions are retained. Strict framing, peer identity and conclusion schema
validation remain enforced.

Earlier native parent `PROTOCOL_INVALID` observations occurred immediately after
child success. The failed connections themselves were not traced, so the delayed-
greeting controls establish the defect without inventing exact historical timing.
Another native response supplied disallowed `gaps_note` and was correctly refused.
Those failures remain in the ledger beside the successful samples.

## Artifacts and limits

[artifact-manifest.json](artifact-manifest.json) pins the explicitly selected
artifacts. Scripts under `harnesses/` are frozen machine-specific experiment
records; adapt their paths and supply an already authenticated native profile to
repeat them. They are not an automatic installer or a claim of portable replay.
No authentication/profile files or native conversation transcripts are published.
The retained workspace and policy text are synthetic. Owned processes ended before
scratch removal; the isolated Codex pin remains for the later MVP-027 evaluation.
The original native profiles were preserved. See the complete [remaining issue
queue](../../../plan/todo.md) for acceptance still assigned to later milestones.

The final CI40951189004 retained a running descendant after native cancellation
was acknowledged. A controlled shared-reaper test reproduces premature resource
release when the leader exits first. The successor fix retains its PID and the
resources until the entire observed process group has exited, preserving the
original one-second acknowledgement grace. All 73 affected Rust tests pass, as
do Clippy, formatting and five real Pi protocol cases (roundtrip, retry, denied
handle, cancellation and forbidden tool), with no owned process or scratch
residue. [group-settlement.json](group-settlement.json) binds the tested source
and binary hashes; final gate/review/CI checks are recorded on PR45. This CI
failure is not relabeled as a passing run. The earlier 26-minute native HTTP
deadline receipt retains its original identity; this cleanup change has its own
short native qualification.
