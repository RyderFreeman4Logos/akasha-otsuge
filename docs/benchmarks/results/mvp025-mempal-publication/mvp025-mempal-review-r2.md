VERDICT: FAIL

Independent read-only native fallback review for issue #25. ACTIONABLE_COUNT=1: HIGH REGRESSION. The complete frozen range was reviewed, including the original converter. The original alias and opener regressions pass, but final publication still permits loss of a moved pinned source. This is native review evidence; no Hermes, external reviewer, delegate, paid model call, real host or live database was used.

Frozen custody: checkout /mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp025; logical and resolved checkout paths are identical. Branch feat/mvp-025-mempal-import. Range origin/main...HEAD contains six commits and 172 changed paths.

- HEAD: 7f8902487a0a745cbd63765a9d85ad1adcbbf189
- Tree: 3a3d7941c48be09dfd7c73a31eacd2c9c711fb60
- Base/origin/main and independently resolved merge-base: 31d4c61ecf6672ae52e3b84bbd00b630bceed3f8
- Original issue start: 2026-11-10T04:22:08.149789+00:00
- Leaf start: 2026-11-10T08:05:42.922986+00:00

Finding R2-1 — HIGH, REGRESSION: final publication can overwrite the moved pinned snapshot

Location: src/storage/mempal/writer.rs:85-91; identity caller :226, manifest caller :361; source verification src/storage/mempal.rs:150, followed by finish at :164.

The shared publish() validates the temporary file's inode through private_file(), then writes and syncs that descriptor. Its final operation is still “fs::rename(temporary, root.join(name))?”. That operation replaces the final pathname without protecting the object currently occupying it. The source-inode guard protects writable openers, but does not protect this destructive rename target.

A source moved into conversion.json or manifest.json after the earlier admission/presence checks can therefore be unlinked by publication. The manifest case occurs after snapshot.verify_unchanged(), so the command can report success after destroying the source. Adding another ordinary pathname check would retain a replacement window.

Fresh deterministic reproduction used the actual reviewed target/debug/akasha-otsuge and a 274,432-byte SQLite database constructed solely from tests/fixtures/mempal21-28.sql. A tiny disposable LD_PRELOAD interposer paused at the real libc rename boundary by first performing the equivalent concurrent source move, then called the original rename unchanged. It did not change the converter's return value, file contents, SQLite reads, admission logic or error handling. The interposer's own successful rename and marker establish that the source reached the final target immediately before the product's replacement operation.

| Final target selected for the controlled move | CLI exit | Source preservation after publication |
| --- | ---: | --- |
| conversion.json | 1, STORAGE_IO | Original source pathname absent; moved pathname replaced by 146-byte identity JSON; source bytes/inode lost |
| manifest.json | 0, success envelope | Original source pathname absent; moved pathname replaced by 311-byte manifest JSON; source bytes/inode lost |

The fixture intentionally performs the initial source move. The defect is the subsequent product overwrite of that moved source, not the fixture's removal of its original name. Both moved-path byte/inode assertions failed. The aggregate probe exited 1 with “REGRESSION: publication overwrote moved pinned source: ['conversion.json', 'manifest.json']”. The separate-source positive control and ordinary completed replay both passed before these probes.

Origin: the conversion publication helper was introduced in this range at 41933018a3835538b3fbba33adc40a60b364c2e4. The frozen base has no src/storage/mempal/writer.rs (git cat-file absence exit 128). The successor adds guards to openers but retains the replacing rename. This is an introduced sibling of R1, not a pre-existing native archive-export finding.

Repair direction: protect final publication against replacing an unexpected occupant, including the pinned source. Use a publication primitive/ownership protocol that cannot overwrite a newly occupied final name, and preserve legitimate incomplete-output recovery. Add deterministic regressions for both identity and manifest publication after reader/writer admission, asserting the moved source pathname, bytes and inode survive. The parent remains the sole source writer; no repair was made.

R1 closure matrix

The full immutable R1 report was read and its SHA-256 verified as cfeb7f43c10ac362bcec61b90d38036d9c52f5445a0d78d72ada4221a690478f. Its archived copy also matches.

