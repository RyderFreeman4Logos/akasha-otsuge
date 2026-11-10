VERDICT: PASS

The complete frozen two-commit resource-observability leaf has zero actionable introduced findings. All four requested focused commands passed once. Source tracing and archive validation support this verdict independently of the supplied green canonical gate. This is an independently shippable observation/configuration leaf; it does not complete issue #24.

**Range and custody**

| Item | Verified identity |
|---|---|
| Repository | `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp024` |
| Branch | `feat/mvp-024-stress` |
| Base, origin/main and merge-base | `37d4d77ec4517d9a35eff7eb85704de6a656d739` |
| HEAD | `ffc279fe7aaf7aa1ac9e6d3160926b90c17abaca` |
| Tree | `5d697d798ebf180c929b33bb81634c73b2c4b57f` |
| Review range | Complete `origin/main...HEAD`, two commits, 47 changed files |
| Initial and final source state | Clean porcelain; all 47 changed working-tree files equal their HEAD blobs |

Commits reviewed, oldest first:

- `a0360330566e486d6fd5ced6c55c8a6e9a203a0f`: documentation-only #23 closure archive and production acceptance checklist.
- `ffc279fe7aaf7aa1ac9e6d3160926b90c17abaca`: configuration, process accounting, opening-budget identity, tests, documentation and compact evidence.

The complete binary/full-index range diff is 208,306 bytes, SHA-256 `ce4a8dad43fd11f71d6b72ca8beb7340fac0493d5beccf5e07c61b54a3a4f1c5`. `git diff --check 37d4d77...HEAD` exited 0.

Source remained read-only. Only the requested external report and focused log were authored. No commits, publication, worktrees, agents, broad gates, canonical rerun, provider calls, config/environment changes or native-child transcript inspection occurred. WD remained parent-owned; Cargo commands ran sequentially under the inherited environment.

**Filesystem instructions**

Actual filesystem inspection preceded source reads. The checkout's logical and resolved root are identical. Both `AGENTS.md` and `CLAUDE.md` were verified absent at each following exact directory; logical and resolved paths are identical:

```text
/
/mnt
/mnt/ssd
/mnt/ssd/mirror-rootfs
/mnt/ssd/mirror-rootfs/home
/mnt/ssd/mirror-rootfs/home/obj
/mnt/ssd/mirror-rootfs/home/obj/project
/mnt/ssd/mirror-rootfs/home/obj/project/github
/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos
/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp024
```

Thus, for example, the two root checks were the exact paths `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp024/AGENTS.md` and `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp024/CLAUDE.md`. Absence is filesystem evidence, not Git absence.

Recursive filesystem traversal followed symlinks. No instruction files exist under the reviewed `src`, `tests` or `docs` trees, including `src/config`, `src/daemon`, `src/storage`, `tests/support`, and both changed archive directories. Those logical directories resolve to themselves. No applicable repository instruction file was found.

The traversal did find these unrelated dependency instructions. Let L be the exact checkout root above and R be `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge`. Each logical/resolved mapping below is exact by prefix substitution:

| Logical path | Resolved path | Lines |
|---|---|---:|
| `L/.spike-work/helix-db/AGENTS.md` | `R/.spike-work/helix-db/AGENTS.md` | 57 |
| `L/.spike-work/helix-db/crates/cli/CLAUDE.md` | `R/.spike-work/helix-db/crates/cli/CLAUDE.md` | 150 |
| `L/.spike-work/lancedb-v040/AGENTS.md` | `R/.spike-work/lancedb-v040/AGENTS.md` | 157 |

These files were read during instruction discovery; their dependency source was outside the review. Both instruction names are also absent at resolved R, `R/.spike-work` and `R/target`. `L/target` resolves to `R/target`; `L/.spike-work` resolves to `R/.spike-work`.

Both instruction names were additionally verified absent in `/home`, `/home/obj`, `/home/obj/project`, `/home/obj/project/github`, `/home/obj/project/github/RyderFreeman4Logos`, `/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge`, its `.git`, and its `.git/akasha-evidence`. The evidence directory resolves to its stated logical path.

Applied `native-code-review` and its scope, evidence, verdict, Rust, receipt-integrity and frozen-artifact rules. The user's prohibition on agents takes precedence over the skill's reviewer-transport preference.

**Complete scope and source review**

Every changed file was inspected as source/documentation or validated as an archived artifact, including compressed logs. The scope comprises:

