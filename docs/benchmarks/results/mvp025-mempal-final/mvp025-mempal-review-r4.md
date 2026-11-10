VERDICT: PASS

Independent, read-only native fallback review for issue #25. ACTIONABLE_COUNT=0. No actionable introduced finding remains in the complete frozen range. R1, R2, R3 and the shared source/output ownership and publication mutation class are closed for this leaf's stated contract. No recursive reviewer, delegate, Hermes retry or paid model call was used. This is native review evidence, not an external-tier verdict or whole-issue completion.

The reviewed checkout is `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp025`, branch `feat/mvp-025-mempal-import`. Logical and resolved checkout paths coincide. The home-relative checkout spelling `/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp025` resolves to that same checkout.

- Complete range: `origin/main...HEAD`, eight commits and 237 changed paths.
- Frozen HEAD: `b98eec93cd2a2d6ca525eaa56d681864280bed05`.
- Frozen tree: `6e1aeea391de14b0a46d4e9fcab7abe41877c6de`.
- Frozen origin/main and independently resolved merge-base: `31d4c61ecf6672ae52e3b84bbd00b630bceed3f8`.
- Original issue start: `2026-11-10T04:22:08.149789+00:00`; leaf start: `2026-11-10T08:05:42.922986+00:00`.

The governing instructions were the supplied review brief, `/home/obj/.codex/skills/native-code-review/SKILL.md`, and the active Ponytail full skill at `/home/obj/.codex/plugins/cache/ponytail/ponytail/5.3.3/skills/ponytail/SKILL.md`. Applicable references read included scope/attribution and regression-only scope; foundations/evidence; boundaries/status/verdicts; Rust/acceptance/isolation; specialized and streaming routes; workflow and exact applicable checks; integrity constraints; durable filesystem/path evidence; missing-root creation TOCTOU; filesystem durable-state-machine review; fail-closed path evidence; fault-injection coverage; disposable-probe integrity; receipt-integrity attestation; historical receipt provenance; and frozen-artifact finalization. Explicit native fallback, no delegation, read-only custody and canonical-gate reuse requirements controlled execution.

Before source inspection, live filesystem checks examined both `AGENTS.md` and `CLAUDE.md` at the checkout and every ancestor: `/`, `/mnt`, `/mnt/ssd`, `/mnt/ssd/mirror-rootfs`, then its `home`, `home/obj`, `home/obj/project`, `home/obj/project/github`, and `home/obj/project/github/RyderFreeman4Logos`. The home-relative checkout spelling and its `/home/obj/project/github/RyderFreeman4Logos` ancestry were also checked.

Both names were checked in every touched subtree: `docs`, `docs/benchmarks`, `docs/benchmarks/results`, its `mvp025-import-runtime-final`, `mvp025-mempal-reader`, `mvp025-mempal-source-guard`, `mvp025-mempal-publication` and `mvp025-mempal-directory` directories, `docs/licenses`, `docs/plan`, `src`, `src/domain`, `src/storage`, `src/storage/mempal`, `tests`, `tests/fixtures` and `tests/support`. All were absent. A final `os.path.lexists` census confirmed all 66 distinct logical instruction paths absent, including dangling-symlink checks. There were no present instruction files, resolved targets or line counts to report; absence was established from the live filesystem, not Git.

Complete changed Rust source/test files were read, including their unchanged surrounding code:

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
| src/storage/mempal/writer.rs | 606 |
| src/storage/mod.rs | 522 |
| tests/archive.rs | 327 |
| tests/support/mempal.rs | 709 |

Also inspected Cargo.toml, every Cargo.lock change, all 29 schema definitions, the complete 343-line structural SQL fixture, CLI help/wiring/exports, MIT attribution, migration documentation, README/storage/provenance/TODO changes, both explicitly named validator scripts, design inventories, public probe scripts, archive manifests, sanitized receipts and relevant compressed failure/focused logs. An independent in-memory SQLite comparison matched all 344 ordered schema columns/types to the fixture. Archive inventories were checked structurally and by member hashes without reproducing repetitive receipt content. All intervening production/test repairs from historical acceptance through source guard, anonymous publication and retained-directory ownership were inspected.

