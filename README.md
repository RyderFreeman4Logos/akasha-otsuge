# akasha-otsuge

A local, traceable memory system for coding agents, written in Rust.

面向 Claude Code、Codex、Hermes Agent 和 pi 的跨 Agent 记忆系统：保留原始证据、版本历史、用户决策和实际召回记录。

**状态：项目初始化，尚无可用 MVP。** 完整开发范围和验收门槛保存在 [规格快照](docs/spec/README.md)。本项目采用 Linux CLI + 本地 daemon + 私有 IPC；CLI 名称统一为 `akasha-otsuge`，替代规格中的 `mem` 占位名。

公开仓库按用户要求采用 [Apache-2.0](LICENSE)。[来源与许可证核查](docs/provenance.md)记录迁移边界。