- Nine code/test files: `src/config/mod.rs`, `src/config/schema.rs`, `src/daemon.rs`, `src/daemon/resources.rs`, `src/storage/mod.rs`, `src/storage/backup.rs`, `tests/config.rs`, `tests/daemon.rs`, `tests/support/daemon_notifier.rs`.
- Three documentation files: `docs/configuration.md`, `docs/memory-resources.md`, `docs/plan/todo.md`.
- All 15 files in `docs/benchmarks/results/mvp023-final`, including its manifest and all 14 members.
- All 20 files in `docs/benchmarks/results/mvp024-observability`, including its manifest and all 19 members.

Read the MVP-024 TODO and the memory/accounting, hot-reload and acceptance sections of `docs/spec/08.md`. Traced affected baseline consumers in configuration resolution/checkpointing, daemon configuration persistence and pinned-job recovery, index ownership/opening, IPC connection admission and shutdown. Unrelated baseline implementation was not re-audited.

**Findings**

Actionable introduced findings: **0**. No severity/file/line/trigger finding is asserted. The requested zero-actionable-defect rule was applied; a green supplied gate alone would not have established PASS.

The source and focused tests support these conclusions:

| Boundary | Evidence and conclusion |
|---|---|
| Effective defaults and overrides | `src/config/schema.rs:145` adds optional u32 settings; `:164` computes target/peak/total. Omitted peak is floor(5 × target / 4), capped at 2048; total defaults to 2048. The existing 1–2048 target domain remains valid. Profiles 512/640, 1024/1280 and 2048/2048 are exercised. Explicit 1024/1152/4096 is exercised. A larger service total remains permitted for external workers. |
| Validation and redaction | `src/config/schema.rs:533` validates target, then peak >= target and <= 2048, then total >= peak. Existing typed parsing rejects negative, oversized or wrong-type u32 inputs. New error-field names enter `SAFE_FIELDS` in `src/config/mod.rs`; messages remain static and values are not echoed. Existing credential/endpoint/argument redaction remains in force. |
| Rejected reload | `ConfigManager::reload_committing` resolves/validates the complete candidate before replacement, persists the checkpoint before publication, and retains active state on rejection or failed persistence. The new config test rejects four cross-budget cases after a valid explicit reload and verifies the accepted digest remains unchanged. Checkpoint units also verify restoration and durable-commit failure preservation. |
| Archives, digests and accepted generations | Both new options default to None and are omitted from serialization. Old configuration bytes do not acquire new fields; diagnostic `memory_limits` is outside the serialized Config used for its digest. Explicit settings enter the digest. The retrieval fingerprint remains based on retrieval/models/access. Restore validates archived configuration; pinned-job recovery checks its generation's restored digest against the accepted job digest (`src/daemon/jobs.rs:1223`). Existing pinned generations are not rewritten by reload. |
| Storage opening identity | `src/storage/mod.rs:279` exposes a stored opening budget. Normal open and both constructors in `src/storage/backup.rs:201` and `:215` capture the same validated configuration passed to `open_tree`. Reload cannot overwrite that identity. The real daemon test proves 2048 remains reported after a requested 512 reload, with reopen required, then reports 512 after restart. This is configuration provenance for engine opening, not a claim that engine caches or RSS equal that number. |
| Index contention | `src/daemon/resources.rs:132` uses `try_lock`: contention reports `inspected=false` and null opening budget. A successfully inspected absent index also has null budget. An existing index reports its own retained `budget_mib`; the code does not substitute the current requested target for an unknown/opened identity. |
| Census and units | `src/daemon/resources.rs:17` reads at most 65,537 bytes to reject files exceeding 64 KiB. The census caps processes at 4096 and total task entries at 32768. `:37` accepts exactly one named counter with one u64 number and kB unit, multiplies by 1024 with overflow checks, and rejects duplicates/trailing tokens. All group and total additions are checked. The measurement unit exercises valid conversion, missing/malformed/duplicate units and overflow. |
| Identity and parentage | `src/daemon/resources.rs:27` parses PPID and start ticks after the final command-name delimiter. Before sampling each descendant, `:65` checks its discovered parent. `:100` rechecks every observed PID's start identity, parentage and exited/live classification before accepting the result. Missing processes, PID reuse or observed identity/parent changes invalidate the sample. This does not claim an atomic fork/exit census. |
| Threads, detached and exited children | `src/daemon/resources.rs:76` reads every task's children, rather than only the process leader. Traversal is independent of sessions/process groups, so a setsid descendant remains included while descended from the daemon. The real notifier test observes the guardian plus plugin and detached grandchild and checks exact RSS/PSS sums. Stable observed zombies/dead processes retain their PID with zero resident/proportional memory; disappearance or a live-to-exited race fails identity validation. Reaped descendants disappear from later samples, as the daemon test verifies. |
| Unknown measurements | Read, parse, bounds, arithmetic or identity failure propagates to `complete=false`, the fixed unavailable error and null groups (`src/daemon/resources.rs:114`). It is not a successful zero sample. A blocking-task join failure also reports incomplete/unavailable. The unit directly exercises an unknown PID. |
| Async ownership and endpoint cost | `resource_status` pins one owned Arc snapshot before awaiting; it holds neither a watch borrow nor the index mutex during sampling. Procfs work runs via `spawn_blocking`, without capturing storage or service ownership. The connection envelope retains request identity and existing same-UID/16-slot admission. Timeout can discard the reply while an already-started read-only blocking sample finishes; it has no durable operation to replay or commit. Collection costs one rollup per observed live process, all task-child files and two identity reads per observed process, plus bounded parsing/sums and ordered bookkeeping. Cardinality/read bounds are not a fixed wall-clock guarantee: smaps_rollup can be expensive for large address spaces. No production latency/load claim is made by the small focused fixtures. |

