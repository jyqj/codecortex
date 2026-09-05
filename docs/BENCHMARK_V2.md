# Benchmark v2：可重放的检索质量与增量正确性

本套件评测**非 embedding** 的生产 MCP 检索路径。它与既有吞吐、RSS 和规模
测试并列：性能通过不等于召回正确；全量/增量一致不等于 parser 语义正确。

## 已实现的执行链

`Manifest → 隔离源码快照 → 真实 MCP → raw.jsonl → 独立评分 → report.json`

- `crates/cc-eval/src/quality.rs`：身份、标签、样本协议、验证、独立评分与报告。
- `examples/quality_run.rs`：只在临时目录建立索引；每次调用原始输出立即落盘。
- `examples/quality_replay.rs`：离线重算；与原报告逐字节比较。
- `tests/incremental_oracle.rs`：持久增量会话与独立全量构建逐步比较。
- `scripts/compare_quality.py`：同任务配对、仓库宏平均、受限制的 cluster bootstrap。

生产排序分数从不充当 gold 或评分输入。测量阶段不累积所有响应正文，完成后
才读取 raw 数据评分。输出目录的 `raw.jsonl` 必须不存在，避免覆盖原始证据。

## 运行与重放

在仓库根目录执行；输出目录每次使用新名称：

```sh
cargo test -p cc-eval --locked --test incremental_oracle
cargo run -p cc-eval --locked --example quality_run -- \
  crates/cc-eval/benchmarks/quality_smoke.json target/quality-v2-run1 3
cargo run -p cc-eval --locked --example quality_replay -- \
  target/quality-v2-run1/raw.jsonl target/quality-v2-run1/replayed.json
cmp target/quality-v2-run1/report.json target/quality-v2-run1/replayed.json
python3 -m unittest discover -s scripts/tests -v
```

第四个可选参数是 `.codecortex.json` 形式的配置文件。runner 写入完整默认值展开后
的配置，并强制关闭 watcher 自动索引；这样两次构建不会暗中竞争。对照配置
`benchmarks/no_graph_retrieval.json` 只禁用 graph retrieval lane，不禁用图富化或
预选图邻居。不要把该实验称为“完全无图”。消融引起预期召回门失败时仍保留报告，
但编译/启动失败不能当作质量分数。

```sh
cargo run -p cc-eval --locked --example quality_run -- \
  crates/cc-eval/benchmarks/quality_smoke.json target/quality-no-graph 3 \
  crates/cc-eval/benchmarks/no_graph_retrieval.json
python3 scripts/compare_quality.py \
  target/quality-no-graph/report.json target/quality-v2-run1/report.json \
  target/quality-ablation.json
```

## 数据隔离与标签

manifest 明确列出 repository ID、revision、完整源码文件映射及任务。源码快照
和 manifest 分别记录 Git blob digest。只将 `repositories[].files` 写入索引目录；
任务、gold、参考修复和标签不进入被检索语料。拒绝路径穿越、绝对路径、`.git`、
索引目录和控制配置文件；任务不得通过 `project_path` 越出隔离项目。

每个 label 用 `file_path + start_line + end_line` 定位，可进一步要求 `symbol`。
同名但不同文件不算命中。`anchor` 要求该源码片段真实出现在返回的 `text` 中，
仅有一个定位条目不算“已读到实现”。manifest 加载时校验 anchor 确实位于标注源码。
没有 anchor 的 label 是**定位标签**，不能据此宣传具备完整代码证据。

`required_groups` 是 AND-of-OR：每个组至少找到一个替代标签，全部组满足才认为
声明的证据集齐全。它只检验作者声明的证据，并不证明 LLM 一定能正确完成任务。

## 指标的确切定义

所有指标使用 MCP 返回数组的原始位置。没有名字的项和重复项仍占位置；同一标签
只获取一次覆盖或 novelty 信用。错误样本保留为零分，不从平均分母中消失。

| 字段 | 定义 |
|---|---|
| `recall_at_5` | 原始前五项覆盖的不同 gold label 数 / 全部 gold label 数。 |
| `reciprocal_rank` | 原始排名中第一个相关项的倒数；找不到或调用失败为 0。 |
| `ndcg_at_5` | **novelty 变体**：每项只取其尚未覆盖 label 的最大 `2^grade-1`，按 `log2(rank+1)` 折扣；ideal 以独立标签降序排列。 |
| `evidence_sufficient` | 完整返回数组是否满足全部声明的证据组；与前五名召回分开。 |
| `correct_abstention` | 无答案任务成功返回空结果数组；工具错误不算正确拒绝。 |
| `schema_error` | 输出缺少声明的结果数组；不静默解释成空集。 |