The source review traced read-only SQLite admission through the retained descriptor, immutable URI, sidecar refusal, schema axes, ordinary-table/column checks, SQLite limits, quick_check, typed row streaming and final identity/hash verification. It traced exact integer/real-bit/text/blob values, rowids and retained relation/deletion/history fields; Unicode/NUL chunk boundaries; grouped source/candidate/review revisions across commit flushes; restrictive scope/validity mapping; unknown metadata; deterministic IDs and replay clocks. Domain::record remains shared construction, ordinary writes pass `None` to candidate_change, and migrated records remain unverified manual-review candidates. Conversion does not emit jobs, grants or runtime bindings, invoke mempal, load extensions, migrate the source or call providers.

The whole writer lifecycle was reviewed: missing-root creation through the retained parent, directory and compatible file-lock admission, allowlisted enumeration, identity and manifest reads, repeated member opens, paired prefix/append handles, buffered write failure and Drop, grouped flushes, complete-output comparison, anonymous publication, file/directory/parent fsync, replay and completion validation. The supported immutable-snapshot assumption remains explicit; a live mutable database is not accepted evidence of consistency.

| Prior finding or shared invariant | Current source and independent evidence | Disposition |
| --- | --- | --- |
| R1: source-as-staging/source-as-lock aliases | writer.rs:67 rejects source descendants before output creation/lock admission; :163 rejects the retained source inode before any output-byte mutation. Fresh archive integration covers eight names through a parent alias; moved-inode unit covers both legacy temporaries, both data members and the lock. | Closed |
| R2: final replacing publication destroys a moved source | writer.rs:244 writes/fsyncs a new O_TMPFILE inode and links it without replacement. There is no named-temp truncate, replacing rename or cleanup unlink. Fresh publisher unit and two actual release-CLI final-occupant probes refuse and preserve moved source bytes/inode. | Closed |
| R3: directory swaps split output or bypass a replacement-directory lock | Output retains parent/root and every used member descriptor. mkdirat/openat/linkat use that owner; enumeration/stats use its procfs directory. validate at :201 binds requested parent/root and members; sync at :237 revalidates after directory/parent fsync. Four fresh release-CLI metadata-fsync swaps refuse and leave empty/independently locked replacements untouched. | Closed |
| Parent replacement with original child moved back; member substitution | Fresh seven-case writer unit passes, including completed manifest replacement. Fresh CLI parent and four member replacements also refuse; actual second CLI invocations remain busy while the first owner is paused. Repeated opens compare retained inode identity at :173. | Closed |
| Error/Drop lock lifetime and replay completion | Writer.output is deliberately last at :27, keeping directory and cloned compatible file-lock descriptors alive through buffered-sink destruction. Constructor locals also unwind sinks before Output. Sink::resume compares prefixes and appends only the missing suffix; complete mode cannot append. Both finish branches at :460 require Output sync/publication validation. | Closed by source trace plus focused recovery/exclusion coverage |

The actor performs each controlled rename. Closure means continued ownership, source/foreign-file preservation and refusal of false success; it does not mean preventing an external process from renaming a directory. No PRE-EXISTING defect is asserted, and unrelated code was not hunted. The task's zero-actionable-findings PASS rule was applied to the whole range.

The three original immutable FAIL reports were read and independently hash-verified:

- R1: `cfeb7f43c10ac362bcec61b90d38036d9c52f5445a0d78d72ada4221a690478f`.
- R2: `d6ec7c7bbaa5843aeca2fd42bdc400330bc0c8aa686c2489803d3bfa954d41ce`.
- R3: `0b27e41b27377143e05ffe1a9e1dbb6ced3ca5745520e0b536c9fa3bb8b15558`.

Their counterexamples and causal RED logs remain valid historical failures. The seven-case directory regression genuinely failed before repair. Directory-focused-r1 predates the final field-drop-order adjustment; directory-focused-r2 covers the final writer. The missing CLI/projection/repeat and dangling-manifest failures remain behavioral REDs. The unchecked SQLite-limit Result compile failure, initial unowned manifest-temp fixture refusal and real-runtime enum expectation mismatch remain separate build/harness failures; none was promoted to a passing product observation or erased.