| R1 invariant or sibling surface | Current source/evidence | R2 disposition |
| --- | --- | --- |
| Original SOURCE=DEST/conversion.json.tmp public CLI counterexample | Writer::new canonicalizes the destination parent and rejects source descendants before lock creation, writer.rs:143-156. Fresh direct-path probe preserves pathname, inode and bytes. | Closed for the original input |
| All eight source/output names, including source-as-lock and symlinked parent | tests/support/mempal.rs:85-126 exercises the public CLI through an aliased parent. Fresh integration execution passes; separate direct-path probe refuses all eight with UNSAFE_MEMPAL_SNAPSHOT before output mutation. | Closed |
| Source moved after reader admission into a writable opener | reader.rs:254 supplies device/inode; writer.rs:43-62 rejects that identity before mutation. Fresh unit test tests.rs:116-154 covers conversion.json.tmp, manifest.json.tmp, blobs.bin, commits.jsonl and the lock. | Closed for those five cases |
| Legitimate incomplete-stage ownership | Corrected test creates owned incomplete output first for data/manifest/lock variants; pristine identity-temp variant is unchanged. Typed refusal and moved-path bytes assertions remain. | Fixture correction valid |
| Final identity/manifest rename target | publish() validates the temporary descriptor, then replaces the final pathname. Fresh controlled public-CLI publication probe loses both moved sources. | OPEN: R2-1 |
| Complete/incomplete replay and crash recovery | Fresh integration and five-phase supervisor pass; extra malformed/foreign-output controls refuse without changing existing payload bytes. | Passing adjacent coverage; does not waive R2-1 |

The preserved source-alias RED genuinely destroyed conversion.json.tmp and accepted source-as-lock. The preserved source-inode RED genuinely adopted the moved conversion.json.tmp inode before the guard. In contrast, the first broader source-guard run passed 63 integration tests and its five-phase supervisor, but the new manifest-temp fixture lacked legitimate stage identity. The existing ownership refusal returned MEMPAL_CONVERSION_CONFLICT before the intended opener; its expectation failure is HARNESS, not a product RED. I inspected the retained original test, failure log, diagnosis and corrected test. The continuation then passed two unit tests, one intentionally ignored child, five SIGKILL phases, formatting and all-target Clippy. None of those historical failures was erased or used as this fresh verdict.

Instructions and path coverage

Read /home/obj/.codex/skills/native-code-review/SKILL.md and its applicable scope/attribution, foundations/evidence, boundaries/verdicts, Rust/isolation, specialized/provenance and streaming routes; workflow, exact checks and tracing; integrity/durable-filesystem constraints; receipt integrity, historical artifact provenance, disposable probes, filesystem durable-state/path-evidence/leaf-TOCTOU, fault-injection coverage and frozen-artifact finalization references. Also read the active Ponytail full skill at /home/obj/.codex/plugins/cache/ponytail/ponytail/5.3.3/skills/ponytail/SKILL.md. The user's explicit native fallback/no-delegation and shared-cache/no-full-gate-rerun requirements controlled execution. An attempted instruction lookup for references/checks-and-tracing.md was absent (read command exit 1); the actual workflow-exact-applicable-checks.md and workflow-trace-dont-assume.md routes were read instead. This was no product/test failure.

Before source inspection, live filesystem checks covered both AGENTS.md and CLAUDE.md at every checkout ancestor: /, /mnt, /mnt/ssd, /mnt/ssd/mirror-rootfs, /mnt/ssd/mirror-rootfs/home, /mnt/ssd/mirror-rootfs/home/obj, /mnt/ssd/mirror-rootfs/home/obj/project, /mnt/ssd/mirror-rootfs/home/obj/project/github, /mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos, and the checkout.

The same names were checked in every touched subtree: docs, docs/benchmarks, docs/benchmarks/results, its mvp025-import-runtime-final, mvp025-mempal-reader and mvp025-mempal-source-guard directories, docs/licenses, docs/plan, src, src/domain, src/storage, src/storage/mempal, tests, tests/fixtures and tests/support. All 50 named paths were absent. A later os.path.lexists census also excluded dangling instruction symlinks. There were no instruction targets or line counts to report; absence came from the live filesystem. Logical and resolved checkout ancestry coincide.

Complete changed Rust source/test files were read, including unchanged surrounding code:

