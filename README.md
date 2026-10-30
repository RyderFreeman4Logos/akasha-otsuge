# akasha-otsuge

A local, traceable memory system for coding agents, written in Rust.

面向 Claude Code、Codex、Hermes Agent 和 pi 的跨 Agent 记忆系统：保留原始证据、版本历史、用户决策和实际召回记录。

**状态：MVP 开发中。** 当前实现权威存储、来源与版本历史、原文采集，以及 [私有 daemon 和管理 CLI](docs/daemon.md)。模型调用和磁盘检索已接入；四个宿主等后续工作按 [28 个 Issues](docs/plan/issues-index.md) 逐项实施；验收进度见 [TODO](docs/plan/todo.md)。完整范围和验收门槛保存在 [规格快照](docs/spec/README.md)。CLI 名称统一为 `akasha-otsuge`，替代规格中的 `mem` 占位名。

构建与验证见 [开发说明](docs/development.md)。配置库的版本切换、资源准备与时间预算见 [配置契约](docs/configuration.md)。

权威存储库提供原子修订、持久幂等回执和独立备份恢复，格式与验证边界见 [存储说明](docs/storage.md)。[来源与采纳规则](docs/domain.md)区分可信输入、机器候选和精确版本的人审；后续 Issues 接入真实宿主。

[历史与召回回执](docs/history.md)提供跨重启的 as-of 状态、固定时点分页和实际输出字节归档，并在 CLI/IPC 读取时复核当前权限。

[采集与恢复协议](docs/capture.md)保存原始字节、规范化事件和分支/压缩关系，提供有界离线 reconcile 组件命令及可查询的缺失状态。

[模型调用](docs/providers.md)通过持久 job 调用独立的 text、embedding 和 reranker 配置，记录分层时间预算；具体兼容性和长测证据见 [#9](https://github.com/RyderFreeman4Logos/akasha-otsuge/issues/9)。

[磁盘检索与渐进原文读取](docs/retrieval.md)提供中英文关键词、向量混合召回、重排和实际回执，并明确索引进度、来源范围及降级状态。

[异步候选提取](docs/extraction.md)支持配置的文本模型或已有 CLI 认证，保留原文引用、条件和候选权限，并提供持久恢复与索引重建。

[Auto-Grill](docs/grill.md)支持单个或组合 CLI 的独立角色讨论，保存原文、问题依赖与分歧，并在共享预算内停止和续接。

[宿主公共契约](docs/adapters.md)定义持久调用绑定、受限上下文读取、Runner 终态与共享预算；[固定参考集及勘误](docs/evaluation/reference-v1-errata-1/README.md)保存 60 项任务与评测规则。真实四宿主接入和模型质量验收仍待后续 Issues 完成。

公开仓库按用户要求采用 [Apache-2.0](LICENSE)。[来源与许可证核查](docs/provenance.md)记录迁移边界。
