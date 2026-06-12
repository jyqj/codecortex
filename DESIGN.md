# CodeCortex —— 设计章程

> 版本：2.4 | 日期：2026-06-12
> 纯代码索引引擎。没有 runtime/session/workflow/memory/skill/knowledge。

本文档记录**为什么**与**边界**。参考细节见：

- [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) —— 架构地图：crate、数据流、关键不变式、扩展点
- [docs/internals/](docs/README.md#深入实现internals) —— 存储 / 索引管线 / 检索 / 并发的深入文档
- [docs/MCP_TOOLS.md](docs/MCP_TOOLS.md) —— 14 个 MCP 工具与用法路径
- [docs/CONFIGURATION.md](docs/CONFIGURATION.md) —— `.codecortex.json` 与环境变量覆盖
- [docs/LANGUAGES.md](docs/LANGUAGES.md) —— 语言层级与框架 resolver
- [docs/CYPHER.md](docs/CYPHER.md) —— 只读 Cypher 子集

---

## 依赖总览

7 crate 工作区，依赖严格单向，无环。

```
cc-model -> cc-db -> cc-parsers / cc-index -> cc-search -> cc-server
                                                              ^
                                                          cc-eval
```

| Crate | 职责 |
|-------|------|
| cc-model | 数据类型、配置、错误（serde、thiserror、blake3） |
| cc-db | SQLite 索引存储：r2d2 池、WAL、FTS5、21 表（+5 FTS5）、schema v5 |
| cc-parsers | tree-sitter AST 提取 + 框架检测 |
| cc-index | 文件扫描、增量索引、Louvain 社区检测 |
| cc-search | 排序式本地检索（FTS5 + grep + 预选/RRF）+ Cypher 子集 |
| cc-server | MCP 服务器（rmcp）、CLI（clap）、CodeIndex、ImpactAnalyzer、FileWatcher |
| cc-eval | 检索质量与延迟评测 harness |

## 设计原则

- **MCP-first，单一目的。** 产品是经 MCP 提供的代码智能——不是 CLI 应用，
  不是 UI。CLI 只为启动服务器和安装 agent 配置而存在。
- **单一数据库。** 所有状态在 `index.sqlite3`。没有 `runtime.sqlite3`、
  没有会话存储、没有遥测落盘。
- **默认确定性、离线。** 一等行为不依赖网络：解析、FTS5、grep、预选、
  Louvain 全部本地。搜索只用词法/排序的本地信号；没有外部模型依赖。
- **图查询只读。** Cypher 子集（`graph_query`）支持 MATCH / OPTIONAL
  MATCH / WHERE / RETURN / ORDER BY / LIMIT / UNION，从不改写索引。
- **依赖严格单向。** crate 单向组合；构建子集因此诚实（每个 crate 独立
  编译、独立测试）。
- **默认增量。** mtime+size 快路径 + 哈希确认、脏传播、文件 watcher 让
  索引保持新鲜，无需全量重建。

## 本项目明确不做

- 不做会话/任务管理
- 不做工作流/回放引擎
- 不做记忆/知识/技能系统（ADR 是仓库元数据，不是 agent 记忆）
- 不做 UI 的 pin/working-set/overlay 命令
- 不做学习/策略优化
- 不做遥测持久化
- 没有 `runtime.sqlite3`（只有 `index.sqlite3`）
