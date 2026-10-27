# akasha-otsuge

A local, traceable memory system for coding agents, written in Rust.

面向 Claude Code、Codex、Hermes Agent 和 pi 的跨 Agent 记忆系统：保留原始证据、版本历史、用户决策和实际召回记录。

**状态：MVP 开发中。** 目前提供 Rust CLI 的 help/version 与开发检查；记忆功能按 [28 个 Issues](docs/plan/issues-index.md) 逐项实施。完整范围和验收门槛保存在 [规格快照](docs/spec/README.md)。本项目采用 Linux CLI + 本地 daemon + 私有 IPC；CLI 名称统一为 `akasha-otsuge`，替代规格中的 `mem` 占位名。

构建与验证见 [开发说明](docs/development.md)。配置库的版本切换、资源准备与时间预算见 [配置契约](docs/configuration.md)。

权威存储库提供原子修订、持久幂等回执和独立备份恢复，格式与验证边界见 [存储说明](docs/storage.md)。[来源与采纳规则](docs/domain.md)区分可信输入、机器候选和精确版本的人审；后续 Issues 接入真实宿主、daemon 与检索。

公开仓库按用户要求采用 [Apache-2.0](LICENSE)。[来源与许可证核查](docs/provenance.md)记录迁移边界。
