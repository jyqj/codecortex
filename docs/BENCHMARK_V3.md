# Benchmark v3：事实、源码预算与增量解析

本轮提供可执行的验收机制，不把手工回归夹具称为泛化 benchmark。继承
[BENCHMARK_V2](BENCHMARK_V2.md) 的隔离源码、真实 MCP、raw JSONL、独立 replay、
失败分母和仓库配对协议。生产系统没有 embedding；tiktoken 仅用于离线计数。

## 四个不同的判定

| 轨道 | 当前实现 | 不代表什么 |
|---|---|---|
| 事实 | `tests/fact_oracle.rs`：12 个 JS/TS/Python 闭世界源码场景，独立列出全部实际调用点和目标，MCP 构建后观察 SQLite 和 MCP 图富化 | 不是所有语言/框架的完整语义金标 |
| 定位/检索 | `quality_adversarial.json`：2 个手工项目、11 文件、13 任务（10 正例、3 负例） | 不是 78 个独立任务，也不是 held-out |
| 源码预算 | `scripts/evidence_budget.py`：源区间并集、准确定位的逐行正文与 cl100k_base token | 不等价于 bytes/4、原生客户端消息或 agent 成功 |
| 增量 | `tests/incremental_oracle.rs`：7 个持续会话测试，逐步与独立 full build 对照 | 两边相等不证明 parser 事实正确 |

正例中 1 个是显式 `evidence_mode=locator` 的同名符号导航任务；另 9 个是
`source` 任务。定位任务仍保留原始响应和定位分数，不进入源码预算满足率分母。
来源模式由 manifest 事先声明，不能根据运行失败临时排除任务。

## 运行

```sh
cargo test -p cc-parsers --locked --test fact_contract
mkdir -p target/audit-run
CODECORTEX_FACT_OUTPUT=target/audit-run/facts.raw.jsonl \
  cargo test -p cc-eval --locked --test fact_oracle
cargo test -p cc-eval --locked --test incremental_oracle
cargo run -p cc-eval --locked --example quality_run -- \
  crates/cc-eval/benchmarks/quality_adversarial.json target/audit-run/default 3
cargo run -p cc-eval --locked --example quality_replay -- \
  target/audit-run/default/raw.jsonl target/audit-run/default/replayed.json
cmp target/audit-run/default/report.json target/audit-run/default/replayed.json
python3 -m unittest discover -s scripts/tests -v
```

输出使用 create_new，不覆盖 raw。事实原始数据包括金标、实际持久化调用、构建
报告与 MCP 响应。它不消费生产 ranking score；调用边不正确就失败，即使命中
源码正确。原始观察保留后离线评分，禁止根据排序输出自动生成 gold。

## 源码核验与精确 token

设置阶段（允许下载，不能算入查询计时）安装 `scripts/requirements-eval.txt`，
提前运行 `tiktoken.get_encoding("cl100k_base")` 填充 `TIKTOKEN_CACHE_DIR`。
评分器验证版本 0.12.0 和数据 SHA256：

`223921b76ee99bde995b7ff738513eef100fb51d18c93597a113bcffe865b2a7`

```sh
python3 -m pip install -r scripts/requirements-eval.txt
export TIKTOKEN_CACHE_DIR="$PWD/target/tokenizer-cache"
python3 -c 'import tiktoken; tiktoken.get_encoding("cl100k_base")'
python3 scripts/evidence_budget.py target/audit-run/default/raw.jsonl \
  target/audit-run/default/budget.json --tokenizer-cache "$TIKTOKEN_CACHE_DIR"
```

评分阶段拒绝缺失/错误 tokenizer 缓存，不临时联网。报告记录 tokenizer、regex、
Python 版本与数据 hash。使用 encode_ordinary：像特殊 token 的源码仍是普通文本。
默认预算 1K/2K/4K/8K/16K，可用 `--budgets` 改变，`--overhead-tokens` 声明固定
协议开销；未声明客户端 framing 不被偷偷估算为真实成本。

