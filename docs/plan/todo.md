# Active work and complete remaining checklist

Updated 2026-10-23 after the session restart. GitHub audit: **26 open Issues,
2 completed Issues**; #3 is the first unresolved dependency. PR #31 remains draft.
This checklist supplements each linked Issue's full acceptance criteria; it does
not narrow them. Check an Issue only after its evidence, review, gate and merge.

Current decision: the tested Helix candidate failed full-data low-memory recovery
in three retained OOM runs. Stop Helix tuning and validate the specification's
SurrealKV 1.0.0/LanceDB 0.40 fallback against the unchanged contract. The checked
diagnostic leaves below are Helix evidence, not acceptance transferred to the
fallback. See [decision record](../benchmarks/storage-decision.md).

## Session operating rules

- Use `/home/obj/.hermes/skills/software-development/wd/SKILL.md` before every
  non-CSA command expected to exceed 90 seconds, including after compaction.
- Resolve the active provider from this session's metadata, then run that skill's
  `scripts/resolve-checkin.sh` immediately before arming one shared parent relay.
  Never reuse a previously resolved TTL or claim cache hits without telemetry.
- At each check-in record exact PID/start time, durable progress, campaign age,
  elapsed time since progress and a bounded next action. Reconcile terminal work
  before resuming it. Do not rerun a full gate for unchanged source.
- Preserve verified corpus shards and partial downloads. A previous running
  snapshot is not proof a process survived restart. Maintain one database writer.

## Restart review and process improvement

#1 and #2 are merged and closed with acceptance evidence. All later work depends
on #3's real storage decision, so issue-count reduction is currently constrained
by that decision. Closing it on diagnostic data would conceal the dependency.

The previous history verifier timed out after 900 seconds without per-wave
checkpoints; the vector builder reached 34,477/38,462 entities before its 7,200s
harness limit, with no OOM. Neither is a pass or, alone, a backend rejection.
The downloader encountered curl HTTP/2 error 92; completed files and partials
remain available. No previous task-owned processes remained at the restart audit.

Improvements: instrument the precise slow query before enlarging its workload;
classify retryable transport failures and preserve individual file outcomes;
separate preparation from measured engine work; publish verified, cohesive leaves
without repeatedly rebuilding unchanged product code. Continue one main Issue.
For the fallback, test direct low-budget reopen after the first real source shard
before admitting the full corpus, so memory-layout failures surface earlier.

## MVP-003 remaining acceptance

- [x] Freeze candidate order and full-size acceptance before measurements.
- [x] Pin Helix/SlateDB source, licenses and isolated Rust lockfile.
- [x] Exercise primitive durable transaction/rollback and process-crash barriers.
- [x] Import 38,462 real 1536-dimensional embeddings (diagnostic only).
- [x] Correct and instrument the historical query probe; verify 100,000 changes
      and exact contents across revision/retraction/restore/scope boundaries.
      The full-database run will use the same exclusive operation controller;
      native 2,048-change seed/replay/verify and process-identity checks pass.
      The earlier 100,000-change receipt is still diagnostic-scale storage.
- [x] Reconcile the interrupted diagnostic vector build and verify real search
      in all eight partitions (24 queries); full-size coverage remains below.
- [x] Resume public corpus downloads and independently verify all 42 source hashes.
- [x] Freeze reproducible >=50GB unique-content / >=1M unique-vector input
      manifest, deduplication/exclusions and import disk reservations; validate
      streaming import and completed-run reconciliation on diagnostic input.
- [x] Freeze distributed 3,328-query input and exact ANN top-20 references for
      32 queries against all one million vectors, with source/binary hashes.
- [x] Prepare 512 additional unvectorized text queries across all eight partitions
      by replaying the complete deduplicated source order. Source/body hashes and
      global memory IDs verify; completed reuse changes no retained files.
      Native mixed-query admission and small real-vector/synthetic-text workload
      now pass. Full-source preparer SIGKILL/resume preserves the first 17 selected
      queries and reproduces all 512 canonical query bytes; terminal artifact
      hashes and completed reuse verify. Full corpus reads remain pending.
- [x] Finish the full corpus import. The first attempt stopped at its disk
      reservation after about 4.7 million documents; the durable prefix remains.
      Native Zstd continuation and journaled run transfer are verified on small
      fixtures. After exact owned-fixture cleanup, explicit compressed continuation
      was admitted and verified the retained 4,723,072-record prefix. The complete
      source stream and all 11,428,831 documents now verify, with 50,001,002,783
      indexed-body bytes (50,000,002,783 eligible unique-content bytes). Its
      high-profile continuation took 12,557 seconds, RSS maximum 886,087,680B;
      database allocated size was 48,987,971,584B after close. Indexes and the
      full-size workloads remain separate acceptance below.
