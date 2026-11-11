VERDICT: FAIL

# Independent conventional review: controlled storage budget reopen

BASE_SHA: a500d531b7a02c575a593a1cbad37b93dcf8cb0a
HEAD_SHA: e81402448411b19cf392c9f6f5014a5d6e92d8e7
TREE_SHA: 4cb277a253704743fb4727f202dda23c874c5052
BRANCH: feat/mvp-024-resource-enforcement
ACTIONABLE_COUNT: 2
REVIEW_KIND: independent read-only conventional source review

The full supplied base..HEAD range was reviewed, including d6552b9's #25 evidence/TODO documentation and the #24 implementation, tests, documentation and causal archive. The two findings below are source-level contract gaps in the new transition. All fresh maintained tests passed. The supplied exact-source canonical gate independently remains PASS; it does not resolve these findings.

## Actionable findings

### R1 — P2 / moderate: synchronous storage opening prevents bounded transition busy replies

Origin: candidate integration regression / stated leaf contract gap. Primary frozen location: `src/daemon.rs:836` (opening the next service inside the transition future); related `src/daemon.rs:844`, `src/daemon.rs:675`, `src/storage/mod.rs:294`, `src/storage/mod.rs:317`, `src/storage/mod.rs:363`, `src/storage/native.rs:62`, and `src/main.rs:955`.

Ordinary use case: an operator changes the budget on an existing store while disk I/O is slow or native database initialization takes appreciable time. Another same-UID client asks for status during that opening interval. The client must promptly receive RESOURCE_BUSY, but can instead wait for the entire native open and eventually hit its transport timeout.

Source evidence: the CLI constructs a `new_current_thread` runtime. The transition is polled directly by `serve`'s select, and calls `open_service(...).await`; that calls `Storage::open`. Despite its async signature, the opening path synchronously performs directory synchronization and invokes `open_tree` / `PinnedTree::open`. The latter starts a native thread but then synchronously blocks the caller in `std::sync::mpsc::Receiver::recv()` until `TreeBuilder::build()` completes. While that recv blocks the daemon's only async executor thread, neither the accept branch nor `reject_busy` can run. The 100 ms timeout in `reject_busy` bounds a write only after acceptance; it does not bound this wait. Merely placing both futures in `select!`, or spawning the open on the same current-thread runtime, does not fix it.

The synchronous storage helper predates this range. The candidate newly reuses it while promising that the existing listener remains responsive throughout close/open. Thus the finding is about this leaf's integration and explicit busy-response contract, not a requirement for the remaining #24 corpus/stress work. The maintained busy test holds an old connection and proves responsiveness during that asynchronous drain; it does not hold native initialization and therefore does not cover this interval. No latency measurement or new fault probe was run in this review.

Minimal repair: keep the listener executor free while the owned transition performs blocking storage initialization and associated filesystem work, using a collected blocking worker or a genuinely asynchronous initialization handshake. Preserve ordered old-storage closure/drop, ownership of the opening result, and cleanup on every error; do not detach opening work or introduce another data-serving writer. Add a maintained deterministic regression that holds the real initialization boundary and checks a same-UID busy hello before releasing it.

### R2 — P2 / moderate: a later transition failure replaces an earlier accept failure

Origin: candidate regression. Primary frozen location: `src/daemon.rs:847`, with the error discarded by `src/daemon.rs:851` before `src/daemon.rs:852` can process it.

Ordinary use case: during a budget transition, resource exhaustion makes listener acceptance fail. The still-owned close/open work subsequently also fails (for example, descriptor exhaustion also prevents the next storage open). The daemon should finish cleanup and return the first accepted failure. It instead returns the later transition error, obscuring the original cause.

Source evidence: the transition accept-error arm constructs `(transition.await, Err(error.into()))`. If the awaited transition returns Err, `let next = next?` immediately returns that later error. The already observed accept error is never considered. This differs from the established `outcome.and(drained)` and `outcome.and(closed)` paths, which retain the original error while still performing cleanup. The defect does not depend on concurrent threads: an accept failure can be observed while the transition is suspended draining a connection or awaiting storage close, followed by a later close/open error.

At base there was no transition accept loop; a normal accept failure became the outcome passed through scheduler and storage drain. Existing maintained tests cover ordinary accept failure, scheduler failure and first-error preservation during drain, but not an accept failure followed by a second failure inside this new transition. This finding is source-derived; no injected double-failure execution is claimed.

Minimal repair: retain the first observed failure as shared transition outcome and collect the remaining owned work without allowing a subsequent error to overwrite it. Keep any failure already recorded before the transition ahead of the later accept error as well. Close a successfully acquired next service before returning. Extend the maintained lifecycle test with distinguishable transition-accept and subsequent close/open failures and assert the first error is returned after cleanup.

## Source assessment of the remaining seams

