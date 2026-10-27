# Active work and complete remaining checklist

Updated 2026-10-26 after the session restart. Live GitHub audit: **26 open
Issues, 2 completed Issues**. #3 is the first unresolved dependency; PR #31
remains draft. The complete queue below preserves every Issue and dependency.
Each linked Issue's full acceptance criteria remain authoritative; this checklist
does not narrow them. Close an Issue only after evidence, review, gates and merge.

## Session operating rules

- Load `/home/obj/.hermes/skills/software-development/wd/SKILL.md` before every
  non-CSA task expected to exceed 90 seconds, including after compaction.
- Resolve this session's active provider TTL afresh for each arm; keep one shared
  parent relay. No cache-hit claim without telemetry.
- Launch long executable tasks with `nohup`, detached stdin and durable logs,
  PID/start-time, admission and terminal receipts. Keep one completion listener.
- At check-ins verify exact process identity, durable progress, original campaign
  age and a bounded next action. Reconcile terminal work before any resume;
  never reset an existing operation deadline or replay ambiguous effects.
- Keep one database writer. Reuse the 42 verified compressed source shards and
  existing writable SSD; stream the 84 GB expansion without retaining it.
- At a requested restart barrier, let native agents and non-durable jobs end
  naturally. Do not launch commits, long gates or new native agents; no handoff
  file unless requested. Resume only after the user confirms the restart.

## Restart review and process improvement

#1/#2 are merged and closed. #3's repeated full-scale failures held up all later
Issues. The final fixed 3,840-second concurrent case has now passed; its final
commit is 392298, distinct from the build interval's ending commit 370032.
The prior pause was intentional: measured work and reviewers ended naturally,
while follow-up validation, archive, final ADR/review and merge remained undone.

The six prepared phases now pass and the archive verifies; finish the publication
tail before any new tuning or full measurement. Reuse unchanged passing receipts, preserve each
historical binary's identity and retain all failures. Consolidate the remaining
checklist instead of appending stale running snapshots. Then implement one Issue
at a time in the specification's dependency order, with focused regressions and
one final exact-HEAD gate/review per candidate. No speculative parallel writers
or repeated full-corpus acceptance on unrelated intermediate changes.

## MVP-003 acceptance and remaining work

The unchanged [storage contract](../benchmarks/storage-contract.md),
[specification](../spec/issues/MVP-003.md) and
[evidence matrix](../benchmarks/storage-evidence-matrix.md) define acceptance.
The [decision record](../benchmarks/storage-decision.md) and immutable archives
retain exact commands, identities, original failures and scope limits.

- [x] Freeze both allowed candidates, licenses, exact source pins, manifest,
  oracle and thresholds. Helix fails three retained full-data low-memory recovery
  OOM runs; only the specified SurrealKV 1.0.0/LanceDB 0.40.0 fallback is evaluated.
- [x] Import and verify 50,000,002,783 unique UTF-8 bytes, 11,428,831 originals,
  1,000,000 real 1536-dimensional vectors and 100,000 deterministic changes.
  Historical verification has 5,163 assertions at its frozen commit point.
- [x] Verify complete lexical/vector coverage, 64 full scoped queries and 1,195
  authority-checked candidates; 32 ANN references cover all million vectors,
  with mean recall@20 0.9125. Freeze 3,840 distributed mixed queries and exclude
  their IDs from mutation.
- [x] Pass both 1,800-second/100-client file-cache-cold profiles, write burst and
  overload. Low RSS P99/max 458,305,536/493,187,072 B; high
  709,939,200/763,740,160 B. Their original native identities remain explicit.
- [x] Verify full backlog drain, high-to-low reopen and direct-low interrupted
  build recovery within the original deadlines. Production reload remains #8/#24.
- [x] Verify immutable history, permissions, real corrections/rescopes/retractions
  and restored bodies/vectors: 16 full-corpus candidates, 64 revisions, 755 native
  commands. Retain native crash, atomicity, derived-corruption, authority/WAL/VLog
  corruption, real kernel ENOSPC and configuration fixtures separately.
- [x] Reclaim full obsolete versions and inactive/partial generations through
  public operations; before/after witnesses preserve history, receipts and query
  candidate sets. Preserve filesystem versus inode-allocation distinctions.
- [x] Repair shared cache ownership and single baseline/delta builder admission;
  keep their causal regressions and failed diagnostics. Bind generation identity
  to the original import manifest and check reserve before legacy binding writes.
  Source 3b3994a passes the exact-HEAD root gate and independent R3 review.
- [x] Bind the existing full database with 139 native commands and 67 identical
  before/after witnesses, preserving state/deadlines without reimport.
- [x] Pass the complete 3,840-second/100-client concurrent rebuild on source
  3b3994a/native 860c32fa: 108,641 completed requests, zero operation errors,
  82,587 distinct seed/workload commits and768 corpus corrections. Both search
  types overlap the full11,428,831-text/1,000,000-vector build. RSS P99/max
  509,325,312/553,222,144 B; no OOM. All latency/throughput/content gates pass.
  108,672 is the planned count, not the executed count.
- [x] Verify completed-run sampled disk bounds: 371 samples, total peak
  137,661,698,048 B and minimum free 397,265,436,672 B; no category cap violation.
  Ten-second inode-allocation samples are not an instantaneous quota or exclusive
  Btrfs extent measurement. Completed reuse, exact process/unit exits and scratch
  removal pass. Retain both earlier full concurrent OOM failures and unknown
  outcomes unchanged.
- [x] Finish six already-prepared phases: all acknowledgements, pre-switch
  witnesses, eight original-vector replays, generation5 catch-up, activation and
  post-switch witnesses. 323 acknowledgement batches verify82,587 receipts; all
  514 follow-up commands pass. Active5 and both watermarks equal392298; immutable
  history/current bodies, indexed receipts and incompatible-bundle refusals pass.
- [x] Archive the full run, manifest binding, follow-ups, frozen source, commands,
  hashes, resource samples and prior gate/review receipts. All2,116 archive
  members pass hash/readback verification; compressed archive size22,486,670B.
- [x] Reconcile the complete crash/resource/config matrix against immutable
  receipts. Distinguish experiment recovery from product daemon/worker behavior;
  do not rerun unchanged checks without a concrete missing acceptance case.
- [x] Publish the sole-backend ADR and unchanged measured operating envelope.
- [ ] Complete the final exact-HEAD gate and independent review, push/read back
  CI, merge PR #31 and close #3. No third backend or relaxed
  threshold is authorized. Product Storage and all later requirements stay below.

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
