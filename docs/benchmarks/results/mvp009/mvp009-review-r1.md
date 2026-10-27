VERDICT: FAIL
ACTIONABLE_COUNT: 1

MVP-009 independent whole-candidate source review, R1. One new deadline regression requires correction. The supplied exact-candidate gate passes, but its tests do not exercise the admission wait that renews the model workflow budget. This report does not claim that the separately running manual acceptances passed.

Repository: `/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge`
Branch: `feat/mvp-009-providers`
HEAD: `69853f1001aeea840982188c791de05f36ff5952`
Tree: `82849e48fb9e2fb390cfdfa115386cdc5910e3ce`
Base / origin/main / merge-base: `a3d278d9f3968d8ce6eacea79d946b0ed880e2ad`
Authoritative scope: the complete `base...HEAD` range, 16 changed paths, 2,226 additions and 65 deletions.

1. **[P2] Preserve the original workflow deadline across configuration restoration.**

   **Origin/location:** introduced by this range at `src/daemon/models.rs:134` (the new `Deadline::start` at lines 134–138), called by `src/daemon/jobs.rs:876`. This is a model-job regression; the existing capture branch retains its pre-wait monotonic end.

   **Concrete failure path:** claim persists the original start and `execution_deadline_ms` at `src/daemon/jobs.rs:853`–858. The executor converts that deadline to `remaining` and an immutable monotonic `end` at lines 873–874, but passes only the duration to `execute_model` at 875–876. Before starting its clock, the new model executor awaits the shared admission mutex at `src/daemon/models.rs:120` and restores the archived generation. It then starts a fresh workflow deadline using the stale duration at 134–138. A reload or capture operation queued behind the first executor lock can acquire this fair mutex before the second acquisition and consume some or all of that duration: reload holds it through its awaited physical persistence (`src/daemon/configuration.rs:141`–164), and capture holds it through its physical batch (`src/daemon/jobs.rs:879`, 919–931).

   For example, if 500 ms remained at jobs.rs:873 and the following restoration/admission wait takes 600 ms, models.rs:134 grants another 500 ms after the original durable deadline has already expired. Provider calls use that renewed workflow as their parent (`src/provider.rs:309`–313), and terminal arbitration tests the same renewed clock (`src/daemon/models.rs:190`–196). A prompt response can therefore produce a succeeded job after its unchanged persisted execution deadline. The `remaining.max(1 ms)` expression also grants positive execution time when the remainder was already zero. The workflow trace describes the newly started remainder rather than the original configured job budget/start.

   **Contract/evidence:** this contradicts the original-budget, monotonic workflow and non-renewal semantics in `docs/spec/04.md:81`, 107, 115, 120 and 131, and the pinned-budget claim at `docs/providers.md:64`–73. This conclusion is from the immutable control flow and concrete lock ownership above; no executable reproduction was run or claimed. The existing virtual-clock test starts a Deadline directly around Client calls (`tests/provider.rs:92`–105), so it does not pass through this second daemon admission acquisition. The passing capture final-batch regression checks the other branch's preserved `end`.

   **Bounded correction:** carry one original monotonic workflow deadline/end through restoration, stage admission, physical requests and terminal result arbitration; do not restart it from a previously sampled duration, and do not turn an exhausted remainder into a positive timeout. Preserve the original configured budget/start separately in the trace. Expired work must fail with the appropriate workflow timeout before a physical send.

   **Regression:** add an ordinary daemon-executor test with a deterministic admission barrier after the original remainder/end is established. Let a competing owner consume the remainder, then release restoration; assert no provider send, a durable workflow-timeout terminal result, and unchanged original deadline/configuration. Include a before-expiry success control and an already-expired case. Use the actual `execute_job -> execute_model` path so a direct Client-only test cannot miss this boundary.

No other actionable regression was found in the reviewed candidate. Pending manual acceptance is recorded as an evidence limit, not fabricated as an additional source defect.

Whole-candidate source assessment:

- Configured profile kind, endpoint, exact model string and dimension drive the request and identity hash from the same restored snapshot. The text, embedding and rerank routes use independent configured profiles. There is no production model-name alias rewrite or hard-coded GB10 identity; the fixed GB10 strings occur only in its explicit manual fixture. Identity proves the selected route/model configuration, not independent attestation of a remote engine.
- The complete accepted plan/generation/digest are persisted. Restoration reads the archived generation and validates its digest; ordinary reload affects new jobs. Existing capture-job serialization and idempotency remain compatible, while the model fingerprint has a distinct discriminator. Running external model jobs become PROVIDER_INTERRUPTED on restart, and ambiguous requests are not replayed. Queued model jobs retain the archived plan.
- Model intake/cancellation reuse daemon-owned mutation operations, the active index and scheduler. The shared Client bounds physical calls to four, with a separate admission clock and 100 ms start spacing. Wakeups reach cancellation waiters as well as the scheduler. The cancellation monitor registers before checking durable cancel/stopping state; the owning job task awaits the request outcome and result publication. Existing shutdown error/drain ownership remains in place. Dropping the HTTP future is correctly documented as not proving remote computation stopped.
- Plan/call/batch/body/response limits are enforced before unbounded processing. Text completion/truncation, unique embedding indexes/count/dimension/finite values, and unique reranker indexes/finite raw scores are checked. Errors are fixed classifications; credential values and arbitrary upstream errors are not copied into diagnostics. TLS verification is not disabled; environment proxies, redirects and transport retries are explicitly disabled. Pinned local reqwest source confirms that its default total and read timeouts are absent; the configured 10-second connect timeout is a separate connection boundary.
- Results use immutable content-addressed Evidence chunks, exact revision/hash/aggregate checks, and a bounded aggregate that fits the IPC message ceiling. Publication occurs before the job points to the archive. Job reads apply current permission checks before loading result chunks. Stage permission checks and live job admission retain their documented scope; future host/provider authentication and retrieval integration are not claimed here.
- The CLI reads a bounded typed plan and submits a durable job through the existing private IPC protocol. The 10-second CLI exchange does not own the provider's lifetime. Canonical and installed harnesses use the actual configured acceptance executable. The manual harnesses have exclusive receipt directories, durable progress/result files and owned daemon/endpoint cleanup; their source was reviewed without running them.

Coverage of every changed path:

