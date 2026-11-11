VERDICT: PASS

# Independent conventional review: controlled storage budget reopen

BASE_SHA: a500d531b7a02c575a593a1cbad37b93dcf8cb0a
HEAD_SHA: 0de2704f5030102291ea492baeda89a3eb183818
TREE_SHA: 498c2d9564cc34358b5a76b39422b5d74fe18017
BRANCH: feat/mvp-024-resource-enforcement
ACTIONABLE_COUNT: 0
REVIEW_KIND: independent, read-only, conventional whole-range source review
RANGE: origin/main...HEAD

No introduced actionable regression was found. This verdict applies to the controlled storage drain/reopen leaf and its stated contracts, including both repair rounds. It does not close #24 or qualify its remaining resource enforcement/stress acceptance. The earlier R1 FAIL is retained unchanged; this is a separate review of the successor.

## Scope and instructions

The merge base and origin/main both resolve to BASE_SHA. The entire four-commit range was assessed: d6552b9 (final #25 evidence/TODO), e814024 (controlled reopen), 622fc8a (blocking-open and first-error repairs), and 0de2704 (owner-first error repair). All 95 changed paths match the supplied scope exactly. All 95 SHA-256 pins matched both before testing and at postflight. Review covered every substantive source/test change, connected callers and ownership paths, changed documentation, and the four archives. It was not restricted to the final helper or inferred from green tests.

Live filesystem inspection reconstructed the instruction census from touched directories and ancestors under both aliases:

- /mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp025
- /ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp025

The latter resolves to the former. All 92 AGENTS.md/CLAUDE.md candidates were absent, including symlink checks. The census covers each root's parents through /, docs, docs/benchmarks, docs/benchmarks/results, all four changed archive directories, docs/plan, src, src/daemon, src/storage, src/storage/crash_tests, tests and tests/support. It exactly matches the supplied instruction list. There were no present instruction files or symlink targets to read. An initial read-only census calculation omitted the explicit /ssd alias and failed its equality assertion; the corrected independent enumeration included both aliases and passed before tests. This was reviewer bookkeeping, not a product failure.

The native-code-review and ponytail-review skills supplied source-review criteria. The explicit user brief controls over generic skill suggestions for external reviewer transports, new probes, delegation, canonical reruns or publication. No .env files or private configuration values were inspected. Archived sanitized evidence was read without reopening private source data.

## Source assessment and R1 follow-through

### Admission, running work and queued identity

`src/daemon/resources.rs:129` compares the actually opened storage budget with the current active snapshot. `src/daemon/jobs.rs:1047` serializes submission with reload through the admission mutex. The existing-job fingerprint check and return precede the added budget-mismatch refusal at line 1113. Thus an already admitted matching submission retains its job identity during job drain; a new identity is refused. During the later close/open interval the documented connection-level busy response applies to all new peers, which must retry.

`claim` at `src/daemon/jobs.rs:1442` also holds admission and checks the current budget mismatch, pause/concurrency and stopping state before changing queued work to running. Queue expiry/cancellation retains its established treatment. Budget drain does not itself set stopping while jobs are still active: the scheduler at line 1693 stops new claims/notification dispatch and waits for its running and notification JoinSets to empty. Consequently normal budget changes do not cancel running model/retrieval work or reset its workflow clock. `execute_job` reconstructs the original execution deadline/budget, and queued recovery validates the archived generation and digest without replacing its accepted plan or queue deadline.

The new maintained running/queued regression checks idempotent return, admission refusal, provider request count, original generation/digest, running start/deadline fields and queued execution budget/queue deadline. The existing deadline/recovery/claim tests also passed freshly. These are functional fixtures, not a measured memory-admission claim.

### Drain ownership, same socket and blocking open

`run` at `src/daemon.rs:662` retains the listener and SocketOwner across `serve`. The generation loop does not rebind the socket or spawn a replacement daemon. `serve` at line 752 retains signal receivers, collects accepted UDS tasks and the scheduler, and then invokes `close_recording`. Scheduler ownership includes running jobs and notification tasks. `Service::operation` at line 143 owns disconnected request work; `close_recording` at line 168 drains those owners before taking the search index and closing storage. No new data-serving connection enters during this transition.

After successful closure the transition snapshots the latest active configuration and last input/readback, drops the old Service, and only then opens the next one (`src/daemon.rs:849-875`). The index owner's Arc is removed after jobs/operations have settled. Storage closure serializes with its writer, and the native close handshake completes before the old service is dropped. The listener's busy branch neither dispatches commands nor obtains a writer.

R1's blocking initialization finding is repaired at `src/daemon.rs:866`: the whole `open_service` operation, including synchronous native initialization and filesystem work, runs in an awaited `spawn_blocking` closure with the existing runtime handle. It is not merely spawned onto the current-thread async executor. The CLI's current-thread runtime is confirmed at `src/main.rs:955`. The transition remains held and collected even after an accept error; no timeout or select cancellation detaches the opening worker.

`reject_busy` at `src/daemon.rs:910` verifies same UID and bounds the hello write at 100 ms. The transition select continues polling the listener while initialization is pending. The native-open regression (`src/daemon/jobs.rs:118`) holds the real initialization handoff and uses an independent thread/current-thread client runtime to require RESOURCE_BUSY within two seconds before releasing it. It passed freshly, as did the real-daemon held-connection regression. These checks establish fixture responsiveness, not a production latency SLO under arbitrary load.

The gate is test-only twice: `src/storage/mod.rs:5` guards the crash_tests module, and `src/storage/native.rs:62` guards the checkpoint call. `src/storage/crash_tests/native_open.rs` keys its map by the canonical fixture authority directory, releases the map mutex before blocking, removes each hold on use, and unregisters/releases on Gate drop. Unrelated missing/unlinked paths return without creating a hold. It does not read a production environment flag or change a runtime-wide setting. The production dependency set is unchanged.

### First error and setup/accept cleanup

The shared first-error latch is initialized with the normal serve outcome (`src/daemon.rs:822`). A scheduler error already collected at the generation boundary retains precedence. The transition records a subsequently collected scheduler error before beginning service close. `close_recording` records each collected owner failure immediately with `get_or_insert`, before awaiting the next owner or storage close; storage-close errors use the same latch. This fixes the additional owner-before-accept hole rather than storing the owner error only after all cleanup returns.

An accept failure during transition records into that same latch and still awaits the entire transition. The accepting-error branch at `src/daemon.rs:893` uses the retained error before considering the next result, so a later transition failure cannot replace it. If opening succeeded while the accept failure was observed, the acquired next service is explicitly closed before return. The biased transition branch lets already-ready ownership completion be processed before a later accept result. Ordinary `close(outcome)` callers use a fresh latch seeded by their primary error and retain their established first-error behavior.

The maintained test at `src/daemon/jobs.rs:193` executes accept-first, scheduler-first and owner-first orderings against real `serve`/close paths, checks the distinct returned codes, verifies the owner set is empty, and proves storage is no longer writable. Its controlled task panics are expected inputs, not ignored failures. All three orderings passed in the fresh library command. Historical RED logs demonstrate the original two failures and the later owner-first failure separately.

Setup was traced beyond that regression: `open_service` closes acquired storage on configuration-manager and clock errors (`src/daemon.rs:682-697`), and closes the constructed Service on initialization/recovery errors (line 746). Signal-registration errors also close the acquired service. Storage's pre-existing open paths close a constructed store if schema/admission validation fails. No later cleanup error replaces the supplied setup error in those paths. Accept failure after successful open has its own explicit close branch. No new injected setup/dual-I/O-failure test was added; these paths were source-assessed rather than represented as newly executed fault cases.

### Whole configuration, repeated requests and shutdown

Explicit reload and the two-observation file watcher both enter `Service::reload` (`src/daemon/configuration.rs:138`, `src/daemon.rs:793`). Reload owns its operation after client disconnect, holds admission, persists the whole checkpoint in `reload_committing`, then publishes the active snapshot and records the input digest/readback. The added wakeup lets the scheduler notice a changed target promptly.

A changed path leaves the entire candidate pending; rejected input retains active/pending state and appends rejected history. The transition obtains configuration only after connections/operations finish, so later successfully applied budgets replace earlier requests. A reverted budget restores ordinary admission while work is still draining. Reopen passes the active resolved snapshot plus `last_config`; `open_service` skips startup reparsing for that controlled reopen. `ConfigManager::restarted` recognizes the unchanged active snapshot and retains the pending/history checkpoint. Invalid current file bytes therefore neither terminate a valid reopen nor partially activate pending paths. The watcher, repeated/reverted, and pending/rejected/readback tests passed freshly.

Stop clears `reopen_ready`, sets stopping and notifies (`src/daemon.rs:244`). An accepted connection can deliver Stop while old connections drain; those tasks finish before the final reopen-ready check. SIGTERM/SIGINT receivers are created outside the generation loop and retained through close/open. The biased check at `src/daemon.rs:769` consumes a queued signal before a new scheduler starts. Thus an accepted shutdown is not reset by constructing a new Service. Fresh tests cover Stop and both signals during running-job drain; signals arriving specifically while native open is held were traced in source, not newly injected. A pending open/close remains owned and awaited; shutdown is terminal, not an assertion of immediate cancellation of native work.

### Retrieval generation contract

The full changed `tests/support/retrieval_lifecycle.rs` was assessed with `src/daemon/retrieval.rs`, not just its new busy assertion. Builders take `index_builder`; queries retain their Arc index and admitted vector/config generation. Ordinary unchanged-budget builds still serve old-generation queries while the new embedding is held. The revised test first proves that behavior, then changes the budget and expects refusal at admission instead of admitting a doomed cache-busy job. It waits for budget reopen before the later successful query. The one-slot control still verifies a queued query uses its admitted model/generation after a switch. Both exact requested selectors passed freshly; no broader retrieval refusal was introduced.

## Actionable and pre-existing findings

ACTIONABLE_COUNT is zero. There are no introduced regressions requiring a repair in this review.

No separate pre-existing source defect was established in the inspected seams. Native opening remains synchronously blocking internally as before; the candidate now isolates that work from the transition listener executor. It is not a new finding against the unchanged storage API. Historical CI42949516550 remains an unresolved failure with unchanged owning source, not a proven pre-existing product bug or a proven candidate regression. Its cause is UNCLASSIFIED.

## Archive and documentation integrity

All archive directory memberships match their manifests exactly, excluding each manifest itself. All 77 member hashes were verified before and after tests; the #25 archive's 28 byte lengths also match. All 25 gzip members decompress successfully in memory. All 43 source pins were checked against the appropriate Git blobs, and current affected pins also match the frozen checkout through the scope hashes.

| Archive | Members | Source pins | Identity |
| --- | ---: | ---: | --- |
| mvp024-storage-reopen | 15 | 7 | HISTORICAL e81402448411b19cf392c9f6f5014a5d6e92d8e7 |
| mvp024-storage-reopen-review-fixes | 19 | 10 | HISTORICAL 622fc8a3ddd8484ee24359e315aeb1802429f2ae |
| mvp024-storage-reopen-owner-order | 15 | 10 | CURRENT frozen HEAD affected source/test bytes |
| mvp025-snapshot-delta-final | 28 | 16 | HISTORICAL 014e15d6cc1c3119d6b16ab08d79a4b028e41249 |

All per-attempt uncompressed log hashes match; available terminal/CI log hashes were also reconciled to decompressed members. Admissions preceding formatting remain distinct from final source pins and committed canonical receipts.

The original archive retains missing-draining PRODUCT_RED (0 pass/1 fail), original-job GREEN (1 pass), default-512-to-512 HARNESS failure (1 pass/1 fail), transition PRODUCT_RED (0 pass/2 fail), transition GREEN (2 passes), Broken-pipe teardown HARNESS failure (16 pass/1 fail, later commands unreached), and final controls (28 passes). Neither harness failure is promoted to causal product evidence.

The review-fix archive retains two PRODUCT_RED failures, the two-regression GREEN including the scheduler control, and its 111-pass/11-ignore broader focused run. The owner-order archive retains the distinct owner-first PRODUCT_RED (0 pass/1 fail) and final 30-pass run. These are historical executions; the fresh results below are separate.

The complete R1 report is byte-identical to the immutable supplied report, still begins/ends VERDICT: FAIL and still has two actionable findings. Its report SHA-256 and archived public-body readback SHA-256 are both `015ab727587cab0e00340966c0332c67af95a5538ae1f4db2a7c6cf9796ad5d1`; its decompressed test log is `ec8b59e0782c9b47903bb0395c7080e39d2ac43da18b57777606f6fd8c78c494`. The historical #25 full report matches `e4d8fe349cfdbbf2d8498d844edd01d668711c45c46afa90d5ae680ac674c186`, and its decompressed tests match `7f5fb301c43a21b05822370decfebc8b257b3745defc2d45eb306d0e311a059e`. Publication records retain those identities; no new GitHub readback or post was performed.

The #25 source pins were checked against 014e15d, not changed current daemon files. Archived merge/publication/acceptance records support the README/TODO distinction between structural changed-snapshot fixtures and six unchanged-real-snapshot units. Counts of 27,511 commits and 1,687,022 revisions, unchanged input/output assertions, cleanup's scoped-process limits, and the historical 599-pass canonical are retained as historical evidence. This review did not repeat migration acceptance or inspect private records. #25 completion is not represented as completion of #24, packaging, quality evaluation, final release or Claude authentication.

For CI42949516550, the retained log explicitly contains HistoryGap at tests/support/codex_caller.rs:72 in the initial bind. All nine preflight base/candidate source/dependency hash pairs were independently checked. The exact local selector and historical local canonical passed, but neither proves the remote cause. Unreached CI sections remain: tests after the Codex binary (including config/daemon/history/Hermes/Pi/providers/storage/doc tests), release build, capture-command checks, installed daemon repetition and UID controls, release help/version and final diff check. No CI was launched or retried. Current hosted CI is not claimed by this report.

Configuration/daemon/memory-resource documentation accurately separates requested/open budgets, draining, unchanged PID/socket, retry identity, pinned jobs and pending/rejected state from absent measured-RSS admission and dedicated cgroups. The TODO leaves #24 stress/enforcement and final gate/review/merge work explicit. Its pending-gate wording is a frozen pre-review checkpoint; the supplied current external gate is assessed below, not silently substituted for older evidence.

## Exact-source canonical prerequisite

The supplied mvp024-reopen-canonical-r3-verified.json binds this exact clean BASE/HEAD/TREE. Its four referenced artifacts (admission, terminal, collection and log) all match their SHA-256 values. The terminal records exit 0 for `just check`. Recounting the saved log yields 21 Rust result rows, 613 passes and 35 intentional ignores including installed repetition. The verified receipt separately records 34 Python tests, 7 Node tests, 165 daemon passes per run plus 7 daemon opt-in ignores, and installed lifecycle/UID controls. Log SHA-256: `ff7b60d7e1529803c74b3a20e207376a80174daf26335c747f9f6a4c50c69855`.

This prerequisite is PASS. It was not rerun. Historical e814024 canonical/CI and 622fc8a canonical/failed CI remain distinct. Installation/live acceptance is credited only to the supplied receipt, not presented as a fresh action by this reviewer.

## Fresh commands and terminal results

Each required command ran once, sequentially, using `/bin/bash` with login initialization disabled and Python subprocess argv. The owning wrapper was collected with terminal exit 0; each child exit is present in the fresh log. No additional test selectors, new probes, files, worktrees or targets were used.

| Exact command | Exit | Passed | Failed | Ignored | Filtered |
| --- | ---: | ---: | ---: | ---: | ---: |
| `cargo test --locked --offline --test daemon budget_` | 0 | 17 | 0 | 0 | 155 |
| `cargo test --locked --offline --lib daemon::jobs::tests` | 0 | 11 | 0 | 0 | 92 |
| `cargo test --locked --offline --test daemon old_generation_serves_queries_while_new_embedding_is_held` | 0 | 1 | 0 | 0 | 171 |
| `cargo test --locked --offline --test daemon queued_search_keeps_its_admitted_model_after_generation_switch` | 0 | 1 | 0 | 0 | 171 |

Total: 30 passed, 0 failed, 0 ignored. The budget substring selects 17 maintained tests, including adjacent budget tests; it is not a claim of 17 distinct reopen scenarios. The library's single error-ordering test contains three orderings.

Fresh `rustc --version`, `cargo --version`, and `git rev-parse HEAD HEAD^{tree}` also exited 0 in the log. `git diff --check origin/main...HEAD` separately exited 0. Hash/census/archive attestation scripts were read-only apart from the declared log append; corrected final attestations exited 0.

Inherited environment was preserved:

- rustc 1.99.1 (58cf8f9c3 2026-10-13); cargo 1.99.1 (5f94df478 2026-08-27), from the inherited Rust 1.99.1 toolchain path.
- RUSTUP_TOOLCHAIN=1.99.1
- CARGO_TARGET_DIR=/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/target
- RUSTC_WRAPPER=/mnt/ssd/mirror-rootfs/home/obj/.local/share/mise-rebuilt/installs/sccache/0.19.0/sccache-v0.19.0-x86_64-unknown-linux-musl/sccache
- TMPDIR=/mnt/ssd/mirror-rootfs/home/obj/tmp/a15-cache-s0s9a8n9

TEST_LOG: /home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/akasha-evidence/mvp024-reopen-review-r2-tests.log
TEST_LOG_SHA256: 7bfc6beaa44ee7cbbe200d0c4a6188f8604ac47ab24f00350888ef3f2a939287

## Post-test custody and limits

Postflight at 2026-11-11T03:32:24.803388+00:00 re-attested the exact BASE/HEAD/TREE/branch above, empty tracked/untracked status, all 95 scope hashes, all archive/source identities and all 92 absent instruction paths. The target symlink remains exactly `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/target`. Both index locks were absent:

- /home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/worktrees/akasha-otsuge-mvp025/index.lock
- /home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/index.lock

All owned test/wrapper processes reached terminal exit. A readable same-UID executable census found zero processes under the shared akasha-otsuge target; zero inspected same-UID executable entries were inaccessible or raced. This supports no remaining test/daemon fixtures from these commands, not a global claim about unrelated processes. No service was operated outside the maintained fixtures.

Only the declared report and test log were intentionally written, apart from ordinary artifacts of the explicitly authorized Cargo/fixture execution in inherited target/TMPDIR. Product files, index/history, hooks, configuration and existing evidence were not edited; no caches were manually changed. Parent verification/publication remains separate.

Not checked anew: full canonical/CI, bulk corpus, private migration acceptance, live services, SSH, external model/provider calls, Daybreak or new adversarial campaigns. No delegation was used.
Remaining limits: measured-RSS admission and dedicated cgroups are absent; stress acceptance remains open, and the historical CI failure cause remains UNCLASSIFIED.

VERDICT: PASS
