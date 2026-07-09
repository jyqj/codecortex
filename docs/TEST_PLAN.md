# 测试计划

最近一次 `cargo test --workspace --all-targets`：1189 passed + 14 ignored
（14 个 ignored：cc-eval 的 5 个真实工作区/增量基准、`scale_bench` 的 4 个
合成规模/冷建相位基准、cc-db 的 `incremental_write_bench` 写阶段基准与
重建压力循环、cc-index 的 `type_catalog_bench`/`catalog_bench` 与
cc-server 的 `graph_traversal_bench` 共 3 个 release-only 微基准）。基线
数字与本文档的一致性由 `scripts/update-doc-baselines.sh` 核对。

## 单元测试

| Crate | 测试数 | 覆盖重点 |
|-------|-------|----------|
| cc-db | 137 | schema v6 rebuild-on-mismatch、chunk 文本编码（含预压缩 blob 边车）、SQL 注入、architecture、ADR、边、frontier、图、查询、批量导出指纹、签名聚合维护、seed 符号快照缓存 |
| cc-eval | 18 passed + 5 ignored | 断言类型（含 field_equals、output_not_contains、field_matches_regex、array_contains_item、带 per-case `min_recall` 的 expected_symbols Recall@5 阈值、expect_error）、语料加载、走真实 MCP 线路的 fixture 集成、合成仓库生成器确定性、ignored 的真实工作区/增量基准 |
| cc-index | 336 | 框架 resolver（16 个，含跨文件）、dispatch 合成、多级 Louvain 社区检测、resolver 层级别名、路由解析来源、脏闭包状态分类、dirty-reload 清除策略、框架检测信号、导出指纹契约、自适应内存预算、三段提交 generation guard、config-linker 签名门、跨构建 catalog cache（remove_files 精确性、TypeCatalog 增量删除/复位、缓存命中/折叠/清槽生命周期与全量重建等价） |
| cc-model | 62 | 路由归一化、数据结构、枚举往返、元素置信度矩阵基线、项目根发现、部分配置默认值、外部缓存目录路径、GraphExplain 信封、tool_graph_subsets 目录一致性 + 矩阵快照 |
| cc-parsers | 179 | 10 种语言的 tree-sitter 解析、符号提取、基于 AST 的 Rust/C/C++ 调用图、spec 驱动的启发式文件内调用边、C/C++/Rust 参数/返回数据流、共享 import 提取缝（import_common） |
| cc-search | 217 | Cypher 解析器/执行器、变长路径上限、正则校验、WHERE/Degree 标识符校验、FTS5/RRF 搜索、grep SQL 作用域、搜索引擎、结果缓存 Arc 复用、图感知结果缓存（epoch 键控、降级结果排除）、目录派生的 fast-path kinds、trigram 子串预选召回 |
| cc-server | 214 | 引擎生命周期、影响分析 BFS、置信度阈值过滤、explore/trace 暴露参数、handler 分发集成、stdio MCP E2E、输出上限、UTF-8 安全截断、图 trace、环检测、flow、构建门串行化、watcher acquire-before-drain、graph_explain 附着 |

依赖严格单向，每个 crate 都能独立编译测试：`cargo test -p cc-db`、
`cargo test -p cc-index` 不需要构建整个工作区。

## Eval 套件（cc-eval）

94 个 corpus 用例，覆盖全部 14 个 MCP 工具 + 错误路径 + 边界条件，横跨
Python/JS/TS/Rust/Go/Java/C/C++ —— 其中 24 个是携带 `expected_symbols`
检索断言的 gold 准确性用例（最近一轮：Avg Recall@5 1.00、Avg MRR 0.92）。

每个用例都经**真实 MCP 线路**分发：进程内 duplex JSON-RPC 连接到与二进制
stdio 服务完全相同的 rmcp `CodeCortexMcpServer`——schema 反序列化、参数
`sanitize()` 校验、handler 分发、输出预算全部在 eval 覆盖之下；schema
漂移会让 eval 失败，而不是只有 E2E 测试能抓到。运行：
`cargo test -p cc-eval`。

用例的权威清单是 `crates/cc-eval/corpus/` 下的 TOML 文件（每用例一个
文件，文件名即用例名）。分布概况：

