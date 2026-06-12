# 基准测试

四类基准全部由 eval harness 自动化，且都走真实 MCP 线路（进程内 duplex
JSON-RPC 对 rmcp 路由，含 schema 校验与输出预算）：

| 基准 | 规模 | 产物 |
|------|------|------|
| fixture 冒烟基准 | 18 文件 | `docs/benchmarks/latest.md` |
| 真实工作区基准 | 本仓库拷贝（最近一轮 234 文件） | `docs/benchmarks/real_workspace_latest.md` |
| 增量延迟基准 | 合成 41 文件 TS 项目 | 仅控制台输出 |
| 合成规模矩阵 | 1k / 10k / 50k 文件 | `docs/benchmarks/synthetic_<scale>_latest.md` |

`docs/benchmarks/` 下的报告由测试生成（英文），是逐轮的权威记录；本文档
记录目标值、运行方法与跨轮结论。

## 目标值

### 索引性能

| 指标 | 目标 | 测量方式 |
|------|------|----------|
| 索引速度（文件/秒） | tree-sitter 语言 >500 | 首次连接时 `time codecortex mcp --project-path <repo>` |
| 增量重索引 | 10 个变更文件 <2s | `cargo test -p cc-eval bench_incremental -- --ignored --nocapture` |
| 内存（峰值 RSS） | 10K 文件仓库 <512 MB | `time -l` 或 `/usr/bin/time -v` |

### MCP 工具延迟（p95）

| 工具 | 目标 | 说明 |
|------|------|------|
| status | <50ms | 读缓存元数据 |
| search | <200ms | 排序式 FTS5 + grep 融合 |
| context | <500ms | 多符号提取 + 源码 |
| node | <100ms | 单符号查找 |
| explore | <300ms | 批量符号检视 |
| trace | <500ms | BFS 寻路 + 源码正文 |
| relations | <100ms | 直接边查询 |
| impact | <500ms | BFS 反向调用者扩展 |
| architecture | <300ms | 按 aspect 聚合 |
| graph_query | <200ms | Cypher 子集执行 |

### 检索质量

| 指标 | 定义 | 目标 |
|------|------|------|
| Recall@5 | 期望符号出现在搜索前 5 的占比 | >0.7 |
| MRR | 首个正确结果的平均倒数排名 | >0.6 |
| 工具调用数 | 回答一个问题的平均调用数（context vs search+node 循环） | context: 1，手工: 3–5 |
| 单调用充分率 | 单次 context() 或 trace(source_mode=body) 可回答的任务占比 | >60% |

当前质量基线：24 个 gold 用例（94 corpus 用例中携带 `expected_symbols`
断言的），最近一轮 Avg Recall@5 1.00、Avg MRR 0.92。由
`cargo test -p cc-eval -- integration_fixtures_and_corpus --nocapture`
产出。

## 运行方法

### Fixture 冒烟基准

```sh
CODECORTEX_WRITE_BENCHMARK=1 cargo test -p cc-eval -- benchmark_fixture
```

3 轮热身基准（1 热身 + 2 测量取最小值），结果写入
`docs/benchmarks/latest.md`。不带 `CODECORTEX_WRITE_BENCHMARK` 时测试照
跑，但不持久化报告。把它当冒烟基线；更大仓库的回归基线是真实工作区基准。

### 真实工作区基准

```sh
CODECORTEX_WRITE_REAL_BENCHMARK=1 cargo test -p cc-eval benchmark_real_workspace -- --ignored --nocapture
```

把 CodeCortex 工作区拷贝到临时目录索引，跑 10 个代表性 MCP 用例。最近一轮：
234 文件，全部工具 p95 < 500ms；见
`docs/benchmarks/real_workspace_latest.md`。

### 增量索引延迟

"10 个变更文件 <2s" 的目标由 3 个 ignored 场景测量，对象是一个合成的
41 文件 TypeScript 项目（1 个 hub 模块 + 30 个导入者 + 10 个独立叶子）：

```sh
cargo test -p cc-eval bench_incremental -- --ignored --nocapture
```

- `bench_incremental_noop` —— 零变更重建（扫描快路径 + 跳过分类，无
  解析/解析工作）；
- `bench_incremental_single_file` —— 单文件正文级编辑（单次重解析，
  导出指纹稳定，无脏传播）；
