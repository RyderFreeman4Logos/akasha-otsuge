VERDICT: PASS
ACTIONABLE_COUNT: 0

MVP-009 independent whole-candidate source review, R2. R1's workflow-clock renewal regression is closed, including the related ready-admission boundary. No additional actionable regression was found in the complete candidate. This is a source-review PASS; the new release's real-duration and GB10 acceptances remain separate required evidence.

Repository: `/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge`
Branch: `feat/mvp-009-providers`
HEAD: `986a0278d5873cb667aeb4ba82118efceb9a3666`
Tree: `a17fb69214b772612e63f056444beaefb87b5557`
Base / origin/main / merge-base: `a3d278d9f3968d8ce6eacea79d946b0ed880e2ad`

The authoritative scope is the complete `base...HEAD` range: 17 paths, 2,464 additions and 70 deletions. Relative to reviewed R1 HEAD `69853f1001aeea840982188c791de05f36ff5952`, the repair changes eight paths, with 271 additions and 38 deletions. Nine complete candidate blobs are byte-identical. I compared immutable bytes before reusing R1's completed whole-candidate coverage, then read the entire repair delta and retraced affected callers and sibling deadline boundaries. The preserved R1 FAIL report rehashes to `d47479368632bb4d2753b0461e1b320fed0a772f16aa73129f2d0cc1e798c640`.

R1 closure and sibling review:

1. **R1-1, P2 original workflow renewal — CLOSED.** `src/daemon/jobs.rs:988`–999 now validates that the stored start plus original execution budget equals the stored deadline, constructs `Deadline::resume` from that original budget/start, and passes the clock into `execute_model` before awaiting configuration restoration. No stale remainder is passed across that await and no exhausted duration is promoted to one millisecond.

   `src/config/deadline.rs:74`–90 maps the persisted elapsed time onto one monotonic clock, shifting both start and end while keeping the configured duration and original wall-clock start for the trace. Checked arithmetic rejects an invalid/future stored start or an unrepresentable time; the existing start constructor still rejects zero duration. Once constructed, subsequent lock waits consume this same clock.

   `src/daemon/models.rs:117` accepts the clock by value. Restoration at 119–132 does not recreate it. Each stage checks expiration after acquiring admission (136–141), the physical call uses that clock as its workflow parent (149–156), and result archiving plus terminal arbitration retain it (160–196). A completed result that crosses the workflow end is still archived and fails with WORKFLOW_TIMEOUT rather than renewing the budget. Cancellation/permanent-error handling and the immutable result archive remain unchanged.

2. **Related provider admission boundary — covered.** `src/provider.rs:295`–300 checks both workflow and queue clocks after acquiring the semaphore/rate-limit admission. This closes the case in which a ready inner future wins a timeout poll despite the deadline already having elapsed. Expiry returns before request/credential construction, provider-clock assignment or `request.send`; the scoped permit is released. Workflow expiry keeps precedence when both clocks have expired. The existing response-completion check (364–369) and the model executor's post-archive check preserve the same end. Queue timing remains separate from the individual provider's configured execution budget.

3. **Capture, recovery and lifecycle siblings — preserved.** The capture branch still computes one end before its loop and checks it before work and after the awaited physical batch (`src/daemon/jobs.rs:1001`–1017 and 1057–1067). The model repair does not rebase that branch. Existing running-model restart recovery still marks the job PROVIDER_INTERRUPTED instead of replaying an ambiguous external request. Queued model jobs still restore their accepted archived generation and digest. Mutation owners, scheduler JoinSet/drain, cancellation notifications, native commit ownership and Storage close are unchanged. There is no additional detached execution path in the repair.

The new daemon regression at `src/daemon/jobs.rs:430` exercises the actual `execute_job -> execute_model` path. It queues execution followed by a second fair-mutex owner, permits the executor's first read, holds its later admission wait, advances the clock and then releases it. The cases cover before-expiry success, expiry during that wait, and already-expired persisted execution. The endpoint counts complete received request bodies. Assertions preserve original durable start/deadline and, in the success case, original configured/start trace fields; both exhausted cases require zero sends. The endpoint task is aborted and awaited, and Storage is closed before final assertions. The provider regression at `src/provider.rs:413` separately covers already-expired entry and workflow/queue expiry during held-slot admission. Its oracle is the stable error plus absent provider-start trace; I do not mislabel that direct test alone as an independent remote-wire census.

Supplied repair evidence supports this causal account:

- The first restore-deadline RED fails while configuring a 10-second workflow against the default 1700-second Runner. It is **HARNESS invalid fixture configuration**, not behavioral product RED.
- The corrected executor RED prints: before-expiry succeeded with one send; after-wait expiry also succeeded with one send; already-expired execution failed only after one send. It reaches and fails the expected terminal-state assertion.
- Executor GREEN prints: before-expiry succeeded with one send; both exhausted cases failed with WORKFLOW_TIMEOUT and zero sends.
- Both retained provider-admission RED logs show a non-null provider-start trace for an already-expired workflow. The later GREEN executes the current three admission cases and passes.
- The focused log contains 49 passing Rust tests (16 library, 17 config, 15 daemon, 1 provider) and passing Clippy. These focused records were collected while HEAD still named R1 and the repair was dirty; they are repair evidence, not an exact-frozen-R2 gate. I did not recreate or execute the RED source. The current source and the complete exact-R2 gate provide current-candidate attribution.

All 17 paths are covered:

| Path | Current review / reused scope | Frozen full-file SHA-256 |
| --- | --- | --- |
| `Cargo.lock` | Byte-identical to R1; retain reviewed reqwest/rustls dependency delta and native Storage pin. | `c7fad7a2882009ef06ad38497d1f0fb54e9cc3d9da7166fccf1960757a9e88ce` |
| `Cargo.toml` | Byte-identical to R1; retain exact HTTP pin/features and runtime/dependency review. | `4d64eb39db2e958177aade3e00fda431f56e906f1958667a31815fa2b441ee58` |
| `README.md` | Read complete repair delta; link to the separate #9 evidence without declaring issue completion. | `5d60af7eecc67d7796ce11809e9199794b195508ab68cd06fa1eb463a4962dfd` |
| `docs/plan/progress.md` | Read complete repair delta and acceptance context; historical source/release identity remains explicit. | `467bf40917e0515b5ae469e78a1944eae04e313b602945e7164d536a2eae0d6a` |
| `docs/plan/todo.md` | Read complete repair delta; repair and historical acceptance checks are distinguished from required new release acceptance. | `6eb516e9de63b891f431337c68624e4b040e875a2e9d16b92c70330d8b58e18d` |
| `docs/providers.md` | Read complete repair delta and affected deadline/diagnostic contract; ordinary runtime remains unverified for a deployment. | `bcd4789f800c7cf67530e666df4c8db94e124a9af795f271ab19638e455b83db` |
| `src/config/deadline.rs` | Complete file read; new resume clock, overflow/future-start refusal, original configured/start trace and unchanged child deadline semantics. | `44af22ada749f397887e01b6e7936de71c4ccb3b26eb9362201a41d6b413b95e` |
| `src/daemon.rs` | Byte-identical to R1; retain dispatch, Client owner, mutation ownership, wakeups and shutdown/drain review. | `fb864030386f4cf92e3619066969a304815b6d2352ba5b2457e3d9807da6678c` |
| `src/daemon/jobs.rs` | Complete repair delta and affected production call chain read; clock construction/validation, fair-lock regression, capture sibling and scheduler ownership. | `84100b7c36cc73a1a5ac1bb36284fe07c8cbc9feac5fc921b89cc26cab05385d` |
| `src/daemon/models.rs` | Complete file read; unchanged archives/cancellation plus carried workflow, pre-stage refusal and terminal arbitration. | `767a569d909faa27feb0a29dc76d6dda196bf5152a364aa0fa5124f315effde0` |
| `src/ipc.rs` | Byte-identical to R1; retain typed model command, frame/transport bounds and durable job caller review. | `1b91c8b0c14b49a8fcecc9dc72d1046985c6ced491a766e846a3cdf2248cb8b1` |
| `src/lib.rs` | Byte-identical to R1; provider module remains exported. | `d1778f5ddbd51f29aa859b8deff01cfad2eceba2ebac03eccf416bcd18fff638` |
| `src/main.rs` | Byte-identical to R1; retain bounded plan CLI, request identity, IPC and output handling. | `a14aeb8db138e378cb3432433175bd76de5c7f7b4243e83a5ae14be778be8b4c` |
| `src/provider.rs` | Complete repair delta, new regression and complete affected call path read; post-readiness clock checks, queue permit release and unchanged request/result handling. | `6df611e0889285ed50c8ff15814f4832fa95380108a3f9fd29139a41f4629382` |
| `tests/daemon.rs` | Byte-identical to R1; retain installed executable selection, daemon lifecycle/fixture cleanup and model test registration. | `5e770eae46642d0e9f7b71c45a5c8eea43a35509b020d5a7b6a9585f64ca67b3` |
| `tests/provider.rs` | Byte-identical to R1; retain three-stage direct-client virtual-clock coverage and its scope. | `0c60c2186dfa3026a9433f868c5c17158a05a760ad611dc5378d2515dcd0d265` |
| `tests/support/daemon_models.rs` | Byte-identical to R1; retain ordinary contract/error/cancel/restart/archive/reload tests and both explicit manual harnesses. | `1f56865d231ec0efa607359343f8c3c262474e353ca270f8afc07e2ce3be0370` |

