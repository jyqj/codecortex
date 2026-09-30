# P0 实施报告｜基线与 benchmark 底座

> 历史报告：以下保留最初 P0 交付时的结果。后续 **G0 已通过，P1-A 已完成**；最新状态见 [P0-CLOSEOUT.md](P0-CLOSEOUT.md)、[P1-A-IMPLEMENTATION.md](P1-A-IMPLEMENTATION.md) 和 tasks.json，不以本历史报告的 blocked 状态覆盖新证据。

> 2026-09-27；基线 `main@4514630dcd26481cf6dbc2aff38824ed71ef06da`。
> 状态：**19 项工程任务完成，P0-020 / G0 总验收受阻**。本地评测底座已可执行；不能把它表述为全平台、严格 CI 或发布认证全部通过。
> 本轮未提交、推送、创建 PR 或合并；没有修改生产索引、解析、检索及 MCP handler 的业务逻辑。后续 P1–P9 共 172 项仍未开始。

## 1. 实际交付

在现有 `cc-eval` 内新增开发用 binary 和 benchmark 模块，不另起引擎、不给生产 CLI 加 HTTP 服务。旧 TOML corpus、断言和进程内评测继续保留。

| 能力 | 已实现与验证的范围 |
|---|---|
| 数据契约 | Suite、SourceLock、Query、AnswerGroup、SymbolGold、ByteSpan、Hit、Row；严格未知字段校验；由同一 Rust 类型导出 JSON Schema |
| 输入锁 | 显式文件清单、路径/正文/字节数摘要、题库/配置/评分版本；Git HEAD 与 dirty；复制前再次检查源内容 |
| OCE 兼容评分 | any-expected Top-1、首项 grade=2/其他=1 的线性 nDCG@10、首次路径去重、Python 风格通配符金样 |
| 原生评分骨架 | primary answer group、Recall@5/10、MRR@10、符号/文件区分、源码跨度覆盖、无答案与错误状态区分 |
| 后端 | 真实 MCP stdio 子进程、可选 OCE HTTP、rg literal 文件序对照；后端参数不含 gold |
| readiness | pending/failed/unknown/partial/ready 分开；失败输入不从分母消失 |
| 报告 | raw、normalized、metrics、scores、latency、resources、failures、gate、Markdown；摘要核验与离线回放 |
| 增量 oracle | 同一输入的独立 A/B 副本；A 增量、B 全量；SQL 事实规范化与公开查询探针；保留目标 UID、解析策略 |
| 资源/统计 | 单调计时、全量重复样本、按 query-family 统计；runner/server/process-tree RSS 分列；不足样本标 inconclusive |
| 维护 | `scripts/code_index_plan.py` 检查依赖/状态/证据与派生 TODO；`scripts/p0-validation.py` 保存本地 SDK/构建/测试记录 |

源码主要位于 `crates/cc-eval/src/benchmark/`、`src/bin/cc-eval.rs`；新夹具、题库、schema 和测试均留在 cc-eval。Cargo.lock 的新 HTTP 依赖属于可选评测特性，`cargo tree -p cc-server` 验证默认生产依赖树没有 reqwest。`scale_bench.rs` 的改动仅为同 crate 格式化。

## 2. 基线与构建

旧版源码通过 `git archive` 固定到独立目录，以独立 target 构建，未清理开发者现有 target，也未用新 benchmark 代码替换旧版产品。

默认 SDK 27.0 的 C 链接探针复现 TAPI/`arm64e.x1` 错误；本机 SDK 15.4 探针通过。构建只在子进程设置 SDKROOT、RUSTFLAGS、RUSTDOCFLAGS，没有修改系统 SDK 或 Rust 默认工具链。

| 命令/验证 | 实际结果 | 证据 |
|---|---|---|
| 固定旧 HEAD：workspace 完整测试，含 doctest | 1308 passed，0 failed，16 ignored | `artifacts/benchmarks/p0-baseline-4514630/workspace-tests.log` |
| 固定旧 HEAD：workspace binaries 冷构建 | exit 0 | 同目录 `build-binaries.log` |
| 新实现：`cargo test --workspace --locked --offline` | 1350 passed，0 failed，17 ignored | `p0-final-validation/command-02.log` |
| 新实现：cc-eval + eval-http 完整测试 | 69 passed，0 failed，11 ignored | `p0-final-validation/command-03.log` |
| 新实现：cc-eval binary build | exit 0 | `p0-final-validation/command-04.log` |
| 显式产品 binary 的真实 stdio 测试 | 1 passed，0 failed | `p0-final-validation/command-05.log` |
| 新 benchmark 包严格 Clippy | exit 0 | `p0-closeout/clippy-benchmark.log` |
| cc-eval 格式检查 | exit 0 | `p0-closeout/format-check.log` |
| CLI 实际 SIGINT 取消 | exit 3，保留 partial manifest/gate | `p0-closeout/cancellation.json` |

上表 `p0-*` 均在 `artifacts/benchmarks/` 下。不同命令有重叠测试，**不能把通过数相加当作独立测试总数**。被 ignored 的规模/soak 测试没有自动变成通过；新真实 stdio 用显式命令另行执行。最终 Rust 源码与验证记录的逐文件 hash 一致。

## 3. 为什么 G0 仍然 blocked

第一，Rust 1.95 MSRV 未安装。`rustup run 1.95.0 cargo --version` 返回 1，明确记录 toolchain-not-installed；当前通过的是 Rust 1.97，不替代最低版本认证。

第二，`cargo clippy --workspace --all-targets --locked --offline -- -D warnings` 返回 101，报出基线原有代码的五处诊断：

