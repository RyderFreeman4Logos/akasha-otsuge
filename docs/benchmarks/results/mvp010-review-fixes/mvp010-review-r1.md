VERDICT: FAIL

ACTIONABLE_COUNT: 5
REVIEW_SCOPE: independent native whole-candidate review of MVP-010 / draft PR #38, origin/main...HEAD.
REVIEW_TRANSPORT: native fallback; the parent reported a failed Hermes Gateway-starting deadline with no verdict. No Hermes retry, delegation, or child reviewer was launched.

## Frozen identity and custody

- Logical checkout: `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010`.
- Resolved checkout: `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010`.
- Branch: `feat/mvp-010-retrieval`.
- HEAD: `987d369635b1d40d172fbfbe97e81258c3b9e811`.
- HEAD tree: `7a8520f69e76b60ebfeea241f8a22f0b64bb9932`.
- `origin/main` and merge base: `a09d9d691840278e2905aa39808dabd01126c663`.
- Whole range: 175 files, 13083 insertions, 576 deletions; both the retrieval implementation commit and the later three-file build change were reviewed.
- `git diff --binary --full-index origin/main...HEAD`: 747735 bytes, SHA-256 `a8b2d96f3ba2a063564897e8834a33e8f35bb1405cd9ea2bc733265f817c8e17`.
- Preflight before source inspection and pre-report postflight both found the exact identities above and empty `git status --porcelain=v1 --untracked-files=all`.
- Every changed working-tree blob, plus the immediate unchanged sources/contracts listed below, matched its frozen HEAD Git blob at postflight. The SHA-256 inventory below records reviewed file bytes; the archive inventory covers all receipt members.
- `readlink target` remains exactly `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/target`. Its physical resolution is `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/target`, the expected mount alias.
- This external report is exclusively created once. Its final SHA-256 and the post-write repository attestation are returned separately after reopening it; no self-referential report hash is embedded.

## Actionable findings

All five findings are ORIGIN REGRESSION against the exact merge base. This review's explicit zero-actionable rule makes even the LOW finding blocking. Findings 1, 2, 4 and 5 follow directly from the frozen source; their triggers were not executed by this read-only reviewer. Finding 3 additionally has a retained failing CI log.

### R1 — A sync batch continues authority reads and provider dispatch after grant shrink

ORIGIN: REGRESSION. SEVERITY: HIGH.
Primary location: `src/retrieval/vector.rs:153` and `src/daemon/retrieval.rs:128`.
Related seams: `src/daemon/retrieval.rs:107`, `src/retrieval.rs:267`, `src/daemon/models.rs:94`, `src/daemon.rs:298`, `src/daemon/retrieval.rs:139`.

Trigger: admit an owner-wide `index sync` whose next 32-transaction batch contains at least two active Memory records, with an alpha record followed by a beta record. Hold the first embedding response, complete `access grant` to alpha (or revoke the grant), then release that response. This management operation can complete while the provider is held because the sync releases `admission` before its native/model work.

The only sync authorization is before the entire lexical/vector batch. The vector loop subsequently calls the unguarded `Domain::active` for the beta record and sends its claim through `Client::call`. Its cancellation future checks stop, explicit cancellation and job existence, but never the live grant; `AccessSet` also does not notify that future. Therefore a later record can be read and sent after the narrowing operation has committed. The last batch can publish progress, activate a generation and finish without another authorization check. The lexical batch has the sibling unguarded read at `src/retrieval.rs:267` across awaited native writes.

Impact: the live grant ceases to bound subsequent retrieval indexing reads and model input. A final `job show` denial cannot retract the newly dispatched content. This violates the current-live-permission requirement in `docs/spec/03.md:170`, particularly its distinction between content already in model context and subsequent unauthorized reads. The new retrieval batch owns this regression; the unchanged generic model-cancellation helper is insufficient for its new authority-reading caller.

Minimal repair: carry the current authority predicate through sync's per-record read and provider admission boundaries, and revalidate before final progress/activation/success publication. Serialize the short permission/read decision with grant mutation; do not hold the management admission mutex over a provider wait. Abort with an explicit denial while retaining honest physical-versus-durable-prefix semantics. Apply the same predicate to lexical and vector batch siblings.