I also verified that the reused ConfigManager/schema, daemon configuration persistence, Storage, Domain history, canonical gate recipe, installed daemon harness, full MVP-009 issue specification and relevant spec04 are byte-identical to R1. The earlier whole-candidate conclusions therefore remain scoped to these exact bytes: independent configured profiles drive both wire requests and recorded identity; plan/request/batch/response/archive/IPC bounds remain enforced; credentials and upstream errors remain absent from diagnostics; TLS verification stays enabled; proxies, redirects and automatic transport retries stay disabled. Accepted jobs retain their configuration/plan identity and durable idempotency, current permissions are checked at their documented read/stage boundaries, and cancellation does not claim to stop remote inference. No future host authentication, Runner, retrieval/index integration or model-quality capability is inferred.

Current exact-candidate gate:

The complete `mvp009-gate2.log` was read, and its SHA-256 matches the dispatch and terminal receipt. The terminal records exit 0 at the requested HEAD in **82.5348502555862 seconds**, UTC `2026-10-27T17:30:32.240470+00:00`. The collection binds the requested tree and reports the gate process absent.

The log names 98 passing Rust tests: 16 library, 9 capture, 5 CLI, 17 config, 15 daemon, 18 Domain, 1 provider and 17 Storage. Eight Python tests pass. Two ignored library entries are subprocess-only launchers exercised by passing crash supervisors. The two manual daemon acceptances remain ignored and excluded from the passing total. The unchanged canonical recipe and actual log cover formatting, all-target Clippy, release builds, capture executable acceptance and help/version.

Installed acceptance reruns all 15 nonignored daemon cases on release SHA-256 `6b6cc71fd9dbb1d8ce904b6ad95ed4227b99a78507e0a05f493ffe387a160c3e`, with private-path and peer denial, kernel control UID 165536 and zero daemon-owned TCP sockets. These are supplied execution observations, not a reviewer test invocation. The unrelated mise audio-condenser cache warnings do not replace any test or Clippy outcome.

Historical manual evidence is separate:

The retained one-test logs and collection receipts report a production daemon/provider success with 1,605.029 seconds of provider elapsed time and a 1,700-second PROVIDER_TIMEOUT; the old release also passed actual GB10 text, 4,096-dimensional embedding and two-score reranking compatibility. Their binary is `0596d1142606b83941b3e6d75568f49e8fd6c2e819f737496ac9f40119ccab3d`, on R1 candidate `69853f1`. Collections report owned processes/scratch absent. I read those supplied ordinary logs and collections, not live services or artifact databases, and did not independently revisit their removed resources or SSD verified files.

These historical results do **not** attest the new release. The changed acceptance documentation retains the old identity and leaves new candidate acceptance/closure outstanding. The ordinary runtime still reports `local_long_timeout_verified=false`; no controlled fixture is promoted into universal provider or future Runner compatibility. Parent owns the next manual acceptance steps.

Recomputed receipt hashes:

| Artifact under .git/akasha-evidence | SHA-256 | Bytes |
| --- | --- | --- |
| `mvp009-review-r1.md` | `d47479368632bb4d2753b0461e1b320fed0a772f16aa73129f2d0cc1e798c640` | 16740 |
| `mvp009-gate2.log` | `ac642e7c3e91ac28645138cb510d84324bea122d730ca707a8c1b00a6e600afb` | 14361 |
| `mvp009-gate2.terminal.json` | `4792b1a3cb18b65db279538cc1c56ff03fe8b545655f566b98cc064925da0d7f` | 235 |
| `mvp009-gate2.collected.json` | `8f0888f14000e4f941d98de63515f9463665700ef2eb5829a572bdecec79d1ae` | 650 |
| `mvp009-r1-fix-evidence.json` | `a7f5290328c5063f21a99f845cf05e8a762011e026cb181bfd6a2ee822190f6a` | 2947 |
| `mvp009-long-r1.collected.json` | `83b36f95a319cd4dbc8eb4f1b21057848b99bc43a424f82f2a878a96c3eea9c2` | 3717 |
| `mvp009-gb10-r1.collected.json` | `369705e02455b23e672250d7f35ea4f833cee9b8d4dd92c065b23ecf0387c4e2` | 5874 |
| `mvp009-long-r1.log` | `693ffc67f75ab8c88904d553c113c196ffe594632d3937f04e8ebe2d5f51c185` | 1372 |
| `mvp009-gb10-r1.log` | `863551b87e270e68222e50e2571de6e1c3ac40425282444631b9b4f0a9d50327` | 1217 |
| `mvp009-restore-deadline-red.log` | `f64f0bcc469d1d1b0433e0dda0979aa58a4540166e1fa8b0625a975445399d00` | 1788 |
| `mvp009-restore-deadline-red2.log` | `f787a9ea8143f5f6c6629cec4bbf25797467e93e0a0e16643497313fed68aa36` | 3347 |
| `mvp009-restore-deadline-green.log` | `70b0f9c49efc4726f6480cf4ff25ae1ebfa198357427634d89e290dc84da4d05` | 1403 |
| `mvp009-expired-admission-red.log` | `485f447d14cf8b20b83ac2e2bbfb60dbf64e8b5c3adb6f211c4247cbf1e3630a` | 2329 |
| `mvp009-expired-admission-red2.log` | `783386286661a6e02513fa03e83fafc693bd62e7f6c6d3ffa7719b41afb43573` | 2318 |
| `mvp009-expired-admission-green.log` | `947e0b12f545ac5c4a2c6a6a919d2ffd4be44cbae99c3d83442b0fcb3be11eb4` | 1211 |
| `mvp009-r1-focused.log` | `b446d05267636527edc9a6be03fa1d13d1991d33e195647f91c87aad1f5c2ae8` | 6332 |

Every focused raw-log hash matches both its evidence index and terminal receipt, and its exit code matches the retained record. The exact gate and historical manual log hashes match their respective supplied receipts. No prior failure was removed or reclassified as a pass.

Instruction, freeze and resource custody:

Applied the previously read `/home/obj/.codex/skills/native-code-review/SKILL.md` and `/home/obj/.codex/skills/review-gate-discipline/SKILL.md`, including regression attribution, frozen review scope, provider identity/physical-attempt ownership, configuration restoration and receipt/finalization rules. The explicit source-only dispatch overrides optional execution workflows. No subagent or extra WD relay was created.

Before reading candidate source, actual filesystem lexistence checks found neither AGENTS.md nor CLAUDE.md at /, /home, /home/obj, /home/obj/project, /home/obj/project/github, /home/obj/project/github/RyderFreeman4Logos or the repository root. Both names are absent in the applicable repository directories .git, .git/akasha-evidence, docs, docs/plan, docs/spec, docs/spec/issues, scripts, src, src/capture, src/config, src/daemon, src/domain, src/storage, tests and tests/support. This includes symlink entries and is not merely a tracked-file search.

Entry custody at UTC `2026-10-27T17:31:29.143301+00:00`: exact branch/HEAD/tree/base/origin-main/merge-base above; base is a commit and ancestor; full tracked/untracked porcelain status empty; index.lock absent; base-relative diff-check exit 0. All 17 current files match their immutable reviewed blobs.

Exit source custody at UTC `2026-10-27T17:34:16.308150+00:00`, after source/evidence review and before exclusive final report creation: exact same branch/HEAD/tree/base/origin-main/merge-base; full tracked/untracked status empty; index.lock absent; diff-check exit 0; all 17 reviewed files unchanged.

Both links were checked lexically and preserved:
`target -> /ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/target`
`.spike-work -> /ssd/mirror-rootfs/home/obj/project/github/RyderFreeman4Logos/akasha-otsuge/.spike-work`.
Neither target was traversed. No full database, raw corpus, shard, archive or child transcript was read.

I performed source/receipt reads and read-only Git/hash/metadata checks only. No tests, probes, builds, live calls, helper compilation, process instrumentation, background work, install, fetch/push, product/index/commit change or canonical rerun occurred. No reviewer-owned process, daemon, relay, child agent, fixture or disposable resource was launched or remains. All read commands returned naturally; the sole write is this authorized report. Parent WD 663 remains parent-owned. After creation the complete report is reread and hashed, followed by a final read-only custody check; those results accompany the external completion.

VERDICT: PASS
