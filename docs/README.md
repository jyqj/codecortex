# CodeCortex 文档

从项目 [README](../README.md) 的快速概览开始，然后按需深入。文档分三层：
**入门与契约**（用工具）、**深入实现**（改代码）、**质量与决策**（验证与
追溯）。

## 入门与契约

| 文档 | 内容 |
|------|------|
| [../DESIGN.md](../DESIGN.md) | 设计章程——依赖总览、设计原则、明确的非目标。 |
| [MCP_TOOLS.md](MCP_TOOLS.md) | 14 个 MCP 工具的参数、**响应形态**、错误契约、输出预算、推荐用法与反模式。 |
| [CONFIGURATION.md](CONFIGURATION.md) | `.codecortex.json` 全字段、排序权重、仓库规模档位、环境变量覆盖。 |
| [CYPHER.md](CYPHER.md) | `graph_query` 的只读 Cypher 子集与 fast path 元数据。 |
| [LANGUAGES.md](LANGUAGES.md) | 语言提取层级、置信度矩阵、16 个框架 resolver。 |
| [GLOSSARY.md](GLOSSARY.md) | 术语表（epoch、lane、PassGate、三段提交……）。 |
| [TROUBLESHOOTING.md](TROUBLESHOOTING.md) | 按症状排障、配置迁移（已移除键 / 行为变更）、稳定性口径。 |

## 深入实现（internals/）

| 文档 | 内容 |
|------|------|
| [ARCHITECTURE.md](ARCHITECTURE.md) | 地图：crate 布局、数据流、**关键不变式**、扩展点目录。 |
| [internals/STORAGE.md](internals/STORAGE.md) | cc-db：连接模型、UnitOfWork、epoch 双时钟、21 张表、FTS5 双维护、重建协议。 |
| [internals/INDEXING.md](internals/INDEXING.md) | cc-index：八步管线（计时聚合为 6 项）、脏闭包、解析阶梯、PassGate、dispatch 合成、三段提交。 |
| [internals/SEARCH.md](internals/SEARCH.md) | cc-search：检索通道、文件预选、RRF/重排、缓存、Cypher fast path。 |
| [internals/CONCURRENCY.md](internals/CONCURRENCY.md) | 锁清单与锁序、一致性窗口、watcher、会话生命周期、epoch 失效协议。 |

## 质量与决策

| 文档 | 内容 |
|------|------|
| [TEST_PLAN.md](TEST_PLAN.md) | 测试布局、eval 语料与断言类型、fixture 项目、集成测试三层。 |
| [BENCHMARK.md](BENCHMARK.md) | 目标指标、四类基准的运行方法、最新结果、写阶段优化史。 |
| [BENCHMARK_V2.md](BENCHMARK_V2.md) | 可重放质量协议、错误分母、回归/held-out 隔离与增量全量对照 |
| [adr/](adr/README.md) | 架构决策记录（ADR）索引与撰写约定。 |
| [../CONTRIBUTING.md](../CONTRIBUTING.md) | 构建、测试、lint、MSRV、提交前检查。 |

生成的基准报告在 [benchmarks/](benchmarks/) 下（由测试产出，英文）。
