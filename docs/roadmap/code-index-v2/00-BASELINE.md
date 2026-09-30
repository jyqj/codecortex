# 00｜基线、问题证据与参考映射

## 1. 审计边界

2026-09-27 复核的本地项目为 `/Users/jin/Desktop/codecortex-rust`，`main@4514630dcd26481cf6dbc2aff38824ed71ef06da`，HEAD 与前一轮审计相同。本轮开始时已有本目录 README、00-BASELINE、01-ARCHITECTURE 三份未提交草稿，未发现业务源码的已跟踪修改；本轮沿用并补齐，不把工作区表述为干净。上轮已盘点全仓并重点阅读索引→检索→MCP 链路；并非逐行验证所有实现。

本轮沿用前轮对存储/并发、SearchPlan、FTS 预选、dirty 传播、catalog cache 和身份的审计证据，并实际定向复核 DESIGN.md、文档入口、schema 管理、查询锁入口及现有 eval 类型。当前实际 schema 是 **v7**：`crates/cc-db/src/index_migrate.rs:7–12`；部分现有文档仍写 v6。这里按源码记录，不沿用旧文档数字。

## 2. 已有验证，不冒充本轮重跑

| 证据 | 上轮结果 | 适用边界 |
|---|---|---|
| `cargo test --workspace --locked --offline --lib --tests` | 1307 passed，16 ignored，exit 0 | 现有本地缓存环境；本轮未重跑，不等于冷构建通过 |
| `cargo test --workspace --locked --offline` | doctest 链接失败 | macOS SDK/TAPI 对 `arm64e.x1` 的识别失败，不能认定为 Rust 业务断言失败 |
| `cargo build --locked --offline -p cc-server --bin codecortex` | 同类 SDK/链接失败 | 未得到这条命令的成功冷构建证据 |
| 内存 SQLite + 当前 BM25 转换公式 | 分数顺序反转被复现 | 隔离 SQL/公式实验，不是新增端到端 MCP 回归 |
| 前轮最后 Git 检查 | HEAD 不变、工作区干净 | 仅指前轮审计收尾；本轮启动时已有三份未提交规划草稿 |

这组证据在本轮上下文已有，P0 要落新的机器可读证据及构建条件。不能把 ignored 的规模/soak 测试当作通过，也不能把快照测试数量当作缺陷不存在的证明。

## 3. 基线问题登记

| ID | 位置（均相对本基线） | 已确认事实 | 尚需验证 / 对应阶段 |
|---|---|---|---|
| B01 | `cc-search/src/preselect.rs:330–336`；`cc-db/src/index_db_retrieval.rs:122–172` | 原始 FTS5 分数经过 `abs` 再取 `1/(1+x)`，与“更负更好”反向 | 补真实 preselect/MCP 回归；P1 |
| B02 | `cc-search/src/plan.rs:121–125,211–213` | 软预选文件被写进 request.file_paths 并成为最终过滤 | 补显式空范围、预选外精确命中、graph/dense 预留；P1/P5 |
| B03 | `cc-index/src/indexer_phases/dirty.rs:157–174,231–256`；Rust/Python 符号构造 | 指纹选 export_name/default_export；源码注释承认非 JS/TS 与部分重导出缺口 | 修改公开签名→未修改导入者的真实全量/增量差分；P2 |
| B04 | `cc-index/src/resolver/catalog_cache.rs:34–45` | 缓存目录与冷目录的同分候选可能因桶顺序选不同目标 | 固定候选顺序并区分真歧义；P2 |
| B05 | `cc-index/src/indexer.rs:758–814`；`cc-parsers/src/import_resolver.rs` | 基础 JS/Python 路径解析 + 独立 Rust workspace fallback | 不把 config_linker 当成编译器模块解析；P3 |
| B06 | `cc-parsers/src/chunker.rs:32–44,62–145` | 顶层符号为主、长符号按行切、gap 分支未统一预算 | 新切块覆盖/大小/原文一致性夹具；P4 |
| B07 | `cc-model/src/id.rs:35–38` | chunk id 是路径+序号，没有区分文档内容版本与向量产物 | 不能据此认定现有 epoch 文本缓存已坏；P4/P6 |
| B08 | `cc-search/src/lanes.rs:62–140,166–229` | 静态同步 lane 注册、join 等待、无完整 optional-lane 生命周期 | 依赖注入、deadline、取消、结构化失败；P5 |
| B09 | `cc-server/src/handlers/context.rs:7–28` | 现有查询在 CodeIndex 读锁内完成 | 本地同步现状不等于 bug；禁止将未来网络等待放入此锁；P5/P7 |
| B10 | `docs/MCP_TOOLS.md:58–67`；`handlers/output_budget.rs` | 一部分结果超预算整体改成前缀预览信封 | 语义化 pack 与完整结构化证据；P5 |
| B11 | `cc-eval/src/runner.rs:345–401` | 一类质量评测以符号名称匹配 Recall@5/MRR | 改用文件+符号+范围；保留 legacy 断言兼容；P0/P8 |
| B12 | `docs/benchmarks/real_workspace_latest.md:3–11` | 历史报告 10 cases、warm 取两次较好值 | 不能据此声称真实 p95；P0/P8 |
| B13 | `cc-db/src/index_migrate.rs:7–21` 与存储/架构文档 | 实际 v7 与文档 v6 漂移 | 文档事实生成与漂移检测；P0/P8 |
| B14 | `cc-index/src/indexer_phases/dirty.rs:94–107` 与 resolver 阶梯 | 当前闭包以 importers 为主要依赖来源；解析还有全局/模糊路径 | 新增同名符号、负向查找失效需场景复现；P2 |