- 每个工具至少 2 个用例（基本路径 + 参数变体）；
- `search` / `context` / `trace` / `relations` / `impact` /
  `graph_query` 配有跨语言的 gold 用例（精确符号查找、模糊前缀、混合
  语义查询、跨文件调用链、类型层级、已知死代码）；
- 6 个 `error_*` 用例锁定错误契约（非法 Cypher、不存在的符号/工具
  参数、空查询等）。

### 断言类型

- `is_success` —— 工具未报错
- `output_contains` —— 序列化输出的子串匹配
- `output_not_contains` —— 负向子串匹配（找到即失败）
- `field_exists` —— JSON 路径存在（支持点路径与数组下标）
- `field_equals` —— JSON 路径上的精确值匹配（String/Number/Bool/Null）
- `field_matches_regex` —— JSON 路径上字符串值的正则匹配
- `array_contains_item` —— JSON 路径上的数组包含指定值
- `min_results` —— 路径上的数组至少 N 项
- `expected_symbols` —— 检索质量：期望符号名出现在结果中；计算 Recall@5
  与 MRR。逐用例通过阈值为 `min_recall`（默认 0.7；单符号精确用例钉为
  1.0）
- `expect_error` —— 用例级标志：预期工具返回错误；跳过 `is_success`

### Fixture 项目

- 18 个源文件，9 种语言，4+ 框架 resolver：
  - JavaScript（4）：routes.js、handler.js、middleware.js、utils.js
  - Python（4）：app.py、api_views.py、models.py、config.py
  - Rust（2）：lib.rs、api_handler.rs
  - Go（1）：main.go
  - Java（1）：UserController.java
  - TypeScript（2）：app_controller.ts、types.ts
  - C（2）：geometry.c、geometry.h
  - C++（1）：account.cpp
  - 服务端/框架（1）：server.py
- 覆盖框架：Express、Flask、Spring、Go 路由器（Gin/Echo/Fiber/Chi/Gorilla）
- p95/max 延迟与输出大小经 `bench::run_benchmark()` 跟踪

## 基准测试

基准的运行方式、最近结果与写阶段优化史见
[BENCHMARK.md](BENCHMARK.md)。测试侧的入口：

- 真实工作区基准：`benchmark_real_workspace`（ignored）把 CodeCortex
  工作区拷到临时目录索引并跑 10 个代表性 MCP 用例；
- 增量正确性：`benchmark_incremental_index_report_correctness`
  （ignored）覆盖全量 → no-op → 单文件增量的报告计数器；
- 增量延迟：`cargo test -p cc-eval bench_incremental -- --ignored
  --nocapture` 跑 3 个场景并硬断言 `dirty_propagation` 状态；
- 合成规模：`cc-eval/tests/scale_bench.rs` 的 1k/10k（/50k）矩阵，
  生成器 ground-truth 兼作规模正确性断言。

## 集成测试

MCP 服务器集成分三层：

- **Eval harness**：94 个 corpus 用例全部走真实 MCP 线路（见上）。
- **分发缝（3 个测试，`mcp_dispatch_seam.rs`）**：锁定线路契约——
  schema 非法参数被拒、未知工具报错、结果以未包装的 handler JSON 到达。
- **Stdio E2E（5 个测试）**：经 rmcp `TokioChildProcess` 启动
  `codecortex mcp` 二进制，列出 14 个工具，然后走真实 MCP stdio 协议
  依次操练 `index`/`status`/`search`、图工具
  （`context`/`node`/`explore`/`trace`/`relations`）、分析工具
  （`impact`/`architecture`/`files`/`graph_query`/`adr`）与项目切换的
  缓存隔离。

## 提交前检查

未配置 pre-commit 钩子。提交前运行：

```bash
cargo fmt --all -- --check \
  && cargo clippy --workspace --all-targets -- -D warnings \
  && cargo test --workspace --all-targets \
  && cargo test -p cc-eval -- integration_fixtures_and_corpus
```

真实工作区性能回归：

```bash
CODECORTEX_WRITE_REAL_BENCHMARK=1 \
  cargo test -p cc-eval benchmark_real_workspace -- --ignored --nocapture
```

改动影响测试数/语料数后，运行 `scripts/update-doc-baselines.sh` 核对本
文档的基线数字。