nDCG 的 policy 固定为 `maximum-new-label-gain-per-hit-v1`，不是任意 chunking 下
标准文档 nDCG 的替代品。一项包含多个 gold 区域时，覆盖率可增加多次，但该项的
novelty gain 只增加一次；跨切块策略优先比较区域覆盖与证据集满足率。

报告同时提供逐次观测、按仓库/任务类别汇总、正负例分母、错误数和 gate failures。
完整的 `任务 × cold/warm × repetitions` 样本网格不可缺项、重复或混入未知任务。

## 延迟、预算与可复现性

`cold_session` 是新 MCP session 的首次工具调用；新连接/引擎缓存冷，OS 文件缓存
保留。计时不包括 session 初始化，亦不声称物理磁盘冷启动。`warm_cache` 是相同
会话中一次预热后的相同请求；保存每次延迟，不取 best-of-two。p50/p95 使用原始
请求样本的 nearest-rank 分位数；小样本尾分位只能作为回归观察。

`output_bytes` 是整个**解包后的 handler JSON**序列化字节数，包括重复正文和元数据，
不包括 JSON-RPC framing，也不是 tokenizer 计数。本轮没有实现严格模型 token
预算评测；不能把 bytes/4 包装成精确 `Coverage@8K tokens`。

header 记录实现 commit、rustc、manifest digest、展开后的配置、构建模式、平台、
RUSTFLAGS 和工作区差异信息。比较历史版本时允许只移植相同 eval harness；必须
保留这种 instrumentation 的变更记录，不能偷偷修补被测生产代码。不同 checkout
必须使用**不同 CARGO_TARGET_DIR**，避免同包名/路径布局的旧工作区产物被误复用。

## 配对比较和统计边界

比较脚本要求相同 manifest、purpose、指标 policy 和非空的同任务集合，不通过
交集静默丢掉失败任务。先在每任务内聚合重复观测，再按仓库聚合差值，最后做仓库
宏平均。增加 repetitions 不会增加独立仓库数。

当前 `quality_smoke.json` 是 2 个手工项目、9 个文件、10 个任务（7 正例、3 负例）：
预选误裁剪、显式范围、图候选补召回、跨 80 行边界的符号、同名消歧、正文证据、
中文注释以及不存在的能力/范围。它是**回归夹具，不是 held-out benchmark**。

对 `purpose=regression` 始终输出空的推断置信区间。只有 `held_out` 且至少 5 个
独立仓库时才输出仓库级 bootstrap 区间；5 不是统计充分性的保证。真实质量结论
还需独立仓库、按时间冻结任务、来源/许可证与人工 gold 审核，不能在这些夹具上
反复调参后宣传泛化改善。真实 issue 的未来补丁只能用于离线标注。

## 增量差分 oracle

同一持久会话依次经历签名修改与文件删除；每步都通过 MCP targeted index 更新，
再与独立临时目录中的全量构建比较。初始夹具必须具有已解析跨文件调用，以防
空图与空图相等而假通过。覆盖 TypeScript、Python、Rust 的两文件依赖场景。

比较符号身份/区间、导入、调用目标、引用目标和 chunk 区间，保留关系种类及解析
策略；不比较 rowid、epoch、墙钟时间等非语义字段。该 oracle 不包含所有图表、
并发进程写入、所有语言特性或 parser 独立正确性。新增路径时应扩展快照投影，
不应仅为消除差异而删除实际目标字段。

非 JS/TS 家族缺少完整 export contract，改为有预算的保守传播：`None == None`
不能证明接口未变。传播来源是 imports 加上**已持久化调用/引用的实际目标依赖**，
即使 import 字符串未解析成功，也可刷新已有绑定。Dirty reload 保留源码未变文件
中 `parser_exact` 的本地绑定与来源信息，其他 resolver 绑定仍清除并重解析。

代价是正文级编辑也可能更新依赖者。全局同名候选桶变化、从未绑定的引用、反射、
未纳入依赖面的关系及超预算闭包仍是边界；不能声称一般性全量等价。

## 后续扩展的验收顺序

先补真实仓库冻结快照及独立 labels，再引入阶段级候选召回、精确 tokenizer 预算、
agent 实际修复/解释成功率、连续编辑与 watcher 故障序列。既有 1K/10K/50K 规模
测试继续覆盖性能；质量优化必须报告召回与开销的共同变化，而不是只公布 warm
缓存命中的速度。