返回 text 必须逐行匹配所声明源码位置；只有通过核验的行可以覆盖 label。
多个片段可以一起覆盖同一区域；重复、无名和错误项占据原位置和 token，不增加
重复信用。AND-of-OR 证据组允许合法替代，不要求返回所有替代实现。

报告明确分开：

- `full_handler_tokens`：整个 handler JSON 的 canonical serialization，包括重复
  正文与元数据，不是 JSON-RPC/chat framing。`production_evidence` 只有整个真实
  输出在预算内才计分；没有擅自删除元数据来假装生产达标。
- `adapter_prefix`：事先固定的 normalized source-prefix adapter，按原始排名取
  最长可放入预算的前缀，不跳过超长项、不访问 gold 选择片段。此结果是离线策略
  实验，尚未接入生产 MCP；不能宣传为生产打包收益。

旧 `maximum-new-label-gain-per-hit-v1` novelty nDCG 仍用于固定切块回归，不是
跨切块主指标。跨切块比较应优先看源码区间覆盖、证据组充分性与真实输出成本。

## 候选阶段与消融

`search.trace_candidates=true` 默认不启用，记录 lane、union、rerank 窗口和最终
位置，便于区分没有候选、排序丢失和输出投影丢失。union 记录最多 512 个定位项；
trace_truncated=true 时不计算阶段召回。它是定位诊断，不是“已交付源码”的证明。
搜索诊断同时记录通道关闭、扫描数、候选上限、工作预算耗尽与读错误，空结果
不失去这些元数据；降级输出不算正确无答案拒绝。

使用同一 `quality_run` 的第四个可选参数：

| 配置 | 实验 |
|---|---|
| `benchmarks/no_graph_retrieval.json` | 旧的仅关 graph lane，对照边界保留 |
| `benchmarks/no_graph_all.json` | 同时关图预选、检索、连接度重排、邻居/测试富化 |
| `benchmarks/bm25_only.json` | 全局同切块 FTS/BM25，只保留词法 lane、人工重排加成全关 |
| `benchmarks/trace_candidates.json` | 启用候选位置诊断；不能混用其延迟作为默认延迟 |

预期消融质量 gate 失败允许保留报告，不允许吞掉构建/启动错误。比较之前检查
报告完整且 tool/schema error=0，并记录退出码。完整无图指**检索路径**，不是
索引时禁止存储图事实。当前 BM25-only 保留预选计算但加分为零，不是最低开销
独立 BM25 实现；不要用它宣称本实现比专用 BM25 更快。

## 增量差分和恢复

比较规范化 symbols/imports/calls/refs/chunks/lookup_dependencies，保留实际目标、
解析策略，排除 rowid/时钟。场景包含签名/删除、未绑定名字变可用、加入第二个
同名候选、缺失模块补齐、import-only barrel 改变，以及 max_files=0 后 no-op
不能清除 incomplete。成功 full build 恢复 freshness；不声称一般 snapshot isolation。

## 真实仓库与 agent 实验的进入条件

另建冻结仓库 revision 与人工复核 label 的 manifest，标注来源/许可证。源码树
不得包含未来 patch/gold。按仓库家族划分 dev/test，先固定主要指标/预算/失败
策略再运行；回归数据不输出推断区间。先按任务聚合重复，再按仓库宏平均。

真实 agent 对照需要固定模型、提示、工具说明、累积 token、墙钟与所有失败：
`rg+read` 和 `rg+read+CodeCortex` 都允许合理多轮操作。不得拿一次整句 grep 与
多轮图检索比较。测试修复成功、解释证据或影响分析各用独立判定；未来补丁改动
文件不自动等于应阅读的证据集合。本轮没有执行此类 paid/model 实验，也没有
声称小型回归结果证明市场优势或跨仓库泛化。