Focused regression needed: use a controlled held first embedding and two project records, shrink the grant through the public CLI, and prove that no later forbidden record is read/dispatched, the job cannot report authorized completion, and resumable completion watermarks are not falsely advanced. The current grant race fixture exercises search publication, not this sync interleaving.

### R2 — A project-scoped receipt can replay archived owner-wide index diagnostics

ORIGIN: REGRESSION. SEVERITY: MODERATE.
Primary location: `src/daemon/retrieval.rs:427` and `src/daemon/retrieval.rs:550`.
Related readers: `src/domain/recall.rs:397`, `src/daemon/jobs.rs:919`, `src/daemon.rs:284`.

Trigger: with an owner-wide grant, run a search scoped to alpha while beta records contribute to global index progress. Use a no-hit query and no applicable global rules, so the receipt has no selected records or source spans outside alpha. After completion, narrow the grant to alpha and request that receipt through `receipt show`, `receipt replay`, or `job show`.

Search embeds `Some(&progress)` whenever the current grant covers `owner_scope`, and its publication check correctly repeats that authorization. However, it persists only the requested alpha scope in `RecallDraft`. Replay checks the receipt scope, selected Memory scopes and source scopes; none records the additional owner-wide requirement of the embedded `index` object. All three public readers therefore return the old global row counts, watermarks and vector call diagnostics under the now-narrow grant. A new search at that same grant instead emits `index: null`, and standalone `index status` is denied.

Impact: narrowing a grant does not enforce the newly documented global-index diagnostic boundary on archived output. The defect is not receipt immutability: exact bytes should remain stored, but access to those bytes must retain every scope requirement. The candidate introduced this global-metadata-bearing receipt and its project scope; the base had no retrieval search producer. The existing rule-shrink test contains an owner-wide selected rule, so denial through that rule masks this metadata-only case.

Minimal repair: durably bind the owner-wide requirement when an output includes global diagnostics and enforce it in the shared recall reader before every replay/show surface. Alternatively omit owner-wide metadata from narrower-scope outputs. Do not rewrite a receipt's archived bytes after a permission change.

Focused regression needed: replay a project no-hit receipt containing global metadata after narrowing to that project; each public receipt/job output surface must deny it, while a receipt originally rendered with `index: null` remains readable under its valid narrow grant.

### R3 — The CI prerequisite omits the standard Protocol Buffers definitions

ORIGIN: REGRESSION. SEVERITY: HIGH.
Primary location: `.github/workflows/ci.yml:18`.
Related changes: `Cargo.toml:26`, `docs/development.md:7`.

Trigger: the declared Ubuntu 24.04 CI runner installs `protobuf-compiler` with `--no-install-recommends` and builds the new Lance dependency graph. `protoc` runs, but `libprotobuf-dev`, which supplies the standard `.proto` definitions needed by Lance, is omitted.

Retained evidence: run 40573097722 / job 122432233913, external `mvp010-ci2-job.log`. Lines 161 and 195–199 show the install command and the omitted recommended `libprotobuf-dev`; line 260 reports libprotoc 3.21.12. Lines 1146–1156 show the `lance-encoding v13.0.0` build failure, missing `google/protobuf/empty.proto`, undefined `google.protobuf.Empty`, and exit 101. The checkout log explicitly says merge `987d369635b1d40d172fbfbe97e81258c3b9e811` into `a09d9d691840278e2905aa39808dabd01126c663`; the CI checkout itself is the PR merge object `ee0883884970b7f30e904ddf68604da4b7942465`, not the feature HEAD object.

Impact: the candidate's declared CI environment cannot compile the new dependency before Clippy/tests run. The successful local exact-HEAD gate does not establish clean-runner prerequisite completeness. The base had neither this Lance dependency graph nor this incomplete installation step.

