VERDICT: PASS

Independent conventional review: PR #64 / #24 software memory admission

BASE_SHA: 8e14ee61c63b4dafe486e98ce5ca1f251f213001
HEAD_SHA: fa88d7cc020899412026ed7b245eb7d2011062b4
TREE_SHA: 7205c02423cd158d80b571ad479f75ad1ea6e8ab
BRANCH: feat/mvp-024-memory-controls
ACTIONABLE_COUNT: 0
RANGE: origin/main...HEAD

No introduced actionable regression was established in the reviewed software-control leaf. This is a source-review verdict supported by the fresh narrow tests below, not an inference from the canonical gate. Issue #24 is not complete.

Scope and authority

Reviewed the entire three-commit range: 3d1cfb4 (PR63 final evidence archive), f980e5a (measured job admission), and fa88d7c (native worker reservations and ownership). All 61 changed paths are inventoried with frozen SHA-256 values below. Both origin/main and the merge base equal BASE_SHA. The canonical binary/full-index diff has 264061 bytes and SHA-256 1095d1bfc11f46d0b792181168e33d2a136f07a20958a77da0739ac99ec3503d.

The supplied scope JSON has SHA-256 3719b423a618b4b8b5e23be84049d20a56043692a483999b0245bb4c619d2183. Its source hashes match both the working files and HEAD blobs. Review includes the complete changed behavior, callers, config serialization/validation, fixtures, documentation and archive integrity; it is not limited to the last commit or resources helper.

Applicable instruction paths were checked on the live filesystem, including logical /mnt/ssd and /ssd aliases, their resolved parents, touched subdirectories, and the additional /home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp025 alias/parents. Final census: 156 AGENTS.md/CLAUDE.md candidate paths absent, including dangling-symlink checks. There were no present files or symlink targets requiring content reads. The supplied census was included. The native-code-review and Ponytail skills supplied review criteria; the user's explicit restrictions superseded generic suggestions for external review, new probes or broader execution. No delegates or external providers were used. No .env or private configuration values were inspected.

Source assessment