- Admission and job identity: `submit_job` serializes with reload using the admission mutex. Its existing-job fingerprint check and return occur before the added storage-budget refusal. New jobs are refused while the active target differs from the opened storage; `claim` checks the same condition after acquiring admission. The job-drain scheduler does not set stopping merely because the budget differs, allowing running jobs to retain their existing clocks. The new regression checks running and queued generation/digest/deadline fields and that no second provider request starts during drain.
- Queued work: the durable job record retains execution budget and queue deadline. Recovery validates the archived generation/digest and does not reset queued records. Claim uses live concurrency/pause constraints but initializes execution deadline from the admitted job's budget. Existing scheduler tests freshly cover original deadline, retry exhaustion and admission-lock behavior.
- Ownership and closure: the scheduler owns running jobs and notifications in JoinSets. The serve transition collects accepted connection tasks and, if necessary, the scheduler before `Service::close` collects disconnected operation owners, drops the search-index owner and closes storage. It extracts the latest active configuration/readback and explicitly drops the old Service before invoking the next open. The listener and SocketOwner remain in `run` across iterations, preserving socket custody and PID. Setup paths after acquired storage close it on manager/clock/initialization errors; an accept error with a successfully opened next service closes that service. R1 and R2 are the exceptions to the claimed responsiveness/error contract, not evidence of a new concurrent data path.
- Configuration: explicit reload and the stable two-observation watcher both call `Service::reload`. Durable `reload_committing` persists a complete checkpoint before publishing the active generation. A path-changing candidate remains wholly pending; invalid input contributes rejected history while retaining active and pending state. Reopen uses the latest active resolved snapshot, restores the checkpoint, and carries `last_config` across the transition, avoiding reparsing a rejected file as new startup input. `ConfigManager::restarted` recognizes the unchanged active snapshot and retains pending/history rather than activating paths. Repeated/reverted budgets use current active state; their maintained fixture passed.
- Shutdown: Stop clears reopen readiness, sets stopping and notifies. Accepted connection tasks finish before the final readiness check, so a Stop received on an already accepted connection can prevent reopen. Both Unix signal receivers live outside the generation loop; the biased pre-scheduler check consumes signals retained across close/open before starting the next scheduler. The production CLI uses a current-thread runtime; separate atomic statements were not misclassified as a demonstrated multithreaded Stop race. Fresh fixtures cover Stop, SIGTERM and SIGINT during running-job drain. Queued signals specifically during native open were source-traced, not freshly injected; R1 also explains delayed processing during blocking open, without claiming that the signal is erased.
- Retrieval: builders use `index_builder`, while readers retain Arc index ownership and admitted vector/config generations. Budget mismatch now refuses new job admission before a new search can enter that path. Ordinary builds with unchanged budget still allow old-generation queries. The changed test preserves its old-generation output/model assertions and separately checks admission-time busy only after a budget change. Both maintained generation controls passed freshly; no change to the retrieval implementation was needed to observe those results.
- Scope limits are explicit in configuration, daemon and memory-resource documentation: measured-RSS admission and managed cgroups remain false, and #24's full stress/enforcement checklist remains open. A functional reopen is not a claim that RSS reaches the configured target. No unrelated pre-existing behavior is promoted to an actionable finding; the pre-existing synchronous open is actionable only where this new leaf's promised responsiveness depends on it.

## Archive and documentation integrity

All 56 changed paths exactly match the supplied scope, and all 56 supplied SHA-256 values matched before and after tests. Both gzip archives were decompressed in memory to verify integrity without materialization or edits.

`mvp024-storage-reopen/manifest.json`: 15 members and all seven current implementation/test pins verified. All seven uncompressed attempt logs match their manifest log hashes. Receipt/log contents retain these classifications:

| Attempt | Classification retained |
| --- | --- |
| red-r1 | PRODUCT_RED: missing draining state; 0 passed, 1 failed |
| green-r1 | PASS: 1 running/queued preservation test |
| transition-red-r1 | HARNESS: 512-to-512 failed to cause a transition; 1 pass, 1 failure is not product RED |
| transition-red-r2 | PRODUCT_RED: delayed/non-busy response and rejected-config reopen failure; 0 passed, 2 failed |
| transition-green-r1 | PASS: 2 transition regressions |
| controls-r1 | HARNESS teardown Broken pipe after stopped daemon; 16 passed, 1 failed, later commands not reached |
| controls-r2 | PASS: 17 budget matches + 2 generation controls + 9 scheduler tests, Clippy/plan/diff checks |

Pre-format admission digests in development receipts are not substituted for final source identity. The manifest's seven pins and the separate committed exact-source gate supply that identity.

`mvp025-snapshot-delta-final/manifest.json`: all 28 member hashes and byte lengths verified, plus 16 source pins against historical commit `014e15d6cc1c3119d6b16ab08d79a4b028e41249`, not current daemon files. Both that commit and base a500d53 resolve to tree `46ac6632cef510642c4653055aa2da6cf2446802`. The decompressed historical review report and tests log match their verified receipt hashes. Archived terminal/publication/merge/CI/acceptance records were checked for the documentation's historical identities and distinctions; these are archived receipts, not a new live GitHub readback.

The #25 README/TODO claims distinguish the completed structural changed-snapshot fixture from six unchanged-real-snapshot units, retain the 599-pass historical canonical result separately from current results, and preserve cleanup's scoped-process limitations and historical unknowns. The change does not present that migration acceptance as #24 stress completion. No private data or prior acceptance was reopened or repeated. The TODO's pending canonical/review/merge wording is a frozen pre-review checkpoint, not a claim that this report or the supplied external gate is absent now.

