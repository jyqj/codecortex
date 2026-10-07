# P7-012 原始验收范围复核与本轮交付

## 判断

**P7-012 的工程机制验收已具备结项证据；它不以单独完成 V19 全部六仓、holdout、统计和真实模型效果认证为前置。** 任务状态升级仍须先完成 P7-011 硬依赖，并由主线程接受、整合这次固定源码及独立审查。此结论依据原始 P7-012 任务细则，不是从 P7-001～010 已 done 推导出来的豁免。V19 整体与 P7/G7 阶段结论保持 open。

参考源码：`f6c860c96f22e39bc3ced5a81891e3070ffbada5`，tree `50890bf1bd5adfe1b38cb12270ffad6ded7b83fd`。本复核不修改仓库任务状态、验收条款或 gate。

## 原文如何分配责任

| 原始权威与位置 | 原文含义 | 对 P7-012 的约束 |
|---|---|---|
| `tasks.json`，P7-012 | 独立 dense rank RRF、partial/unavailable 透出、不混 cosine/BM25；验收是“timeout与无命中可区分，declared full coverage有证据”；validations 为 V11/V19 | 保留这些字段。必须证明该机制的状态、融合和覆盖真实性，不能只交一个新类型。 |
| `artifacts/checkpoints/p7-implementation-planning-20261002/TASK-BRIEFS.md:413–449` | 443–445 明确要求 Timeout + semantic_deadline 对比 Complete + candidate_count=0；446–449 要求 full 声明回指 lane receipt/SourceVerifier 诊断，并以改变 raw score 不改变 fused 序检验不混分数 | 这是直接的任务级 V11/V19 验收映射。没有把六仓质量集作为这一机制任务的独立完成门。 |
| 同目录 `IMPLEMENTATION-ORDER.md:159–174` | G3-P7 将 V11/V19 明确列为 timeout/empty、full coverage、cosine/BM25 这三类断言 | 与 task brief 一致；仍不能用单项完成声称整批 011～015 通过。 |
| `04-PHASES.md:121–132` | 011～015 为 dense 过滤/融合/部分覆盖、deadline/status/MCP、竞争资源；016～020 为 fake 故障、离线默认、条件 live、质量/费用消融、G7 | 机制实现与整包质量/成本实验的责任是分别列出的。 |
| `TASK-BRIEFS.md:634–661` 与 `IMPLEMENTATION-ORDER.md:112–120` | P7-019 的 fake 腿验证三臂、费用、CI 可复算和同输入同预算；真实 corpus + provider 的 V19 全口径质量腿 blocked，归 P8 live 线 | 整体消融不能由这次 012 的小夹具顶替；也不能倒置为 012 先完成 019/后续质量线。 |
| `TASK-BRIEFS.md:665–687`、`06-VALIDATION.md:57–61` | G7 工程/fake 与 live 效果分别记录；每项仍需当前 run-id | 所有该测的机制负例仍要通过；没有真实模型证据便不能发布真实语义收益结论。 |
| `09-BENCHMARK.md:146–159` | 数据集表标题明确是规划目标；D2 是六仓约 600 题，D5 held-out 标 P8 认证 | 六仓和干净 holdout 是真实质量路线的要求，没有消失，也没有文本要求每一项机制修复独立重复整个路线。 |
| `09-BENCHMARK.md:179–189` | 全口径消融要求配对、多轴统计/CI；PR fast、nightly、release 的范围分开 | 这次测试不能冒充全质量实验、完整 holdout、release 性能或真实模型认证。 |
| `02-CONTRACTS.md:124–126`（C16） | 完成需接线、正负回归、旧回归、性能/质量对照、文档、恢复/回滚、当前 SHA 证据 | 不是免验收。本轮用当前真实读路径故障对照、融合数值不变性、受限内存/读边界、恢复和完整回包覆盖机制责任；不声称 release SLA。 |

这里“full coverage”是 lane 对已声明 hard scope 的运行/数据覆盖声明，不是“找回了所有语义相关答案”的质量保证。`C10` 的 lane 状态、`C12` 的降级缓存规则和 task brief 的 receipt/SourceVerifier 指向共同限定了它的含义。`09-BENCHMARK.md:79` 另外明确 timeout 不能算正确 no-answer。

## 与 P7-001～010 既有 done 的关系

`artifacts/checkpoints/p789-blocking-analysis-20261002/DECISIONS-RECORDED.json` 的 D1+D2 记录了不授权真实付费 provider、local/live 双轨与后续质量腿 blocked；它本身明确不代表任务已实施。

`round14/p7-batch1-closure-audit.json` 和 `round15/p7-batch2-closure-audit.json` 均明确逐项 PASS 后只翻对应任务状态；`dual_track_accounting` 保留 live blocked，`red_lines.no_gate_inference` 禁止由 mock done 推断正式 gate 通过。这能验证项目已有“任务工程验收与完整阶段认证分别记账”的做法。**不能因此把一切 V11/V19 都改成 live-only 或自动豁免。** 012 的依据仍是上表直接写出的自身验收映射。

