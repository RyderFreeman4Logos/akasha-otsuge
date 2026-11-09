# MVP-022 investigation acceptance

The [incident protocol](../incidents.md) reuses the existing Editor, Runner,
permission checks and patch transaction. The issue base is `94b22af`; focused
source/test phases extend through `2f334fa`. Native acceptance,
the final exact-candidate gate, independent review and hosted CI are separate
stages; no final completion is claimed by the focused results below.

## Focused behavior

| Evidence | Result and boundary |
| --- | --- |
| Configuration | 21 tests: opt-in defaults, finite budgets/deadlines, named executors, priority/grouping policy and hot reload. Archived default config digests remain valid. |
| Durable ledger | 100 distinct events form one reported group, with all references paged and preserved across restart; replay does not increment count. No executor means visible review, with no Runner start. |
| Grouping and priority | Configurable deduplication and trigger priorities, recurrence escalation and critical-event urgency retain explanatory reasons. |
| Evidence reader | Paged 52 KB logs and non-UTF-8 payloads reconstruct exact bytes/hashes. Complete automatic and manual policies differ; partial-source logs remain candidates. |
| Classification | Code defects, environment failures, Agent misuse and changed requirements do not modify memory. Missing recall and insufficient evidence stay open; stale conditions can correct only the listed target. These are controlled reports, not a model-quality score. |
| Live authority | Eight controls cover automation tightening/loosening, trigger/executor/read-root revocation, operator dismissal, token exhaustion and interrupted restart. At most one Runner starts and duplicate events do not repeat work. |
| Recovery | Real SIGKILL after job admission exposed an orphaned incident association after queue expiry. The repair reconnects the persisted job and preserves its original timeout, with zero Runner attempts in the expired case. Shared Editor response/patch checkpoints retain their existing crash tests. |
| Command wrapper | An invalid signal is rejected before execution; a valid signal carries a real rustc error into a controlled code-bug investigation without changing memory. The wrapper refuses command replay. |
| Old API oracle | The program is constructed from the exported v2 memory's function call and fails under the real compiler; the independent new-API program compiles and prints v2. Exact v1/v2 searches return only their matching memory, while the owner-scoped Caller search returns neither. Source memories precede the Caller fence. |

The latest focused incident run has **9 passed, 1 opt-in ignored** and all-target
Clippy passes. The ignored case is the explicitly authenticated native workflow,
not silently counted as success.

## Retained failures

Behavioral REDs cover missing incident configuration, missing outcome signal
admission, absent job dispatch, absent context handles, unsupported grouping
policy, missing code-bug classification, lost post-admission crash association,
and unsupported wrapper signals. Passing later phases do not relabel them.

Harness failures are separate: source metadata was initially mistaken for a
source span; a memory-history assertion used the wrong JSON location; early
compile attempts used a private hash helper and an unsupported formatter. The
scope filter initially expected a project query to include an owner-level
incident; the correct owner query passes without changing scope enforcement.

Native R1 bound the Caller before configuring its native executor, so Consult
correctly retained its original controlled route. R2 repeated a TOML table and
never reached daemon readiness. The new no-auth route preflight then exposed a
one-start/depth mismatch; it now verifies the intended profile at admission.
R3 completed a genuine Codex Consult but selected no API memories: the fixture
had seeded them after the immutable Caller fence. A local failing assertion
proved the ordering error; fixture preparation now precedes binding. None of
these failures weakens production permissions, source fences or budgets.

R4 confirmed a second fixture assumption error: the native Caller is owner-scoped
and lexical search filters the exact scope, so its Consult correctly did not
select either project/version memory. No production scope rule changed. The
native acceptance now constructs its failing program from the exported v2 claim
and tests the actual failure-to-investigation chain. It makes no native recall
or autonomous program-generation claim. Three public retrieval controls prove
the owner/v1/v2 distinction before any model call.

