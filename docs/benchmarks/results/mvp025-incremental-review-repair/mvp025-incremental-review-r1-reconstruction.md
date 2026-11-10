VERDICT: FAIL
ACTIONABLE_COUNT=2

# Reconstructed independent native review — retained evidence reconciliation

This is a complete reconstructed report of the completed review, grounded in the four named durable artifacts and a bounded independent source check. It is not the recovered original or a byte-for-byte reproduction. The original report bytes are unavailable. The summary's claimed original SHA-256 `8a2863b41808b17753dd4e92af871bcd726c0aadd5676fead6a8edb805b95f70` cannot be verified or reproduced. The original report path now contains a 748-byte CLI last-message summary, not the full report. Its actual hash matches the exit receipt's `report_sha256`, documenting the overwrite rather than recovering the lost report. Neither original artifact was modified.

The original reviewer completed with FAIL and two actionable regressions. Independent frozen-source reconciliation supports both. Hermes had already failed; direct Codex fallback was authorized. This reconstruction does not initiate another full review, delegate, run Cargo/builds/gates/probes, fetch, or change source, index, refs, configuration, or historical receipts. The only written artifact is this report. All execution results below are retained historical results, not new executions.

## Identity, time and scope

- Checkout: `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp025`.
- Requested home spelling: `/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp025`; this exact path is absent in this filesystem. It was not silently substituted or represented as a symlink.
- `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp025` resolves to the `/mnt/ssd/...` checkout.
- Branch: `feat/mvp-025-incremental`.
- HEAD: `085640133221975943ec4ebc7d28cc86d5df879b`.
- Tree: `b7e72a589d1123df67d0056326fd727d986f652c`.
- `origin/main` and merge-base: `fe30c78169fbf87bfa47cf06912b7c64d3a418b3`.
- Authoritative range: `origin/main...HEAD`; full stat is 106 files, 3350 insertions, 152 deletions. Production/test changes concern incremental import publication/admission/recovery and incident fixture handling; much of the range is archived evidence.
- Parent issue start remains `2026-11-10T04:22:08.149789+00:00`; leaf start remains `2026-11-10T12:23:08.907438+00:00`.

Current read-only identity checks match all supplied pins; `git --no-optional-locks status --porcelain=v1` is empty. Retained verification compared all 106 changed filesystem blobs with HEAD and recorded binary full-range diff length 389561 bytes, SHA-256 `f49ea2b3e61db6e5ca00ccb6e0b183bcc6f205927f6ee4a1ef41bafa66437c44`. This invocation read the full diff stat and relevant source, not the whole range or unrelated archived evidence. The four relevant live source files were independently compared with HEAD bytes and matched:

| File | SHA-256 |
| --- | --- |
| src/storage/import_publish.rs | ac060d65427ca2633033d670fe7dd51553341f8c749be2e6118a5e15ef93a1f8 |
| src/storage/import.rs | 19a831c82af6eb743619f362d54bfdc44558ae14e1882f257c5de6a760b1818d |
| src/storage/layout.rs | 7c0667f0925d3d23175f33b738738deba310040729c3c672b402766cce79de94 |
| src/storage/mod.rs | caf6429f7b59509978da485bb4ea972d183029e1aee068015662ab8169b0267d |

## Instructions and review boundary

Read `/home/obj/.codex/skills/native-code-review/SKILL.md` and its scope/attribution, regression-only scope, status/verdict/report-boundary, durable-filesystem and path-evidence references. Applied their origin-first classification and separation of focused checks, canonical gates, review, and acceptance. The user explicitly bounds this assignment and overrides fresh execution/full-range-review workflows.

Retained `inventory2.py` performed a filesystem census of 156 exact AGENTS.md/CLAUDE.md logical paths across the home, `/mnt/ssd`, and `/ssd` spellings, ancestors and relevant nested directories: zero present. Its predecessor's strict home-path resolution failed and is recorded below. Current filesystem checks likewise found no AGENTS.md, AGENTS.override.md or CLAUDE.md in the checkout tree or applicable ancestors. A further 198-path census covered the three spellings, changed-path ancestor directories and the report destination's ancestors, including its resolved path; zero present. Therefore there are no present instruction files with a resolved target or line count to report; absence was checked with filesystem existence/lexists, not inferred from Git. Report directory spelling and resolution are both `/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/akasha-evidence`.