- [ ] Observe full-size hot/cold read coverage; partition labels and process
      restart alone do not prove file-cache coldness.
- [ ] Index every declared searchable text/vector record at full size.
- [x] Implement bounded admission and 100 independent harness clients with actual
      conflicts, durable latency/error records and per-second checkpoints;
      full-size measured cases and recovery remain below.
      Exact-child SIGKILL after a durable write, refusal of ambiguous replay,
      explicit abandonment, subsequent execution and prior-receipt reuse pass.
- [ ] Run >=30 minutes each at 512MiB and 2GiB; meet every frozen rate/latency/RSS
      threshold; include cold reads, burst, overload fairness and bounded drain.
- [ ] Verify atomic revision/head/receipt/job/sequence commit; request-hash reuse;
      historical pages; current head/scope revalidation and live permissions.
      Small real-index fixtures now cover stale candidates, permission races,
      historical content preservation, principal isolation and malformed/missing
      heads. Imported revision-zero current/history views, original scope membership
      and forged candidate rejection pass 114 native commands. A deterministic race
      proves historical scans release the writer and recheck revocation before
      return; the unchanged 100-client diagnostic passes after that fix. Full-size
      coverage and the generation lifecycle remain required.
      The current-read workload now checks real pinned corpus content as well as
      concurrent synthetic revisions. Seven negative oracle controls, 123 native
      corpus/pagination commands, visibility/reopen and 5,163 checks over 100,000
      changes pass. The 60-second case passes with 144 real current checks, but
      full-duration memory, unvectorized text and generation-backed reads remain.
      A 141-command extension verifies restored generation candidates recover
      their source identity from authority, reject forged derived provenance,
      obey changed scope/live permissions and preserve original history.
      The mixed fixture independently verifies 70 text-only current reads and 36
      BM25 queries across eight partitions, with 15 real ANN queries. The full
      3,840-query population is admitted but has not yet queried full indexes.
- [ ] Verify generation-specific indexed receipts and gap-free watermarks,
      concurrent writes/reindex, failed-build rollback and model separation.
      A 96-command revision-only diagnostic passes real text/vector reads,
      provenance rejection, four process-crash barriers, activation and bounded
      prefix advancement; the full-size obligations below remain open.
      The extended 106-command fixture verifies persisted text-only policy and
      not-required vector completion. A separate eight-source-document generation
      runs 100 clients for 60 seconds with background indexing, reaches both
      watermarks without gaps and passes independent derived-content/job-count
      verification. Full-size execution and concurrent rebuild remain required.
- [ ] Backfill the full corpus into a candidate generation with resumable coverage
      accounting. The explicit corpus-generation path has bounded atomic cursor
      batches and native fixture coverage; full-size execution remains pending.
      A dedicated operation controller now checks exclusive import/case ownership,
      disk reservations, exact binary identity and explicit resume. Native
      controller-crash/live-child refusal and committed-cursor reuse pass; the
      first full-size initialization stopped at its original 7,200-second unique
      index observation limit. Ordinary immutable-source lookup now passes 146
      corpus checks and the small mixed workload while retaining duplicate
      rejection. Native index abort/replacement and explicit stopped-run
      abandonment pass. Full low-budget cleanup OOM-killed its 640MiB group;
      limiting compaction ranges alone also failed. Both original failures are
      retained. Reducing compressed read-ahead also failed; Helix is rejected for
      this tested recovery criterion. Full selected-backend indexes and backfill
      still require acceptance on the specified fallback.
- [ ] Measure and enforce disk admission for ordinary indexed writes and reindex,
      including retained native text splits; verify the explicit rebuild bound.
      Native per-write and index admission now have 26-command diagnostic coverage,
      including refusal of an insufficient full-workload plan and continued
      reads/idempotent receipt reuse. Full-size allocation and physical disk-full
      recovery remain required; logical input allowances are not physical bounds.
- [ ] Test low-budget reopen of full data/backlog/interrupted builds and
      2GiB-to-512MiB draining with original work deadlines preserved.
- [ ] Test all required commit/index/watermark/activation crash barriers,
      derived corruption/rebuild, authority corruption, isolated disk-full,
      child resource failure, invalid config and competing reloads.