## Canonical prerequisite

The supplied `mvp024-reopen-canonical-r1-verified.json` binds this exact base/HEAD/tree and clean state. All four referenced artifacts matched their recorded hashes, and the terminal receipt reports exit 0. Recounting the retained log gives 21 Rust result rows, 611 passes and 35 intentional ignores (including installed repetition); the verified receipt records 34 Python tests and installed lifecycle/UID checks. The log SHA-256 is `fd0a85931bdc19c508fbb3572429fd580f5d05851e58f6d31485584a4e6a55ce`. The canonical gate was not rerun, and its installation evidence was not represented as a fresh live-runtime check by this reviewer.

## Fresh validation

Commands ran once each, sequentially, from the frozen checkout using `/bin/bash` with login initialization disabled and Python subprocess argv (no zsh/mise initialization). Both owning wrapper processes were collected with terminal exit 0. Each Cargo exit is explicitly recorded in the declared test log.

| Exact command | Exit | Passed | Failed / ignored | Filtered out |
| --- | --- | --- | --- | --- |
| `cargo test --locked --offline --test daemon budget_` | 0 | 17 | 0 / 0 | 155 |
| `cargo test --locked --offline --lib daemon::jobs::tests` | 0 | 9 | 0 / 0 | 92 |
| `cargo test --locked --offline --test daemon retrieval_tests::lifecycle::old_generation_serves_queries_while_new_embedding_is_held -- --exact` | 0 | 1 | 0 / 0 | 171 |
| `cargo test --locked --offline --test daemon retrieval_tests::lifecycle::queued_search_keeps_its_admitted_model_after_generation_switch -- --exact` | 0 | 1 | 0 / 0 | 171 |

Total: 28 passed, 0 failed, 0 ignored. The two additional existing selectors address the concrete changed retrieval-admission/old-generation seam, which the `budget_` name filter does not select. These passes do not exercise blocked native initialization or transition double-error precedence. `git diff --check a500d53..HEAD` also exited 0. No new tests, probes or fixture files were added.

Inherited toolchain: rustc `1.99.1 (58cf8f9c3 2026-10-13)`, cargo `1.99.1 (5f94df478 2026-08-27)`, resolved from `/home/obj/.rustup/toolchains/1.99.1-x86_64-unknown-linux-gnu/bin/`.
`RUSTUP_TOOLCHAIN=1.99.1`.
`CARGO_TARGET_DIR=/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/target`.
`RUSTC_WRAPPER=/mnt/ssd/mirror-rootfs/home/obj/.local/share/mise-rebuilt/installs/sccache/0.19.0/sccache-v0.19.0-x86_64-unknown-linux-musl/sccache`.
`TMPDIR=/mnt/ssd/mirror-rootfs/home/obj/tmp/a15-cache-s0s9a8n9`.
The lexical target symlink remained the supplied `/ssd/.../akasha-otsuge/target`; no new target was created.

TEST_LOG: /home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/akasha-evidence/mvp024-reopen-review-r1-tests.log
TEST_LOG_SHA256: ec8b59e0782c9b47903bb0395c7080e39d2ac43da18b57777606f6fd8c78c494

## Instructions and final custody

Live filesystem checks, rather than Git absence, independently reconstructed the parent/touched-directory instruction census and matched all 76 declared AGENTS.md/CLAUDE.md paths. All were absent, so there were no present instruction files or symlink targets to read. Checks covered `/` and every ancestor and touched subdirectory under both `/mnt/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp025` and `/ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge-mvp025`, including docs/benchmarks/results and both archive directories, docs/plan, src/daemon and tests/support. The supplied user scope overrides broader skill instructions about external review transports, new probes, canonical reruns or publication.

Post-test attestation at 2026-11-11T02:35:23.435332+00:00 confirmed the exact HEAD/tree/branch above, empty tracked/untracked status, all 56 scope hashes, all 43 archive member hashes and 23 current/historical source pins. Both worktree and common index locks were absent:

- `/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/worktrees/akasha-otsuge-mvp025/index.lock`
- `/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.git/index.lock`

All owned Cargo/wrapper processes completed. A post-test same-UID executable census found no matching daemon/library test executables or fixture daemon processes under the shared target; no examined same-UID proc entries were inaccessible or raced away. No global unrelated-process ownership claim is made. Only the declared report and fresh test log were intentionally written, apart from the ordinary artifacts of the explicitly authorized maintained tests in the inherited target/TMPDIR. Product files, Git history/index/hooks, configuration and existing evidence were not edited.

No Daybreak, new adversarial campaign, delegation, SSH, live-service operation, private config/.env inspection, external model/provider probe, bulk corpus acceptance, new worktree, or GitHub publication was performed. Clippy/full canonical validation is credited only to the verified supplied gate. Parent verification/publication remains separate. The remaining risk in this leaf is delayed busy responses during blocking native initialization and loss of the first error on the transition accept-failure path.

VERDICT: FAIL