| Path | Lines |
| --- | ---: |
| src/domain/mod.rs | 824 |
| src/domain/review.rs | 594 |
| src/main.rs | 965 |
| src/storage/archive.rs | 275 |
| src/storage/mempal.rs | 167 |
| src/storage/mempal/projection.rs | 381 |
| src/storage/mempal/reader.rs | 309 |
| src/storage/mempal/tests.rs | 154 |
| src/storage/mempal/writer.rs | 372 |
| src/storage/mod.rs | 522 |
| tests/archive.rs | 327 |
| tests/support/mempal.rs | 709 |

Also reviewed Cargo.toml, every Cargo.lock change, the full structural SQL fixture, all 29 schema definitions, CLI help/wiring, MIT attribution, migration documentation, changed README/storage/provenance/TODO material, both explicitly named validator scripts, archive inventories and sanitized receipts. An independent in-memory SQLite comparison confirmed all 344 ordered schema columns/types against the structural fixture. No extension, migration, mempal executable or live input was used.

The trace covered immutable read-only SQLite through the retained /proc/self/fd descriptor; sidecar refusal and source hash/identity recheck; exact schema axes, unknown objects and bounded values; lossless typed rows, IDs, relations, deletions and retained history; UTF-8/NUL chunking; source/candidate/review grouping across commit boundaries; scope/validity restrictions; unverified provenance and candidate authority; lock exclusion, private-file/link checks, deterministic observation clocks, prefix comparison, append, complete replay, malformed outputs and publication/fsync boundaries. Ordinary domain writes retain the None proposal-revision path. Historical conversion emits evidence, sources, candidates and reviews without job/grant/native-binding authority. The final-name publication mutation is the uncovered exception described above.

Canonical and historical evidence

Read mvp025-mempal-canonical-r2-verified.json; SHA-256 34cda034a186d75ea2e01f3c951215d4123eb0bf391cdad3b0c4d151bf1424be. Its exact clean HEAD/tree/base match this review. All four referenced hashes matched: admission 95ceba36e09fab604b1728a646585ce1d0ed7f7696594b0c04218603af06b9a2, terminal be148261497c78891fe62ce085b244169a0d3a5304e943f1dab595cbebc01737, collected 29c9b3df20b4969f554be044be409249a052e4de311fae88860af12eaac2ead0, and full log fd6fe4b15c5a51471ceb03f1c0aebc608212f21d912d6c51f83b308fd4728062.

The raw terminal/collected receipts agree on just check exit 0 in 471.3785756751895 seconds. Independently counted its 21 Rust result rows: 576 passes and 34 ignores including installed repeat; daemon 159 passes per run. Read justfile and scripts/check.sh: this supplied gate includes formatting, all-target Clippy, workspace tests, release and installed checks. These are historical exact-tree gate results, not fresh reviewer execution. The unchanged full gate was not repeated and does not close R2-1.

Verified all 100 reader-archive member sizes/hashes and all 16 historical source hashes against acceptance commit 41933018a3835538b3fbba33adc40a60b364c2e4, tree 8a7c5efc799a7251cca42422ae358c9d1c62006e. Exactly five current files differ: mempal.rs, reader.rs, writer.rs and the two regression-test files. All five match the source-guard candidate_file_sha256 values. Inspected their complete repair differences. Verified all 30 source-guard members, and the historical runtime-final archive's 17 members and 22 source hashes against its candidate pin.

The retained old-release to new-debug compatibility receipt reports byte-identical replay of all four conversion/archive files on a small controlled fixture, with an independent typed-row/span oracle and rejected corruption control. It is supplied controlled evidence, not a fresh full private-data acceptance. The original real acceptance remains frozen to 4193301: public summaries report all 3,525,645 typed rows, 596,178 source chunks, 503,615 candidates/open reviews, two empty imports, one repeat, two byte-identical exports and two isolated daemon starts. I did not open private SQLite snapshots, converted archives, private source/archive fingerprints, native-child transcripts or secrets.

Historical missing CLI/projection/repeat behavior and dangling-manifest failures remain genuine behavioral REDs. The SQLite-limit compile failure is build/harness evidence, and the real-runtime enum expectation mismatch is a retained harness failure corrected without product edits. These classifications remain separate from both canonical PASS and this review FAIL.

Fresh focused execution