The supplied exact-HEAD canonical receipt `mvp025-mempal-canonical-r4-verified.json` has SHA-256 `34aa6945b8b3cbfb64a8e904098b9d4ca1e83a54a486fb38c661df509a7c4a62`. Its HEAD/tree/base and clean-before/after assertion match this checkout. All referenced hashes matched: admission `45b37985e87df04a6eb80b614b838dff339f2afbda1044b00b23ba4f60eff978`, terminal `62b4443fdadce4e8a1d6cc834777157941eca0b7e8178b3977d746e8821848c8`, collected `60e3d2dd20ae656f118ab27e49f3a5684e0295439bf9e001f309ada42e7134dd`, and complete log `e858a1b36cf1f4545169efee9c028798e242e71e000304989e46c82987d0d5b4`.

This is supplied canonical `just check` evidence, exit 0 in 444.3367311293259 seconds, not a fresh reviewer execution. Independent log counting confirmed 21 Rust result rows, 578 passes and 34 ignores including installed repeat. The receipt reports 159 daemon passes per run and installed lifecycle/UID checks. Inspection of justfile/scripts/check.sh confirmed the aggregate gate's formatting, Clippy, workspace tests, release build and installed checks. The unchanged complete gate was not repeated and did not substitute for source review.

Verified every public archive member's size/hash: runtime-final 17, reader 100, source-guard 30, publication 29 and directory 34. Verified runtime-final's 22 source pins against `fa801c91d241582ee8e8d93dddf10f12397076ca`; reader's 16 against historical acceptance `41933018a3835538b3fbba33adc40a60b364c2e4`; source-guard's five against `7f8902487a0a745cbd63765a9d85ad1adcbbf189`; publication's writer against `d3fe10577759570a7746932fe32080886a59eede`; and directory's writer against frozen HEAD/current bytes. The current writer SHA-256 is `9d63182b0c9fd2b0e2d4bbb560d1db100c5f276cb548dc1a49b835bcd1fbdecd`.

Exactly five reader-pinned files now differ: mempal.rs, reader.rs, writer.rs and the two regression-test files. The source-guard writer and publication writer are intentionally superseded; the other four source-guard pins remain unchanged. No claim is made that all 16 historical source hashes equal this candidate. The retained old-release/new-debug compatibility receipt and independent oracle establish unchanged controlled-fixture archive bytes and rejected corruption. Historical directory/publication scripts were inspected, not rerun against their replaced baseline paths.

Real acceptance remains bound to `4193301`: supplied sanitized summaries cover 3,525,645 typed rows, 596,178 native source chunks, 503,615 candidate spans/open reviews, two empty imports, one repeat, two byte-identical exports and two isolated daemon starts. These are historical acceptance receipts. No private SQLite snapshot, converted private archive, private fingerprint/path, native-child transcript or secret was inspected, and no new full private-data acceptance is claimed.

Fresh execution used the canonical admission environment, selecting `/home/obj/.rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin/cargo` and rustc: cargo 1.99.1 (5f94df478 2026-08-27), rustc 1.99.1 (58cf8f9c3 2026-10-13). Preserved RUSTUP_TOOLCHAIN=1.99.1, the inherited sccache wrapper/socket, CARGO_TARGET_DIR=`/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/target` and TMPDIR=`/mnt/ssd/mirror-rootfs/home/obj/tmp/a15-cache-s0s9a8n9`. The lexical target symlink equals that exact `/ssd/.../target`; its resolved path is `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/target`. No alternate build target or corpus copy was created. Initial ordinary-PATH version discovery emitted read-only mise-cache warnings; accepted execution used the pinned binaries and succeeded.