No native transcripts, unrelated evidence files, credentials, private databases or .env files were read. A lightweight memory registry search yielded older MVP-020 context; it supplied no facts or decisions used in this report.

## Finding 1 — HIGH, REGRESSION: cleanup deletes a substituted foreign directory and returns success

Location: `src/storage/import_publish.rs:298-304`; relevant helpers `Root::open` at 48-69, `Root::validate` at 76-82, and `Root::remove` at 84-98.

After the exchange, `.archive-import` is supposed to name the marker's old authority directory. `Replacement::cleanup` checks `self.old.directory.matches(&stage)` using pathname metadata. It then separately calls `Root::open(admission, ".archive-import")`. Between those operations the name can be replaced with another directory. `Root::open` pins whatever inode the later openat finds and records that inode as its own `id`; it checks same effective UID and mode 0700 but never compares the newly opened descriptor with `self.old.directory`. `Root::remove` validates that the name still matches this newly adopted inode, walks its contents through `/proc/self/fd/.../.`, deletes them, validates again, and unlinks the directory. These checks consistently validate the foreign inode rather than the marker-authorized old inode.

The vulnerable boundary is authorization of the destructive descriptor: a successful pathname check does not authorize a later descriptor opened from that name. NOFOLLOW rejects symlinks, not a substituted real directory. The data-root admission lock/identity remains valid throughout this reproduction. The probe uses a synthetic same-UID private foreign directory; this is not evidence of cross-UID deletion or remote exploitability.

Retained public-CLI ptrace probe `leaf_probe.py` paused the exact cleanup openat syscall before execution, after the old pathname check. It asserted that `.archive-import` still had the marker's old inode and `authority` the new inode; parked the old tree; substituted a separate foreign directory containing `victim`; and resumed the unmodified CLI. Both runs exited 0 after 1639 syscall stops, deleted the foreign victim and directory, removed the replacement marker, and preserved the parked old history. The traced child was reaped with exit 0 in both runs. This is successful reproduction of unwanted deletion, not a failing setup or an assertion-only hypothetical.

Attribution: this branch adds `import_publish.rs`, Replacement, EXCHANGE publication and this cleanup path; they are absent at the frozen base. The regression is supported by the responsible new source and retained candidate execution. No base executable was materialized or dynamically tested, and no dynamic exact-base equivalence is claimed.

Minimal cohesive fix: open/pin the cleanup root, compare its descriptor-derived device/inode against the marker's expected old directory before any deletion, and refuse on mismatch while preserving the marker and all foreign contents. Retain pathname/admission checks for subsequent replacement detection. Do not merely repeat a pre-open pathname check. A focused regression should pause at this exact check/open boundary and assert foreign bytes survive and the command refuses; existing coarser foreign-stage/crash checks did not cover it. This report does not claim that one comparison proves every possible deeper tree race safe.

Severity: HIGH because the real public import path destroys unrelated same-UID data and reports success. Merge/admission is blocked.

## Finding 2 — HIGH, REGRESSION: ordinary open acknowledges local writes before pending publication durability

Locations: `Replacement::publish`, `src/storage/import_publish.rs:273-287`; `Storage::open/open_admitted`, `src/storage/mod.rs:292-330`; `Admission::sync`, `src/storage/layout.rs:85-89`; import replay completion, `src/storage/import.rs:270-274`.

Publication writes/syncs the marker and its parent, then exchanges `.archive-import` and `authority`. Cleanup starts with `admission.sync()`. If the data-parent fsync fails after EXCHANGE, the command correctly returns an error but leaves the new visible authority, old staged history and pending `.archive-replace.json`. Namespace visibility is not proof of durable publication. Import replay explicitly completes synchronization/cleanup after validating the input, but ordinary `Storage::open` only acquires shared admission, opens the store and validates admission identity. It never loads the pending marker or synchronizes the parent. Opening an existing authority does not take the new-root sync path. Export and daemon startup can therefore admit the unresolved publication; daemon bootstrap commits can receive durable-looking success without first establishing durability of the directory exchange that makes their store reachable.