1. Measurement and classification. [src/resources.rs:17](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/resources.rs#L17) bounds each pseudo-file at 64 KiB, and [src/resources.rs:329](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/resources.rs#L329) bounds the traversal at 4096 processes and 32768 tasks. It reads every enumerated task's children, validates parentage and PID start identity, uses checked RSS/PSS arithmetic and rechecks observed identities before returning. Failed/missing reads return None; only positively identified zombies use zero memory. The parser rejects malformed, duplicate and overflowing counters. Registered roots are matched by PID/start and their observed descendants inherit that reservation's classification; all other observed children belong to daemon_group. Independently managed model servers and the invoking client/test harness are outside this daemon-rooted traversal. [src/resources.rs:404](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/resources.rs#L404) reports null groups and RESOURCE_MEASUREMENT_UNAVAILABLE on failure and explicitly emits atomic=false. A successful traversal is not an instantaneous census; concurrent fork/exit and short-lived memory peaks remain disclosed limitations.

2. Job admission, replay and deadlines. [src/daemon/jobs.rs:1095](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/daemon/jobs.rs#L1095) checks the stored job fingerprint and returns an existing matching job before the new memory refusal. New identities must satisfy live pressure and existing queue/concurrency controls before persistence. [src/daemon/jobs.rs:1508](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/daemon/jobs.rs#L1508) handles cancellation and queue/execution expiry before memory admission, then rereads time after sampling and refuses to grant a running lease if expiry occurred during the sample. [src/daemon/jobs.rs:1765](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/daemon/jobs.rs#L1765) skips new notification/incident work under pressure but still visits queued claims, collects running owners and waits on work/ticks. Pressure alone does not set stopping or rewrite accepted generations/deadlines. The private memory-pressure unit fixture also checks replay and queued identity; that specific unit is source-assessed and covered by the supplied canonical suite, not claimed as selected by the seven fresh commands.

3. Reservation arithmetic and bounded state. [src/resources.rs:193](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/resources.rs#L193) adds the unused share of each active allocation separately to measured service RSS, plus unused daemon peak headroom. Growth of one worker cannot subtract another worker's unused reservation. [src/resources.rs:227](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/resources.rs#L227) serializes reserve/sample/insert, uses the greater of the configured estimate and retained completed-profile positive peak, rejects zero estimates, checked-add overflow, exhausted IDs and more than 256 active allocations. Retained profile entries are bounded at 256, with a conservative maximum for excess profiles. [src/resources.rs:134](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/resources.rs#L134) transfers positive sampled allocation peaks into that retained table. Sample-count saturation is explicit. The synchronous state mutex is not held over procfs awaits or over destruction of a Reservation; the async admission mutex covers sampling and insertion. Accepted allocations survive total-budget lowering; can_start/admission refuses new work while the projection no longer fits.

4. One authority and reload ordering. [src/daemon.rs:670](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/daemon.rs#L670) constructs one process-lifetime Resources. [src/daemon.rs:856](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/daemon.rs#L856) carries the same Arc into controlled reopen. [src/daemon/resources.rs:20](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/daemon/resources.rs#L20) takes the estimate from the job's pinned resolved config, including named overrides, while taking global memory limits from the active config. Callers hold Service admission; the unchanged reload path at [src/daemon/configuration.rs:137](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/daemon/configuration.rs#L137) uses that same mutex. Preparation and actual spawn recheck live limits under admission. Thus a reload between reservation and either start boundary is observed before that boundary; admitted running handles retain their leases. The Resources-level reservation lock additionally serializes direct concurrent reserve calls. New process construction resets sampled history; same-PID reopen retains it.

5. Native paths and ownership. The Host dispatch at [src/daemon/consult.rs:49](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/daemon/consult.rs#L49) covers Codex, Claude, Hermes and Pi. All four start paths call register_process immediately after spawn: [src/adapter/codex/runner.rs:174](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/adapter/codex/runner.rs#L174), [src/adapter/claude/runner.rs:204](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/adapter/claude/runner.rs#L204), [src/adapter/hermes/runner.rs:180](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/adapter/hermes/runner.rs#L180) and [src/adapter/pi/runner.rs:194](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/adapter/pi/runner.rs#L194). The RunLease owns the reservation ([src/adapter/budget.rs:180](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/adapter/budget.rs#L180)); the existing handle cleanup closures retain a cloned lease until reap finishes. Reviewed ordinary completion, cancellation, start/send failure, preparation errors and caller-drop paths. No new release-on-timeout shortcut was introduced. The existing reaper's bounded caller wait does not discard its background owner.

Version/preparation probes use task-local scope ([src/resources.rs:104](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/resources.rs#L104)); it belongs to the polled run future rather than a mutable global. [src/adapter/native.rs:14](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/adapter/native.rs#L14) retains this optional reservation and transfers it into the reaper cleanup closure, including Drop. The shared version_output and Hermes export paths use that owner. When preparation fails, the outer lease can disappear while the probe's reaper retains capacity until settlement. Probe context does not leak into unrelated futures. Standalone probes have no worker reservation and remain daemon core helpers.

6. All worker consumers and refusal propagation. Consult/delegates/fallbacks reserve and check preparation/start at [src/daemon/consult_run.rs:147](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/daemon/consult_run.rs#L147); extraction does so at [src/daemon/extraction.rs:238](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/daemon/extraction.rs#L238), and grill at [src/daemon/grill.rs:405](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/daemon/grill.rs#L405). All use Service.resources. Consult maps delegate RESOURCE_BUSY to DELEGATION_UNAVAILABLE; neither error is in the explicit fallback allowlist, so memory denial cannot become a cheaper fallback start. The Codex hook mapping preserves RESOURCE_BUSY at [src/adapter/codex/hooks.rs:265](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/adapter/codex/hooks.rs#L265).

7. Core-helper headroom. Standalone registration probes are checked before start in [src/daemon/claude.rs:122](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/daemon/claude.rs#L122), [src/daemon/codex.rs:71](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/daemon/codex.rs#L71), [src/daemon/pi.rs:80](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/daemon/pi.rs#L80); Hermes checks both version and export boundaries at [src/daemon/hermes.rs:112](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/daemon/hermes.rs#L112). These run under Service admission without consuming a worker estimate. The notification plugin path at [src/daemon/notifier/stdio.rs:582](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/daemon/notifier/stdio.rs#L582) checks live memory under that mutex before spawn and releases it after the child/group is owned. With worker reservations present, memory_available includes daemon peak headroom and every unused allocation; with no worker reservations, config validation already requires total >= daemon peak and measured daemon RSS must remain below target. Notification guardians/plugins/descendants are unregistered core helpers. Their existing reaper retains its dispatch permit through cleanup.

8. Configuration and status. Optional runtime/named estimate fields use default plus skip_serializing_if=None ([src/config/schema.rs:154](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/config/schema.rs#L154), [src/config/schema.rs:247](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/config/schema.rs#L247)); both reject zero and the runtime default remains 1024 MiB. Omitted fields preserve previous serialized objects/digests. The config diagnostic field allowlist was adapted; no unrelated routing, model, timeout or fallback defaults changed. [src/daemon/resources.rs:76](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/daemon/resources.rs#L76) distinguishes requested/open budgets, reopen need, measured/projected draining and software versus cgroup enforcement. [src/resources.rs:287](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/resources.rs#L287) distinguishes current sample data, accumulated sampled peaks and absent observations. Positive retained history survives reopen; no kernel-maximum claim is made. docs/memory-resources.md, examples/config.toml and docs/plan/todo.md keep the software-only boundary and remaining #24 work explicit.

9. Tests and fixture integrity. Production Resources uses real procfs. The private Service unit fixture supplies per-instance measurements ([src/daemon/jobs.rs:85](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/src/daemon/jobs.rs#L85)); this is cfg(test) state, not a production or process-global measurement override. Public daemon integration fixtures exercise actual subprocess procfs. The concurrency fixture's 64 MiB estimate applies only to bounded Python protocol workers; the production default is unchanged. [tests/support/daemon_notifier.rs:671](https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/fa88d7cc020899412026ed7b245eb7d2011062b4/tests/support/daemon_notifier.rs#L671) derives the guardian PID independently from the plugin's /proc stat parent, verifies all four specific core PIDs, excludes the test harness, checks RSS/PSS sums, and after release waits for a complete service census containing only the daemon. Cleanup ownership is also freshly exercised by the four native cleanup tests. The lowering/reopen test checks original generation/deadlines, retained reservations and positive profile history, and the estimate-refusal test verifies no native-start marker.

Findings and attribution

ACTIONABLE_COUNT: 0. No introduced actionable source or evidence-integrity finding requires repair. No separate pre-existing source defect was established in the inspected seams. The historical CI failure below is unresolved evidence, not a newly attributed product regression. No severity/repair entries are invented for documented software-only limitations.

Archive integrity and historical boundaries

The final PR63 archive contains exactly 26 manifest members plus manifest.json; all member hashes and all ten historical source pins verify. The archived R2 scope's 95 source pins also verify against 0de2704f5030102291ea492baeda89a3eb183818. Its tree equals BASE_SHA's tree, 498c2d9564cc34358b5a76b39422b5d74fe18017. Current memory-control source was not used as the historical oracle. Gzip members decompress successfully. The public-body archive ends with the exact immutable original R2 report.

The original R2 report remains SHA-256 21bdad0502cbfc6d87b999dd3e0c5a34dd4a3d6d53cc9a51ca3a975f734e6ef5. Its separate five line-coordinate corrections were verified against that historical commit and remain visible: src/daemon.rs 866 -> 855 (spawn_blocking), 893 -> 877 (accept error), 910 -> 891 (reject_busy), 793 -> 794 (watcher tick); src/daemon/jobs.rs 1442 -> 1446 (claim). These are parent corrections, not a rewritten independent report.

The owner-order archive's 15 member hashes and ten historical source pins verify; README/manifest now explicitly label 0de2704 pins historical. Historical CI42949516550's HistoryGap failure remains UNCLASSIFIED. Local diagnostic passes and later CI42951891062 success neither explain nor waive it. No migration, old acceptance or historical CI was rerun.

Canonical prerequisite: supplied evidence, not a fresh gate

mvp024-software-memory-canonical-r1-verified.json SHA-256: 93499fec63cb6a7070ad6b8e8f9910cf8af70bd9d7fd2071bb96f16ead6d7965. Its BASE/HEAD/TREE match this clean checkout, and all four referenced artifact hashes match. The saved log independently recounts 21 Rust result rows, 625 passes and 35 intentional ignores including the installed repeat. The receipt separately records 34 Python tests, 7 Node tests and installed lifecycle/UID checks. Terminal exit is 0 for just check. Canonical log SHA-256: 42455e4d276ed75daee8078e513e37df4bd387633a12899a6792861dcdc42555. This satisfies the prerequisite; no new full gate, Clippy, hosted CI or installation was launched.

Fresh functional executions

Each command below ran once, sequentially, through subprocess argv under /bin/bash with login initialization disabled. No zsh/mise startup ran. The owning wrapper exited 0 and all child exits were collected.

| Exact fresh command | Passed | Failed | Ignored | Filtered | Exit |
| --- | ---: | ---: | ---: | ---: | ---: |
| `cargo test --locked --offline --lib resources::tests` | 4 | 0 | 0 | 104 | 0 |
| `cargo test --locked --offline --lib adapter::native::cleanup_tests` | 4 | 0 | 0 | 104 | 0 |
| `cargo test --locked --offline --test config` | 25 | 0 | 0 | 0 | 0 |
| `cargo test --locked --offline --test daemon memory_` | 8 | 0 | 0 | 167 | 0 |
| `cargo test --locked --offline --test daemon codex_tests::total_budget_lowering_keeps_running_worker_and_learned_peaks -- --exact` | 1 | 0 | 0 | 174 | 0 |
| `cargo test --locked --offline --test daemon codex_tests::consult_refuses_an_executor_estimate_that_does_not_fit_before_native_start -- --exact` | 1 | 0 | 0 | 174 | 0 |
| `cargo test --locked --offline --test daemon codex_tests::consult_hook_submits_and_expands_durable_results_inside_the_read_only_sandbox -- --exact` | 1 | 0 | 0 | 174 | 0 |

Total: 44 executed tests passed, 0 failed, 0 ignored. The memory_ substring selected eight maintained tests, including adjacent memory-named cases; it does not represent eight separate admission tests. No additional maintained selector or invented probe was run.

Inherited rustc 1.99.1 (58cf8f9c3 2026-10-13), cargo 1.99.1 (5f94df478 2026-08-27), RUSTUP_TOOLCHAIN=1.99.1. Cargo resolved to /home/obj/.rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin/cargo. CARGO_TARGET_DIR=/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/target; RUSTC_WRAPPER=/mnt/ssd/mirror-rootfs/home/obj/.local/share/mise-rebuilt/installs/sccache/0.19.0/sccache-v0.19.0-x86_64-unknown-linux-musl/sccache; TMPDIR=/mnt/ssd/mirror-rootfs/home/obj/tmp/a15-cache-s0s9a8n9. Existing SSD target and sccache were reused.

TEST_LOG: /home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/akasha-evidence/mvp024-software-memory-review-r1-tests.log
TEST_LOG_SHA256: a4aa64b797acf50906f8d2d9084da51b85e028f925ffa26901631b30aea3544f

Post-test custody

Postflight at 2026-11-11T07:25:57.582675+00:00 re-attested exact HEAD/tree/branch/base/origin-main, empty porcelain status, all 61 working-file/frozen-blob hashes, the full-range diff, both archives and their historical pins, and absent instruction paths. git diff --check BASE...HEAD exited 0. Worktree and common index.lock were absent. A readable same-UID executable census found no process under the shared target, with zero inaccessible/raced entries. All seven owned test processes and their wrapper terminated; no test/review job is left running. This is scoped process evidence, not a claim about unrelated services.

Only this report and the declared log were intentionally written, apart from ordinary execution artifacts of the expressly authorized Cargo tests/fixtures in inherited target/TMPDIR. No product edits, index/history changes, hooks/config changes, manual cache changes, worktrees/targets, GitHub comments or relays were requested or performed. Report persistence is checked through the declared logical path, and a final repository/hash attestation follows the write. The report's own SHA-256 is recorded externally rather than embedded into itself.

Frozen changed-path inventory (SHA-256)

All following hashes are bound to HEAD_SHA above and were verified against both Git blobs and working files.

| Path | SHA-256 |
| --- | --- |
| `docs/benchmarks/results/mvp024-storage-reopen-final/README.md` | `fad8d2788f5c70817ac64ee9c4664c46e75f0d405bc0a4a7e9e31e63119206c9` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/manifest.json` | `2406a4b1bfdb6ce24a7aade76d0e9ae2a24ed52ebf1b2b38c874ad7bf66b1b1c` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-canonical-r3-preflight.json` | `1eb7e1640404baa8c3c7d1e6895053894c87439a94b8e566324c3837345c08cc` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-canonical-r3-verified.json` | `cfa229482aa383d0381b6ddb2e4608859e767662eaee8ca5bc5d542caea259b6` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-canonical-r3.admission.json` | `7b766ab66f08b8a7a9e74dc370554d4b52dd5628b4187028bac306312d0b4058` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-canonical-r3.collected.json` | `0d3eba0784393c2ab1a233d4886d51e1e9d868906aed0be749d888e773b3e29c` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-canonical-r3.log.gz` | `7992af7be857bcccfcd10002ddaf7acd416e933958e2c31e939399c8d2fe7d98` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-canonical-r3.terminal.json` | `89ac15d4b721823356017efa85bc0189f105c6cfae55c441c366849334510bcc` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-ci-r3.json` | `028699004d94b38ed209b76e893df86a3bf2c9c6c082fab6d7b57bea12ac0eac` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-ci-r3.log.gz` | `2f5910bff49517ce2cd39d39716b8ef6b51f108e1913d47f1ea2cdef0c0d8126` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-completion-publication.json` | `cc653868efe8220dfccb8719d37cfe36712f1b35a29486dbfd6537d61fe400ae` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-merged.json` | `475dcce6362d71abdcb051d48e228a9a15ba7fac061901ae8a0433533f8739a5` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-owner-order-range.json` | `632cd54ad61602ec2e918d8242a964b3f7a232101200841314ac8d5c8db54baa` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-review-r2-citation-corrections.json` | `c7e498527faf64cf07e4c90fcc5c4d71e80a2c07b17437689dcb55ccabdf5d8a` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-review-r2-cleanup.json` | `ceae6263eb8d792887070ae25c6f1ce1d265748d1fb12e0a1a51325776c6fd9e` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-review-r2-public-body.md.gz` | `537449477fc155e52d680c422bdf6bc3f93ca3ea4cbf75f5882de8d7b4caffea` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-review-r2-publication.json` | `6afd0ea1a887c57a9474cb1af5bf1a0815a60eec14070658abce1a5b9ef9c062` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-review-r2-scope.json` | `00e9bd1db3421335549a013dddb2109356bd69005edd5838fd02ca4f42a1423e` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-review-r2-tests.log.gz` | `df075b967f4a1e3ae004fa8eabed5fe99b4cb7e37c994da7fdc5eb539d743984` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-review-r2-verified-r2.json` | `2c987072cc992de99b9310a1e0afbdc1b85cbf5b2f5b1bc848fe7b40d7880312` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-review-r2-verified.json` | `813d94d42b2a610bd47e3edd6da963702d8efb6c0d82bdf4d445109f41a13847` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-review-r2.admission.json` | `a5dd4405db737bfdd6b7ec7827b43dbfef7499f98245d1990224647b4f932ae1` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-review-r2.collected.json` | `d2614dca3fdf677bb8ac1eb933d8600dcd00ca3c46f59403048b6015f42eaf0f` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-review-r2.exit.json` | `44212a5f1e765b6411d8a3053c198dc98d2f05e22815d34f1ade2347568ba667` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-review-r2.log.gz` | `ebbf3e1bd7309ac166660033cf9be8c415cc341188edf6b0154f626d555e33bc` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-review-r2.md` | `21bdad0502cbfc6d87b999dd3e0c5a34dd4a3d6d53cc9a51ca3a975f734e6ef5` |
| `docs/benchmarks/results/mvp024-storage-reopen-final/mvp024-reopen-review-r2.terminal.json` | `c6760e595bd56f8c0806261d7e58e22e1c5a2b619b1bc8469fde68def0d19ac6` |
| `docs/benchmarks/results/mvp024-storage-reopen-owner-order/README.md` | `f6fd138a876fe58c5d8f24b37b1ac438b3e32f6a75e2ff0e143dd9056e19ed21` |
| `docs/benchmarks/results/mvp024-storage-reopen-owner-order/manifest.json` | `329a91b62f043e1b2093614140fb53ceccd6a89ae69ae837d812326bc5672125` |
| `docs/memory-resources.md` | `3304d915ee95611bbd295258e41fc0bc45f3108ff95f4ed66cfc0a67aab40f9b` |
| `docs/plan/todo.md` | `ba0c32c5d7779a81b05a70d4f7b09fbd877af2ddb0cbf08e54514c25d8e0cad8` |
| `examples/config.toml` | `d4b8de0f18e15a7178d82602563a9e3988c574c69d53f701b2a19da011c77741` |
| `src/adapter/budget.rs` | `a16df52cc1c4a31178690182603426d0794bf4ac14a6ade384e55b8e4e524892` |
| `src/adapter/claude/runner.rs` | `fd38d25a09760f71f49d8b53b4aa750eb464217aa08b9b815775347f0efaee73` |
| `src/adapter/codex/hooks.rs` | `a7a9b239a96772f25b58f4733fda9a8ca4f26a5841dee1ab1ef1729383628450` |
| `src/adapter/codex/runner.rs` | `0f4fec875f84dff054ab901ab364deb5bdaa492191be9381365ce677e3856f48` |
| `src/adapter/hermes/native.rs` | `d40e00767620ed57da7a037f62fe8090844b39c7d6587ffa8d5279f45f84289e` |
| `src/adapter/hermes/runner.rs` | `7624394268b7e683c97957dd6d886900e5c4424f65b46cbc07e2d7a9aa490331` |
| `src/adapter/native.rs` | `98cf5823793e0b3019d29946de48b3696ee3a42d9ebceb871f57fbf19765d86f` |
| `src/adapter/pi/runner.rs` | `cb6d5e19b36a7064239a98a8e0ab01aa977e36be2dc32cce0688f6f7b09cf3f0` |
| `src/adapter/runner.rs` | `75d063b7a27649769d3f6d23a8f39f633bf31d3769a259de40959596a693e107` |
| `src/config/mod.rs` | `4cf74d4a7a89f5947e95effd0213e60be1279202197495f9952d0bff32b302df` |
| `src/config/schema.rs` | `744d8dff4042d2d43d06396cf3e06c49720bd0d73c1f682b1a9b9b3359bb6584` |
| `src/daemon.rs` | `386fef808eeb63ddff924d4d8ce7eacbf566b639eb3bd8dba1abba512b85c799` |
| `src/daemon/claude.rs` | `c2d1ea3eb6883e0b90a24853811d18b29390401bd0a4616e54abccbb06c275e1` |
| `src/daemon/codex.rs` | `4b6d1c5579bb9500e159e1e33e7f3601881163bb9e0f1bef29a2b4d5b8dd2545` |
| `src/daemon/consult_run.rs` | `d73d16fd66415caccdbad5fc0d2e84e9075f281572f8ebb44ad41e798dbace2f` |
| `src/daemon/extraction.rs` | `984d98e3de680e3ab10d4988539a3ef02a30beff63d3ef223cafff95254ab986` |
| `src/daemon/grill.rs` | `59742ee6ed18e2fc32d6351e2e5b31eec396e784f5d177a5b0d8d3b9fcd88753` |
| `src/daemon/hermes.rs` | `07ac3a7e7c53ed94bdf43af61c9eb2a7b32f83cc26d494082175ea7fcba4e871` |
| `src/daemon/jobs.rs` | `0f27d2ead17fd5262841648733728d11e0b3840d1d74c6501bcdbcba4b45cd35` |
| `src/daemon/notifier/stdio.rs` | `c764e0a5b634510b92f6703498c47007d565f0c78c4f1f6b96e0c66887b67ae0` |
| `src/daemon/pi.rs` | `bff71b14b253f0e6fcbacce08db6248c3a3b50ddfae14e16099f760a3d494b49` |
| `src/daemon/resources.rs` | `38a3a62194e479ce375c45fcc5911dab291c5f99679d772b8bce797f5be4044d` |
| `src/lib.rs` | `e7219878ffef246bb63559a395052d238ca8115c7ee3e0c7960fb660864e3af5` |
| `src/resources.rs` | `29b81c18a36d32e47745810f6f998ff15d7b111cf8a78fbbc1fe66f5aa784b05` |
| `tests/config.rs` | `ab2dd9324f0612eb5dc74e18117f33744414eaf7d151aa6ecde42c2330114c45` |
| `tests/daemon.rs` | `062d6c5419aaaede28166d2f52493589595f2a99094c99a108184679fd97c81d` |
| `tests/support/daemon_codex.rs` | `8667e14a4199f099e0fe89cb81b2ccbdf8cfb9377d8a3c3eda849f9359155dce` |
| `tests/support/daemon_notifier.rs` | `3f9f4ae2519968abe35ba6952abb905bc155768660d4b55df937d4103525c7b9` |
| `tests/support/daemon_retrieval.rs` | `3773fed2f36914657ffcc6843e333614501ff9125f021b917c1ea59442cb6593` |

Limits

Not run: new full gate/CI, Daybreak, new red-team/adversarial campaigns, external providers, live services, SSH, private-data/bulk-corpus experiments, migration or old acceptance. Concurrency/lifecycle conclusions combine source tracing with the named maintained fixtures; unexecuted interleavings are not presented as newly tested.

managed_cgroup_limits=false is intentional. These software controls cannot cap growth of an already running process, establish kernel memory maxima, or establish the full 50 GB/100-client operating envelope. Hard cgroups, kernel peaks and full production acceptance remain #24 work. Parent owns WD, publication and any subsequent acceptance; this report does not close #24.

VERDICT: PASS