| 原文件/位置 | 诊断 |
|---|---|
| `cc-search/src/lanes.rs:503` | unnecessary_sort_by |
| `cc-search/src/lanes.rs:542` | needless_option_as_deref |
| `cc-index/src/memory_budget.rs:101` | unused content_carry_budget |
| `cc-index/src/indexer.rs:746` | unnecessary_sort_by |
| `cc-index/src/indexer_phases/analysis.rs:60` | too_many_arguments |

这些生产文件本轮未改动。没有降低 `-D warnings` 标准，也没有为了基准收口偷偷实施 P1/P2。CI 已添加评测、真实 stdio、锁检查与可选特性的 MSRV 步骤，但**没有运行 GitHub Actions，不声称远端 CI 通过**。

上述阻塞不否定已运行的本地评测，但阻止把 P0 总验收标成 done。任务 P0-001–019 的 done 表示工程产物及其已执行本地验收完成；未完成的认证集中列在 P0-020/G0，未以“完成环境脚本”替代 MSRV 成功。

## 4. 第一批可复算结果

使用从固定旧 HEAD 构建的产品，通过真实 MCP 子进程运行。此表是**当前产品基线，不是优化后提升结果**；P0 没有改排序或索引逻辑。

| 数据集 | 独立题数 / 请求行数 | 正答案平均 Top-1 | 正答案平均 nDCG@10 | 测量状态 |
|---|---:|---:|---:|---|
| 自编三文件 smoke | 11 / 33 | 0.700000 | 0.700000 | exit 1：S11 无答案样例返回弱相关结果 |
| 实际 CodeCortex 七文件子集 | 14 / 42 | 0.857143 | 0.928571 | exit 0：有效基线，不是质量认证 |

smoke 的质量均值按 10 个有答案问题统计，另 1 个 no-answer 作为独立硬结果记录，没有从请求/失败统计中删除。重复请求先按题聚合；不是 33 或 42 个独立检索问题。

两组均未发现返回源码正文与本地被测副本不符的证据；这不意味着每个结果都回答了问题。两个报告通过离线回放后 metrics 摘要逐字节一致。42 请求的小样本比较正确返回 `inconclusive` / exit 1，而非认证 p95。

另跑了 rg literal 对照，原始结果在 `p0-final-runs/rg-subset`；它按字面匹配与文件序运行，不伪装成 BM25，也不将这个小样本作对外系统排名。

## 5. 已经钉住的生产缺陷

| 编号 | 实际观察 | 下一阶段 |
|---|---|---|
| B01 | 数据库强 BM25 命中经过现有预选转换得到更低分；实际 preselect 路径复现 | P1-002 |
| B02 | 明确存在的符号，因无关文件获得 soft hints 且预选 limit=1 而被排除 | P1-004 |
| B03-Python | 仅改 provider 签名，未修改 consumer 的 call edge 仍指向已经不存在的旧 UID；全量构建更新目标 | P2 |
| B03-Rust | 仅改 provider 签名，lib.rs 的调用边保留旧 UID；全量构建指向新 UID | P2 |
| B15 | 无答案问题返回词法/图弱相关结果，现有 hybrid fallback 的行为需要单独审视 | P1 查询/无答案策略 |

Python/Rust 的 A/B 差异出现在 symbols、symbol_refs、call_edges；oracle 没有把 target UID、策略或符号内容从比较里删掉。两个 mutation 运行均正确返回 1，并保留两侧完整规范化结果。观察测试通过只代表“夹具和观察机制可工作”，不代表这些产品缺陷已修。

## 6. OCE 参考落实情况

固定参考 `oce-ai/oce-benchmark@d4f10554a18e31599d1e46d5d56da6588d4aa86c`。

实际读取并暂存两份外部 JSONL/metadata，校验 Git blob SHA 与实际数量，运行本轮 importer：cc-switch 100 题、Flask 100 题，各自 10 类×10 题，均 exit 0。外部原始文件和导入题库只存在临时目录，未直接 vendor 到仓库；保留摘要、计数和权限使用范围 receipt。

这不等于 200 道题的标准答案已经全部语义复核，也不等于对真实 OCE 服务跑出了分数。HTTP adapter 的验证来自本地受控 stub；没有真实模型调用、费用或私有源码外传。`must_contain` 保持 annotation-only，不偷换兼容评分含义。

## 7. 明确未完成的能力

- Rust 1.95、严格全仓 CI、Linux/远端 Actions 运行证据：如上未放行。
- 真实 OCE 服务对照、全部外部 gold 人工复核、付费 embedding：未运行。
- P8 多仓约 600 题、100k 文件、持续峰值内存、并发尾延迟、完整留出集：不属于本轮完成声明。
- P0 oracle 是八张表加公开查询的诊断骨架，不宣称覆盖未来所有语义 manifest/关系类型。
- 子模块输入在 P0 明确拒绝，需要后续扩展独立锁；并非忽略子模块后宣称支持。
- 原生评分目前是 group/symbol/span 骨架，完整 facet/有序图约束继续按后续阶段扩展。
- 本轮 gold 经过来源阅读与独立于检索结果的二次检查，没有外部人工评审或正式 holdout 认证。

## 8. 下一轮入口

先完成 **P0-020 / G0 收口**：在兼容 SDK 下提供 Rust 1.95 真实验证，处理已登记的基线 Clippy 阻塞并重跑相关回归；不能直接把 unknown/not_run 改成 passed。收口后进入 P1-A，以 B01/B02 夹具驱动红绿测试。P1–P9 源码优化、embedding 集成和 PR 发布均未在本轮执行。

使用命令见 `crates/cc-eval/benchmarks/USAGE.md`。任务状态源为 `tasks.json`，Markdown 使用 `python3 scripts/code_index_plan.py --write` 派生；不要手工只勾选 TODO。