Retained `fault.c` interposes fsync only when the FD identifies the configured data parent and authority's inode has changed from the old inode. It appends a witness and returns EIO. Import old succeeds; import new fails with `STORAGE_IO`, leaving the actual marker and old stage. Under the same fault environment, ordinary export succeeds with no additional parent-fsync witness. Two corrected continuation runs launch real foreground daemon commands, observe readiness using `daemon status`, stop cleanly, export local sequences 8 and 11 respectively, and show the replacement marker still exists with zero parent-fsync attempts. These are two reproductions of daemon admission/local writes, not two fresh independent EIO injections: the continuations reuse retained fixture state. The initial `probe.py` and `probe2.py` each independently reached the EIO import/export observation before their later setup failure.

Positive control: replaying the complete successor archive without the injected failure validates it, cleans the marker and old stage, and preserves exactly the locally extended sequence on export. This proves recovery capability when explicitly replayed, not automatic ordinary-open reconciliation. Sequence 11 follows the earlier run's local history; it is not an independent initial sequence-2 fixture.

Attribution: the new replacement marker/EXCHANGE pending state is branch-introduced and ordinary-open admission omits handling of it. Base did not have this incremental replacement state machine. A generic existing first-import durability issue, if any, is not being reclassified; the finding concerns this new post-exchange pending publication. No dynamic exact-base run occurred.

Minimal cohesive fix: centralize pending-publication handling in the ordinary storage admission boundary before exposing the tree or acknowledging any local write. Either fail closed on a pending marker and require validated import replay, or perform properly exclusive, identity-validated reconciliation and successful data-parent synchronization before admitting the store. Do not run destructive cleanup under a shared ordinary-open lock or remove the marker before its validated recovery contract is satisfied. An open-time retry must propagate EIO and deny access if durability still cannot be established. Cover public export and daemon startup under post-exchange EIO as well as the successful replay/local-tail control.

Severity: HIGH data-durability defect. A subsequent crash can invalidate the namespace publication supporting acknowledged writes. Retained experiments establish missing synchronization and successful writes, not actual power-loss loss. No power-cut test is claimed.

## Additional observation — PRE-EXISTING ordinary-open data-root substitution

The corrected continuation also paused pathname mkdir of `data/authority`, parked the originally admitted data root, substituted a same-UID foreign root containing `keep`, then resumed ordinary daemon open. The command failed (exit 1) after creating a valid `authority/format.json` and `authority/kv` in the foreign root; `keep` survived. The added admission validation detects the mismatch only after open has already mutated the replacement root.

Independently comparing base source shows `layout::open_root` is identical, and the prior `Storage::open` body is the same pathname-based logic now named `open_admitted`; the branch adds an outer admission wrapper. Thus mutation through the replaced pathname is PRE-EXISTING by source attribution, with a new late refusal wrapper. It is not a third introduced regression and is excluded from ACTIONABLE_COUNT. The retained probe ran candidate only. There is no dynamic exact-base reproduction or claim that the base returns the same error; its return behavior can differ because the wrapper is new. A cohesive future repair would carry pinned-root authority into creation/open before mutation.

## Focused historical Rust evidence — 61 passes, not whole-issue acceptance

All six retained Rust test commands ran in the frozen checkout with `--locked --offline`, all exited 0. Exact selectors/counts and wall seconds:

| Command | Passed | Ignored | Filtered | Wrapper seconds |
| --- | ---: | ---: | ---: | ---: |
| `cargo test --locked --offline --test archive -- --nocapture` | 23 | 0 | 0 | 1.953 |
| `cargo test --locked --offline --lib storage::crash_tests -- --nocapture` | 11 | 3 | 81 | 4.950 |
| `cargo test --locked --offline --test storage -- --nocapture` | 17 | 0 | 0 | 1.523 |
| `cargo test --locked --offline --lib daemon::jobs::import_tests -- --nocapture` | 7 | 1 | 87 | 1.975 |
| `cargo test --locked --offline --test daemon import -- --nocapture` | 2 | 0 | 164 | 1.931 |
| `cargo test --locked --offline --lib daemon::jobs::consult::editor_tests::incident_admitted_job_survives_sigkill_and_keeps_its_original_queue_deadline -- --exact --nocapture` | 1 | 0 | 94 | 5.379 |

