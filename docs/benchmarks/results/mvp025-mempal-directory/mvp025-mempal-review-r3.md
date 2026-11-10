VERDICT: FAIL

Independent read-only native fallback review for issue #25. ACTIONABLE_COUNT=1: HIGH REGRESSION. R1 and R2's specific source-loss counterexamples are closed, but destination directory identity is not retained across writer admission, data opens and publication. A fresh public-CLI probe bypassed another process's output lock and published split output. The zero-actionable-findings requirement therefore fails. No delegate, Hermes retry, paid model, real host or live database was used.

Frozen scope

- Checkout: /mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp025; logical and resolved paths coincide.
- Branch: feat/mvp-025-mempal-import.
- Complete range: origin/main...HEAD; seven commits, 202 changed paths.
- HEAD: d3fe10577759570a7746932fe32080886a59eede.
- Tree: f211c7a886832dd3edbe8d5f9d33144e4cfa0dbc.
- origin/main and independently resolved merge-base: 31d4c61ecf6672ae52e3b84bbd00b630bceed3f8.
- Original issue start: 2026-11-10T04:22:08.149789+00:00. Leaf start: 2026-11-10T08:05:42.922986+00:00.

Finding R3-1 — HIGH, REGRESSION: directory replacement redirects writes outside the held output lock

Locations: src/storage/mempal/writer.rs:197, :252, :265, :270, :385; shared publisher :85-89 and :120-121.

