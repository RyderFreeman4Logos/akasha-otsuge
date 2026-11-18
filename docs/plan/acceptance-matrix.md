# MVP-028 逐项验收记录

核对日期为 2026-11-18 UTC。证据基线为 [a177d6e23568][baseline]。本表按产品契约列出 24 项结果。测试源码给出可运行入口，公开结果链接到报告或回执，各自保留实际运行的提交、环境和限制。[该提交的公开 CI][main-ci] 通过。正式发行包、同一发行提交的完整验收和下载复测均待 [#28][i28]，当前没有可填写的 release 产物。

“组件通过”表示已有公开运行覆盖该行的组件行为。“局部通过”列出已证实的部分和剩余判据。“发行待测”表示仍需从正式候选包执行。每行的发行状态均为待测。[验收组与产品门槛][docs-spec-09-md] · [MVP-028 原始验收][docs-spec-issues-mvp-028-md] · [计划与真实 Issue 索引][docs-plan-issues-index-md]

| 原始需求与验收判据 | 计划事项 | 回归用例 | 公开运行或基准 | 结论与缺项 |
| --- | --- | --- | --- | --- |
| [REQ-01][docs-spec-01-md-l17] 新项目与 KISS。新名称、新仓库、Rust 核心；只迁移需要的能力；只保留一种正式存储后端。 | [#1][i1] [#26][i26] | [CLI][tests-cli-rs] | [后端选型][docs-benchmarks-storage-decision-md] | 局部通过。Rust 核心和唯一正式后端可核对，干净安装及依赖盘点待 #26。 |
| [REQ-02][docs-spec-01-md-l18] 按需求助。consult 可带一句短 hint，也可省略；不要求调用方输出上下文包；不自动逐轮注入记忆。 | [#10][i10] [#16][i16] [#27][i27] | [无 hint][no-hint] · [短 hint][short-hint] | [原生 consult][docs-benchmarks-results-mvp016-readme-md] | 局部通过。已有可选 hint 的真实调用；#27 待做成对意图区分、主上下文开销与相关性计分。 |
| [REQ-03][docs-spec-01-md-l19] 调用现场。准确绑定会话、子 Agent、分支和事件截止点；同 cwd 多 Agent 不串线；历史覆盖范围可检查。 | [#7][i7] [#11][i11] [#12][i12] [#13][i13] [#14][i14] [#15][i15] [#16][i16] [#21][i21] | [身份与分支][tests-adapter-context-rs] | [采集与 SIGKILL][docs-plan-progress-md-l206-l217] | 局部通过。原文与截止点有回归，[Codex][codex-host]、[Hermes][hermes-host]、[pi][pi-host] 有实接记录。#12 待补真实 Claude 同机隔离、恢复与身份。 |
| [REQ-04][docs-spec-01-md-l20] 四工具兼容。Claude Code、Codex、Hermes、pi 各自同时提供 CallerAdapter 与 AgentRunner；任一单独安装可使用，组合可配置。 | [#11][i11] [#12][i12] [#13][i13] [#14][i14] [#15][i15] [#16][i16] [#18][i18] [#26][i26] [#27][i27] | [路由契约][tests-support-adapter-matrix-rs] | [原生宿主样本][docs-benchmarks-results-mvp016-readme-md] | 局部通过。#12 缺 Claude 原生认证。#26/#27 待交四个隔离单宿主和覆盖全部 Caller、Runner 的真实交叉记录。 |
| [REQ-05][docs-spec-01-md-l21] 原始历史。持续采集可访问的用户消息、可见回复、工具记录；保存多轮压缩和分支的原始依据；增量、去重、可恢复。 | [#7][i7] [#12][i12] [#13][i13] [#14][i14] [#15][i15] [#17][i17] | [原文、分支、恢复][tests-capture-rs] | [采集与重放][docs-plan-progress-md-l206-l217] | 局部通过。已记录增量与真实进程中断。#12 尚缺已认证 Claude 的完整采集、压缩和恢复链。 |
| [REQ-06][docs-spec-01-md-l22] 渐进调研。外部 Agent/workflow 自主读取历史与代码，按需搜索，可委派子孙任务；整棵任务树共享预算。 | [#11][i11] [#16][i16] [#18][i18] [#20][i20] [#27][i27] | [共享预算][tests-adapter-budget-rs] | [调研与委派记录][docs-benchmarks-results-mvp016-readme-md] | 局部通过。已见受限原文工具与实际调用。#27 尚待证明参考任务中调研的正确覆盖、调用成本和轨迹隔离。 |
| [REQ-07][docs-spec-01-md-l23] 来源与权威。原文、用户确认、用户 taste、代理建议、Auto-Grill 产物、推断均有来源；不能伪造人的认可。 | [#5][i5] [#17][i17] [#18][i18] [#19][i19] [#20][i20] [#25][i25] [#27][i27] | [授权与来源][tests-domain-rs] | [三种策略和 PTY 确认][docs-benchmarks-results-mvp019-readme-md] | 局部通过。已验证来源、确认和采纳分离。#27 待在冻结样本中证明全体零越权和零伪造。 |
| [REQ-08][docs-spec-01-md-l24] 可配置模型处理。提取、embedding、reranker、调研模型可分别配置；无向量服务时基础检索仍可用；没有额外付费 API 的强依赖。 | [#9][i9] [#10][i10] [#17][i17] | [混合与降级][tests-support-daemon-retrieval-rs] | [无 Akasha 模型 profile 的提取][docs-benchmarks-results-mvp017-readme-md] | 组件通过。独立 provider 配置和无向量时的基础检索有回归，单 pi 实测有原始读回。发行包重放待 #28。 |
| [REQ-09][docs-spec-01-md-l25] Auto-Grill。支持一个或多个可配置 runner 进行需求探索与复核；保留分歧、证据和停止条件，不要求固定两家模型。 | [#18][i18] [#19][i19] | [讨论与恢复][tests-support-daemon-grill-rs] | [单 pi 及 pi/Codex 混合样本][docs-benchmarks-results-mvp018-readme-md] | 组件通过。记录来源、分歧、预算停止和恢复。样本达到预算时的未完成轮次保留在报告中。 |
| [REQ-10][docs-spec-01-md-l26] Review 与校准。支持全人工、按策略、自动运行；人工纠正与 taste 能更新后续行为；不要求每条记忆人工审阅。 | [#5][i5] [#18][i18] [#19][i19] [#20][i20] [#22][i22] [#23][i23] | [策略与确认][tests-support-daemon-review-rs] | [真实 CLI、合成操作员][docs-benchmarks-results-mvp019-readme-md] | 组件通过。三种模式与 taste 有检查，人工确认示例用真实 PTY 驱动；发行场景待 #28。 |
| [REQ-11][docs-spec-01-md-l27] 持久历史。记忆修订、提交、历史状态、差异、实际返回 receipt 跨重启保留；可高效查看过去状态。 | [#6][i6] [#7][i7] [#10][i10] [#16][i16] [#19][i19] [#20][i20] [#21][i21] [#25][i25] [#27][i27] | [as-of 与回执][tests-support-domain-history-rs] | [域与存储运行记录][docs-plan-progress-md-l218-l226] | 局部通过。重启可取回历史和原始返回，#27 待核对冻结任务中旧版本、撤回和回执使用，规模分页另见 #65。 |
| [REQ-12][docs-spec-01-md-l28] 外部编辑。外部 Agent 使用受控 patch 接口修正记忆；expected\_revision 防止覆盖并发修改；回滚产生新版本。 | [#20][i20] [#22][i22] | [受控 patch 与回滚][tests-support-daemon-editor-rs] | [Editor 接口及原生执行][docs-benchmarks-editor-acceptance-md] | 组件通过。已验证并发冲突、审计、撤回及追加新版本的回滚。发行端到端复核待 #28。 |
| [REQ-13][docs-spec-01-md-l29] 运行反馈。测试、运行事件与人工纠正可以创建调查；关联 receipt 和执行产物；暴露不直接等于因果。 | [#21][i21] [#22][i22] | [失败调查][tests-support-daemon-incident-rs] | [实际 Codex 调查][docs-benchmarks-incident-acceptance-md] | 组件通过。真实失败关联执行记录与 recall，调查可给出有证据的操作或转人工；发行串联待 #28。 |
| [REQ-14][docs-spec-01-md-l30] 通知扩展。统一 Notifier，持久 outbox、去重、重试；可用的本地通知输出与 stdio 插件接入。 | [#23][i23] | [outbox、重试与 ack][tests-support-daemon-notifier-rs] | [本地与 stdio 验收][docs-benchmarks-results-mvp023-final-readme-md] | 组件通过。独立投递和确认、重启与插件失败均有回执；发行链中的最终通知待 #28。 |
| [REQ-15][docs-spec-01-md-l31] 配置热重载。校验、原子应用、版本号、失败保留旧配置；定义进行中任务语义及少数需重启项。 | [#2][i2] [#9][i9] [#19][i19] [#22][i22] [#23][i23] [#26][i26] | [热重载契约][tests-config-rs] | [在途与新任务的策略][docs-benchmarks-results-mvp019-readme-md] | 局部通过。运行中任务沿用旧代、新任务采用新代。#26 待验升级后配置、历史和 pending jobs 的保留。 |
| [REQ-16][docs-spec-01-md-l32] 本地长超时。本地 provider 请求及本地 runner 默认 timeout 1700 秒；外层不得无意截短；排队、执行、取消语义明确。 | [#2][i2] [#9][i9] [#16][i16] [#24][i24] [#26][i26] [#27][i27] [#65][i65] | [Runner 剩余预算][tests-support-runner-bridge-deadlines-rs] | [生产 provider 墙钟][docs-providers-acceptance-md] | 局部通过。provider 和真实 pi Runner 分别做过长测（见下文）；正式发行身份、宿主组合与规模中的截止点待 #26/#27/#65。 |
| [REQ-17][docs-spec-01-md-l33] CLI 与本地通信。CLI + daemon + 私有 IPC；MVP 核心不依赖 MCP 或 TCP 服务；调研轨迹不回灌主 Agent。 | [#8][i8] [#11][i11] [#12][i12] [#13][i13] [#14][i14] [#15][i15] [#26][i26] | [私有 socket、CLI][tests-daemon-rs] | [安装版 IPC 及跨 UID][docs-plan-progress-md-l186-l205] | 局部通过。私有 IPC 与隔离已实测；#12/#26 待补 Claude、单宿主安装及发行包的无 MCP 核心流程。 |
| [REQ-18][docs-spec-01-md-l34] 磁盘存储。不采用 SQLite 作为正式运行后端；先验证现代磁盘检索引擎；完整版本和数据不以缓存为唯一副本。 | [#3][i3] [#4][i4] [#10][i10] [#24][i24] [#65][i65] | [权威存储][tests-storage-rs] | [存储候选与历史 oracle][docs-benchmarks-storage-evidence-matrix-md] | 局部通过。单一权威后端及磁盘检索已有证据。#65 仍需生产规模的实际索引、内存和冷开记录。 |
| [REQ-19][docs-spec-01-md-l35] 资源与并发。daemon 存储组默认 512MiB 目标、最高 2GiB 配置档；100 客户端、50GB 可检索数据验收；不足时磁盘/排队/背压。 | [#3][i3] [#8][i8] [#24][i24] [#65][i65] | [资源控制][tests-support-daemon-resources-rs] | [生产默认值与小规模对照][docs-benchmarks-results-mvp024-native-memory-readme-md] | 局部通过。#65 仍需满足两档内存、50 GB 实索引、100 活跃客户端持续负载和延迟/积压门槛。 |
| [REQ-20][docs-spec-01-md-l36] 检索一致性。修订和派生任务原子登记；索引有进度；撤回版本不再错误返回；新修订可等待或查增量。 | [#3][i3] [#10][i10] [#17][i17] [#20][i20] [#24][i24] [#27][i27] [#65][i65] | [撤回和版本过滤][tests-support-retrieval-regressions-rs] | [队列重放及索引重建][docs-benchmarks-results-mvp017-readme-md] | 局部通过。提交和派生工作、进度、已撤回内容的受控回归已有记录。#65 规模并发与 #27 冻结评测待验。 |
| [REQ-21][docs-spec-01-md-l37] 恢复与备份。持久确认、崩溃恢复、备份还原、磁盘满受控失败；不得静默丢失已确认写入。 | [#3][i3] [#4][i4] [#6][i6] [#7][i7] [#8][i8] [#17][i17] [#23][i23] [#24][i24] [#25][i25] [#65][i65] | [崩溃和备份][tests-storage-rs] | [备份、恢复及故障结果][docs-storage-validation-md] | 局部通过。恢复、已确认写入和还原有单组件证据。#65 待在规定规模、冷开及故障矩阵验证。 |
| [REQ-22][docs-spec-01-md-l38] 迁移与退出。mempal 数据只读导入、幂等、来源与导入报告；中立格式导出，不强制继承旧实现。 | [#25][i25] [#26][i26] | [只读、幂等、增量][tests-support-mempal-incremental-rs] | [旧数据转换][docs-benchmarks-results-mvp025-mempal-final-readme-md] · [中立导入][neutral-import] | 局部通过。#25 的真实只读迁移、中立导出与重导入已合入。#26/#28 仍需从发行包重跑并保留原存储。 |
| [REQ-23][docs-spec-01-md-l39] 证据与评测。需求—Issue—测试—提交可追踪；必要运行数据不塞回主上下文；使用真实任务评测。 | [#1][i1] [#2][i2] [#3][i3] [#10][i10] [#11][i11] [#21][i21] [#22][i22] [#24][i24] [#27][i27] [#65][i65] | [冻结集合校验][scripts-check-reference-py] | [参考集和勘误][docs-evaluation-reference-v1-errata-1-readme-md] | 局部通过。已冻结 60 项和 10 组混淆意图。#27 待按参考 profile 计分并公开失败、上下文开销、外部开销；#65 待补规模数据。 |
| [REQ-24][docs-spec-01-md-l40] 可安装 MVP。安装、首次配置、四宿主连接、日常操作和恢复文档完整；最终验收以可运行产品及证据为准。 | [#1][i1] [#26][i26] | [安装实物与 UID][scripts-check-daemon-command-py] | [本机安装版控制][docs-plan-progress-md-l186-l205] | 发行待测。本机安装版有检查；#26 的干净 Linux 首装、升级、卸载、四宿主指南实跑以及 #28 下载发行包重测仍待完成。 |

## Issue 与合入提交

下表把每行的计划事项接到实际合入的 PR 和不可变提交。一个事项有多个 PR 时列出实现和独立验收所需的主要合入点。所有列出的提交均为上述基线的祖先。[#12][i12] 和 [#65][i65] 的已合入部分仍有公开的未完成验收。

| 计划事项 | 已合入的 PR 与提交 |
| --- | --- |
| [#1][i1] | [PR 29][p29] [9c60f50][c29] |
| [#2][i2] | [PR 30][p30] [349431a][c30] |
| [#3][i3] | [PR 31][p31] [13065fc][c31] |
| [#4][i4] | [PR 32][p32] [95ff8f5][c32] |
| [#5][i5] | [PR 33][p33] [9b9f483][c33] |
| [#6][i6] | [PR 34][p34] [42035bd][c34] |
| [#7][i7] | [PR 35][p35] [960320a][c35] |
| [#8][i8] | [PR 36][p36] [a3d278d][c36] |
| [#9][i9] | [PR 37][p37] [a09d9d6][c37] |
| [#10][i10] | [PR 38][p38] [d566101][c38] · [PR 39][p39] [dc8e27c][c39] |
| [#11][i11] | [PR 40][p40] [19efca1][c40] |
| [#12][i12] | [PR 41][p41] [f98131c][c41] |
| [#13][i13] | [PR 42][p42] [d49be08][c42] |
| [#14][i14] | [PR 43][p43] [24eda5a][c43] |
| [#15][i15] | [PR 44][p44] [c43c087][c44] |
| [#16][i16] | [PR 45][p45] [bbce316][c45] |
| [#17][i17] | [PR 46][p46] [d64051e][c46] |
| [#18][i18] | [PR 47][p47] [2da9a2e][c47] |
| [#19][i19] | [PR 48][p48] [4609349][c48] |
| [#20][i20] | [PR 49][p49] [dc2134f][c49] |
| [#21][i21] | [PR 52][p52] [94b22af][c52] |
| [#22][i22] | [PR 53][p53] [554796b][c53] · [PR 69][p69] [2512229][c69] |
| [#23][i23] | [PR 54][p54] [6eb77a1][c54] · [PR 55][p55] [37d4d77][c55] |
| [#24][i24] | [PR 56][p56] [c059fda][c56] · [PR 63][p63] [8e14ee6][c63] · [PR 64][p64] [0a09888][c64] |
| [#25][i25] | [PR 57][p57] [3b6bf74][c57] · [PR 58][p58] [b37c613][c58] · [PR 59][p59] [31d4c61][c59] · [PR 60][p60] [fe30c78][c60] · [PR 61][p61] [235c409][c61] · [PR 62][p62] [a500d53][c62] · [PR 68][p68] [9f2ba15][c68] |
| [#65][i65] | [PR 66][p66] [9253a8b][c66] · [PR 67][p67] [b71b18d][c67] · [PR 70][p70] [63acf39][c70] · [PR 71][p71] [96e2b5f][c71] · [PR 72][p72] [c51f97b][c72] · [PR 73][p73] [1c6eed5][c73] · [PR 74][p74] [bf15768][c74] · [PR 75][p75] [a177d6e][c75] |
| [#26][i26] [#27][i27] [#28][i28] | 本基线无已验收的发行、参考模型终评或安装复测产物。 |

## 仍需执行的验收

- 四宿主与质量。[#12][i12] 缺已认证的 Claude Code 单宿主实接。Codex 或 pi/Hermes 的代跑结果按实际宿主记录。[#27][i27] 需四个独立 Caller/Runner、全部 16 条契约、覆盖四种角色的真实交叉路径，并按冻结参考集公开零身份/权限/撤回违规、全部关键决定和适用条件、正确条目覆盖率至少 90%、实质无关条目至多 10% 及至少 10 组有无 hint 对照。保留读数、token、费用、延迟和失败。依赖 Daybreak 的测试暂缓。[宿主替代及暂缓记录][docs-plan-todo-md-l1632-l1654]
- 两次墙钟记录的路径不同。生产 daemon/provider 对 1605 秒延迟在 1605.030 秒成功，超时对照在 1700.001 秒失败。真实 pi 1.4.0 Runner 使用受控本地端点，1605 秒延迟在 1612.327485 秒成功，另有超时清理对照。发行前把适用的原始回执绑定到候选包，注明重测和沿用证据的身份。[provider 原始结果][docs-providers-acceptance-md] · [pi Runner 长测][docs-benchmarks-results-mvp016-readme-md]
- 规模与恢复。[#65][i65] 承接 [#24][i24]。保留已公开的存储试验、受控内存与生产前缀成绩。仍需可追溯 50 GB 数据与实际索引、100 个活跃客户端至少 30 分钟、512 MiB / 2 GiB 两档的资源组采样、吞吐与延迟、积压、降预算、冷开、磁盘满、崩溃恢复和正确性。[原始负载与门槛][docs-spec-08-md-l136-l154] · [生产样本范围][docs-benchmarks-production-scale-md]
- 发行。[#26][i26] 从干净 Linux 安装、保存配置及 pending jobs 升级，再验证卸载保留用户历史、恢复与四宿主操作说明。[#28][i28] 还需同一候选包完成采集、consult、提取与 Auto-Grill、策略或 review、真实软件 outcome、调查修订、通知、历史原文核对及备份恢复，发布不可变标签和校验和，再从下载件复验。[完整九步流程][docs-spec-09-md-l64-l78]

## 已关闭事项复核

核对时原计划 28 项中 24 项已标为 COMPLETED。17 项的 GitHub 关闭记录列有 PR，另外 7 项（[#1][i1], [#2][i2], [#6][i6], [#7][i7], [#10][i10], [#20][i20], [#25][i25]）已按上表人工核对合入提交。额外关闭的 [#50][i50] 是测试监督修正，见 [PR 51][p51] [6319cb5][c51]。当前五个开放事项为 [#12][i12]、[#26][i26]、[#27][i27]、[#28][i28]、[#65][i65]。这些数字描述跟踪状态。

[#24][i24] 于 2026-11-11 07:31 UTC 随 [PR 64][p64] 关闭，该 PR 交付软件准入和 worker 预留。08:28 UTC 创建的 [#65][i65] 保留了未满足的生产内存、规模及恢复门槛。[#12][i12] 则在 [PR 41][p41] 合入后保持开放，给出了实现合入与真实宿主验收分开跟踪的实例。后续关闭以每个原始判据、实际测试环境和可读取回执为依据，分项合入应更新对应行并保留未通过项。

[docs-spec-09-md]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/09.md
[docs-spec-issues-mvp-028-md]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/issues/MVP-028.md
[docs-plan-issues-index-md]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/plan/issues-index.md
[main-ci]: https://github.com/RyderFreeman4Logos/akasha-otsuge/actions/runs/44018016951
[baseline]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/a177d6e23568b627ec9eef593d88289bc8634041
[i28]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/28
[docs-spec-01-md-l17]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L17
[i1]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/1
[i26]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/26
[tests-cli-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/cli.rs
[docs-benchmarks-storage-decision-md]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/benchmarks/storage-decision.md
[docs-spec-01-md-l18]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L18
[i10]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/10
[i16]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/16
[i27]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/27
[docs-benchmarks-results-mvp016-readme-md]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/benchmarks/results/mvp016/README.md
[docs-spec-01-md-l19]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L19
[i7]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/7
[i11]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/11
[i12]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/12
[i13]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/13
[i14]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/14
[i15]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/15
[i21]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/21
[tests-adapter-context-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/adapter_context.rs
[docs-plan-progress-md-l206-l217]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/plan/progress.md#L206-L217
[docs-spec-01-md-l20]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L20
[i18]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/18
[tests-support-adapter-matrix-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/support/adapter_matrix.rs
[docs-spec-01-md-l21]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L21
[i17]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/17
[tests-capture-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/capture.rs
[docs-spec-01-md-l22]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L22
[i20]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/20
[tests-adapter-budget-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/adapter_budget.rs
[docs-spec-01-md-l23]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L23
[i5]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/5
[i19]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/19
[i25]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/25
[tests-domain-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/domain.rs
[docs-benchmarks-results-mvp019-readme-md]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/benchmarks/results/mvp019/README.md
[docs-spec-01-md-l24]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L24
[i9]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/9
[tests-support-daemon-retrieval-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/support/daemon_retrieval.rs
[docs-benchmarks-results-mvp017-readme-md]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/benchmarks/results/mvp017/README.md
[docs-spec-01-md-l25]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L25
[tests-support-daemon-grill-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/support/daemon_grill.rs
[docs-benchmarks-results-mvp018-readme-md]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/benchmarks/results/mvp018/README.md
[docs-spec-01-md-l26]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L26
[i22]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/22
[i23]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/23
[tests-support-daemon-review-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/support/daemon_review.rs
[docs-spec-01-md-l27]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L27
[i6]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/6
[tests-support-domain-history-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/support/domain_history.rs
[docs-plan-progress-md-l218-l226]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/plan/progress.md#L218-L226
[docs-spec-01-md-l28]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L28
[tests-support-daemon-editor-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/support/daemon_editor.rs
[docs-benchmarks-editor-acceptance-md]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/benchmarks/editor-acceptance.md
[docs-spec-01-md-l29]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L29
[tests-support-daemon-incident-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/support/daemon_incident.rs
[docs-benchmarks-incident-acceptance-md]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/benchmarks/incident-acceptance.md
[docs-spec-01-md-l30]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L30
[tests-support-daemon-notifier-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/support/daemon_notifier.rs
[docs-benchmarks-results-mvp023-final-readme-md]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/benchmarks/results/mvp023-final/README.md
[docs-spec-01-md-l31]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L31
[i2]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/2
[tests-config-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/config.rs
[docs-spec-01-md-l32]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L32
[i24]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/24
[i65]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/65
[tests-support-runner-bridge-deadlines-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/support/runner_bridge_deadlines.rs
[docs-providers-acceptance-md]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/providers-acceptance.md
[docs-spec-01-md-l33]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L33
[i8]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/8
[tests-daemon-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/daemon.rs
[docs-plan-progress-md-l186-l205]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/plan/progress.md#L186-L205
[docs-spec-01-md-l34]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L34
[i3]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/3
[i4]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/4
[tests-storage-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/storage.rs
[docs-benchmarks-storage-evidence-matrix-md]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/benchmarks/storage-evidence-matrix.md
[docs-spec-01-md-l35]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L35
[tests-support-daemon-resources-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/support/daemon_resources.rs
[docs-benchmarks-results-mvp024-native-memory-readme-md]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/benchmarks/results/mvp024-native-memory/README.md
[docs-spec-01-md-l36]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L36
[tests-support-retrieval-regressions-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/support/retrieval_regressions.rs
[docs-spec-01-md-l37]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L37
[docs-storage-validation-md]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/storage-validation.md
[docs-spec-01-md-l38]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L38
[tests-support-mempal-incremental-rs]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/support/mempal_incremental.rs
[docs-benchmarks-results-mvp025-mempal-final-readme-md]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/benchmarks/results/mvp025-mempal-final/README.md
[docs-spec-01-md-l39]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L39
[scripts-check-reference-py]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/scripts/check_reference.py
[docs-evaluation-reference-v1-errata-1-readme-md]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/evaluation/reference-v1-errata-1/README.md
[docs-spec-01-md-l40]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/01.md#L40
[scripts-check-daemon-command-py]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/scripts/check_daemon_command.py
[p29]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/29
[c29]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/9c60f50022a4d5b29338e8603e08153037926856
[p30]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/30
[c30]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/349431a350e9e4ae3364b126c4d2a7133dd37d22
[p31]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/31
[c31]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/13065fc84be7725a154dd1e2d6d82e0cee36bc8e
[p32]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/32
[c32]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/95ff8f5e15305a0b6520b772069bd4c28f978ae1
[p33]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/33
[c33]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/9b9f4837dd05b9ab91ea00d6614d7480279e7e92
[p34]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/34
[c34]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/42035bdb56a0ea8b76cc6f895042d151ad8c54dd
[p35]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/35
[c35]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/960320a5af2d8530d9b5f93cda1a93c82405fc66
[p36]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/36
[c36]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/a3d278d9f3968d8ce6eacea79d946b0ed880e2ad
[p37]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/37
[c37]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/a09d9d691840278e2905aa39808dabd01126c663
[p38]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/38
[c38]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/d5661016c3bfb057f4b01799035ef84c45d8450c
[p39]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/39
[c39]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/dc8e27cd4329dcbc41cdf37cf63a4eafcee548c2
[p40]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/40
[c40]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/19efca183484d71ee3a0dc6cdeb2f6b293a4cd1a
[p41]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/41
[c41]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/f98131cf71ce498e5708d517ea3a5ea2031efbc9
[p42]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/42
[c42]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/d49be08e90ff0af9520679471065c1a83434111a
[p43]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/43
[c43]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/24eda5a9bee2166777a77c71d29ed4cbca96e6cf
[p44]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/44
[c44]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/c43c087ab375c08b6a3195508e4e229070474f03
[p45]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/45
[c45]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/bbce3165c6a0aed91bd1ff4d5810724d0afae968
[p46]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/46
[c46]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/d64051e2b550bec90b06fd5b20b32f9430570bf0
[p47]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/47
[c47]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/2da9a2ec302b42d6b210210737a27e2dee4728a9
[p48]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/48
[c48]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/460934961009ff41ae1b3def573e81dda4fa7d0e
[p49]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/49
[c49]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/dc2134f92f0a99ded36df71d8284bf9046dd282d
[p52]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/52
[c52]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/94b22af5dcd367499b013cbdcbe1508eb8123aa1
[p53]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/53
[c53]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/554796bbfbd1be2b11e27e8a1c3798b661c89548
[p69]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/69
[c69]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/2512229320e7fab90cc2a587cf7643e71569eda1
[p54]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/54
[c54]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/6eb77a1345a5f46f245497ceae1e77f70b68b2bb
[p55]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/55
[c55]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/37d4d77ec4517d9a35eff7eb85704de6a656d739
[p56]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/56
[c56]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/c059fda030e1c7f5d4ec922f793088450c6bc4f0
[p63]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/63
[c63]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/8e14ee61c63b4dafe486e98ce5ca1f251f213001
[p64]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/64
[c64]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/0a098882b08a7f197c0483b81c1ffd9f1d81ef39
[p57]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/57
[c57]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/3b6bf74d57b275e154195571700d64406832c2d3
[p58]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/58
[c58]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/b37c6134d98bcce0a3d5b973e95564f62e92d48e
[p59]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/59
[c59]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/31d4c61ecf6672ae52e3b84bbd00b630bceed3f8
[p60]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/60
[c60]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/fe30c78169fbf87bfa47cf06912b7c64d3a418b3
[p61]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/61
[c61]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/235c409e8c79766cef84da193d7f6b8338157d58
[p62]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/62
[c62]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/a500d531b7a02c575a593a1cbad37b93dcf8cb0a
[p68]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/68
[c68]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/9f2ba152529a796383204a8a98dd89a5c6af6616
[p66]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/66
[c66]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/9253a8b799421db3893e0e67dd2cfb55ca8c4b79
[p67]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/67
[c67]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/b71b18d9b959c4b2214405da812570ff1d39d772
[p70]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/70
[c70]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/63acf39d2138bc582ee746b6eefede970fd3f8cc
[p71]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/71
[c71]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/96e2b5f07f1dea0289f82d88bc89b43c754d4fc1
[p72]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/72
[c72]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/c51f97b1b0eb36d3e6d3b254cf1b8eebc04c57c2
[p73]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/73
[c73]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/1c6eed5d35569cd3f7427853fd0f0a0a47ac6275
[p74]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/74
[c74]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/bf1576828c2797f28f702370857b8bbc523e515e
[p75]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/75
[c75]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/a177d6e23568b627ec9eef593d88289bc8634041
[docs-plan-todo-md-l1632-l1654]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/plan/todo.md#L1632-L1654
[docs-spec-08-md-l136-l154]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/08.md#L136-L154
[docs-benchmarks-production-scale-md]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/benchmarks/production-scale.md
[docs-spec-09-md-l64-l78]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/spec/09.md#L64-L78
[i50]: https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/50
[p51]: https://github.com/RyderFreeman4Logos/akasha-otsuge/pull/51
[c51]: https://github.com/RyderFreeman4Logos/akasha-otsuge/commit/6319cb54d6ea37f07ef2d5648dde5a1da629b20e
[no-hint]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/support/daemon_codex.rs#L130
[short-hint]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/tests/support/daemon_pi.rs#L56
[codex-host]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/adapters/codex-0.164.0/README.md
[hermes-host]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/adapters/hermes-0.22.5/README.md
[pi-host]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/adapters/pi-1.4.0/README.md
[neutral-import]: https://github.com/RyderFreeman4Logos/akasha-otsuge/blob/a177d6e23568b627ec9eef593d88289bc8634041/docs/benchmarks/results/mvp025-neutral-import-final/README.md