Total: 61 passed, 0 failed, 4 intentionally ignored child entry tests. Supervisors launch the relevant isolated children. The two last commands have interleaved log output; totals are taken from their distinct result rows, not the aggregate tail. The exact incident selector has a nonzero selected count of 1. Import crash tests exercise the documented staged/head/marker/exchange/synced/cleaned/publication boundaries, local-tail preservation, foreign-stage refusal and owner exclusion. These focused passes do not cover the smaller check/open race or admission after failed exchange fsync and cannot waive either finding.

Retained build: `cargo build --locked --offline --bin akasha-otsuge`, exit 0, 235.155 seconds. It overlapped the first public probes. First probe CLI bytes hashed `e04447bfcdf915fefd289b0becc9275e1a5bb68c2eaf5469a07c74081dc981c8`; repeat probes after the build hashed `15d61aa50b49f1ddfd9415083129010c185868e6737b052fe5560a0070a84d3d`. Both ran `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/target/debug/akasha-otsuge`, resolving through `/mnt/ssd`. Do not conflate these binaries with the canonical installed binary. The retained repeats address the changed debug artifact; they are not a claim of byte equality between builds.

## Probe/setup ledger and exact invocation evidence

Let `S=/mnt/ssd/mirror-rootfs/home/obj/tmp/a15-cache-s0s9a8n9/mvp025-native-r1-50st779w`. Every script below was invoked as `python3 S/<name>` in the frozen checkout. This is exact path substitution notation, not an instruction to rerun. Embedded disposable sources remain in the named test log after scratch removal.

| Script/invocation, in retained order | Exit / seconds | Classification |
| --- | --- | --- |
| verify.py | 0 / 0.271 | Identity, 106 frozen blobs, archive manifests and exact canonical receipt verified historically |
| probe.py | 1 / 15.890 | EIO import/export observation succeeded; later readiness assertion failed; owned children reaped |
| probe2.py | 1 / 0.833 | EIO import/export observation succeeded; daemon setup/readiness failed; owned children reaped |
| diagnostic.py | 0 / 0.042 | Diagnostic wrapper succeeded; daemon itself exited 1, stdout 0 bytes, IO_ERROR |
| continue_probe.py | 0 / 1.647 | Fixture-only correction: daemon status and short AF_UNIX path; sequence 8, recovery and substitution assertions passed; 2 children reaped |
| leaf_probe.py | 0 / 1.759 | First exact cleanup substitution reproduction; traced child reaped |
| inventory.py | 1 / 0.024 | Strict resolution of absent exact home spelling raised FileNotFoundError; instruction-census setup failure |
| inventory2.py | 0 / 0.980 | Non-strict logical-path census with lexists, frozen-blob and tool inventory completed |
| leaf_probe.py | 0 / 1.848 | Repeat deletion reproduction after completed debug build; traced child reaped |
| continue_probe.py | 0 / 0.830 | Repeat sequence 11 and recovery/substitution assertions; 2 children reaped |

`probe.py` polled unsupported top-level `status`; `probe2.py` corrected to `daemon status` but retained the long socket name. The diagnostic's daemon stderr was `{"schema_version":1,"error":{"code":"IO_ERROR","message":"Could not complete input or output"}}`. The continuation corrected the status command and used short socket paths `S/s` and `S/t`; no product source was changed. The error itself is generic; the successful corrected fixture supports the setup explanation. Neither failed attempt proves a product daemon regression or complete end-to-end success. Their earlier import/export evidence remains valid. There are no additional fresh Rust failures in this review.

