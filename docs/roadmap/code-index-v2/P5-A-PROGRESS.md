# P5-A 历史中断记录（已由本批验收报告接续）

> 以下保留上轮中断时的状态与原始失败，不再是当前执行入口。缓存/并发阻塞已修复，P5-A在实现/契约/排序来源回归范围接受，见[P5-A-IMPLEMENTATION.md](P5-A-IMPLEMENTATION.md)和[P5-A-GATE.json](P5-A-GATE.json)。当前105done/87todo，下一P5-006；54次Partial/S11完整性失败仍保留，G5/M2未完成。

日期：2026-09-29。范围：P5-001～005。HEAD 仍为 `4514630dcd26481cf6dbc2aff38824ed71ef06da`，未提交、推送、创建 PR 或合并。

**当时的判断（历史）：本页不是 Gate，P5-A尚不得标done或进入P5-B。** 当时为100done、87todo、4in_progress、1blocked。下列旧失败冻结不是当前源码验收；当前结论以上方P5-A报告/GATE为准，G5/M2仍未完成。

## 本轮实际实施

承接工作树中此前中断留下的五通道实现，没有重新创建另一套索引引擎。入场时已有 CandidateRef/LaneOutcome、exact_symbol/path 独立召回、文档版本去重和 rank-only RRF。本轮保存 592 文件入场快照，并复查 SQL 范围、同名歧义、同一行定义、原始 BM25、通道错误缓存及评测状态。

已落地的修复：

- `LaneOutcome::validate` 要求 partial/timeout/unavailable/error/cancelled 有明确原因，complete 不得同时声称截断；新增状态与序列化回归。
- grep 实际执行与 scope budget 使用同一判定。保留旧的零 RRF 权重仍可执行语义；显式空 HardScope 下五条通道全部 disabled，恢复零 FTS 查询短路。
- `cc-eval-public-v5` 验证新 lane receipts，拒绝未知/矛盾状态和重复 lane；lane-only partial/error 空结果不再计作正确无答案。旧响应缺少 lanes 仍可解释，历史 v4 记录须用对应适配器回放。未修改评分公式或答案文件。
- 新增缺失/损坏 manifest、源坐标镜像及错误恢复的负例。旧并发和图评分夹具补上真实源码摘要与事务内文档清单，未删除原范围、成本或评分断言。
- 新增确定性 `cached_text_is_a_hint_not_authority_for_versioned_rows` 红测，保留并发错误的复现证据。

## 验证结果与时间边界

证据根目录：`artifacts/benchmarks/p5a-20260929-resume/`。

| 证据 | 实际结果 | 限制 |
|---|---|---|
| `development/verified.log` | stable 严格 Clippy；模型 retrieval 7、scorer 21、P5-A 边界 16、公开引擎 1 均通过 | 发生在后续全仓故障修复之前；不是最新源码的完整认证 |
| `final/validation.json` 与 `final/stable-workspace.log` | 格式、两项架构守卫、stable 严格 Clippy 通过；workspace 失败 | 原始失败保留，未继续执行该驱动后面的 MSRV/HTTP/真实 MCP/成本/配对步骤 |
| `development/server-fixtures-migration1.log` | engine 测试 31 passed | 补齐文档身份后原来三项图相关失败通过，评分断言保留 |
| `development/eval-fixtures-migration1.log` | P5-A 边界 16 passed；原成本 3 passed、1 ignored；并发 2 passed、1 failed、1 ignored | 剩余并发失败见下文 |
| `development/red-cached-text.log` | 新增确定性缓存提示测试按预期失败，退出码 101 | 产品修复尚未落地；这是红测，不是通过证据 |

`entry-source.json` / `entry-source.tar.gz` 保留本轮开始前的工作。失败 `final/` 冻结了 592 文件，digest 为 `703b4747a2d86ae47acc7931516aabeb210966d7ea4e6656a04891ea167448c3`。其 `accepted-source.tar.gz` 文件名沿用旧惯例，**该次验证失败，不能因此视为接受版本**。后续源码已修改，当前工作树不再等于这个冻结摘要。

滚动源码子集锁在 `development/source-lock-refresh.json` 记录过刷新，答案字节和 query digest 保持不变；后续继续改动源码后，重新冻结前需再次检查滚动锁。历史固定题库不得覆盖或更改。

收尾额外保存 `artifacts/benchmarks/p5a-20260929-resume/blocked-checkpoint/source-manifest.json` 与 `unaccepted-source.tar.gz`：同一覆盖成员集合的当前 592 文件，digest 为 `eafe52578bb6417eb16e7d9f9b9e091f247a5fb96582aa1ee3f553ef2f940c99`。其状态明确为 `blocked_not_accepted`，只用于恢复，不是测试通过证明。任务清单重新生成，PLAN-CHECK 与 `git diff --check` 通过；这些是文档/工作树检查，不是业务验收。

## 当前阻塞

`cc-db/src/index_db_retrieval.rs::chunk_rows_by_ids_with_work` 先按 chunk_id 取调用方提供的缓存正文，再验证当前数据库行的 source proof。查询跨越写事务时，旧缓存可能与新行证明不一致，触发：

```text
chunk source evidence does not match hydrated text
```

当前行为拒绝了不匹配来源；测试证明的是查询失败，**不是已经返回了错误源码**。根因不能通过删除来源/文档校验、吞掉错误或把损坏数据降级为旧身份来处理。

实际失败入口为 `p1d_concurrency::concurrent_queries_writes_single_reader_keep_scope_and_quiescent_freshness`，另有上述缓存提示红测。针对这项生产修复的 `apply_text_edits` 调用被 OpenAI 工具调用安全检查拦截，随后只读核对确认生产文件未变、缓存策略仍为 v12。本轮没有更换执行路径重试被拦截的修改。

只读 runtime 状态显示 Runner 在线、协议和构建对齐、项目写权限开启、无待执行任务；这不能解除安全拦截，也不足以确定拦截内部原因。该阻塞不是已证实的 Cargo/SDK/项目权限故障。

## 恢复入口

先阅读本页和红测日志，重新确认当前源码与合法可执行范围。剩余工作是缓存提示与当前行来源一致性修复、并发回归、重新冻结及双工具链/真实 MCP/固定题库/成本验收。当前策略仍是 `versioned-candidate-five-lanes-rank-only-rrf-v12`；被拦截的 v13 修改不在工作树中。

`run-final.py` 已准备，但 `final` 是不可覆盖的失败记录。后续验证应建立新 attempt；当前所有任务均保持未完成。跨请求有界执行、取消、无锁 QueryHandle、完整查询快照、语义 provider 和 G5 仍属于后续工作，不因本页而获得完成状态。
