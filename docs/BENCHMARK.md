# 基准测试

四类基准全部由 eval harness 自动化，且都走真实 MCP 线路（进程内 duplex
JSON-RPC 对 rmcp 路由，含 schema 校验与输出预算）：

| 基准 | 规模 | 产物 |
|------|------|------|
| fixture 冒烟基准 | 18 文件 | `docs/benchmarks/latest.md` |
| 真实工作区基准 | 本仓库拷贝（最近一轮 330 文件） | `docs/benchmarks/real_workspace_latest.md` |
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

### MCP 工具延迟（p95，warm 口径）

下表 p95 目标按 **warm 口径**（同会话缓存命中路径）衡量；cold（新会话
首查，各级缓存为冷）仅在基准报告中列报，不设门槛。

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

工具延迟以 µs 粒度分 cold/warm 两列：cold 为每用例新建 MCP 会话后的首次
调用（新 IndexDb 连接 → 新 db_identity，graph 邻接缓存与 SQLite 页缓存为
冷、SearchEngine LRU 为空；OS 文件缓存保留），warm 为同会话 1 热身 + 2
测量取最小值（缓存命中路径）。结果写入 `docs/benchmarks/latest.md`。
不带 `CODECORTEX_WRITE_BENCHMARK` 时测试照跑，但不持久化报告。把它当
冒烟基线；更大仓库的回归基线是真实工作区基准。

### 真实工作区基准

```sh
CODECORTEX_WRITE_REAL_BENCHMARK=1 cargo test -p cc-eval benchmark_real_workspace -- --ignored --nocapture
```

把 CodeCortex 工作区拷贝到临时目录索引，跑 10 个代表性 MCP 用例。最近一轮：
330 文件，全部工具 p95 < 500ms；见
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
trace 的 cold/warm 延迟（µs 粒度；cold = 每次迭代新建 MCP 会话使各级
缓存失效后的首查，3 次；warm = 同会话重复相同调用，1 热身 + 7 测量）——
全部走真实 MCP 分发路径。生成器的 ground-truth 事实
兼作规模正确性断言（8 项：needle 进前 5、hub 影响面含已知调用者、调用链
可 trace、环闭合……）。带 `CODECORTEX_WRITE_BENCHMARK=1` 时报告持久化到
`docs/benchmarks/synthetic_<scale>_latest.md`。

最近一轮（release，本机——完整阶段分解见各规模的产物文件）：

| 规模 | 冷态全量索引 | DB 大小 | 增量 p50（1 文件） | 增量 p50（5% 批量） | 工具查询（cold p50） | ground truth |
|------|--------------|---------|--------------------|--------------------|-----------------|--------------|
| 1k（5,568 符号） | 0.85s | 24.7 MB | 64ms | 218ms | 0.2–21ms / warm 亚毫秒 | 8/8 |
| 10k（55,617 符号） | 10.0s | 244.0 MB | 345ms | 2.47s | 0.2–100ms / warm 亚毫秒 | 8/8 |
| 50k（278,074 符号） | 82.7s | 1220.6 MB | 1.82s | 17.1s（2400 文件） | 最大 631ms（`search_hybrid_mixed_terms`），其余毫秒级 / warm 亚毫秒 | 8/8 |

- 三档报告均以 µs 粒度分列 cold/warm（cold = 每次迭代新建 MCP 会话；warm =
  同会话缓存命中），冷查询延迟以 cold 列为准；上表"工具查询"取 cold p50 区间。
- 冷态全量索引 604–1180 文件/s（50k–1k）：10k 已达 1000 文件/s，超过 >500
  目标；50k 604 文件/s，write cascade 已治（见下第五轮），残余 gap 转移到
  单文件增量的 resolve 地板（`add_symbols` seed O(repo)，见末段）。

## 写阶段优化史（10k 基准）

