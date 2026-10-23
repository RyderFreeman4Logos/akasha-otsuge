# 来源与许可证

- 项目名称与 CLI：`akasha-otsuge`；GitHub owner：`RyderFreeman4Logos`。
- 用户明确要求公开开源、Apache-2.0。该要求覆盖规格中的默认私有仓库选择。
- 规格 v0.1 的 41 页于 2026-10-22 从用户指定的 Notion 页面及其子页读取；原始工具内容、来源 URL、页面编辑时间和逐文件 SHA-256 保存在 `docs/spec/`。
- 初始化时检查本地 `mempal/LICENSE` 与 `Cargo.toml`：MIT，原版权标记为 `Copyright (c) 2026 Alex`。
- **目前没有复制 mempal 源代码。** 后续若移植代码，在此登记源仓库、不可变 commit、源/目标路径、改动及适用的 MIT 版权与许可文本。数据迁移须只读并由 MVP-025 验收。
- Apache-2.0 许可证全文来自 GitHub licenses API 的 `apache-2.0` 模板；项目版权见 `NOTICE`。
- 存储诊断的 `docs/benchmarks/results/visibility-diagnostic/*invalidation*/input.jsonl` 是八条公开数据的摘录，来源于 [Qdrant 的 DBpedia 数据集](https://huggingface.co/datasets/Qdrant/dbpedia-entities-openai3-text-embedding-3-large-1536-1M/tree/4a9731217921bc476a0f03544f11f22ae4903fa5)。原文标题与正文合并，向量转换为 float32，增加诊断 key/partition；未改写正文含义。数据集标记 MIT，DBpedia 原文遵循 CC-BY-SA-4.0；这些数据保留来源许可，不改为项目代码的 Apache-2.0。固定源文件与校验和见 [sources.json](../spikes/data/sources.json)，选取文件的校验和见相邻 `admission.json` 与 `result.json`。
- `docs/benchmarks/results/generation-diagnostic/` 的请求记录重用同一公开摘录与向量，适用上述来源许可；模型来源指纹绑定发布者仓库、固定版本、预计算时间与 float32 表示。该实验未调用模型提供商。
- `docs/benchmarks/results/corpus-backfill-diagnostic/` 复用上述八条公开记录，并加入 17 条明确标为 synthetic 的小型回填测试文本；这些测试文本不计入完整语料规模。请求、原文一致性结果与向量结果保留相同数据来源许可。
