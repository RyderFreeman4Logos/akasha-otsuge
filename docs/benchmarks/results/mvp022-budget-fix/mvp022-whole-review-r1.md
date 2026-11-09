VERDICT: FAIL

The frozen range has one MODERATE introduced defect: incident outcome reads perform payload storage work before reserving their read budget. The canonical gate and required independent focused check pass, but neither covers this exhaustion path.

Reviewed identities:

| Item | Verified value |
|---|---|
| Repository | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp022` |
| Base / `origin/main` | `94b22af5dcd367499b013cbdcbe1508eb8123aa1` |
| HEAD | `c17c272dbecec55e0db50cd3a7117a4a86bb65d5` |
| Tree | `dc14dbe2ee6ec08742164f8c752ff522374c9639` |

Base ancestry verified. Initial and final filesystem-backed Git status were clean, including untracked files. HEAD, tree and `origin/main` remained unchanged.

**Finding 1 — MODERATE, REGRESSION: reserve read budget before loading the incident outcome**

Location: [src/daemon/incident_context.rs:44](/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp022/src/daemon/incident_context.rs:44), with the late reservation at line 45.

`incident_view` calls `read_outcome(...).await` before `lease.charge_read(...)`. This loads and deserializes the complete outcome payload and validates its attached receipts, claims and log metadata before deciding whether the read is permitted by the remaining budget.

A causal counterexample follows directly from the current call chain:

1. An investigation reaches its 128-read limit while retaining output, token and workflow budget.
2. The investigator requests `context_read` for `incident:outcome`.
3. `incident_text` calls `incident_view`, which loads and validates the outcome.
4. The subsequent read reservation fails with `BUDGET_EXHAUSTED`.
5. `Research::execute` converts that dispatch failure into a tool error response and records a gap.
6. `RunLease::charge_read` leaves usage unchanged on refusal and does not latch exhaustion. Another request can therefore repeat the payload read before receiving the same refusal.

Thus the configured read limit does not bound these outcome payload reads. Other limits still bound the session; this is not an unlimited execution claim. The retained error gap also prevents this path from silently obtaining stronger patch authority.

Attribution is **REGRESSION**: the incident reader and its dispatch route are new in this range. The unchanged base budget behavior does not itself perform the newly introduced payload reads.

Reserve a conservative bounded allowance before calling `read_outcome`, and account for the relevant payload work within that allowance. Add a focused exhaustion regression that observes no outcome payload access after reservation refusal. This finding is established by source reasoning; no additional runtime probe or source modification was performed.

**Instruction and source inspection**

Before source inspection, I checked actual filesystem `AGENTS.md` and `CLAUDE.md` names in the checkout and its ancestor chain, then walked nested directories while following symlinks and avoiding resolved-directory cycles. No such instruction files were found. Their resolved targets and line counts are therefore not applicable.

The checkout’s logical and resolved paths are identical. Its `target` link resolves to `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/target`; a separate instruction-name scan beneath that link also found no instruction files. The source-tree walk covered 310 directories; the shared build-tree instruction scan covered 2,154 directories.

Applied review guidance came from `/home/obj/.codex/skills/native-code-review/SKILL.md` and its relevant evidence, attribution, lifecycle, Rust, receipt and verdict references, together with the active Ponytail rules.

I audited the complete `base...HEAD` source range, including:

- `src/config/{mod,schema}.rs`, `src/incident.rs`, `src/editor.rs`, `src/outcome.rs`, `src/outcome_command.rs`.
- `src/daemon/{incident,incident_context,editor_workflow,consult,consult_research,consult_run,jobs,outcome}.rs`.
- `src/adapter/context.rs`, `src/daemon.rs`, `src/ipc.rs`, `src/main.rs`, `src/lib.rs`.
- Changed configuration, Editor crash, daemon incident/native and Codex fixture tests.
- Shared budget, context, runner, permission, memory preparation, adoption and patch transaction code needed to assess those changes.

I read `docs/spec/issues/MVP-022.md`, `docs/incidents.md`, `docs/benchmarks/incident-acceptance.md` and the changed supporting documentation.

Apart from the finding above, the reviewed paths preserve the following boundaries:

- Explicit opt-in signals and outcome attribution commit atomically. Five trigger kinds, grouping, recurrence priority, event replay and the 256-pending bound are coherent.
- Admission retains original configuration and clocks. The SIGKILL repair reconnects an existing admitted job before queue-expiry handling without renewing deadlines or replaying Runner work.
- Current grants, trigger revocation, executor revocation, interruption and publication checks remain enforced.
- Owner-scoped Callers can target narrower memories. Exact target revisions and final immutable scope/kind guards remain intact.
- Outcome/log handles expose paginated UTF-8 or lossless hex envelopes with attributable fragment spans. Original source completeness remains distinct from actually-read envelope coverage.
- Unread evidence, partial logs, workspace/artifact gaps, tool errors and conflicts retain limitations and prevent stronger adoption.
- Classification permits patches only for `memory_error` and `stale_conditions`. Complete non-memory explanations can dismiss; missing recall and insufficient evidence remain open.
- Candidate publication, active adoption, incident state and future notification acknowledgement remain separate.
- Wrapper signal validation precedes command execution, and its existing durable marker still refuses command replay.
- Shared Editor response/patch checkpoints and protected human decision/preference rules remain in place.

**Canonical gate evidence**

I reused `/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/akasha-evidence/mvp022-canonical-r1-verified.json`; I did not rerun the broad gate.

Its SHA-256 is `c202681247a71683bd8b14c5b256f074cf4b4287d0bd5d2cf6b1fb2bfda44575`. It binds the exact reviewed base, HEAD and tree to clean-before/after custody and `just check`, exit **0**.

All four referenced artifact hashes independently matched:

| Artifact | SHA-256 |
|---|---|
| Admission | `acecd05280789d80dfe1d5f540ceb51de0874757c06da547fd50dcdf9a709140` |
| Terminal | `4f00c73b587420440502b9a97d776e89f55ad9a0e893f9cc77be62daf82e8d20` |
| Collected | `4b24c3174d571699ed954b0000b4c9ad8beb9bad51eed173593ffc3747fb6fdb` |
| Complete log | `d1544be2f538660828255d83eb3f6aa3913f4e9a903c065cac2dc6dca0a63a6b` |

Independent parsing of the canonical log found **20 Rust result rows, 484 passes and 31 ignores**, including installed repetition. Both daemon runs recorded **133 passed, seven ignored**. The receipt also records successful installed lifecycle and UID-denial controls with zero TCP sockets. These remain historical exact-candidate gate evidence, separate from this review’s focused execution.

**Independent focused check**

Executed exactly once:

```text
cargo test --locked --test daemon incident_tests:: -- --nocapture
```

Result: **exit 0; nine passed, zero failed, one explicitly ignored native test, 130 filtered out**. Test execution took 6.50 seconds.

Complete stdout/stderr was saved to:

`/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/akasha-evidence/mvp022-whole-review-r1-tests.log`

SHA-256: `2e898bd4bf887ab908df42b63eab93f98fb4ff19236ae6c05aeb0e1b79110fd9`.

Cargo reported **1.99.1**. The inherited `RUSTUP_TOOLCHAIN=1.99.1`, TMPDIR, external Cargo target, sccache wrapper and sccache UDS were retained. No retry or additional Cargo test was run.

**Archive and acceptance evidence**

The manifest verifies **192 files and 176,664 bytes excluding the manifest**. Every entry’s stored byte count/hash and uncompressed byte count/hash matched. Archived terminal-to-log hashes also matched.

Manifest SHA-256:

`1bd3b9ed0dc698958623af23625784fa2a9f128bfe6c884be49a601128479fd0`

I inspected the classification record, admission-crash RED/GREEN receipts, native R1–R6 diagnostics and terminal receipts, selected failed logs, and R7’s terminal receipt and structured final result. Historical failures remain failures:

- R1–R4 retain route configuration, TOML, capture-fence and exact-scope fixture errors.
- R5 retains budget exhaustion at 136,544 reported tokens against its 128,000-token acceptance budget.
- R6 retains the incorrect unconditional-automatic-adoption assertion and its unreached final controls.
- The admission-crash RED retains the missing job association after queue expiry.

R7’s uncompressed final-result SHA-256 matches:

`8018e2c77e652a6e3a88c863542e64faefa76630b285ff02b90ed7870efb7495`

R7 records authenticated Codex **0.164.0**, configured **gpt-6.1-sol**, one investigator start, 25 reads and 254,840 reported tokens. It demonstrates actual compiler failure followed by a v2-only candidate correction, retained previous adoption, open/high incident, preserved uncertainty, unchanged v1/unrelated memory, independent successful compiler control and replay without another revision.

The fixture constructs its failing source from exported v2 memory. R7 therefore proves neither autonomous recalled-memory reliance nor native automatic adoption. Its source/test files are unchanged between archived source `2f334fa7dafb048b11ee4d5b105509896ed7b12f` and this HEAD. Cleanup and unchanged-native-profile claims are recorded historical receipt evidence, not freshly executed acceptance.

No source edits, commits, worktrees, publication, agents, broad gates, provider/native acceptance, child-transcript inspection or global environment changes were performed. I did not write the review report or its self-hash; the invoking CLI owns those bytes.

Not checked: fresh native/provider acceptance, hosted CI or stress behavior. The introduced read-admission defect remains the blocking risk despite passing gates.

VERDICT: FAIL