Used the canonical admission PATH and pinned /home/obj/.rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin/cargo and rustc: cargo 1.99.1 (5f94df478 2026-08-27), rustc 1.99.1 (58cf8f9c3 2026-10-13). Preserved RUSTUP_TOOLCHAIN=1.99.1, the inherited sccache wrapper/socket, CARGO_TARGET_DIR=/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/target and TMPDIR=/mnt/ssd/mirror-rootfs/home/obj/tmp/a15-cache-s0s9a8n9. The lexical readlink target matched that exact /ssd path before and after. No additional Rust target or corpus copy was created.

| Fresh command | Actual result |
| --- | --- |
| cargo test --locked --offline --test archive --test cli --test domain --test storage | Exit 0: archive 21, CLI 5, domain 20, storage 17; 63 passes, zero failures/ignores |
| cargo test --locked --offline --lib storage::mempal::tests:: -- --nocapture | Exit 0: 2 passes, 1 intentionally ignored child, 82 filtered |
| python3 -B docs/benchmarks/results/mvp025-mempal-reader/mvp025-mempal-real-audit-r1.py --self-check | Exit 0: typed-row self-check PASS |
| python3 -B docs/benchmarks/results/mvp025-mempal-reader/mvp025-mempal-archive-oracle-r1.py | Exit 0: typed-row and typed-cell self-checks PASS |
| python3 -B SCRATCH/probe.py SCRATCH | Exit 0: archive/source hash verification, schema comparison, separate-source/replay control, eight direct source aliases and five malformed/conflicting output cases |
| cc -shared -fPIC -Wall -Wextra -Werror -o SCRATCH/rename_probe.so SCRATCH/rename_probe.c -ldl | Exit 0: tiny disposable publication interposer |
| python3 -B SCRATCH/rename_probe.py SCRATCH | Exit 1: both final-publication moved-source cases fail preservation assertions |
| git diff --check | Exit 0 |

Fresh total: 65 distinct Rust tests passed, one supervisor child intentionally ignored in ordinary enumeration, two oracle self-check invocations passed, and one adversarial probe failed across two source-loss subcases. All five supervisor children reached identity, partial, before-publish, manifest-renamed and published checkpoints; each was actually killed/reaped with signal 9, then recovered/imported 100 candidates without duplicates and preserved source bytes. These exits remain explicit and separate from the later failing publication probe.

SCRATCH was /mnt/ssd/mirror-rootfs/home/obj/tmp/a15-cache-s0s9a8n9/mvp025-r2-review-v9dKA0NL. Exact scripts, commands, source hashes, CLI observations and exits are retained in the compact focused log:
 /home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/akasha-evidence/mvp025-mempal-review-r2-tests.log
SHA-256: b6306a3effb40f233c5b24c5a9ebb60a9a14e99cd148d9ed01f7e7c52acb4719 (34,473 bytes).

The exercised debug binary SHA-256 was 97a0f83e7d1fa6de04b39e5286c8ff4b8a2ca7abda10b38c0d9232d3ba79603e. The resident release binary matches the supplied canonical hash a5a37f3a8cdc05dea1f7e4d751544a48593e61b8bf9912ce4eb389a2d65c88fe; that observation is not a fresh installed/live smoke.

Limits and final custody

No PRE-EXISTING finding is asserted and unrelated code was not hunted. The task's zero-actionable-findings requirement controls the verdict. Different-source incremental application remains the next #25 leaf; issue #25 remains OPEN. Resource stress #24, packaging #26, evaluation #27, final release #28 and actual Claude authentication #12 are separate.

Pre/post checks retained the exact HEAD/tree/base, clean porcelain status and lexical target symlink. All 172 changed filesystem blobs matched frozen HEAD bytes. Test and probe subprocesses exited; a /proc CWD/FD census found no remaining consumer of the exact owned scratch before removal. That scratch, including tiny interposer, fixture databases and archives, was removed. Only this new report and the requested focused log persist; no product/test/doc file, Git index/ref/config, provider/model/auth/reasoning setting or live mempal checkout was changed. Final report write is followed by another frozen-state/blob attestation and hash readback.

Skipped the unchanged canonical rerun and all private/live-host execution; no new full private-data, installed-source, model-quality, 50 GB/100-client RSS, power-loss/ENOSPC or Claude-auth acceptance is inferred.
Risk: final identity/manifest publication can still overwrite a concurrently moved pinned snapshot; R2-1 must be repaired before this leaf passes.

VERDICT: FAIL