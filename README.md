# CodeCortex

Rust 实现的代码图谱索引与分析 MCP 服务器。CodeCortex 为代码库构建语义
索引，通过 14 个 MCP 工具向 AI agent 提供排序搜索、影响面分析、架构透视
与图查询。

纯代码智能——不提供 UI 或交互式 CLI 产品，MCP-first（CLI 仅用于启动
MCP 服务器和安装 agent 配置）。

## 当前重构进度

<!-- code-index-progress:start -->
Code Index V2 共 **192 项任务：150 done / 1 in_progress / 41 todo**。

当前阶段：**P7｜provider与dense端到端**；计划状态：`in_progress`；更新日期：`2026-10-07`。
下一任务：**P7-011｜dense范围与hydrate守卫**（硬依赖已完成）。

| 当前下一项、进行中任务及其未完成前置 | 状态 | 硬依赖（任务状态） |
|---|---|---|
| P7-011｜dense范围与hydrate守卫 | `todo` | P6-020 (done)、P7-009 (done)、P7-010 (done) |
| P7-012｜融合与部分覆盖语义 | `todo` | P7-011 (todo) |
| P7-013｜查询总deadline和模型故障退化 | `todo` | P7-012 (todo) |
| P7-014｜配置/status/MCP全链贯通 | `in_progress` | P7-013 (todo) |

进度入口：[重构总览](docs/roadmap/code-index-v2/README.md) · [逐项 TODO](docs/roadmap/code-index-v2/05-TODO.md) · [唯一任务状态源](docs/roadmap/code-index-v2/tasks.json) · [执行交接](docs/roadmap/code-index-v2/08-HANDOFF.md)。
任务完成数不等同发布认证；以各任务证据和适用验证范围为准。

> 本块由 `scripts/code_index_plan.py --write` 从 `tasks.json` 生成；无参运行校验全部进度入口。
> 源文件 SHA-256：`cc453fe700a01dd756d35e543d2ceae1f0b748bf97e9542ba0519767f5359ccc`。
<!-- code-index-progress:end -->

## 快速开始

从源码构建：

```bash
cargo build --release
```

安装进你的 AI agent（自动检测 Claude Code、Codex CLI、Cursor、Gemini
CLI、OpenCode、VS Code、Zed）：

```bash
codecortex install
```

agent 连接时 MCP 服务器自动启动。也可以手动拉起：

```bash
codecortex mcp --project-path /path/to/project
```

从包含 `.git` 或 `.codecortex.json` 的目录树内启动时，服务器会发现该项目
并在首次连接时自动索引（默认上限 50,000 文件）。如果你的 MCP 客户端从
其他工作目录启动服务器，调用一次 `index(path)`，或带 `--project-path`
手动启动。

## 14 个工具一览

| 分组 | 工具 |
|------|------|
| Setup | `status`、`index` |
| Discovery | `search`、`context` |
| Deep dive | `node`、`explore`、`trace` |
| Analysis | `relations`、`impact`、`architecture` |
| Utilities | `files`、`graph_query`、`ingest_traces`、`adr` |

所有工具常驻可用——没有激活或域系统。典型工作流：

```
index(path) -> status() -> context(task) -> explore(symbols) -> trace(from, to) -> graph_query(cypher)
```

完整参数、响应形态、推荐用法路径与反模式见
[docs/MCP_TOOLS.md](docs/MCP_TOOLS.md)。

## 文档

| 文档 | 内容 |
|------|------|
| [docs/README.md](docs/README.md) | 文档索引（入门契约 / 深入实现 / 质量决策三层） |
| [DESIGN.md](DESIGN.md) | 设计章程：原则与非目标 |
| [docs/ARCHITECTURE.md](docs/ARCHITECTURE.md) | 架构地图：crate、数据流、关键不变式、扩展点 |
| [docs/internals/](docs/README.md#深入实现internals) | 子系统深入：存储 / 索引管线 / 检索 / 并发 |
| [docs/MCP_TOOLS.md](docs/MCP_TOOLS.md) | 14 个 MCP 工具的参数、响应形态与错误契约 |
| [docs/CONFIGURATION.md](docs/CONFIGURATION.md) | `.codecortex.json`、排序权重、环境变量覆盖 |
| [docs/LANGUAGES.md](docs/LANGUAGES.md) | 语言层级与框架 resolver |
| [docs/CYPHER.md](docs/CYPHER.md) | 只读 Cypher 子集（`graph_query`） |
| [docs/GLOSSARY.md](docs/GLOSSARY.md) | 术语表 |
| [docs/TEST_PLAN.md](docs/TEST_PLAN.md) | 测试套件与 eval 语料 |
| [docs/BENCHMARK.md](docs/BENCHMARK.md) | 基准指标与运行方法 |
| [docs/TROUBLESHOOTING.md](docs/TROUBLESHOOTING.md) | 排障、配置迁移、稳定性口径 |
| [CONTRIBUTING.md](CONTRIBUTING.md) | 构建、测试、lint、MSRV |

## 亮点

- **30 种语言标识符**，10 种完整 tree-sitter 解析；16 个语义框架
  resolver（Express、Flask、Spring、Axum、Rails、Django……）。见
  [docs/LANGUAGES.md](docs/LANGUAGES.md)。
- **排序式本地搜索**——FTS5 + 正则 grep + 文件预选，经 Reciprocal Rank
  Fusion 融合，再按文件路径 / breadcrumb / 时近性加成重排。见
  [docs/internals/SEARCH.md](docs/internals/SEARCH.md)。
- **影响面分析**——BFS 反向调用者扩展、社区边界、跨服务 HTTP 影响、git
  共变分析。
- **增量索引**——mtime+size 快路径 + 哈希确认、脏传播、自动索引的文件
  watcher。见 [docs/internals/INDEXING.md](docs/internals/INDEXING.md)。

## 历史进度快照（2026-09-30）

以下为当时记录；当前任务状态见本页顶部生成区。

Code Index V2 共 192 项任务：**118 done / 1 in_progress / 73 todo**。
P5-016～018 已通过新冻结双工具链验证及独立审计；当前实施 P5-019 查询质量/成本/并发消融，
本次保存的是开发进度快照，不是 G5/M2 或发行认证。

进度入口：[重构总览](docs/roadmap/code-index-v2/README.md) ·
[逐项 TODO](docs/roadmap/code-index-v2/05-TODO.md) ·
[唯一任务状态源](docs/roadmap/code-index-v2/tasks.json) ·
[最新验收](docs/roadmap/code-index-v2/P5-D-RUNTIME-IMPLEMENTATION.md) · [历史快照](docs/roadmap/code-index-v2/CHECKPOINT-2026-09-30.md)。

## 许可证

MIT
