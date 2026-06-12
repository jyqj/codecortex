# CodeCortex

Rust 实现的代码图谱索引与分析 MCP 服务器。CodeCortex 为代码库构建语义
索引，通过 14 个 MCP 工具向 AI agent 提供排序搜索、影响面分析、架构透视
与图查询。

纯代码智能——没有 UI、没有 CLI 产品形态，MCP-first。

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
| [CONTRIBUTING.md](CONTRIBUTING.md) | 构建、测试、lint、MSRV |

## 亮点

- **30 种语言标识符**，10 种完整 tree-sitter 解析；16 个语义框架
  resolver（Express、Flask、Spring、Gin、Axum、Rails……）。见
  [docs/LANGUAGES.md](docs/LANGUAGES.md)。
- **排序式本地搜索**——FTS5 + 正则 grep + 文件预选，经 Reciprocal Rank
  Fusion 融合，再按文件路径 / breadcrumb / 时近性加成重排。见
  [docs/internals/SEARCH.md](docs/internals/SEARCH.md)。
- **影响面分析**——BFS 反向调用者扩展、社区边界、跨服务 HTTP 影响、git
  共变分析。
- **增量索引**——mtime+size 快路径 + 哈希确认、脏传播、自动索引的文件
  watcher。见 [docs/internals/INDEXING.md](docs/internals/INDEXING.md)。

## 许可证

MIT