Writer::new checks the destination directory and locks destination/.mempal-convert.lock. It retains a PathBuf and the lock file, but no directory descriptor tying subsequent operations to that admitted directory. Each publish independently executes “.open(root)?”, pins that directory only for that call, and then drops it. The data sinks subsequently reopen “destination.join(\"commits.jsonl\")” and “destination.join(\"blobs.bin\")” through the pathname. The final manifest publisher resolves the pathname again. No comparison establishes that these directories belong to the held lock or the original admission.

A rename/replacement at the identity metadata fsync boundary therefore leaves conversion.json and the held lock in the old directory while creating data files and manifest.json in the replacement. The command returns success despite lacking a conversion identity in its reported destination. A replacement at manifest metadata fsync instead returns success with an empty requested destination and the complete output in the parked directory. Pinning the directory inside one publication protects that individual link, but does not establish writer-wide directory identity.

Fresh reproduction used the actual current release CLI, SHA-256 35a2e2eab97b753dd4d200e7d27e45dffd996f2627c7a865e96917f706ec4bc2, matching canonical R3. All SQLite inputs came from the committed structural fixture. The tiny owned LD_PRELOAD interposer recognizes identity or manifest JSON at the actual fsync boundary, performs only the controlled directory rename/replacement, emits an observation marker and calls the original fsync. It does not alter product bytes, return values, SQLite reads or lock calls.

| Controlled interleaving | Actual CLI exit | Observed output |
| --- | ---: | --- |
| Replace destination during identity metadata fsync | 0 | Requested directory has blobs.bin, commits.jsonl, manifest.json; parked directory has conversion.json and the held lock |
| Replace destination during manifest metadata fsync | 0 | Requested directory is empty; parked directory contains the completed archive |
| Replace destination with a directory whose lock the independent Python process already holds | 0 | Converter writes blobs.bin, commits.jsonl and manifest.json behind that other process's lock; its own lock and conversion.json remain parked |

For the third case, Python held a real exclusive flock throughout the candidate invocation. The observed replacement lock inode matched Python's held descriptor. A normal second CLI invocation, while Python still held that same lock, returned exit 1 / MEMPAL_CONVERSION_BUSY. This is a non-vacuous exclusion control. After Python released the lock, replay of the split output returned exit 1 / MEMPAL_CONVERSION_CONFLICT. The source remained unchanged throughout.

The original directory probe and the independent locked-replacement confirmation each ended with an assertion failure, exit 1. Both failures and their scripts remain in the focused log. They establish one finding, not two. The external actor performs the rename; the defect is the converter's subsequent unadmitted writes and success under the wrong directory/lock identity. No claim is made that the converter must prevent an external actor from renaming a directory.

Origin: this converter and its pathname-based recovery/locking protocol were introduced within the reviewed range, including 41933018a3835538b3fbba33adc40a60b364c2e4. Frozen base lookup of src/storage/mempal/writer.rs fails with Git exit 128 because that file does not exist there. This is a range regression, even though part of the defective protocol predates the final R3 commit. No PRE-EXISTING finding is asserted; unchanged neutral-export code is not blamed.

Repair direction: bind admission, enumeration, lock, metadata, data handles and publication to one retained validated directory identity; fail closed if the requested destination ceases to identify it. Use descriptor-relative operations rather than reopening the destination pathname between phases. Keep the non-replacing anonymous-file protocol and source-inode checks. Add deterministic public-path coverage for directory replacement during identity and manifest publication, including an independently held replacement-directory lock. Parent remains the sole source writer; no repair was made.

R1/R2 closure and full-class audit

Read both original reports and verified their exact SHA-256 values: R1 cfeb7f43c10ac362bcec61b90d38036d9c52f5445a0d78d72ada4221a690478f; R2 d6ec7c7bbaa5843aeca2fd42bdc400330bc0c8aa686c2489803d3bfa954d41ce. Their archived copies match.

| Invariant | R3 evidence and disposition |
| --- | --- |
| R1 SOURCE=DEST/conversion.json.tmp and source-as-lock | Source/output ancestry refusal occurs before lock creation. Fresh archive integration covers eight names through a symlinked parent and passes. Original counterexamples closed. |
| Source moved after reader admission into a writable member | Shared private_file checks source device/inode before mutation; all recognized existing members now pass it. Fresh moved-inode unit covers both former temporary names, both data files and lock. Closed for these paths. |
| R2 moved source occupying conversion.json or manifest.json | O_TMPFILE creates a new inode; linkat uses a live descriptor and does not replace an occupant. Fresh shared-publisher regression and actual release-CLI fsync probes preserve moved pathname, bytes and inode for both names; both CLI cases refuse. Closed. |
| Former named temporary source moves | No named temporary is truncated, renamed or unlinked. Fresh public CLI preserves both moved sources. Identity-temporary case exits 1; manifest-temporary case exits 0 while preserving the externally moved source, consistent with the stated contract. |
| Anonymous inode lifetime and failed publication | File RAII retains the descriptor through link and directory fsync; failure has no replacing fallback. Two added public-CLI SIGKILL probes at identity/manifest anonymous-file fsync return signal 9, leave no named staging file, and recover/replay successfully with private single-link files and unchanged source. |
| Prefix/data appends, replay and crash recovery | Validated paired sink descriptors compare identity; append occurs only after byte-prefix match. Fresh integration and five-phase supervisor pass. Complete replay preserves bytes/clocks; foreign, corrupt, linked and busy-output controls remain covered. |
| Directory/lock identity across those operations | OPEN: R3-1. Per-publication directory pinning does not preserve the writer's admitted directory and lock identity. |

The shared destructive source-byte mutation paths from R1/R2 are closed by the new publication primitive, not by another check-before-rename window. The broader ownership/publication class is not closed because of R3-1. Passing source-preservation checks do not waive output exclusion and recovery failures.

Inspected the retained causal alias RED, moved-inode RED, two-final-name publisher RED, R2 FAIL and historical focused logs. The first manifest-temp setup failure remains HARNESS: ownership admission refused an unowned stage before the intended opener. Its corrected fixture establishes legitimate incomplete output without weakening the source-preservation assertions. Missing CLI/projection/repeat and dangling-manifest behavior remain genuine behavioral REDs. The SQLite-limit compile failure and real-runtime enum-casing mismatch remain separate build/harness failures.

Instructions and source coverage

Read /home/obj/.codex/skills/native-code-review/SKILL.md and applicable references: review-scope-and-attribution; regression-only-review-scope; native-review-foundations-and-evidence; native-review-boundaries-status-and-verdicts; native-review-acceptance-rust-and-isolation; native-review-specialized-reference-routes; reference-index-provenance-and-specialized; workflow; workflow-exact-applicable-checks; workflow-trace-dont-assume; disposable-probe-invocation-integrity; receipt-integrity-attestation; hard-constraints-integrity-and-evidence; hard-constraints-durable-filesystem-and-path-evidence; filesystem-durable-state-machine-review; fail-closed-path-evidence-review; fault-injection-test-coverage-review; historical-receipt-artifact-provenance; read-only-rust-gate-isolation-and-receipt-recovery; frozen-review-artifact-finalization. Also read the active Ponytail full SKILL.md at /home/obj/.codex/plugins/cache/ponytail/ponytail/5.3.3/skills/ponytail/SKILL.md. Explicit native fallback, no delegation and canonical reuse instructions controlled execution.

Before source inspection, live filesystem checks examined AGENTS.md and CLAUDE.md at every ancestor: /, /mnt, /mnt/ssd, /mnt/ssd/mirror-rootfs, /mnt/ssd/mirror-rootfs/home, /mnt/ssd/mirror-rootfs/home/obj, /mnt/ssd/mirror-rootfs/home/obj/project, /mnt/ssd/mirror-rootfs/home/obj/project/github, /mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos, and the checkout.

The same names were checked under every touched subtree: docs, docs/benchmarks, docs/benchmarks/results, its mvp025-import-runtime-final, mvp025-mempal-reader, mvp025-mempal-source-guard and mvp025-mempal-publication directories, docs/licenses, docs/plan, src, src/domain, src/storage, src/storage/mempal, tests, tests/fixtures and tests/support. All 52 paths were absent. A later lexists census also excluded dangling instruction symlinks. There were no instruction targets or line counts to report; absence was checked on the filesystem, not inferred from Git. Logical/resolved checkout ancestry coincide.

Complete changed Rust source/test files were read, including surrounding code:

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
| src/storage/mempal/writer.rs | 443 |
| src/storage/mod.rs | 522 |
| tests/archive.rs | 327 |
| tests/support/mempal.rs | 709 |

Also inspected Cargo.toml, all Cargo.lock changes, the full SQL fixture, schema definitions, CLI help/wiring/exports, MIT attribution, migration documentation, changed README/storage/provenance/TODO material, both explicitly named validators, publication interposer/harness, archive manifests and sanitized receipts. Independent in-memory SQLite comparison matched all 344 ordered columns/types in 29 schema tables. Inspected every intervening production/test change from historical acceptance through source guard to R3.

The source trace covered retained read-only SQLite descriptor identity and immutable assumptions, sidecars and final source recheck, schema axes/limits/refusals, typed SQL values/rowids/relations/deletions/history, Unicode/NUL chunking, source/candidate/review grouping across batched commits, unverified authority, restrictive scope/validity mapping, deterministic observation clocks/IDs, file permissions/links/locks, prefix reconciliation, publication and fsync boundaries. Ordinary Domain writes retain the None proposal-revision path. Historical conversion emits evidence, sources, candidates and reviews without creating jobs, grants or runtime bindings. No migration, extension loader or live mempal call is introduced.

Canonical and historical evidence

Canonical R3 verified receipt SHA-256: a5617728295796469f6cbacdc7dcdbb17285368f87644b03a42423c51f1d9aa2. Exact HEAD/tree/base and clean status match. All referenced hashes verified:

- Admission: 18994123d122bbdc5757a28a5492d8adc7eb34a57c0e7b252e80f3921005080d.
- Terminal: 83549334298ffdc268c56ff17d83f89a50325e5ba8434a93db2cfa9141551991.
- Collected: ea100472dd9ca7a7deceb5679d1b571bdd53a35ac03f15cc3d8426fb7d3bea4d.
- Complete log: 120eb3b09313be1d04fd5c3a39ec00e04128bf5d6dc75c961b1beec9ccc03afd.

This supplied exact-tree just check reports exit 0 in 439.890872 seconds. Independently counted 21 Rust result rows: 577 passes, 34 ignores including installed repeat; daemon 159 passes per run. Read justfile and scripts/check.sh to confirm formatting, Clippy, workspace tests, release and installed checks. The unchanged canonical gate was not repeated; it does not close R3-1.

Verified member sizes/hashes in all four archives: runtime-final 17, reader 100, source-guard 30, publication 29. Verified runtime-final's 22 source pins against fa801c91d241582ee8e8d93dddf10f12397076ca; reader's 16 source pins against historical acceptance 41933018a3835538b3fbba33adc40a60b364c2e4/tree 8a7c5efc799a7251cca42422ae358c9d1c62006e; source-guard's five candidate pins against 7f8902487a0a745cbd63765a9d85ad1adcbbf189; publication's writer pin against current HEAD. Exactly five reader-pinned files differ now: mempal.rs, reader.rs, writer.rs and the two regression-test files. Source-guard's writer is intentionally superseded; its other four pins still match. The current writer hash is 5bf724ff34e4752af2bdf0f2f5fb34106d8c28e43c31e9c157c523f3694cbc97.

The retained old-release/new-debug replay and independent oracle establish byte-identical controlled-fixture compatibility, not new full private-data acceptance. The historical publication baseline hash is a5a37f3a8cdc05dea1f7e4d751544a48593e61b8bf9912ce4eb389a2d65c88fe; the shared release is now the R3 binary. The unchanged historical publication script was inspected, not rerun.

Sanitized real-acceptance summaries remain bound to 4193301: 3,525,645 typed rows, 596,178 source chunks, 503,615 candidates/open reviews, two empty imports, a repeat, two byte-identical exports and two isolated daemon starts. These are supplied historical receipts. No private snapshot/archive, private fingerprints/paths, native-child transcripts or secrets were inspected.

Fresh focused execution

Used canonical admission PATH and pinned /home/obj/.rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin/cargo and rustc: cargo 1.99.1 (5f94df478 2026-08-27), rustc 1.99.1 (58cf8f9c3 2026-10-13). Retained RUSTUP_TOOLCHAIN=1.99.1, the canonical RUSTC_WRAPPER/sccache socket, CARGO_TARGET_DIR=/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/target and TMPDIR=/mnt/ssd/mirror-rootfs/home/obj/tmp/a15-cache-s0s9a8n9. Lexical readlink target matched that exact /ssd path before and after. No new Rust target or corpus copy was created.

| Fresh command | Result |
| --- | --- |
| cargo test --locked --offline --test archive --test cli --test domain --test storage | Exit 0: archive 21, CLI 5, domain 20, storage 17; 63 passes |
| cargo test --locked --offline --lib storage::mempal:: -- --nocapture | Exit 0: 3 passes, 1 intentionally ignored supervisor child, 82 filtered; includes publisher regression and all five actual SIGKILL phases |
| cargo clippy --locked --offline --all-targets -- -D warnings | Exit 0, clean |
| python3 -B docs/benchmarks/results/mvp025-mempal-reader/mvp025-mempal-real-audit-r1.py --self-check | Exit 0, typed-row self-check |
| python3 -B docs/benchmarks/results/mvp025-mempal-reader/mvp025-mempal-archive-oracle-r1.py | Exit 0, typed-row/cell self-checks |
| python3 -B SCRATCH/evidence.py | Exit 0: frozen blobs, live instruction census, archive/source hashes, schema comparison, canonical counts |
| python3 -B SCRATCH/probe.py | Exit 1: control/replay and four moved-source preservation cases pass; two directory-replacement success assertions expose R3-1 |
| python3 -B SCRATCH/lock_probe.py | Exit 1: independent replacement-directory lock bypass confirmed; normal busy control passes |
| python3 -B SCRATCH/anonymous.py | Exit 0: two anonymous-file SIGKILL/recovery/replay cases and split-output replay refusal |
| git diff --check | Exit 0 |

Fresh total: 66 Rust tests passed, one intentionally ignored supervisor child, five supervised SIGKILL children plus two additional public-CLI SIGKILL children. Each supervised phase recovered/imported 100 candidates without duplicates and preserved its source. Seventeen public-CLI probe invocations have recorded exits. Three tiny interposer compilations succeeded with -Wall -Wextra -Werror. Two adversarial scripts failed on the same introduced directory/lock finding; these are product assertion failures, not setup failures. No full gate result was substituted for either failure.

Focused log: /home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/akasha-evidence/mvp025-mempal-review-r3-tests.log.
SHA-256: a8d079b3b5c388c7d839508b9727dd5a57d8e1732580bb647d315088498ee3b2; 31,322 bytes. It preserves commands, probe scripts, child observations/exits, both assertion failures and cleanup.

Limits and final custody

Issue #25 remains OPEN. Different-source incremental application is the next leaf; #24 resource stress, #26 packaging, #27 evaluation, #28 final release and actual Claude authentication #12 remain separate. No installed-source commit, model quality, 50 GB/100-client RSS, power-loss/ENOSPC or Claude-auth acceptance is inferred.

Pre/post-test and pre-report attestations retain the frozen HEAD/tree/base, clean porcelain status and identical lexical target symlink. All 202 changed filesystem blobs match frozen HEAD. All test/probe subprocesses exited; a scoped /proc CWD/FD census found no remaining consumer of exact owned scratch /mnt/ssd/mirror-rootfs/home/obj/tmp/a15-cache-s0s9a8n9/mvp025-r3-native-dykw9bie before its removal. Scratch, interposers, fixture databases and outputs were removed. Only this new report and the requested log persist. No product/test/doc file, Git index/ref/config, provider/model/auth/reasoning setting or live mempal checkout was changed. Final artifact write is followed by frozen-state/blob attestation and hash readback; the report contains no self-embedded digest.

Skipped the unchanged complete gate, historical baseline rerun and all private/live-host execution as instructed.
Risk: directory replacement can bypass output writer exclusion and produce false success with split, unreplayable conversion output; R3-1 blocks this leaf.

VERDICT: FAIL