The status fields are observations taken at different points. Requested limits/generation are pinned together; index/storage identity and later process memory do not become a simultaneous snapshot. RSS can double-count shared pages; PSS apportions them. The implementation reports `atomic=false`, on-demand kind and a sample timestamp. It does not monitor continuous peaks.

**Documentation and separately open work**

`docs/memory-resources.md` matches the implemented leaf: requested versus opening budgets, restart-needed indication, lock-unavailable index state, all-thread descendant sampling, independent model servers/calling clients excluded, null unknowns, non-atomic observations and no peak/OOM guarantee. The implementation installs neither software memory admission nor managed cgroup limits; both enforcement flags explicitly remain false. Existing parent/system limits are not represented as project-installed enforcement.

The spec's enforced admission, safe automatic draining/reopening, cgroups, continuous current/peak accounting, full production corpus/index/workload, 100-client load, latency/throughput and fault/recovery acceptance remain unchecked in MVP-024. Their absence is explicitly outside this independently shippable leaf and is not classified as an introduced defect. Issue #24 must remain open.

Pre-existing observations are separate: engine cache selection and index reopening/ownership behavior predate this range; this leaf reports their opening configuration rather than resizing them. Historical #23 IPC timeout causality remains unknown, and notifier/provider idempotency remains declarative. Nothing in this review resolves or relabels that history. No additional pre-existing defect is asserted.

**Fresh focused commands**

Each requested command ran exactly once, sequentially, with no retry:

| Command | Passed / failed | Numeric exit | Command seconds |
|---|---:|---:|---:|
| `cargo test --locked --test config` | 24 / 0 | 0 | 0.252 |
| `cargo test --locked --lib daemon::resources::tests` | 1 / 0 | 0 | 0.239 |
| `cargo test --locked --lib config::manager::tests` | 2 / 0 | 0 | 0.236 |
| `cargo test --locked --test daemon memory_status_ -- --nocapture` | 2 / 0 | 0 | 0.520 |

The daemon test body summary was 0.30 seconds. Full stdout/stderr, including warnings and all four numeric exits, is retained unchanged at `/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/akasha-evidence/mvp024-observability-review-r1-tests.log`: 6,341 bytes, SHA-256 `121a31bbab3f020e381b1fd92e1a8acaa6ef372b30f474411d0058f08d2f201d`.

Cargo resolved through `/home/obj/.local/share/mise/shims/cargo` to version 1.99.1. Inherited `RUSTUP_TOOLCHAIN=1.99.1`, shared `CARGO_TARGET_DIR=/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/target`, supplied TMPDIR, sccache wrapper and socket were preserved. Denied mise cache-write warnings appeared; all requested commands exited zero. No extra runtime probe, formatter, Clippy or broad gate was executed in this review.

**Evidence binding and history**

| Archive | Manifest SHA-256 | Members excluding manifest | Member bytes |
|---|---|---:|---:|
| `mvp024-observability` | `590e723055e2e039fa6cac9859f7f6908de1a55a6735158447e1a2d2937a219e` | 19 | 45,413 |
| `mvp023-final` | `a2eee31990a1833b280139d6543a477fb85cae0b2b52d2589cfca7c311af9f52` | 14 | 75,938 |

