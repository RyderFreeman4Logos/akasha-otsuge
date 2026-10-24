# Active work and complete remaining checklist

Updated 2026-10-23 after the session restart. GitHub audit: **26 open Issues,
2 completed Issues**; #3 is the first unresolved dependency. PR #31 remains draft.
This checklist supplements each linked Issue's full acceptance criteria; it does
not narrow them. Check an Issue only after its evidence, review, gate and merge.

Current decision: the tested Helix candidate failed full-data low-memory recovery
in three retained OOM runs. Stop Helix tuning and validate the specification's
SurrealKV 1.0.0/LanceDB 0.40 fallback against the unchanged contract. Diagnostic
leaves below identify their candidate; no acceptance transfers between backends.
See [decision record](../benchmarks/storage-decision.md).

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

## MVP-003 acceptance and remaining work

The unchanged [storage contract](../benchmarks/storage-contract.md), linked Issue
and original specifications remain authoritative. The [decision record](../benchmarks/storage-decision.md)
and its immutable result archives retain all failed attempts, source identities,
commands and scope limits; historical diagnostics are not repeated in this queue.

Completed, with the limits shown:

- [x] Freeze candidate order, licenses, exact source pins, oracle and thresholds.
      Helix failed three full-data low-recovery OOM runs; only the specified
      SurrealKV1.0.0/LanceDB0.40.0 fallback is under evaluation. No backend selected.
- [x] Freeze and verify42 public compressed source shards,50,000,002,783 unique
      UTF-8 content bytes,11,428,831 documents and1,000,000 real1536-dimensional
      vectors. Import and verify every original record at low budget;84GB of
      expanded input is streamed, not retained.
- [x] Seed100,000 deterministic changes and verify5,163 historical assertions on
      full storage. Fresh verification identities preserve their original
      deadlines; expired prior operations remain expired.
- [x] Backfill both full generations and verify complete native text/vector index
      coverage. Generation2 uses scope columns. This is functional coverage;
      interrupted/recovered build peak memory failed and remains below.
- [x] Verify64 full scoped BM25/ANN queries and1,195 authority-checked candidates.
      The32 ANN references cover all million vectors; mean recall@20 is0.9125.
- [x] Freeze3,840 distributed mixed queries, including512 unvectorized queries
      across eight scopes. Source preparation SIGKILL/resume preserves exact
      canonical output, and native/controller admission rejects malformed input.
- [x] Prove owned-file cold-cache preparation with fsync/fadvise/mincore and a
      warm-file eviction control. The first full1800s low workload verifies
      50,536 recorded requests,4,320 synthetic and4,320 corpus current reads,
      581 unvectorized current reads and198 unvectorized BM25 queries across all
      eight scopes. Correctness/cold coverage passes; search performance fails.
- [x] Implement bounded100-client admission, explicit interrupted-case
      reconciliation and independent current/derived-state oracles. Single-search
      admission leaves ordinary read/write permits available. Fixed GNU allocator
      startup settings prevent the observed freed-arena retention.
- [x] Verify full100k backlog drain, restricted admission while draining, controlled
      high-to-low reopen with original deadlines, and generation2 activation.
      These are experiment-controller results; production hot reload belongs to
      the dependent daemon implementation.
- [x] Retain diagnostic atomic commit/conflict/idempotency/history and live
      visibility checks;103 lifecycle commands with six SIGKILL barriers;
      85 physical derived-corruption/quarantine/rebuild checks; authority/WAL/VLog
      corruption and real kernel ENOSPC recovery. Their small-fixture scope does
      not replace the full scenarios below or production automatic quarantine.
- [x] Retire the rejected Helix experiment database with exact ownership, process,
      lock and artifact checks;38,603,812,864 allocated bytes removed. Preserve
      source shards and receipts. Do not claim later reuse of that database.

Remaining acceptance, in execution order:

- [ ] Pass both required1800s/100-client profiles at all original rates, P95 and
      memory limits; then full burst/overload and <=300s drain. The first full
      low case runs1811.135s, RSS P99/max430,141,440/453,545,984B, no OOM, and
      reaches both watermarks at147083 with7.912s drain. BM25 achieves0.968889/s;
      BM25 and ANN P95 exceed60s, so the case fails. High/full burst/overload
      remain unrun. Retain the failure while investigating691 delta-index full
      replacements,8,286,832 repeated tokenizations and467,116 progress events.
- [ ] Complete an uninterrupted full scoped build within the640MiB low peak and
      reopen interrupted builds directly at low budget. The prior vector merge
      OOMs at812/1000; recovery completes but RSS691,322,880B exceeds the limit.
      The fixed allocator setting has not yet passed a fresh full build.
- [ ] Measure a full rebuild while the100-client steady workload continues in the
      same authority/process group. Prove ordinary commits and completed BM25/ANN
      queries overlap the build, keep the old generation active/readable, verify
      failed-build rollback and model/dimension separation. The new optional
      rebuild entrypoint passes the small native fixture:1,698 requests, nine
      commits and both old-generation search types overlap the build. Eight
      receipt mutations are rejected;103 lifecycle commands also pass. Full-size
      overlap remains unmeasured.
- [ ] Finish full-size corpus-candidate corrections/retractions/restores and live
      permission checks. Ordinary current/history bodies and scoped filtering
      have full mixed-workload coverage, but its synthetic corrections do not
      mutate the actual search candidates; permissions stay fixed in that case.
- [ ] Verify full generation-specific indexed receipts and gap-free watermarks
      across concurrent rebuild, activation and failure. Existing full backlog
      and mixed-workload watermarks pass; reindex overlap remains distinct.
- [ ] Measure physical authority/current/next-generation/scratch categories and
      enforce the actual rebuild bound. The81,212,254,720B admission floor is
      shared by all generations, but45GB projected rebuild plus4GB scratch is
      not physical proof; a failed build retained6.15GB scratch before cleanup.
      Preserve30GiB headroom and read/idempotent-receipt availability at refusal.
- [ ] Reconcile the complete required crash/resource/config matrix against
      immutable receipts: commit/reply/index/watermark/activation barriers,
      authority and derived corruption, child failure/exhaustion, isolated disk
      exhaustion, invalid config and competing reloads. Retain the distinction
      between experiment recovery and future product automatic recovery. Do not
      rerun a proven small-fixture check unless changed code or a gap justifies it.
- [ ] Finish the specified fallback evaluation, publish the sole-backend ADR,
      exact-source gates/review and raw evidence, merge PR#31 and only then close
      #3. Both candidates failing is a real blocker; no third backend or reduced
      thresholds are authorized. The complete dependent queue remains below.

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