Minimal repair: explicitly install `libprotobuf-dev` alongside `protobuf-compiler`, and document both as build prerequisites. Obtain a matching CI result for the repaired candidate. The parent already identified this repair; it has not been applied to the frozen review candidate.

### R4 — Cosine-vector validation errors lose the failed build's durable call trace

ORIGIN: REGRESSION. SEVERITY: LOW.
Primary location: `src/retrieval/vector.rs:211` and `src/retrieval/vector.rs:222`.
Related seams: `src/retrieval/vector.rs:182`, `src/provider.rs:161`, `tests/support/daemon_retrieval.rs:950`.

Trigger: return a structurally valid embedding response with the configured dimension, but with every coordinate zero, or a finite JSON/f64 value that overflows when converted to f32. For example, a two-dimensional all-zero response passes the generic provider's shape/finite-f64 checks and reaches the cosine-vector validator.

`progress.last_call` is set only in memory. It is durably published immediately only when `outcome.error` was already set by the generic provider. The later f32/nonzero validation returns `PROVIDER_OUTPUT_INVALID` directly, before the final batch progress publication. Consequently `index status` after that failed build still has no `last_call` or has an older call, and cannot report the failing call's identity, trace and validation error. Even the in-memory `last_call.error` was populated from the earlier successful provider outcome rather than the cosine failure.

Impact: the public failure diagnostics are incomplete for a real malformed-vector class, contrary to `docs/retrieval.md:85` and the checked-off failed-model diagnostic acceptance. The durable prefix correctly stays unchanged; the defect is lost provenance, not a false indexed prefix. This validation and progress owner are entirely new in the candidate. The retained failure-trace regression tests transport failure only.

Minimal repair: classify all embedding conversion/cosine-validation failures through the same durable failure-progress path, preserve the pre-batch watermark, and set the final validation error on the saved call diagnostic. Cover zero vectors and finite-f64/f32-overflow responses, including readback after restart.

### R5 — One legal large Memory can make the mandatory rule scan fail every search

ORIGIN: REGRESSION. SEVERITY: MODERATE.
Primary location: `src/domain/history.rs:470`.
Related seams: `src/domain/history.rs:386`, `src/daemon/retrieval.rs:267`, `src/storage/mod.rs:190`, `src/domain/mod.rs:253`.

Trigger: store an otherwise valid active Fact in a scope, with its serialized `MemoryRecord` just below the permitted 128 KiB storage-record ceiling. The draft's allowed conditions/assumptions and rationale permit this size. Make it the first/only state membership in that scope. The `TimedMemory` state representation adds the revision/key/previous/recorded-at envelope, so it exceeds 128 KiB even though the stored record is legal. Then run any ordinary search in that scope, including a query that does not match this Fact.

Every search first calls the new `applicable_rules`, which invokes `state` with exactly `max_bytes: 128 * 1024`. `state` measures the larger `TimedMemory`, and returns `RESULT_TOO_LARGE` when its first entry cannot fit. The error propagates before the `is_rule` filter and before the search's whole-entry dropping/partial-receipt logic. A non-rule Fact that would never be selected can therefore fail all searches in its scope (and, when encountered in an ancestor scope, narrower searches).

Impact: valid data defeats the candidate's promised bounded partial-result behavior instead of producing the declared budget gap/truncation receipt (`docs/retrieval.md:113–117`). The base state API correctly reports that a caller's requested page cannot fit an entry; the regression is the new mandatory caller choosing a ceiling smaller than a legal entry's wrapper and propagating it for irrelevant non-rules.

Minimal repair: give the bounded internal rule scan enough byte allowance for at least one maximum legal wrapped Memory, retaining its scan limit and final output limits; or explicitly handle an oversized scanned entry as an honest incomplete rule scan. Keep the final whole-entry output/character/source budgeting. Add a public search regression with a maximum-size valid non-rule record and assert a bounded incomplete/no-hit result with an exact receipt, rather than an unrelated `RESULT_TOO_LARGE` job failure.

## Coverage and supporting evidence