The fsync helper was compiled by the retained scripts with `cc -shared -fPIC -Wall -Wextra -Werror <fault.c> -ldl -o <fault.so>`. Public commands used `[debug_binary, --config, fixture_config, ...]`: `archive import S/old`, `archive import S/new` (or `S/attempt2/old` and `S/attempt2/new`); `archive export <durability>/export-with-eio`; foreground `daemon run`; corrected `daemon status`; `daemon stop`; `archive export S/attempt2/durability/after-daemon`; replay `archive import S/attempt2/new`; and `archive export S/attempt2/durability/after-recovery`. The ptrace leaf initializes `S/leaf` with the old archive and imports the successor while pausing only its cleanup openat. All archives/data/configuration were synthetic owned scratch fixtures.

Retained source identities: leaf probe `a3bfc9f62a070ebf37c436fb4f0666802f92f8ca01673f739eb8915f51d42b70`; continuation `c3b42d9d17b83df2411d9766ef8e9ea854e307b3b56dd9fc76f846efd7410597`; fault interposer `f53114885753fc4818474f4d380cb53e36ed8cc6900979ce2f26f52591bb1af6`; initial probes `e06e0c2f3683814c3e222de655314b90bccf35d2234cc055e2b586890f01fab5` and `4e27645fd3424fc267c4e575505ab017c4b8886a223a4629c004cbd2240edbb1`. Source text was read as retained evidence; none was executed here.

## Canonical reuse, identity and historical failure boundary

Reuse the retained exact-candidate `just check` receipt, not a new run: HEAD/tree/base match above, clean before/after, exit 0, 493.48120874818414 seconds. Admission UTC `2026-11-10T13:17:00.236033+00:00`, terminal UTC `2026-11-10T13:25:13.699527+00:00`; collected classification PASSED_EXACT_CANONICAL with process absent; verified UTC `2026-11-10T13:25:40.139215+00:00`. Retained admission process identity was PID 3346228, start_ticks 613451721. The named log's verify.py historically hashed the artifacts and reconciled 21 Rust result rows: 588 passes, 0 failures, 34 ignored including the installed repeat; daemon 159 passes and 7 opt-in ignored in each run. These are separate from the review's 61 passes.

Installed smoke receipt: binary SHA-256 `dfea5b3ae8069392ee619cd04cbe08946e8670fdd5e3941379337cea962aa8fb`; lifecycle/private-UID denial/peer-UID denial passed, TCP sockets 0, kernel-control UID 165536. This is historical controlled installed evidence, not current live deployment attestation or provider/product stress acceptance.

Canonical artifact hashes below are directly present in the named verified JSON and were checked by the retained verifier. The underlying four artifacts were not reopened by this reconstruction, per its evidence boundary:

| Artifact suffix after mvp025-incremental-canonical-r2 | SHA-256 |
| --- | --- |
| .admission.json | 5ca95918732f25fb9aead5892fa7722bfb51ea83efcf54584d2543baa35db36e |
| .terminal.json | 055b15905f85df91d5259f6af9d2e85e5062312dcffa0b9884c888e2eaadfabb |
| .collected.json | 586938ce4d8158c973d630be66daa95f2f9d69396ef03c8297cb911dd670baf1 |
| .log | 1569c10b321e970a4ece8048fae094ccf4e1f30d2177f7d277fb42fa87c26581 |

Historical incident fixture failure remains UNCLASSIFIED. At historical HEAD `6e692d6dbb0ac329372166a65d3029c913621049`, canonical-r1 exited 101 with child 0 passed/1 failed/94 filtered and aggregate 84 passed/1 failed/10 ignored. Later focused runs passed 1 test (5.08, 9.52 and 5.18 seconds) and an 85-pass/10-ignored suite passed; negative-control rows each failed 1 test. These observations do not determine the cause of the original suite failure. Retained source hash `aa9d6f8de62c550775c9da3ec9a34e3f2f77c26ff42dc8b064f48cdf971c2b03` for the owner test was identical at base and that failed historical HEAD; unchanged test bytes alone do not prove unchanged runtime/environment behavior or waive a failure. Retained incident production hash was `da5ea261be3b9eabe2e2322b42a1e70292b619a3d6719082837b2e97adae9ae7`. Current focused/canonical passes are green receipts at current identity, not a causal diagnosis of the historical failure.