Both exact member sets, every member byte count and SHA-256, and all source bindings validate. All 11 observability source hashes match final candidate bytes. All 12 #23 source hashes were checked through `git show fc88baf240d710f71a91f5c886d9fec4e21929f2:<path>`, not against this modified checkout. That historical candidate's tree remains distinct from this review's tree.

Every gzip log decompresses and its raw SHA-256 matches its terminal/CI receipt. Collected terminal objects match the standalone terminals. The observability history remains:

- Baseline effective-limits and daemon-status tests compiled and failed public assertions, each Cargo exit 101; enclosing RED runner exit 1. Raw log SHA-256 `cb8c30a0da2fdc87b978db0539baafe62d1e80695b8c5422ca8a6a566918e09e`.
- Baseline guarded-plugin/detached-descendant test compiled and failed its public complete-measurement assertion, exit 101. Raw log SHA-256 `e4cf8e8a44991089e7b3e2372e56144587efebb818813f76a80090481c16a108`. These are three behavioral REDs across two runs, not missing-symbol compile failures.
- The first attempt named GREEN actually failed compilation with E0063 at both backup constructors, then lines 201 and 214. Cargo exited 101 and the Python runner exited 1. Raw log SHA-256 `b2fa8efa43673df3a3681c2a3cb696e96b51ef55730974aa4b70a9524726a132`. It proves neither behavioral GREEN nor execution of later checks.
- Repaired GREEN-r2 completed 24 config, one measurement, two checkpoint and two daemon tests, all-target Clippy and diff check; runner exit 0 in 40.73460656777024 seconds. Raw log SHA-256 `0b77e09e08d64b65857bfb8e26c1fa87f34a3cf36d7d3cdde471a4cec485e89c`.

Both implementation-attempt command lists place `cargo fmt --all` before focused tests. The successful pre-commit admission identifies `a036033...` and a pre-format dirty diff. It is not an exact committed-head gate and its admission diff is not a hash of final formatted source bytes. The manifest explicitly binds the final source bytes; the later clean canonical receipt supplies separate exact-HEAD evidence.

The new archive includes historical post-merge #23 CI42750140146 SUCCESS on `37d4d77...`, including cache restored/hit. Its uncompressed log SHA-256 is `92290c21d04d9f20759a68ce5587d7339e94f566392461f5bded5b3fc77f0e42`. The #23 archive also retains its old candidate gate/review/CI/publication/closure records. These establish archived identities only: no live remote refresh occurred and none is promoted to validation of the #24 candidate. Earlier FAILs and unknown-causality history were not rewritten.

**Supplied canonical gate, not rerun**

The supplied verified receipt is 2,203 bytes, SHA-256 `04543813c473486f817e9a444dd500c8fe5061231e5a1428d83b2a8240cb7d98`. Its base/HEAD/tree agree with this frozen review. All four referenced artifact hashes validate:

| Artifact | SHA-256 |
|---|---|
| Admission | `ee3783d24179ac25c3e1f943ba2e06d887ffc09a93303ee9aa8612542b679a44` |
| Terminal | `4ae58c71182d1d1278f641cb2174c871e4b411c9c33047a4875903c9aca9db4c` |
| Collected | `1c6a9fc5bb40571d4d67b3e739275981d449722ae104673a9920a1d0034a7710` |
| Raw log | `b92a6b4ea32c327f6072b7569f1f6e581ba6c71a21579af29e5461a7becbcaa9` |

Admission records the exact HEAD/tree, empty dirty diff and no untracked files. The terminal's log hash matches the actual log and the collected terminal agrees. The existing `just check` reports exit 0 in 453.47827878408134 seconds, with 20 Rust result rows totaling 541 passes and 31 ignored, including installed repeat; each daemon run has 157 passes and seven ignored. The verified receipt separately records installed lifecycle/UID checks and zero TCP sockets. This is authenticated supplied evidence, not a newly executed canonical or installation check.

**Final attestation**

Final HEAD is `ffc279fe7aaf7aa1ac9e6d3160926b90c17abaca`; tree is `5d697d798ebf180c929b33bb81634c73b2c4b57f`; origin/main and merge-base remain `37d4d77ec4517d9a35eff7eb85704de6a656d739`. Porcelain with all untracked files is empty. All 47 changed source/document/artifact paths still equal the frozen HEAD blobs. The report and focused log are outside the reviewed source tree.

Skipped: canonical rerun, broad gates, provider/live production acceptance and publication, as requested. Risk: on-demand sampling has no atomicity, continuous-peak, fixed-latency or installed-memory-enforcement guarantee; issue #24 remains open.

VERDICT: PASS