| Path | Review coverage | Frozen full-file SHA-256 |
| --- | --- | --- |
| `Cargo.lock` | Complete base-relative dependency delta; locked reqwest/rustls/hyper additions and existing native Storage pin. | `c7fad7a2882009ef06ad38497d1f0fb54e9cc3d9da7166fccf1960757a9e88ce` |
| `Cargo.toml` | Complete manifest; exact reqwest 0.12.28 pin, disabled default features, JSON/rustls selection, unchanged runtime and Storage ownership. | `4d64eb39db2e958177aade3e00fda431f56e906f1958667a31815fa2b441ee58` |
| `README.md` | Entire changed description and its explicit pending real-service/long-duration qualification. | `74ad19c6ad10ef23ca294abe8bd0a2bddafd9075c00eefed96d025d1a22d1887` |
| `docs/plan/progress.md` | Complete candidate diff and surrounding acceptance bookkeeping; prior #8 completion kept separate from #9. | `74de0dff2434e45f06bc4198054a9915fb208cd8c12732a718d69e0bd5a96cd9` |
| `docs/plan/todo.md` | Complete candidate diff and remaining #9 acceptance checklist; no completed #9 claim inferred. | `11ab3faaf46bf4d8e42c1d0cf002ff3b85158ab9a2d48a3cfabdc1810593e6cb` |
| `docs/providers.md` | Complete contract, examples, bounds, identity, clocks, cancellation/restart semantics and manual acceptance instructions. | `8bb85976c0e921ca07b097033022ab8841fe7d7fad9a76890ad35dcf140d43f5` |
| `src/daemon.rs` | Complete current daemon body plus diff; Client construction, model dispatch, broadcast wakeups, mutation owners, scheduler/connection drain and Storage close. | `fb864030386f4cf92e3619066969a304815b6d2352ba5b2457e3d9807da6678c` |
| `src/daemon/jobs.rs` | Complete production job paths and all candidate changes; existing test coverage reused from identical base spans, with new Client initialization/wake callers checked. | `41e892e3c36aa26b359934ab27445c20c9d6a14c8e203abf204cfdc2e06fd5e8` |
| `src/daemon/models.rs` | Complete new implementation; archived configuration restoration, cancellation monitor, stage authorization, result chunking/loading, terminal arbitration. | `8beef0c5ae7324b8d00c286739ef1678e59841d0486ccf223c94f82b70062f73` |
| `src/ipc.rs` | Complete current framing and command schema; new model_jobs capability/ModelRun route, transport bounds and client deadline separation. | `1b91c8b0c14b49a8fcecc9dc72d1046985c6ced491a766e846a3cdf2248cb8b1` |
| `src/lib.rs` | Complete module export change. | `d1778f5ddbd51f29aa859b8deff01cfad2eceba2ebac03eccf416bcd18fff638` |
| `src/main.rs` | Complete current CLI body; bounded plan read/parse, ModelRun routing, request identity, IPC and output/error paths. | `a14aeb8db138e378cb3432433175bd76de5c7f7b4243e83a5ae14be778be8b4c` |
| `src/provider.rs` | Complete new provider implementation and unit test; config-to-wire identity, admission, timeout/cancellation, HTTP defaults, all three output parsers. | `c4962da0c551eae6179677e92bb241520d392bda8df4de80cf1e203e3f252b3e` |
| `tests/daemon.rs` | Only added module registration differs from the previously reviewed base; inherited daemon process/fixture cleanup and installed-binary selection reused after identity checks. | `5e770eae46642d0e9f7b71c45a5c8eea43a35509b020d5a7b6a9585f64ca67b3` |
| `tests/provider.rs` | Complete virtual-clock endpoint and test; direct Client 1699/1700/three-stage assertions and their daemon-boundary limitation. | `0c60c2186dfa3026a9433f868c5c17158a05a760ad611dc5378d2515dcd0d265` |
| `tests/support/daemon_models.rs` | Complete new fixtures/tests, including ordinary negative routes, result/reopen and reload controls, cancel/crash, short wall-clock control, and the two ignored manual harnesses. | `1f56865d231ec0efa607359343f8c3c262474e353ca270f8afc07e2ce3be0370` |

The base tree is `9a2f2d9a84bf6b098ff31496114f56e100209267`, exactly the previously reviewed MVP-008 R2 tree at `7552d97ae4e33387e496cc0980103537dcdbeeb0`. I verified unchanged blob identities before reusing that review's coverage for inherited code. In particular, ConfigManager/schema/deadline, daemon configuration persistence, Storage, Domain history, the canonical recipe, and installed harness scripts are byte-identical. The prior report hash is `1e02a90844ba37952c1114cd6692d102d1b14a41e0ee554782cca4bdc28ef159`. The new provider/model code and all changed callers/tests/docs/dependency deltas were assessed for this complete range; this is not a last-commit-only review.

Supplied execution evidence, independently read and rehashed but not rerun:

| Evidence under .git/akasha-evidence | SHA-256 | Bytes |
| --- | --- | --- |
| `mvp009-gate1.log` | `443dabab5298b3addcb341ff1c3ba5edbd6234ed4351192b1ebdc80ee0da8f8b` | 16504 |
| `mvp009-gate1.terminal.json` | `10362dd49a60c1d524d6a809af553dd9bd6a4e42db77d5b4baded0c9fc0004f2` | 237 |
| `mvp009-gate1.collected.json` | `17c81192d7ce5db2993f26ee00b8b5352edf782697059af91629ed423f7d8abc` | 652 |