Other historical failures retained by verify.py are not fresh review failures: focused-r1 22 passed/1 failed, focused-r3 final row 6 passed/1 failed/1 ignored, recovery-red-r1 0 passed/1 failed, incremental-red-r1 rows 1 passed/1 failed and 0 passed/1 failed, and an older Mempal PR-update job exited 1 without a Rust result row. They remain historical evidence rather than omitted failures or additional actionable findings. No fresh diagnosis of those archived attempts was attempted.

## Toolchain, custody, cleanup and limitations

Retained tool inventory selected Cargo from `/home/obj/.rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin/cargo`, resolving under `/mnt/ssd`; rustc 1.99.1 (58cf8f9c3 2026-10-13); just 1.48.1 through `/usr/local/bin/just` resolving to `just.bare-backup`. `CARGO_TARGET_DIR` and lexical checkout `target` were `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/target`; this exact readlink spelling was rechecked here without treating the resolved mount spelling as lexical identity. Historical TMPDIR/TMP/TEMP were `/mnt/ssd/mirror-rootfs/home/obj/tmp/a15-cache-s0s9a8n9`; sccache wrapper was the existing mise-rebuilt 0.19.0 binary with server UDS at that scratch root's `s`. No toolchain or configuration was changed here.

Original review exit receipt: process exit 0 means reviewer completion, not source PASS; elapsed 978.0168713917956 seconds; transport independent Codex exec after Hermes configured endpoint unavailable; UTC `2026-11-10T13:46:14.810606+00:00`; clean_after true. Runtime was codex-cli 0.167.0 with binary hash `cdcb45aaa01d0f14e298985e09465aebccd47f64ecd298aea44814af7b62bbda` and companion hash `83f3e174c2da2f1ca8655cce686e302509640d34c0f4407c1155056f64536e0f`; companion help exited 0. This is retained provenance, not re-executed runtime verification.

Original postflight ran `git diff --check`, identity/merge-base and porcelain status successfully; staged names were empty and index.lock absent. Final cleanup recorded S removed, no related visible consumers, inaccessible processes 0, all owned test/build/probe children exited, 106 changed blobs verified and clean status, with unchanged frozen identities. Individual failed probes' finally blocks reaped children; both continuation runs reaped 2 and both traced runs reaped their child. This reconstruction created no probe/build children or scratch fixture and relies on that cleanup receipt for the historical process census, without claiming a new global process audit.

The canonical gate and focused checks do not override source defects. No dynamic exact-base execution, fresh build/test/probe, power-loss experiment, private/live acceptance, new authentication check, provider acceptance, hosted CI validation, issue closure, installation or publication is claimed. Scope is this frozen review and its retained evidence, not whole MVP-025 acceptance. Both data-safety regressions require cohesive repair and subsequent exact-candidate review/gates before admission.

## Named retained input integrity (current reads)

- `mvp025-incremental-review-r1.md`: 748 bytes; SHA-256 `d405a84e8e5b48fe613a0038bc0084dc8090d5cf9744ea0dd6e43eb041facb17`.
- `mvp025-incremental-review-r1-tests.log`: 123246 bytes; SHA-256 `85c291d71a0121bc777a454bb933c9d4d063f0be4c03c56e5e16006d2073c414`.
- `mvp025-incremental-review-r1.exit.json`: 2197 bytes; SHA-256 `b479535a4a5943013d132d9e24c882239b957fc186020b92542b96a801346d96`.
- `mvp025-incremental-canonical-r2-verified.json`: 2187 bytes; SHA-256 `85854c5d6c05b82ea8c830ed59c30989e71e8543a374fc49a2cbbc9ff745104c`.

These hashes identify the retained inputs, not the missing original report. This report does not embed its own hash. The original summary, raw log and receipts are preserved intact.

Skipped fresh validation and whole-issue acceptance as requested. Remaining risks are foreign-directory data deletion and acknowledgment before publication durability.
VERDICT: FAIL
