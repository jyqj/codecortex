# P1-A 实施与验证报告

2026-09-27，HEAD `4514630dcd26481cf6dbc2aff38824ed71ef06da`，本轮改动未提交。**P0-020/G0 已先收口，随后 P1-001–005 全部完成。累计 25 done、167 todo；P1 全阶段/G1 尚未完成。** 没有推送、PR、合并或付费模型调用。

## 1. 前置 G0 证据

见 [P0-CLOSEOUT.md](P0-CLOSEOUT.md) / [P0-GATE.json](P0-GATE.json)。在子进程 SDK15.4 环境下，Rust1.95.0 与 stable1.97.0 均通过严格全仓 Clippy（含 eval-http）、完整测试/doctest、二进制构建、真实 stdio。原五处 lint，以及最低版本额外发现的 C/C++ guard / oracle 布尔条件均作等价修正，未降低 -D warnings。Rust 默认工具链和系统 SDK 未改。G0 冻结证据先落盘、任务先更新，之后才改 P1 检索逻辑。

## 2. 已实施的五项任务

| 任务 | 实现 | 验收依据 |
|---|---|---|
| P1-001 | P0 B01/B02 观察夹具转为正向断言，补公共 API 与真实 MCP 红绿测试 | 旧实现 B01/B02 断言失败；旧 G0 MCP 两项失败；新实现全部通过 |
| P1-002 | 文件摘要 BM25 用 `base + x/(1+x)`，x=max(-raw,0)，保持单调、有界、权重不变 | 真实 SQLite/preselect/MCP；有限数单调测试；保留原始负分诊断 |
| P1-003 | 新增 HardScope/SoftHints，范围模型、SQL、调用方参数与缓存完整接线 | None 与 Some(empty)序列化/行为区分；交集属性测试；公共 API 空范围与缓存回归 |
| P1-004 | 不再把 preselected 文件写回 request.file_paths；软线索只影响打分 | 无关 pinned/recent/boost + limit1 不再排除合法词法/精确目标 |
| P1-005 | 参数与所有重复 path/lang DSL 取交集，未知/空过滤显式失败；图补充同范围 | 参数/DSL冲突、重复限制、空列表、图补充跨范围回归，真实 MCP 验证 |

### 接线中额外确认并修复的根因

`cc-server::build_context_search_request` 原来丢弃 `file_paths/languages`，已显式传递，避免上层类型看似支持而查询不执行。`ChunkScope` 原来把空集合当不限范围，已生成 SQL 假条件。query hash 原来未区分可选集合字段的有无，空硬范围与空软提示可以别名；现在逐字段保留有无标记，软提示因 rank-decay 保留顺序。图补充节点原来能在主命中正确过滤后再次越界，现在在去重与预算前筛选。

以上是本批范围契约的必要修复，不把 P1-009/P1-011 后续完整矩阵提前标 done；对应任务已注明完成部分，后续不重复实现。没有修改 P2 的增量解析算法，没有接 embedding。

### 旧测试如何迁移

旧 pinned 列表“交换顺序缓存键相同”的断言违反已有 rank-decay 语义，改为顺序不同键不同。图排序的历史固定分数没有直接用新输出覆盖：保留原 alpha/beta 常量，利用原始 SQLite BM25 独立推导唯一预期增量 `0.04*(x-1)/(x+1)`。图连通度、排序翻转与其余权重继续锁定。

初次 MCP 红测试因夹具少传 index.path 而失败，这条属于无效测试准备证据，已在 `red/evidence-classification.json` 标明；正式红证据使用修正夹具后的 `red-v2/stdio.log`，失败原因确实为排序和漏召回，而非握手/参数错误。

## 3. 最终验证（冻结后的同一源码）

原始 receipt：`artifacts/benchmarks/p1a-20260927/final/validation.json`，覆盖源码/配置/夹具/题库/CI与验证脚本摘要；收尾复核无漂移。