- [ ] Validate the specified SurrealKV/LanceDB fallback after Helix's measured
      low-recovery failure. Pin Rust dependencies/licenses; prove primitive
      transactions/history, bounded import and search before the full unchanged
      workload/recovery matrix. No third backend or reduced thresholds.
      Exact dependencies and license inventory are now retained. Twenty native
      primitive commands pass commit-crash atomicity, conflict/rollback, history,
      idempotent replay, real BM25/IVF queries and separate-process reopen in a
      640MiB group without swap. Bounded/resumable import, full-scale indexes,
      authority/outbox semantics and the complete matrix remain unfinished.
      The bounded import now passes 21 native recovery/input checks and preserves
      original record hashes; 38,462 real vectors stream successfully at the high
      profile. Small committed-WAL corruption and isolated kernel ENOSPC during
      commit pass, including prior-history preservation and successful retry.
      Inline-value reopen was stopped after retained heap values caused sustained
      memory throttling. Native VLog now passes the same 38,462-vector high import
      and direct low reopen with all original bytes verified (3.223s, sampled
      RSS max 78,217,216B). Compressed format 2 then verifies every original byte and
      source/scope mapping on that shard, at 500,682,752 allocated bytes (about
      61% less). Low reopen is 4.242s with sampled RSS max 71,561,216B, no memory
      throttling/OOM. Large VLog corruption, bounded decode and legacy-format
      rejection also pass. Full data and sustained memory acceptance remain pending.
      Corpus-derived generation backfill now passes 39 native checks, including
      four cross-store SIGKILL/resume barriers, native append receipts, complete
      index coverage, reopened scope-filtered BM25/ANN and bundle validation.
      This is a synthetic baseline; ordinary changes, activation and real/full
      corpus index measurements remain pending. The compressed full import now
      completes at 37,799,542,784 allocated database bytes. Its initial low reopen
      fails at systemd's soft 1024 file limit against 2,202 VLogs; verification with
      an explicit service-local 8192 limit now passes every original byte and
      source/scope mapping for all 11,428,831 documents in 3,570.278s. Sampled
      RSS maximum is 133,865,472B, with no OOM; memory.high throttling is retained.
      Original memory/swap limits and the observation deadline are unchanged.
      This proves full low reopen/verification, not the mixed workload or indexes.
      The controller/deadline extension passes 47 native commands. Real-shard
      backfill succeeds; default Lance shuffle/window buffering OOMs on index
      merge. Existing 16MiB shuffle and no-offset-preload controls then complete
      the same native build with its original deadline preserved, RSS sampled
      max 464,904,192B. Sixteen real queries across all eight scopes and 272
      independently checked source candidates pass. Full indexes and all ordinary
      change/permission/activation obligations remain open.
      Fallback authority now passes 53 native checks for atomic revision/head/
      receipt/job/sequence writes, history, scope moves, retraction/restore and
      current permissions. Three release units include a deterministic concurrent
      revocation race and corrupted head/sequence refusal; 47 search regressions
      pass on the preceding candidate. Encoded-record escape expansion has a real
      failing/passing regression. The fallback now shares the unchanged frozen
      history model with Helix. Ten owned-runner cases pass 2,048 changes/replay
      and 5,163 historical checks; a fourth native unit rejects work after the
      original persisted deadline. On real-shard storage, all 100,000 changes
      (311.838s) and 5,163 historical checks (4.007s) now pass; sampled RSS maxima
      are 183,025,664B and 81,698,816B, no OOM. Full-corpus history now passes
      all 100,000 changes and the same 5,163 checks. All 11,428,831 text records
      and 1,000,000 vectors are backfilled in 830.918s, sampled RSS max
      334,905,344B, without OOM. Allocated storage after backfill is 68,256,649,216B.
      Full FTS build r1 OOMs during tokenization at 1,072,926 documents; failure,
      journal and original deadline are retained. The bounded-I/O successor is
      separate acceptance. Full indexes and sustained workloads remain required.
      The lifecycle implementation now applies real delta indexes, validates
      embedding bundles/source hashes, records gap-free watermarks and atomically
      activates generations. Its 103-command fixture covers six SIGKILL cuts,
      failed-build old reads, model separation and live permissions. A deterministic
      release unit proves ordinary writes continue while index publication pauses.
      On the real 38,462-vector shard, all 100,000 outbox jobs now reach both
      family watermarks in 40.045s; 352 current active text rows and no synthetic
      vectors are materialized, and generation 1 activates at sequence 100,000.
      The shared 100-client harness passes a real-shard 60s mixed case: 1,698
      requests, 144 synthetic/144 corpus current checks, 72 BM25/15 ANN queries,
      50 final derived heads, writes 22.2/s with P95 219ms, RSS max 299,061,248B.
      Burst and 12,031-request overload also pass; overload drains in 0.621s.
      A separate 16-record fixture verifies 70 unvectorized current reads and 36
      BM25 queries across all eight scopes plus 15 real ANN queries. Both backends
      use one unchanged scheduler, query validator and independent content oracle.
      Invalid full-scale admission rejects before authority initialization in both.
      Full-corpus activation, both 30-minute profiles and
      the remaining failure matrix remain required. Read-only status works below
      the write reservation; new
      mutations still fail. Catch-up rates use this attempt's starting watermark.
