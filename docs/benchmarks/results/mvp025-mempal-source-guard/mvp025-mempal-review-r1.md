VERDICT: FAIL

Independent native fallback review for issue #25; ACTIONABLE_COUNT=1 (HIGH REGRESSION). This is native content evidence, not a Hermes or external-tier verdict. No delegate or reviewer was launched.

Frozen custody

- Checkout: /mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp025 (logical and resolved paths identical).
- Branch: feat/mvp-025-mempal-import.
- Complete reviewed range: origin/main...HEAD, merge-base/origin/main 31d4c61ecf6672ae52e3b84bbd00b630bceed3f8.
- HEAD: 5e10a62ea75d7908e00fb8f8568f1bf351c2f35e.
- Tree: 4d9227e902e48eeeb7cad75d4edf3a045adec67c.
- Five commits, 141 changed paths. Product acceptance pin: 41933018a3835538b3fbba33adc40a60b364c2e4, tree 8a7c5efc799a7251cca42422ae358c9d1c62006e. Subsequent changes are documentation/evidence.
- Original issue start: 2026-11-10T04:22:08.149789+00:00; leaf start: 2026-11-10T08:05:42.922986+00:00.

Finding R1 — HIGH, REGRESSION: identity staging can destroy the source snapshot

Location: src/storage/mempal/writer.rs:81-87, with admission at :163-176 and :191-212; caller src/storage/mempal.rs:85.

Writer admission permits an existing conversion.json.tmp without a conversion identity. publish() then opens that file and executes “file.set_len(0)?” before writing and renaming it to conversion.json. Writer receives only the source hash, so it cannot exclude the retained SQLite source inode from its writable staging files.

Reproduction uses only the committed structural fixture:

1. Create an owned mode-0700 destination directory.
2. Create a valid schema21/fork28 SQLite fixture at DEST/conversion.json.tmp, mode 0600, and close SQLite.
3. Invoke target/debug/akasha-otsuge --config FIXTURE_CONFIG migrate mempal DEST/conversion.json.tmp DEST.

The 274,432-byte snapshot passes source admission. Conversion returns exit 1 / UNSAFE_MEMPAL_SNAPSHOT, but the original source pathname is gone: its inode was truncated, filled with conversion identity JSON and renamed. The final source-stability check detects damage after the destructive operation; it does not protect the source. No symlink, hardlink, race, extension, real database or unusual privilege is needed.

A fresh assertion-based probe ran a separate-source positive control first: exit 0 and exact source preservation. The alias case then returned exit 1, source_exists=false, source_unchanged=false, and failed the source-preservation assertion. An earlier exploratory fixture independently observed the same result. Both attempts remain in the focused log; both owned scratch directories were removed.

Origin evidence: this conversion writer and its staging admission are introduced in the reviewed range, specifically the recovery implementation at 41933018a3835538b3fbba33adc40a60b364c2e4. The frozen base has no src/storage/mempal/writer.rs or migration command. This is an introduced source-preservation defect, not a pre-existing archive-export issue. No base build was needed to establish that attribution.

Repair direction: reject source/output overlap before writable output admission, and prevent staging publication from opening/truncating the pinned source inode. Add the above public-CLI regression asserting that refusal preserves both the source pathname and bytes. Keep interruption recovery of genuinely owned staging files. The parent remains the sole source writer; no repair was made here.

Instructions and coverage

Read /home/obj/.codex/skills/native-code-review/SKILL.md and applicable scope/attribution, foundations/evidence, boundaries/verdicts, Rust/isolation, specialized/streaming routes, receipt integrity, disposable probes, workflow/checks/tracing, hard constraints, filesystem durable-state, and frozen-artifact finalization references. The explicit user instruction authorized direct native fallback after Hermes failure and prohibited recursive delegation and canonical-gate repetition.

Before source inspection, filesystem checks covered AGENTS.md and CLAUDE.md at every ancestor from / through the checkout:

/, /mnt, /mnt/ssd, /mnt/ssd/mirror-rootfs, /mnt/ssd/mirror-rootfs/home, /mnt/ssd/mirror-rootfs/home/obj, /mnt/ssd/mirror-rootfs/home/obj/project, /mnt/ssd/mirror-rootfs/home/obj/project/github, /mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos, and the checkout.