The terminal receipt records exit 0 on the exact requested HEAD in 105.26805703528225 seconds, completed at UTC `2026-10-27T16:49:23.542124+00:00`. The collected receipt binds the exact tree and reports the gate process absent. The complete log names 96 passing Rust tests (14 library, 9 capture, 5 CLI, 17 config, 15 daemon, 18 Domain, 1 provider, 17 Storage), eight Python tests, formatting/Clippy, release builds, capture executable acceptance and help/version. Two library entries are ignored child launchers exercised by their passing crash supervisors; two daemon entries are the manual GB10 and real-duration acceptances and remain excluded.

The installed release reruns all 15 nonignored daemon tests and reports binary SHA-256 `0596d1142606b83941b3e6d75568f49e8fd6c2e819f737496ac9f40119ccab3d`, private-path and peer denial with kernel UID 165536, and zero daemon-owned TCP sockets. These are supplied harness observations, not a reviewer invocation of the installed runtime. The unrelated mise audio-condenser cache warnings are present but do not replace any passing test/Clippy result.

At dispatch the parent-owned 1605-second success / 1700-second timeout run and actual GB10 three-service compatibility run were still pending. I did not inspect their live services, databases or later outcomes. Neither is credited as passing here, nor is the two-second harness control credited as near-1700-second acceptance. The documentation keeps these checks pending and `local_long_timeout_verified` false. No model-quality, future Runner, full-corpus, publication, merge or issue-completion claim follows from this review.

Instruction and scope custody:

Applied `/home/obj/.codex/skills/native-code-review/SKILL.md` and `/home/obj/.codex/skills/review-gate-discipline/SKILL.md`, including scope/regression attribution, lifecycle/physical-attempt ownership, provider identity, frozen-configuration restoration, receipt integrity and frozen-report finalization. The dispatch's source-only restrictions govern optional execution workflows. No additional agent or WD relay was created.

Actual filesystem lexistence checks found neither AGENTS.md nor CLAUDE.md at /, /home, /home/obj, /home/obj/project, /home/obj/project/github, /home/obj/project/github/RyderFreeman4Logos, or the repository root. Both names are also absent at the applicable repository directories .git, .git/akasha-evidence, docs, docs/plan, docs/spec, docs/spec/issues, scripts, src, src/capture, src/config, src/daemon, src/domain, src/storage, tests and tests/support. These are filesystem absence observations, including symlink entries, not a tracked-file-only search.

Source was read from immutable Git blobs. Lexical readlink checks preserve:
`target -> /ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/target`
`.spike-work -> /ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.spike-work`.
Neither artifact target was traversed. No full database, raw corpus, shard, archive or native-child transcript was read.

Entry custody at UTC `2026-10-27T17:00:12.585615+00:00`: exact branch/HEAD/tree/base/origin-main/merge-base above; full tracked/untracked porcelain status empty; index.lock absent; base type commit and ancestor check successful; base-relative diff-check exit 0; all 16 working-tree files matched the immutable reviewed bytes.

Exit source/evidence custody at UTC `2026-10-27T17:12:20.846211+00:00`, after the source/receipt review and before exclusive final report creation: the same branch, HEAD, tree, base/origin-main/merge-base, empty full porcelain status, absent index.lock, ancestor and diff-check results; all 16 reviewed files and both lexical links unchanged. All three gate artifact hashes match the earlier reads. The completed report is reread and hashed and custody checked once more after creation; that final observation and full-file report hash accompany the external completion message.

Verification and resource limits: this is a source-only review. I ran no tests, probes, builds, helper compilation, live service call, process instrumentation, background work, fetch/push, source/index/commit mutation or full gate. Every ordinary read/hash/metadata command returned naturally. No reviewer-owned daemon, child agent, relay, long-running process, temporary fixture or disposable resource exists. The only write is this authorized report; parent-owned manual runs and WD relay 635 remain under the parent's ownership.

VERDICT: FAIL