R5 reached the real investigator but exhausted the fixture's 128,000-token limit
at 136,544 reported tokens, 17 reads and one start. Native accounting includes
cached input. The evidence-heavy acceptance now allows a finite 512,000 tokens;
production defaults, one-start bound, permissions and workflow clocks are
unchanged. This failure is retained separately from the earlier fixture errors.

R6 completed the real investigation and produced a v2-only candidate, retaining
gaps and a high-priority review. The harness incorrectly required unconditional
automatic adoption and stopped before its final history/replay checks. R6 stays
a failed harness run; production authority was not relaxed to satisfy it.

## Real failure to handling

R7 passed the complete opt-in test on source `2f334fa` using authenticated
Codex CLI 0.164.0, its existing `gpt-6.1-sol` profile and rustc 1.99.1 on Linux
x86_64. The actual investigator started once, made 25 tool reads and reported
254,840 tokens including cached input. The run took 100.78 seconds.

The controlled Caller supplies an exact target and the harness constructs a
program from the exported v2 claim. The real compiler rejects its old API call.
The investigator reads the memory, original, current code and complete outcome
logs, then submits a `memory_error` correction confined to the original v2 scope.
It retains an invalid-argument tool gap, the lack of an observed successful
fixed-program run and the distinction between fixture construction and autonomous
memory reliance. These gaps keep the patch a **candidate**, with the previous
adoption intact and the incident **open/high** for review. This run does not prove
automatic native adoption; complete-evidence automatic adoption is covered by
the controlled tests above.

The remaining assertions verify v1 and unrelated memory exports are byte-for-byte
unchanged, compile/run the independent fixed program successfully, and replay the
same outcome without another memory revision. Native profile configuration is
unchanged, no owned process remains, and the owned temporary fixture is removed.

The [archive manifest](results/mvp022/manifest.json) verifies 192 compact files,
176,664 bytes excluding the manifest, including every failed attempt. Its SHA-256
is `1bd3b9ed0dc698958623af23625784fa2a9f128bfe6c884be49a601128479fd0`.
The native [terminal receipt](results/mvp022/mvp022-native-codexr7.json) binds the
compressed final result to SHA-256
`8018e2c77e652a6e3a88c863542e64faefa76630b285ff02b90ed7870efb7495`.
No native child transcripts or credentials are published. The exact canonical
gate, independent whole-range review and CI remain separate publication stages.

## Independent review and read-admission repair

The first exact gate on `c17c272` passed in 274.629 seconds: 484 Rust executions
including the installed repetition, 133 daemon tests per run, 29 Python tests,
Node checks, release build and installed lifecycle/UID isolation. Independent
review also passed its nine focused incident cases, then found a MODERATE
regression by tracing the failure path: outcome data was loaded before reserving
the read budget. The full [FAIL report](results/mvp022-budget-fix/mvp022-whole-review-r1.md)
remains immutable; passing gates did not waive the finding.

A new test-only per-Service counter proved that a denied read still accessed the
outcome payload. Commit `4063f86` reserves the bounded storage-record allowance in
the shared incident reader before loading. The regression covers both description
and text entry points, read-count and byte exhaustion, three repeated refusals,
and permitted-read controls. Six Editor unit tests including recovery, nine
incident integrations and Clippy pass after the fix. The raw-log branch already
reserved its bytes before loading and retains its existing lossless-read tests.

The [repair manifest](results/mvp022-budget-fix/manifest.json) preserves 22 files,
38,520 bytes excluding its manifest; SHA-256
`4e65ca467d8eb55835340ae20932fe2d9fad6cac981d579f3b37e4463f6ed1e1`.
R7 remains historical native acceptance; it was not rerun after this admission
repair. A conservative extra frame for each of its 25 reads totals at most
3,579,440 charged bytes, below the unchanged 4,194,304-byte limit. This bound is
not a fresh provider result. Fresh exact-candidate gate, whole-range review and
CI are required before merge.