The same two names were checked in every touched subtree: docs, docs/benchmarks, docs/benchmarks/results, docs/benchmarks/results/mvp025-import-runtime-final, docs/benchmarks/results/mvp025-mempal-reader, docs/licenses, docs/plan, src, src/domain, src/storage, src/storage/mempal, tests, tests/fixtures, tests/support. All were absent. A subsequent lexists check also excluded dangling instruction symlinks. There were no instruction targets to resolve or line counts to report; absence was checked on the live filesystem, not inferred from Git.

Complete changed Rust files were read, including surrounding unchanged code:

| Path | Lines |
| --- | ---: |
| src/domain/mod.rs | 824 |
| src/domain/review.rs | 594 |
| src/main.rs | 965 |
| src/storage/archive.rs | 275 |
| src/storage/mempal.rs | 167 |
| src/storage/mempal/projection.rs | 381 |
| src/storage/mempal/reader.rs | 305 |
| src/storage/mempal/tests.rs | 113 |
| src/storage/mempal/writer.rs | 342 |
| src/storage/mod.rs | 522 |
| tests/archive.rs | 327 |
| tests/support/mempal.rs | 665 |

Also inspected Cargo.toml (48 lines), every Cargo.lock change, schema.json (1,436 lines), the SQL fixture (343 lines), CLI help/wiring, README, migration/storage/provenance documentation, MIT attribution, TODO changes, both named validator scripts and sanitized evidence. Parsed all 29 schema table definitions and independently compared every ordered column/type with the fixture through in-memory SQLite. Inspected evidence inventories, identities, commands, failure classifications, summaries and compressed focused logs without dumping repetitive archive content. Historical reports were hash-checked as archived evidence, not adopted as this verdict.

The source trace covered retained descriptor identity, sidecar refusal, read-only immutable SQLite flags, exact schema axes, unknown objects, limits and typed streaming; UTF-8/NUL chunking; atomic source/candidate/review grouping across commits; deterministic identities and clocks; review/provenance reuse; scope and validity restrictions; output locking, private-file/link checks, prefix reconciliation, complete-output comparison and publication fsync ordering. Ordinary domain writes retain the None proposal-revision path. Conversion emits inert SQL evidence, unverified sources/candidates and review state, without job, grant or native-binding records. The focused regressions exercise these paths; R1 exposes an uncovered admission-before-mutation boundary.

Evidence verification and canonical reuse

Read mvp025-mempal-canonical-r1-verified.json from the requested evidence directory. Its SHA-256 is bebb2e6597e43f297f4215dc38ab1652d7dfffe76d135fb7ab3726a0b8096fac. All four referenced artifact hashes matched: admission, terminal, collected receipt and complete log. The terminal and collected receipts agree on exit 0 and the exact HEAD. The complete canonical log SHA-256 is d6873de50f8d7866f714ef45fa0c8d6e9acb03573eca4931653d1965983a6f83.

This is supplied exact-HEAD just check evidence: 475.4750908212736 seconds, 574 Rust passes and 34 ignores including installed repeat, 159 daemon passes per run. Read justfile and scripts/check.sh to verify the gate includes formatting, all-target Clippy, workspace tests, release build and installed checks. The unchanged full gate was not repeated. Its success does not waive R1.

Verified all 16 source hashes in docs/benchmarks/results/mvp025-mempal-reader/manifest.json against the acceptance commit, frozen HEAD and current filesystem. All matched. Verified all 100 public member byte counts/hashes. The historical runtime-final archive's 17 member hashes and 22 source hashes matched its historical candidate/merge pins; these are separate historical evidence.

The public real-data summaries consistently report 3,525,645 typed rows, 596,178 native source chunks, 503,615 candidates/open reviews, two fresh imports, one repeat, two byte-identical exports and two isolated daemon starts. These are inspected supplied acceptance receipts, not fresh private-data execution here. Original private snapshots, converted archives, private hashes/paths, native-child transcripts and secrets were not opened.