早先快速审计中“V19 全域未闭合，因此 012 一定只能保持 open”的判断过宽。本次找到直接任务细则后修正该责任归属；不修改或抹掉旧的 V19 失败、inconclusive、holdout custody 等记录。

## 本轮实际修复及验收

基线生产 `65eb87d70bd7bfd10d251b5d1cbb25850958196b`。独立 fixture commit `6d860d0a9fc28bc89331789e6c9e1c4e0a7197be` 只新增 `crates/cc-server/tests/p7_dense_artifact_coverage.rs`；4 个测试在原生产上 **2 pass / 2 fail**，缺失与损坏均错误返回 Complete。最初 fixture 的私有 digest 构造编译错误另行保留，未计为行为红例。

生产修复独立 commit `f6c860c96f22e39bc3ced5a81891e3070ffbada5`，只改 `crates/cc-semantic/src/vector/exact.rs` 与 `crates/cc-server/src/semantic_wiring.rs`。原 Vec API 保持；新增每次 exact 扫描的缺失、损坏和无效引用计数，只计通过行空间与 hard scope 的候选。无新增修复 IO 或 ledger 写入。active-space Unavailable 和既有 publication Partial 优先；其余 Complete 只要丢过 eligible artifact 就变为不可缓存的 Partial，同时保留有效候选。

| 当前固定源码上的验证 | 结果 | 覆盖的任务条件 |
|---|---:|---|
| 冻结的新 artifact fixture | 4/0/0 | 真实 worker 发布；missing/corrupt；有效候选保留；空 Partial 与合法空 Complete；重复 warm 失败/恢复/再次失败，epoch 不变；hard scope、k=0、inactive space；最终源证据回包 |
| exact 单元 | 21/0/0 | 全 scan 计数、过滤/空间先行、bounded batches、旧 Vec 的 score bits/排序、取消/超时错误、数值 oracle |
| 原 5 个 HTTP feature targets | 13/0/0 | 实际 stdio/loopback 的范围与非单位向量 oracle；原 acceptance matrix 的超时/错误/容量、重复恢复、完整回包 |
| 原 manifest 与 allocation 集成 | 11/0/0 | 实际 SQL 空间隔离/删除、cache 输入与内存边界、损坏与 IO 错误区别 |
| 既有 fusion 单元 | 10/0/0 | rank-only、raw-score 不变性、Partial 可融合但状态不折叠、failed lane 零票、timeout ≠ Complete+0、完整覆盖回指诊断、稳定 ties |
| strict clippy（semantic tests；server lib + 新 target）及 workspace fmt | 通过 | 当前源码静态与格式门 |

合计 **59 个所选 Rust 测试，0 fail / 0 ignored**，不把基线的重复运行加入总数。每条命令前后 769 个 Cargo/crate 输入完全相同，manifest SHA-256 `bb08eb49c0740ccf19cc54dc18fa31674a32c5c78ade4332cd6122b7e80cfeb1`。修复后 stdio binary SHA-256 `a9bbd171437f573a4ea7f7c2827d999726772fb4220c3be7c72248c88527980e`；各 target executable 独立记录。未修改原 oracle/gold、budget、任务与 registry。

独立审查由 `build_environment` 读完整三条源码及 worker 生命周期边界，结论无阻塞；没有重复执行这些 Rust 测试。原样报告在 `p7-012-evidence/independent-review.json`，作者执行结果在 `p7-012-evidence/validation.json`，两者执行归属分开。

## 允许与仍须保留的状态

主线程在接受精确源树、绑定当前测试/审查并将 P7-011 依赖按证据完成后，可以将 **P7-012 的工程机制任务改为 done**，不删 V11/V19，也不改原 acceptance。implementation_notes 应说明此次实际修复位置偏离原 scope 草案的原因：融合和 explain 已在现有代码实现，遗漏的真实性缺口在 exact 结果丢弃 cache 失效信号与组合根覆盖声明之间。

仍需独立保持未完成的范围：V19 多仓 corpus/compat/native/holdout/facet-span/统计与真实语义收益；P7-018 条件 live；P7-019 自己的机制消融与质量/费用证据；P7-020/G7 整体工程和发布范围验收。这次不出 full-workspace-green、完整 P7 或真实 provider 成功结论。现有 65eb87 默认范围的四项环境/性能失败仍由主线程保留其原记录。

回退继续使用已有 local strategy / 关闭 semantic，也可只回退本次两个 commit；没有 schema、缓存布局或持久化写入迁移。实际 cache 文件恢复后，同 epoch 的完整响应恢复已由回归直接执行。