| 检查 | stable 1.97.0 | Rust 1.95.0 |
|---|---:|---:|
| 全仓 all-targets + eval-http Clippy，-D warnings | exit0 | exit0 |
| workspace 全部测试，含 doctest | 1366 passed / 0 failed / 19 ignored | 同左 |
| cc-eval + eval-http | 80 passed / 0 failed / 13 ignored | 同左 |
| workspace binaries / eval binary 构建 | exit0 | exit0 |
| 显式 P1-A MCP 子进程回归 | 2 passed | 2 passed |
| 显式原 P0 MCP 子进程回归 | 1 passed | 1 passed |

`cargo fmt --all -- --check` 和 `git diff --check` 通过。不同命令重叠，不把通过数相加；19个默认ignored不是自动通过，其中本批MCP按显式命令另行验证。未运行GitHub Actions/Linux；CI已经加入P1-A真实MCP步骤，但配置存在不等于远端执行成功。

## 4. 固定输入的真实 MCP 配对 benchmark

使用**同一 runner、同一冻结源码、同一题库/答案、同一配置和评分器**，只更换 G0/P1-A 产品 binary。原始 P0 七文件子集和三文件 smoke 在改 P1 之前复制、原 digest 验证通过。没有把目标工作区同步变化混入对照。

| 数据集 | 独立问题 / 测量行 | G0 Top1 → P1-A | G0 nDCG@10 → P1-A |
|---|---:|---:|---:|
| 原 CodeCortex 七文件子集 | 14 / 42 | 0.8571428571 → 0.8571428571 | 0.9285714286 → 0.9379235538 |
| 原 smoke | 11 / 33 | 0.700000 → 0.700000 | 0.700000 → 0.700000 |

子集变化仅 R12：期望文件从第三位升至第二位，单题 nDCG 0.5→0.6309297536，其余13题指标不变。全体平均 +0.0093521253。按7个独立query-family得到的差值区间下界为0，不声明显著提升；42请求不足以认证尾延迟，正式 compare 仍返回 `inconclusive`/exit1。没有把一次墙钟更快写成加速结论。所有前后结果均通过离线 replay，metrics 摘要未变。

smoke 的质量均值按10个有答案问题计算；第11个 no-answer 仍失败，前后 run 都保持 exit1，没有删题或换gold。Python/Rust 两项签名变更 mutation 也重跑，仍 exit1，保留 P2 目标UID过期问题。

### 当前源码题库的独立迁移

CI仍须验证当前源码题库。因为归一化代码从 plan.rs 移到 scope.rs，当前工作区 corpus 显式增加 scope.rs / retrieval.rs；R13记录两个真实定义位置的替代答案，R14主答案迁移为scope.rs，并重新核验来源摘要。元信息记录 `p1a-source-layout-20260927`。这是新数据版本，不用于上表配对结论。迁移前题库与源码保存在 frozen-inputs，详情见 `dataset-migration.json`。

## 5. 范围与保留限制

本批 HardScope 是同一项目索引内的过滤，不是跨租户访问控制。路径沿用字符串前缀，不冒充完整Windows/大小写/路径规范化；后续P1-B继续测试。图连通度是项目级排名先验，返回的图证据受限；复杂图候选补足与异常/扫描预算诊断仍有后续任务。grep保留既有扫描上限，本批没有完成新的“先软范围再有界全局回退”调度。纯过滤但无检索词不启动空正则grep；name:foo保留实际检索词。

`symbol` 模式没有因此获得新DSL；现有14个MCP工具、hybrid/symbol字段和默认离线产品边界未扩大。没有ANN、embedding、LSP、LLM调用，也没有宣称P8多仓/100k/holdout认证。

## 6. 下一批与产物

下一批 **P1-B：P1-006–010**，路径规范/边界，词法lane独立范围，grep分段扫描/诊断，graph lane候选补足，精确符号/路径保底。任务源 [tasks.json](tasks.json)，派生 [05-TODO.md](05-TODO.md)，本批门 [P1-A-GATE.json](P1-A-GATE.json)。

证据根目录：`artifacts/benchmarks/p0-g0-20260927/` 与 `artifacts/benchmarks/p1a-20260927/`。源码和题库不再变动；回滚以保存的G0变更边界为准，不清除原始失败/比较工件、不重写旧基准结果。