Full changed Rust source and tests were read, along with full changed human-authored configuration/build/docs files. Immediate callers and contracts were traced through the Storage writer/serialized-read boundary, configuration checkpoint restoration, Domain authority/receipt readers, CLI/IPC/job surfaces, generation selection, native stream limits and provider request validation. Unchanged supporting reads are scoped below rather than claimed as a repository-wide audit.

The reviewed paths are listed with line counts and SHA-256 below. `Cargo.lock` was fully parsed structurally: 229 base packages, 585 candidate packages, 356 additions, no removals, and no changes to existing package identity/checksum metadata. Existing dependency-list changes and new direct pins were inspected. The relevant pinned backend is LanceDB 0.40.0 at `b1b080b97c3edd2fd9f2aae8a3b71ff19487b1f8`, Lance 13.0.0 and locked Arrow 58.4.1. This is not a fresh dependency security audit.

The 143 archive members were all read for hashing (328160 bytes), and all 107 JSON files were parsed during the evidence inspection. The 35 run summaries, terminal outcomes and failure classifications were checked against their logs. Every manifest member matches. `SHA256SUMS` is `0551b4e64d1d6f3d6285c1f5c2f6f2727f528422367ba883487117afc9e1147a`; with the manifest itself this covers 144 changed archive files. Original compile errors and unsuccessful files whose names contain “green” remain failures. The first keyword RED collector records scratch absence, not an explicit process-absence boolean; no stronger process claim is inferred. The final-focused count correction preserves the actual 27 daemon cases, and the separate empty-library control adds the thirteenth public retrieval case. These dirty-worktree histories are supporting development evidence, not exact-final-HEAD acceptance.

The pre-existing exact-candidate canonical gate receipt was reused, not rerun: `bash scripts/check.sh`, exit 0, 75.04238677583635 seconds, HEAD `987d369...`, 116 ordinary Rust tests, 8 Python tests and 28 tests using the actually installed release daemon. Its raw log SHA-256 is `b9c7cb9162caf67ccef07c318797c8adc3941511bcd0a934efd3864b04cf7a6f`. Supplied postflight/installed evidence binds release and installed SHA-256 `11c5220eab73b28d3f6d3b9b75f88078a0f55e209801db8eaea327b1f941c1a1`, private path and actual other-kernel-UID peer denial, and no daemon-owned TCP listener. The capture component command is the debug example already built by cargo test; installed daemon acceptance remains release-based. The inherited mise audio-condenser warning is unrelated to the successful compiler/test result. Earlier parent-HEAD gate1 and the external-launcher CARGO_HOME harness failure do not replace this exact gate or this review.

The CI failure and independent source review are separate from that local gate. No build, test, fixture, native query, large database, network/model call, fetch, service, formatter or product mutation was run by this reviewer. Suggested regressions above are repair guidance, not executed validation.

## Instruction-file evidence

Before source reads, both `AGENTS.md` and `CLAUDE.md` were checked by actual filesystem `lexists` at the logical checkout, each lexical/resolved ancestor, and every affected nested directory, including possible untracked or dangling instruction symlinks. The spec-only nested directories were checked before their reads. None exists. The final census repeats this and includes the external report's parent directories. Therefore there is no present instruction file to report as read-to-EOF and no applicable line count; absence is filesystem evidence, not a Git-tracked-files inference.

The census table gives logical directory -> resolved directory. At every row, both `<directory>/AGENTS.md` and `<directory>/CLAUDE.md` are ABSENT (line count: not applicable).

Pre-report postflight UTC: `2026-10-27T20:13:54.760528+00:00`.