- `bench_incremental_dirty_closure` —— hub 的导出面变化，30 个导入者
  全部经脏闭包提升 Skip → DirtyResolveOnly 并重新解析。

每场景 1 热身 + 5 测量，打印 p50/p95/max 总延迟与
`IndexReport.phase_timing` 的逐阶段分解（scan_diff/parse/resolve/write/
postprocess/analysis，主导阶段在前）。报告计数器与 `dirty_propagation`
状态硬断言（三个场景都应为 `normal`；出现
`budget_exceeded`/`partial_closure`/`disabled` 即失败）。唯一的延迟门是
宽松的 p95 ≤ 2000ms 健全性界——每个场景最多重解析 1 个文件、重新解析至多
30 个导入者，正好包夹"10 个变更文件"的目标；更紧的相对断言在共享硬件上
易抖，刻意不加。

### 合成规模矩阵（1k / 10k / 50k 文件）

确定性合成仓库生成器（`cc-eval/src/synth.rs`：TypeScript/Python/Rust
模块 + 已知调用图——文件内扇出、跨文件桥接链、有界扇入 hub、三元环，外加
YAML 配置与 Express 风格路由文件；`(seed, target_files)` 给定则输出
字节级一致）喂三个 ignored 规模基准：

```sh
cargo test -p cc-eval --release --test scale_bench bench_synthetic_1k -- --ignored --nocapture
cargo test -p cc-eval --release --test scale_bench bench_synthetic_10k -- --ignored --nocapture
CODECORTEX_BENCH_50K=1 cargo test -p cc-eval --release --test scale_bench bench_synthetic_50k -- --ignored --nocapture
```

每轮测量冷态全量索引墙钟与 DB 大小、增量重建延迟（单文件正文编辑与 5%
批量，1 热身 + 3 测量）、search / find_symbol / impact / graph_query /
trace 的 p50/p95——全部走真实 MCP 分发路径。生成器的 ground-truth 事实
兼作规模正确性断言（8 项：needle 进前 5、hub 影响面含已知调用者、调用链
可 trace、环闭合……）。带 `CODECORTEX_WRITE_BENCHMARK=1` 时报告持久化到
`docs/benchmarks/synthetic_<scale>_latest.md`。

最近一轮（release，本机——完整阶段分解见带日期的产物文件）：

| 规模 | 冷态全量索引 | DB 大小 | 增量 p50（1 文件） | 增量 p50（5% 批量） | 工具查询（p50） | ground truth |
|------|--------------|---------|--------------------|--------------------|-----------------|--------------|
| 1k（5,568 符号） | 0.89s | 25.5 MB | 84ms | 252ms | 0–4ms | 8/8 |
| 10k（55,617 符号） | 20.2s | 256.8 MB | 684ms | 3.6s | 亚毫秒 | 8/8 |

## 写阶段优化史（10k 基准）

增量写阶段经历三轮优化，结论沉淀为
[internals/INDEXING.md](internals/INDEXING.md#写阶段性能注记) 的结构性
约束：

1. **批量 FTS5 删除**：10k 5% 批量写阶段 p50 17.8s → 6.1s；
2. **rowid 对齐的 FTS 删除 + 内存 test-edge 匹配**：冷态构建 86s →
   高 10 秒段（17.5–20.2s，随机器负载浮动），单文件写 → ~540ms；
3. **去掉真实 DB 写之外的包装开销**：层级边只为批内文件重生成（不再每轮
   全量 ~46k 边）；框架检测不再因 >20 变更文件回退全仓扫描；config-link
   解析在缓存 token 集为空时短路；逐文件 DELETE 合并为逐表批量 `IN`
   删除；写连接语句缓存扩到 64 槽（默认 16 槽被 ~17+ 轮转语句打穿，
   逐行重 prepare）。

10k 净效果：单文件写 p50 540ms → 58ms，5% 批量写 3.55s → 2.3s——残余的
批量成本是真实的 B-tree/索引/FTS 分词工作，不是分发开销。工具查询延迟
跨规模持平。

## 对比基线

- grep/find 循环：每个问题 5–10 次工具调用
- context(task)：1 次调用，应覆盖 70%+ 的符号需求
- trace(source_mode=body)：1 次调用拿到带正文的完整调用路径