| Fresh command | Actual result |
| --- | --- |
| `cargo test --locked --offline --test archive --test cli --test domain --test storage` | Exit 0: archive 21, CLI 5, domain 20, storage 17; 63 passed, zero failed/ignored/filtered |
| `cargo test --locked --offline --lib storage::mempal:: -- --nocapture` | Exit 0: 4 passed, 1 intentionally ignored supervisor child, 82 filtered |
| `cargo clippy --locked --offline --all-targets -- -D warnings` | Exit 0, clean |
| `python3 -B docs/benchmarks/results/mvp025-mempal-reader/mvp025-mempal-real-audit-r1.py --self-check` | Exit 0, typed-row self-check |
| `python3 -B docs/benchmarks/results/mvp025-mempal-reader/mvp025-mempal-archive-oracle-r1.py` | Exit 0, typed-row/cell self-checks |
| `python3 -B SCRATCH/probe.py` | Exit 0: 11 forced interleavings, 61 actual public CLI exits; recovery, repeat, import and byte-identical export for every case |
| `python3 -B SCRATCH/schema_probe.py` | Exit 0: eight actual CLI cases covering missing required/optional tables, unknown table/view/column/type, extension version and invalid UTF-8; all preserve source bytes |
| `python3 -B SCRATCH/evidence.py` | Exit 0: frozen blobs, instruction census, archive/historical pins, schema equality and canonical counts |
| `git diff --check` | Exit 0 |

Fresh total: 67 Rust tests passed; one supervisor child intentionally ignored in ordinary enumeration. Five actual supervisor children reached `mempal-identity`, `mempal-partial`, `mempal-before-publish`, `mempal-manifest-renamed` and `mempal-published`; every child was killed and reaped with signal 9. Each phase recovered/imported 100 candidates without duplicates and preserved its source. The five individual exits are explicit in the log; kill_at's actual wait/status assertion was inspected at src/storage/crash_tests.rs:163. Two validator self-check invocations and both adversarial CLI scripts passed. There were no fresh product assertion or setup failures.

The fresh public probe used a new bounded pause interposer compiled with `cc -shared -fPIC -Wall -Wextra -Werror ... -ldl`, exit 0, and the exact canonical release SHA-256 `1cc8b18a8be688cef9d2c5a9f0c43a08ad131eda2533ebab348c9d6f7e6fcb73`. It paused real metadata fsync calls; the independent Python actor performed the namespace changes, then released the original fsync unchanged. All 11 conversions refused; six actual busy controls proved exclusion. Their 44 recovery/repeat/import/export invocations succeeded. The eight schema cases add seven expected refusals and one supported optional-table success: 69 public CLI exits in total. The current debug binary separately matched the retained directory candidate hash `c993b2044b491ae4d1dbefcef8cdd1365ea62186ee2900f2418cda2cd00d821c`.

The focused log is `/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/akasha-evidence/mvp025-mempal-review-r4-tests.log`, 39,882 bytes, SHA-256 `1355ec6d944a2ef10ac752a5e12de2ad2edc006af2af33cd642ee214901685db`. It retains exact commands, counts, every supervisor phase, CLI exits, disposable probe source/hashes and cleanup. SCRATCH was `/mnt/ssd/mirror-rootfs/home/obj/tmp/a15-cache-s0s9a8n9/mvp025-r4-native-u1ei5q6s`.

Pre/post-test and final artifact custody retain the exact HEAD/tree/base, clean porcelain status, identical lexical target link and all 237 changed filesystem blobs equal to frozen HEAD. All test/probe consumers exited. A scoped /proc CWD/FD census found no remaining scratch consumer before exact owned scratch removal; fixtures, native stores, outputs, interposer and scripts were removed. Only this new report and requested log persist. No product/test/doc file, old report, Git index/ref/config, provider/model/auth/reasoning setting or live mempal checkout was changed. Final report readback, logical/resolved artifact identity and post-write frozen-blob attestation precede external hashing; no self-embedded report hash is used.

Issue #25 remains OPEN. Different-source incremental application is the next leaf. #24 resource stress, #26 packaging, #27 evaluation, #28 final release and actual Claude authentication #12 remain separate; this report authorizes no publication or whole-umbrella closure.

Skipped the unchanged complete gate and all private/live-host execution as instructed; installed-source commit, model quality, 50 GB/100-client RSS, power-loss/ENOSPC and Claude-auth acceptance remain unestablished.
Risk/limit: conversion requires an independently prepared immutable snapshot and Linux O_TMPFILE/procfs/non-replacing-link support; this PASS covers the stated conversion/recovery leaf only.

VERDICT: PASS