表中的 crate 简写均在仓库 `crates/` 下。B03/B05/B14 不提升为“所有语言/所有场景必然失败”，实施以最小夹具及规范支持范围判定。

## 4. 保留资产

| 能力 | 处理 |
|---|---|
| seven-crate 单向依赖、MCP-first | 保留；cc-semantic 仅在 P6 新增且可关闭 |
| scoped scan、共享 WalkManifest、并行解析、memory budget | 保留；仅补正确性与可观测成本，不重写扫描引擎 |
| PreparedBuild、build gate、分段 commit | 保留；增加新产物/版本守卫，网络永不进入锁区 |
| WAL、FTS 镜像、multi-insert、zstd 前置 | 保留；新表沿用类型化事务，不退回逐文件逐行写 |
| seed/catalog/file-state cache | 保留性能收益，补稳定性与等价验证 |
| GraphExplain、BuildExplain、score_trace | 延伸语义，不另造互相矛盾的解释体系 |
| framework/infra/dispatch/community/Cypher | 保持功能覆盖，先建立回归；非本轮重写对象 |
| ProjectSession | 仅视为 MCP 项目资源生命周期，不扩展用户会话系统 |
| overlay_files | 当前只证明路径级提示；不宣称已索引编辑器未保存文本 |

## 5. 外部参考（冻结 SHA）

### R-A：Astrolabe

仓库 `wojiushirencai/Astrolabe`，SHA `3e230f7a26555eab42ef039bda25834afca338e0`。

- `crates/astrolabe-core/src/resolvers/typescript.rs`：项目元数据 detect/resolve 分离、别名作用域、配置继承、workspace 入口。映射 P3，不照搬输出目录猜测和固定 exports 条件顺序为语言真理。
- `crates/astrolabe-core/src/lsp/pool.rs`：按需启动、失败冷却、idle/RSS 管理、generation/in-flight 生命周期。映射 P9，参数必须重测。
- 不把其索引范围与 CodeCortex 的全部图/框架/全文功能视为等价，不直接引用加速倍数。

固定来源：
`https://github.com/wojiushirencai/Astrolabe/tree/3e230f7a26555eab42ef039bda25834afca338e0`

### R-O：OCE

仓库 `oce-ai/oce`，SHA `da312e1b8a6ece7a239a4c5df880a2f6b967ccc7`。

- `src/oce/infrastructure/astchunk/cast_chunker.py`：AST 确定边界，正文由源码范围取得，小块合并。映射 P4。
- `src/oce/domain/services/retrieval.py`：召回、rerank、selector 分离及依赖注入。映射 P5/P7。
- `src/oce/domain/services/selector/coverage_selector.py`：覆盖、重叠、每路径配额。注意其首个候选可以绕过一次预算判断；本项目测试首块也必须受限。
- `src/oce/infrastructure/embed/openai_embedder.py`：批量约束、并发、查询指令、结果数量/维度校验。映射 P7；不默认采用超长输入平均池化。
- `src/oce/domain/services/indexing.py`：staging、pending、embedding 开关与 ready 语义。映射 P6/P7；CodeCortex 需独立 lexical/structural/semantic 状态。
- 参考其分层，不引入其完整服务部署作为依赖。个人/服务模式的产品事实以冻结源码为准。

固定来源：
`https://github.com/oce-ai/oce/tree/da312e1b8a6ece7a239a4c5df880a2f6b967ccc7`

### R-B：OCE Benchmark

仓库 `oce-ai/oce-benchmark`，master commit `d4f10554a18e31599d1e46d5d56da6588d4aa86c`，commit 时间 `2026-09-04T09:43:58Z`。本轮通过 GitHub 连接器读取 evaluator 完整返回范围、lock checker、evaluation guide、出题 skill 主体和两份 metadata；没有实际运行上游评测或逐题审阅全部 200 道标准答案。

- `scripts/run_retrieval_eval.py`：公开 HTTP、路径去重、任一期望文件 Top-1、线性 gain nDCG@10、suite/错误和资源统计。
- `scripts/verify_benchmark_lock.py`：固定 commit 检查；本项目扩展为 dirty/实际输入 manifest 的严格验证。
- 两份 metadata 分别固定 cc-switch `40cac1a68edf8c9e7b3a89125cf40bb93a348404`、Flask `22d924701a6ae2e4cd01e9a15bbaf3946094af65`，均声明 100 题；导入时仍需核验实际题数和答案。
- 本次未发现明确的 LICENSE 声明，不能默认为可直接复制代码/题库。默认独立实现方法、接受外部数据路径，并记录来源与使用权限。

详细行为映射、评分 profile、模块文件树、数据集治理和各阶段门禁见 [09-BENCHMARK.md](09-BENCHMARK.md)。固定来源：
`https://github.com/oce-ai/oce-benchmark/tree/d4f10554a18e31599d1e46d5d56da6588d4aa86c`

### R-S：SQLite

官方 FTS5 文档 `https://www.sqlite.org/fts5.html#the_bm25_function`：BM25 返回更优匹配的数值更低。2026-09-27 本轮重新查阅；仅支持 B01 的分数语义，不用外部文档替代本地实现证据。

## 6. 后续变更的证据规则

每次开工记录目标 SHA/dirty 文件/语言与构建工具链。源码位置移动时更新映射而非伪造行号。外部代码借用必须固定文件 SHA、核对具体许可证/NOTICE/第三方归属；参考思想与直接复制代码分别登记。本轮没有复制外部实现。