| Logical directory | Resolved directory | Instruction files |
|---|---|---|
| `/` | `/` | both ABSENT |
| `/home` | `/home` | both ABSENT |
| `/home/obj` | `/home/obj` | both ABSENT |
| `/home/obj/project` | `/home/obj/project` | both ABSENT |
| `/home/obj/project/github` | `/home/obj/project/github` | both ABSENT |
| `/home/obj/project/github/RyderFreeman4Logos` | `/home/obj/project/github/RyderFreeman4Logos` | both ABSENT |
| `/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge` | `/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge` | both ABSENT |
| `/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git` | `/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git` | both ABSENT |
| `/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/akasha-evidence` | `/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/akasha-evidence` | both ABSENT |
| `/mnt` | `/mnt` | both ABSENT |
| `/mnt/ssd` | `/mnt/ssd` | both ABSENT |
| `/mnt/ssd/mirror-rootfs` | `/mnt/ssd/mirror-rootfs` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home` | `/mnt/ssd/mirror-rootfs/home` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj` | `/mnt/ssd/mirror-rootfs/home/obj` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project` | `/mnt/ssd/mirror-rootfs/home/obj/project` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github` | `/mnt/ssd/mirror-rootfs/home/obj/project/github` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/.github` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/.github` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/.github/workflows` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/.github/workflows` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/benchmarks` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/benchmarks` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/benchmarks/results` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/benchmarks/results` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/benchmarks/results/mvp010` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/benchmarks/results/mvp010` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/plan` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/plan` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/spec` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/spec` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/spec/issues` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/spec/issues` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/scripts` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/scripts` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/config` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/config` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/daemon` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/daemon` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/domain` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/domain` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/retrieval` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/retrieval` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/storage` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/storage` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/tests` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/tests` | both ABSENT |
| `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/tests/support` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/tests/support` | both ABSENT |
| `/ssd` | `/mnt/ssd` | both ABSENT |
| `/ssd/mirror-rootfs` | `/mnt/ssd/mirror-rootfs` | both ABSENT |
| `/ssd/mirror-rootfs/home` | `/mnt/ssd/mirror-rootfs/home` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj` | `/mnt/ssd/mirror-rootfs/home/obj` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project` | `/mnt/ssd/mirror-rootfs/home/obj/project` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github` | `/mnt/ssd/mirror-rootfs/home/obj/project/github` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/.github` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/.github` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/.github/workflows` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/.github/workflows` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/benchmarks` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/benchmarks` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/benchmarks/results` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/benchmarks/results` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/benchmarks/results/mvp010` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/benchmarks/results/mvp010` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/plan` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/plan` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/spec` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/spec` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/spec/issues` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/docs/spec/issues` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/scripts` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/scripts` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/config` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/config` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/daemon` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/daemon` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/domain` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/domain` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/retrieval` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/retrieval` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/storage` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/src/storage` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/tests` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/tests` | both ABSENT |
| `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/tests/support` | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp010/tests/support` | both ABSENT |

## Reviewed file inventory

Full changed-file reads except the explicitly structured generated-lock review:

| Path | Lines | SHA-256 |
|---|---:|---|
| `.github/workflows/ci.yml` | 23 | `4068e356a83f4506eabe4f886144cd982898b18e16b48ff92f2ed0adf524ca77` |
| `Cargo.lock` | 6682 | `ce102473f52cfd95017624a70cc80ceb35c961c294e83f23b1e4505f51c5a73b` |
| `Cargo.toml` | 46 | `f926afc79b09f486ea6b5f8f4c497b77d85d83eb1d511ad6c0502dbe7edca913` |
| `README.md` | 21 | `1c8d80d3e857d89555a7cee4ece0853513c2a8a34bf95f1e8cd8f3d52872e373` |
| `docs/daemon.md` | 160 | `e5b6948eb5aa0f8e306a9d843dcf1d842b26bdd767c24d13177498f02460407f` |
| `docs/development.md` | 66 | `1e160d1f8bf5c717bbf3682563107d8faf6da196cd4ef3dc99a348ce993cf43b` |
| `docs/plan/progress.md` | 168 | `0407c0b702b48bb4d55ca9f923a6a1cec28373d2b377f4e90855c57c5717a6b4` |
| `docs/plan/todo.md` | 336 | `6908a3fc67c8dd7897362bb2bcc8c0000e3a3f58c686acc2871158329bf2cad4` |
| `docs/retrieval.md` | 182 | `57c2e91ab19e22bd0deed659cc54f376f28e4e89c432e633bd0b4b2f6bb6f3df` |
| `scripts/check.sh` | 15 | `bc47ccd9ee4e53a88d4dc77a558c55d0a43ac878482df09c8d1ec64a48e835e2` |
| `src/config/schema.rs` | 548 | `2b060ec865c619c649c4f40551423137b6cd9167f8fe2ea123a63b175af047d5` |
| `src/daemon.rs` | 628 | `9cede96b8fd1ec94c92b23570d0ade2c9d1e96121a97198cd4dc5128439b0a37` |
| `src/daemon/jobs.rs` | 1245 | `b4705f273a80cca42ae18937a7f7825efd9537838d8670d86a05e27542ecbd48` |
| `src/daemon/models.rs` | 203 | `7b31dcf0fb312aee1eabbddf0155c341d9898ccc7a59326f74e32ee820c7f8f2` |
| `src/daemon/retrieval.rs` | 608 | `7da7c9f9ea24588998e824462c01908230276f4b2d7ebca789a53a56387573cf` |
| `src/domain/history.rs` | 628 | `46676ca1e921db6fadfb8d38aeb0e018d07e80fec1cbd394d5dc51026c898a3c` |
| `src/domain/mod.rs` | 747 | `537b4c9d12f750290579d0e28dc46760201963f81ea2cb3470653c33a66d759e` |
| `src/domain/recall.rs` | 565 | `f11e2785de8c49bb224e26ecfb15a30aa5797d3c72835c18c3be382a95fa016a` |
| `src/ipc.rs` | 284 | `8cdedc92f880e0a11d2383bf262e2929ba9361aa0f3fd7d595b97a2bcbd78154` |
| `src/lib.rs` | 9 | `8aa8e4fd9bdece21a2fb6e5417997cf505520cfdb6496eddbc42f1041f7e6c66` |
| `src/main.rs` | 346 | `491571124fafc361ee2a0edef94774067430393093f3355cdfc48e143b747411` |
| `src/provider.rs` | 505 | `b316a0f9a0138fe32fc5dca006ff17130c473ee2370d8b5cfa92865145af1062` |
| `src/retrieval.rs` | 478 | `f3f96a3b01ea220630b22af7fbe65c29be89b6d316acfb3a86be5a8abf7e3149` |
| `src/retrieval/tests.rs` | 271 | `27d9fb07d1015861476ba755285e028ff19215799de2fd8d38a4ca7e3a81e988` |
| `src/retrieval/vector.rs` | 423 | `7337d5d202e3fe03f1b4a358f854e751b5a94d1e5af9796a59768bafb2d68c6f` |
| `src/storage/crash_tests.rs` | 311 | `8449f2eeabeeb4ab49f2c446444f3e1635fd4ec6b105ab7c40d34be175e34344` |
| `tests/config.rs` | 547 | `28b20769d15ff8d2ce0c747deb16482bc6932e7c68d66aa109217df78bdfd8f6` |
| `tests/daemon.rs` | 1084 | `2c43e27e6ee3c4453feee972f21fe2c13f32c2fbf81b06d6515e516e4d8424ae` |
| `tests/support/daemon_models.rs` | 686 | `be415b76723deb342af7f9c53b5ff6a06edbcb584d0c6f56a764d1ea2a477b04` |
| `tests/support/daemon_retrieval.rs` | 1255 | `cac551651496a44174d6ed685a57df0c70e2a3fa4069de39370335104371137c` |
| `tests/support/domain_history.rs` | 1053 | `894ecfd265c5b1de74e91cdc23ad2dca7d92fd4500373acd1f5fbd8778c97043` |

Unchanged immediate supporting sources/contracts (full `configuration.rs`, MVP-010 and spec 05; relevant authority/record sections of the remaining files):

| Path | File lines | SHA-256 |
|---|---:|---|
| `src/storage/mod.rs` | 508 | `317fc9c8c822d14f16ac19326a656a541f47a9376520d56d38656af0eb50a4ca` |
| `src/daemon/configuration.rs` | 202 | `97f60164b4689367ad02bcb50ebd3a71ede97b6fdc426c1e04e713b7711dd5f7` |
| `src/domain/schema.rs` | 210 | `425743de21fca21ccb75b86aaa19519ec2f60693bd4e0091d77b1c1e9dc9ed25` |
| `docs/spec/issues/MVP-010.md` | 36 | `a80f000ef907c6262244939bdee0cb94e8acf8565ed9f87c7df1baf25d1275bc` |
| `docs/spec/05.md` | 203 | `905804a47b7504b2758f386d37e7af45d5cf362b5488a2031a8fcdcebe810fcc` |
| `docs/spec/03.md` | 258 | `71f9a15ab14d7395cf726d7a08ef050aa517047b4ef0cd5c1a54e7f9715b04ee` |

External supplied evidence read and independently hashed:

| File under `.git/akasha-evidence/` | Bytes | SHA-256 |
|---|---:|---|
| `mvp010-gate3.admission.json` | 246 | `fdb24f3268d81a7d21a2c4e069ee70cd911a1b21d9bc740feeb5acf6cbaed4cb` |
| `mvp010-gate3.launch.json` | 163 | `57017ba93030077d75d993d09fba9161f9c250c76d676ef44d5b7197f3a8185b` |
| `mvp010-gate3.terminal.json` | 236 | `c1481ddc9e8e8abaea9cba242beb5a44ea276bf31a0fd6b1326686e12d987fce` |
| `mvp010-gate3.log` | 18917 | `b9c7cb9162caf67ccef07c318797c8adc3941511bcd0a934efd3864b04cf7a6f` |
| `mvp010-gate3.collected.json` | 482 | `01f16888f6360f9407353c3ee7065e5cccf63726e1e1061215ea62e4c9925b0e` |
| `mvp010-gate3-postflight.json` | 260 | `72f644ade320cc6f0635865a9d2b1d8c0e7d314ab9f6dec9ac256f925d6f55ce` |
| `mvp010-ci2-job.log` | 79675 | `d217ba6381cced88f879c0a33235b83037b9c7e4daf18f382151bf63cd081d3b` |

## Applied review rules and limits

The applicable native-code-review and review-gate-discipline skills were read and applied. Native references covered scope/attribution, regression-only comparison, foundations/evidence, boundaries/status/verdicts, Rust/acceptance/isolation, lifecycle/delivery, streaming/output, provider identity, materialized-index authority revalidation, specialized routes, integrity/runtime/filesystem/configuration constraints, receipt integrity, workflow and frozen artifact finalization. Review-gate references covered ordered routes, the mandatory front half, operational addendum, schema/specialist half and the retrieval wire ledger. The already-failed Hermes transport was not retried. No nested review or separate watchdog was started; the parent retained its shared relay and CI watcher.

Within the reviewed scope, the implementation uses the actual disk ICU FTS/scope indexes and optional actual model-bound cosine IVF-flat/RRF/reranking path, separate contiguous lexical/vector ledgers, physical updates before watermark publication, bounded streams and source/output budgets, fixed sync targets, generation pinning and a single shared Session/cache owner. Source and tests cover stale revision/lifecycle filtering, publication checks, idempotent replay, original workflow clocks, captured UTF-8 ranges and exact receipt bytes. These mechanisms do not waive the five concrete gaps above.

The declared search corpus is active Memory records. Captured transcripts remain progressively readable sources and are not falsely counted as all indexed content. Real model quality, the complete 50 GB/100-client product envelope, full repeated rebuild/old-generation reclamation, and later host workflows remain separate #17/#24/#27 and dependent acceptance. No unrelated audit or speculative baseline findings are included. Passing small fixtures and earlier storage-spike measurements are not promoted into those later claims.

Risk: live authorization and archived output exposure require repair before promotion; clean-runner CI also currently fails.
Unchecked by execution in this review: the four source-derived trigger cases, builds/tests and real model/full-corpus behavior. Existing exact gate and archive receipts remain attributed to their original runs.
Owned resources at completion: none. No service, process group, scratch database, relay or child reviewer was created. Only this immutable external report was written.

VERDICT: FAIL