Historical failures remain distinct: missing CLI/refusals, missing projection, repeat refusal and dangling-manifest behavior are genuine behavioral REDs. The unchecked SQLite-limit Result compile failure is separate harness/build evidence, not behavioral RED. The real-runtime enum-casing mismatch is a retained harness expectation failure corrected without product edits. None was erased or relabelled as a final clean gate.

Fresh focused execution

Working directory was the exact checkout. Used the canonical admission PATH, selecting /home/obj/.rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin/cargo and rustc: cargo 1.99.1 (5f94df478 2026-08-27), rustc 1.99.1 (58cf8f9c3 2026-10-13). Initial ordinary PATH probes resolved through mise shims and printed read-only cache warnings; actual test execution used the pinned binaries.

Preserved RUSTUP_TOOLCHAIN=1.99.1, the inherited sccache wrapper, CARGO_TARGET_DIR=/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/target and TMPDIR=/mnt/ssd/mirror-rootfs/home/obj/tmp/a15-cache-s0s9a8n9. Lexical readlink target matched that exact /ssd target before and after. No alternate build target or corpus copy was created.

Commands and results:

- cargo test --locked --offline --test archive --test cli --test domain --test storage
  — exit 0; archive 20, CLI 5, domain 20, storage 17 passed: 62 passes, zero failures/ignores/filtered cases.
- cargo test --locked --offline --lib storage::mempal::tests::killed_mempal_conversion_reconciles_output_without_duplicate_candidates -- --exact --nocapture
  — exit 0; 1 passed, 83 filtered. Five actual SIGKILL phases: identity, partial, before-publish, manifest-renamed, published. Each recovered/imported 100 candidates without duplicates and preserved its source.
- python3 -B docs/benchmarks/results/mvp025-mempal-reader/mvp025-mempal-real-audit-r1.py --self-check
  — exit 0; typed-row self-check passed.
- python3 -B docs/benchmarks/results/mvp025-mempal-reader/mvp025-mempal-archive-oracle-r1.py
  — exit 0; typed-row and typed-cell self-checks passed. No private audit invoked.
- python3 -B /mnt/ssd/mirror-rootfs/home/obj/tmp/a15-cache-s0s9a8n9/mvp025-review-probes-vp6ql4r4/probe.py /mnt/ssd/mirror-rootfs/home/obj/tmp/a15-cache-s0s9a8n9/mvp025-review-probes-vp6ql4r4
  — separate-source control passed; source-alias preservation assertion failed, exit 1. Exact disposable script and CLI argv are retained in the log.
- git diff --check — exit 0.

Fresh total: 63 Rust tests passed, two validator self-check invocations passed, one adversarial source-preservation assertion failed after its positive control passed. The probe used the freshly tested debug binary, SHA-256 02c6ddbaad4b3fcab4f6ab6303044d04b6301d58af8498712dc9b70523a92e70. The existing release binary separately matched the acceptance/canonical hash 4577f94a820aab625e127dd7a4b39a54b0cc5d040563b2d227924be7ae06a217.

Focused log: /home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/akasha-evidence/mvp025-mempal-review-r1-tests.log
SHA-256: f03a2a4825debbe1dd4236308219b5869b3efbb48fb254456ff116645308da40.

Limits and final custody

No PRE-EXISTING defect is asserted; unrelated code was not hunted. Different-source incremental application is the next #25 leaf. Issue #25 remains OPEN. Resource stress (#24), packaging (#26), evaluation (#27), final release (#28), and actual Claude authentication (#12) remain separate.

Post-test and pre-report checks retained the exact HEAD/tree/base, clean porcelain status and identical lexical target symlink. All 141 changed filesystem files matched frozen HEAD bytes. Test/build consumers exited; both exact owned probe scratch directories were removed after their subprocesses exited. No owned scratch remained. The only persistent review writes are this new report and the requested focused log. No product files, Git index/refs/config, provider/auth/model/reasoning configuration or live mempal checkout were changed.

Skipped the unchanged full gate rerun and all private/live-host execution as instructed; no installed-source provenance, model quality, 50 GB/100-client RSS, power-loss/ENOSPC or Claude-auth acceptance is inferred.
Risk: the source/output staging alias in R1 causes irreversible source loss and must be fixed before this leaf can pass review.

VERDICT: FAIL