增量写阶段经历四轮优化，结论沉淀为
[internals/INDEXING.md](internals/INDEXING.md#写阶段性能注记) 的结构性
约束：

1. **批量 FTS5 删除**：10k 5% 批量写阶段 p50 17.8s → 6.1s；
2. **rowid 对齐的 FTS 删除 + 内存 test-edge 匹配**：冷态构建 86s →
   高 10 秒段（17.5–20.2s，随机器负载浮动），单文件写 → ~540ms；
3. **去掉真实 DB 写之外的包装开销**：层级边只为批内文件重生成（不再每轮
   全量 ~46k 边）；框架检测不再因 >20 变更文件回退全仓扫描；config-link
   解析在缓存 token 集为空时短路；逐文件 DELETE 合并为逐表批量 `IN`
   删除；写连接语句缓存扩到 64 槽（默认 16 槽被 ~17+ 轮转语句打穿，
   逐行重 prepare）；
4. **签名门聚合化 + seed 缓存**（本轮）：postprocess 签名门输入改为
   写时维护的行哈希聚合（`graph_sig_aggregates`，不再全表扫描）、
   community 门在聚合空间投影决策（仅判 RUN 才载边）、resolver seed
   引入跨构建缓存（`symbols_seed` token 校验）——10k 单文件增量总延迟
   684ms → 345ms（postprocess 251ms → ~0），5% 批量 3.6s → 2.98s。

10k 净效果：单文件写 p50 540ms → 58ms，5% 批量写 3.55s → 2.3s——残余的
批量成本是真实的 B-tree/索引/FTS 分词工作，不是分发开销。工具查询延迟
（warm 口径）跨规模持平。

### 50k 写阶段：FK CASCADE 逐父行开销（第五轮）

10k 上残余的批量成本到 50k 暴露一个被 976a626（写连接 cache+mmap）压低
但仍存在的热点：`db_replace_delete`（替换前删除批文件数据）里的
`DELETE FROM files` 经 ON DELETE CASCADE 逐父行触发子表删除。SQLite 的
CASCADE 对每个 files 行触发一次各子表 DELETE（父行数 × 子表数次内部语句
+ FK 检查），在多百 MB 库上比"每子表一次批量 `DELETE … WHERE file_path
IN`"慢 2–5 倍。50k 5% 批量（2400 文件）实测 `incremental_batch` ~38s，
其中 `db_replace_delete` ~24s（files cascade 每批 200 文件 0.4–2s）是绝对
大头；FTS 触发器（`symbols_fts`/`file_paths_fts`）经隔离实验确认只占
~0.6%（单次 ~12µs × 文件符号数），真正成本是逐父行 CASCADE 的语句/FK
开销 + 子表索引维护。

修复：`delete_files_data_chunk_keep_test_edges` 在 `DELETE FROM files` 前
显式批量删除 6 个 CASCADE 子表（`call_edges`/`symbol_refs`/`chunks`/
`imports`/`literal_index`/`symbols`，routes 已在前序循环），使 CASCADE 无
行可删。50k 5% 批量 write p50 **~36s → ~14.3s（-60%）**，ground truth 8/8
不变；10k 上收益小（CASCADE 本就轻）。残余 write 成本集中在
`db_replace_insert`（symbols/call_edges 行 INSERT + 多索引维护）与
`db_commit`（WAL fsync），是真实存储引擎工作。详见
[INDEXING.md](internals/INDEXING.md#写阶段性能注记)。

## 冷态全量索引：resolve 阶段 O(N²) 定位与修复

50k 首次实测曾暴露**冷态全量索引的显著超线性**（10k 17.9s → 50k 432s，
5 倍文件 24 倍时间，~116 文件/s）。分相位 profiler（`profile_cold_build_scaling`）
+ `time_step` 子相位计时把它精确定位到 **resolve 阶段**（16k 时占墙钟 65%，
1k→16k 时间比 290×，指数 ≈ N²·⁰⁴），再用段级原子计时下钻到 **symbol_refs
的名字解析**：

- 根因：tree-sitter 解析器把**函数局部变量**（`left`/`right`/`value`/`label`）
  也并入全局 `by_name` 目录，这些名字的候选桶随文件数线性增长（16k 时单桶
  ~5000）。对它们的引用按名解析时构建巨型 fuzzy pool，再逐候选做
  `is_import_reachable`（含字符串分配）——O(桶) per ref × O(N) refs = O(N²)
  （16k 实测 `global_cand` 累加 1.57 亿）。
- 修复：候选上限 `CODECORTEX_RESOLVER_MAX_POOL`（默认 256）——同名符号超上限即
  判名义不可解（global-unique / fuzzy / `find_best` 兜底均早停）。被数百符号
  共享的名字本就无法靠路径启发式消歧（import-distance 选一个是噪声），早停为
  未解析既消除 O(N²) **又提升精度**（局部变量引用不再解析到他文件随机同名）。
  辅以 `find_best` 同文件优先改走 `by_file_*` 嵌套索引（O(1)）、suffix 阶用
  `by_qname_leaf` 叶段索引替代整表扫描。

净效果（ground truth 全程 8/8，DB 略减——少了噪声 REFERENCES 边）：

| 规模 | 冷建（前→后） | resolve 子相位 | 5% 批量 resolve |
|------|---------------|----------------|-----------------|
| 16k（profiler） | — | 29.1s → 1.4s（21×） | — |
| 10k | 17.9s → 10.0s（1.78×） | — | 553ms → 155ms（3.5×） |
| 50k | 432s → 121.8s（3.55×） | — | — |

resolve 不再是瓶颈。write 阶段的 FK CASCADE 逐父行开销经第五轮修复（见
写阶段优化史，50k 5% 批量写 ~36s → ~14.3s）；`chunks_fts` external-content
方案评估为 NO-GO（与 rowid 对齐删除路径冲突，需删除时解压 chunk 文本）。
**单文件增量 resolve 的 O(catalog) 地板（50k ~0.8s，随 seed 符号数超线性）
是下一轮课题**：`build_catalog` 的 `add_symbols` 把全部 seed 符号（278k）
重建进 9 个 catalog map，`seed_symbol_cache` 已缓存 seed Vec 但 catalog map
每 build 重建；深度优化 = SymbolCatalog 跨 build 复用（增量更新），中等
复杂度架构改动。

## 对比基线

- grep/find 循环：每个问题 5–10 次工具调用
- context(task)：1 次调用，应覆盖 70%+ 的符号需求
- trace(source_mode=body)：1 次调用拿到带正文的完整调用路径