- [ ] Publish ADR, exact-source gates/review, raw receipts and CI; merge PR #31
      and close #3 only with its acceptance complete.

## Complete Issue queue

- [x] [#1 [MVP-001] 建立 Rust/Linux 项目骨架与可执行开发门槛](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/1) — dependencies: none.
- [x] [#2 [MVP-002] 版本化配置、热重载与 deadline 配置契约](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/2) — dependencies: #1.
- [ ] [#3 [MVP-003] 非 SQLite 存储 spike 与唯一 backend 决策](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/3) — dependencies: #1, #2.
- [ ] [#4 [MVP-004] 实现唯一正式 Storage、原子提交与恢复基础](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/4) — dependencies: #3.
- [ ] [#5 [MVP-005] 定义并执行来源、authority、scope 与 taste 数据语义](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/5) — dependencies: #4.
- [ ] [#6 [MVP-006] 持久历史视图、as-of 查询与实际 recall receipt](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/6) — dependencies: #4, #5.
- [ ] [#7 [MVP-007] 采集归一化、原文归档及分支/压缩恢复协议](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/7) — dependencies: #4, #6.
- [ ] [#8 [MVP-008] CLI、私有 IPC daemon 与持久后台作业](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/8) — dependencies: #2, #4, #7.
- [ ] [#9 [MVP-009] 可配置本地模型客户端与 1700 秒全链长测](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/9) — dependencies: #2, #8.
- [ ] [#10 [MVP-010] 磁盘检索、混合召回、重排与有预算的原文读取](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/10) — dependencies: #5, #6, #9.
- [ ] [#11 [MVP-011] CallerAdapter/ContextSource 与 AgentRunner 公共契约](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/11) — dependencies: #7, #8, #10.
- [ ] [#12 [MVP-012] Claude Code CallerAdapter 与 AgentRunner 实接](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/12) — dependencies: #11.
- [ ] [#13 [MVP-013] Codex CallerAdapter 与 AgentRunner 实接](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/13) — dependencies: #11.
- [ ] [#14 [MVP-014] Hermes CallerAdapter 与 AgentRunner 实接](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/14) — dependencies: #11.
- [ ] [#15 [MVP-015] pi CallerAdapter 与 AgentRunner 实接](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/15) — dependencies: #11.
- [ ] [#16 [MVP-016] consult 可选短 hint、渐进调研与子孙共享预算](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/16) — dependencies: #12, #13, #14, #15.
- [ ] [#17 [MVP-017] 异步提取候选、来源引用与可重建索引作业](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/17) — dependencies: #5, #7, #9, #10, #16.
- [ ] [#18 [MVP-018] 单一或组合 Runner 的 Auto-Grill 工作流](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/18) — dependencies: #16, #17.
- [ ] [#19 [MVP-019] 生效策略、review 队列、人工校准与 taste 使用规则](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/19) — dependencies: #5, #6, #18.
- [ ] [#20 [MVP-020] 外部 Memory Editor、范围受限 patch 与并发修订](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/20) — dependencies: #16, #19.
- [ ] [#21 [MVP-021] Run、Git/工作区、build/test 与 incident 证据关联](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/21) — dependencies: #6, #11, #16.
- [ ] [#22 [MVP-022] 执行失败触发调查、自动修订或高优 review](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/22) — dependencies: #20, #21.
- [ ] [#23 [MVP-023] Notifier 插件、可用本地/stdio 实现与持久投递](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/23) — dependencies: #8, #19, #22.
- [ ] [#24 [MVP-024] 50 GB、100 客户端、内存预算与故障恢复压力验收](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/24) — dependencies: #4, #8, #10, #16, #17, #20, #23.
- [ ] [#25 [MVP-025] mempal 只读迁移、中立导出与往返验证](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/25) — dependencies: #4, #5, #6, #7, #19.
- [ ] [#26 [MVP-026] Linux 安装、升级、卸载与四宿主使用文档](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/26) — dependencies: #12, #13, #14, #15, #23, #24, #25.
- [ ] [#27 [MVP-027] 质量评测、主上下文开销与组合回归](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/27) — dependencies: #16, #18, #19, #20, #22, #24, #26.
- [ ] [#28 [MVP-028] 需求逐项终验、发布和完成证据归档](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/28) — dependencies: #1, #2, #3, #4, #5, #6, #7, #8, #9, #10, #11, #12, #13, #14, #15, #16, #17, #18, #19, #20, #21, #22, #23, #24, #25, #26, #27.

Every leaf retains the complete linked acceptance criteria. No local fixture
substitutes for the real four-host, provider-duration or release acceptance.
