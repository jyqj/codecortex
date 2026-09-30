# P1-D 实施与验证报告

2026-09-27；HEAD `4514630dcd26481cf6dbc2aff38824ed71ef06da`；继续 P1-C 未提交工作。**P1-016–020 完成，G1 本地限定范围通过。累计40项done、152项todo；下一批P2-A。** 不提交、推送或创建 PR，不调用模型，不改变全局 SDK/默认工具链。

## 1. 实现范围

P1-016：请求局部 SQL/正文工作账单，词法 FTS、hydration 与三段 grep。外层语句 rows/vm_steps/fullscan_steps/sorts，正文 storage_reads/zstd_decodes/legacy/cache/UTF-8 bytes，候选计数；公开 MCP cost 与离线 costs.jsonl。没有安装进程全局 tracing 回调或以全局计数差值冒充单个请求。

P1-017：受控 cache 完成交错复现旧正文晚到污染；文本缓存改为查询捕获的 `(index_epoch, chunk_id)`。真实 SQLite/WAL 单连接读池读写与连接等待/错误恢复；真实 MCP 并发调用另有独立回归。不是全查询事务快照、deadline 或吞吐认证。

P1-018：图富化消费详细检索的同一个 SearchPlan/HardScope，不再重复解析第二套约束；最终 hydration 与 SQL 限额前守卫保留。旧公开便利函数继续委托同一实现，不提供可切换的旧隐式范围政策。

P1-019：同步 SEARCH、MCP_TOOLS、CONFIGURATION 的预算、缓存键、BM25 公式及成本边界；Markdown 标记中的 JSON 示例由同一测试提取、参数校验并通过真实 MCP 执行，不维护第二份例子。

P1-020：最终严格回归、成本对照、固定输入质量报告与任务证据已收口；B01/B02 保持正向阻断回归，不再使用基线豁免。G1 是本阶段限定合同通过，原 B03 / S11 仍独立失败，不改 gold、不将所有原始 gate 改绿。

## 2. 证据层级

`checks/red-late-cache.log` 是旧缓存回填的有效受控红测试；完成新一代写入和读取之后，再模拟旧工作者完成填充，随后词法读取错误返回旧正文。修复后相同语义断言转绿。这不是实时生产故障统计，也不证明 P5 原子快照语义。

新增字段遗漏的编译错误、测试误调用 ReadOps::read_conn、结构化编辑参数拒绝和 task-card alias 拒绝属于开发/工具准备问题，保留失败记录与恢复，不算产品收益。

成本第一次共享 target 实验虽然命令退出 0，但两个源码变体复用了同一测试二进制，被人工复核判为无效反事实。`cost-experiment/invalid-attempt.json` 记录作废原因；原始目录保留。有效对照必须使用独立冷 target，核验不同 binary digest、相同输入/配置/编译器与唯一声明的源码变动。成本用例是实际 core 白盒诊断，不充当真实 MCP 黑盒尾延迟。

### 完整 MCP 路径新增阻塞及修复

真实 MCP 读池=1 的初次并发验证失败：初始 full index 等待约30秒，随后的64次查询与8次增量调用在25秒 watchdog 内未完成。`IndexDb::stats()` 持有唯一连接时调用会再次 checkout 的 `get_metadata()`；独立3秒 stats 回归准确复现。修复增加共用的 `get_metadata_on(conn, key)`，复用已有连接，不增加 pool size、不延长等待、不更改既有元数据空值语义。专门回归与相同真实 MCP 请求均转绿。

首轮完整 workspace 验证中原有 native watcher 忙碌窗口测试失败；单独运行通过，但没有据此抹除失败。测试前置从“数据库已提交”加强为“提交完成且 auto_indexing 释放”，从固定 sleep 改为有界观察订阅任务就绪。另加测试专用队列注入验证实际 gate-before-drain 三次忙碌后队列不丢失；native 文件通知测试继续启用，生产 watcher 算法未修改。它们不证明所有平台的通知可靠性。

## 3. 冻结源码的最终严格验证

最终覆盖摘要：`aa25c36d70e40dc3233568159caffbdec90cc2262081be4c2b7e20b432b86d73`。命令、退出码、工具链、二进制与447个覆盖文件摘要位于 `artifacts/benchmarks/p1d-20260927/final-v2/validation.json`。共34条验收命令退出0；后续配对与收尾哈希复核无漂移。先前失败的 `final/validation.json` 没有覆盖或删掉。

| 检查 | stable 1.97.0 | Rust 1.95.0 |
|---|---:|---:|
| 全仓all-targets + eval-http严格Clippy | exit0 | exit0 |
| workspace完整测试，含doctest | 1409 passed / 0 failed / 28 ignored | 同左 |
| cc-eval + eval-http | 112 passed / 0 failed / 22 ignored | 同左 |
| workspace/eval binaries | exit0 | exit0 |
| P1-D成本/文档/并发真实MCP | 1 / 1 / 1 passed | 同左 |
| P1-C契约/解释真实MCP | 1 / 1 passed | 同左 |
| P1-B/P1-A/P0真实MCP | 2 / 2 / 1 passed | 同左 |
| watcher专项复跑 | 10 passed | 同左 |
| 核心并发矩阵额外复跑 | 2次均passed | 同左 |

计数存在重叠，不相加；ignored不算成功。每套工具链真实MCP并发为64次搜索、8次增量请求，单连接读池，scope错误0，结束后正文为第8次写入；核心矩阵C=1/4/8/16，每读者16组fresh+cached查询、每档32次写入，断言静止后的新正文及范围。测试watchdog不是产品全局deadline，未认证开放环吞吐或原子整次查询快照。

## 4. 固定输入的G0/P1-C/P1-D对照

相同新runner、冻结源码/题库/答案/配置，只改变精确二进制。共12组、459次查询；metrics、query-slices与costs.jsonl全部离线重算摘要一致。成本缺失的G0/P1-C输出为unavailable，不当作零。

| 数据 | 每版题数/请求数 | G0 Top1 / nDCG | P1-C Top1 / nDCG | P1-D Top1 / nDCG |
|---|---:|---:|---:|---:|
| 原七文件源码集 | 14 / 42 | .8571428571 / .9285714286 | .8571428571 / .9379235538 | 同P1-C |
| 原三文件smoke | 11 / 33 | .7 / .7 | .7 / .7 | 同P1-C |
| 定位/范围回归 | 8 / 24 | .625 / .625 | 1 / 1 | 同P1-C |
| 六文件双语意图 | 18 / 54 | .625 / .65625 | .625 / .65625 | 同P1-C |

G0定位集还有3条E05越界证据，新版本为0；对应正式比较保持failed（基线完整性失败）。其余正式比较均inconclusive，不据小样本宣称显著性或p95。P1-D相对P1-C没有新的质量增量；表中G0到P1-D改善包含前批成果，不能重复算成本轮新增收益。十四工具旧新schema/unknown字段契约收据一致。

S11继续3次no-answer失败；smoke均值只含10个正答案问题。Python/Rust mutation均equal=false/exit1，保留symbols、symbol_refs、call_edges差异。双语中文无标识符4题没有命中标准答案的限制仍在，未添加生产查询词典或目标路径特判。

## 5. 质量与成本同看：独立release机制实验

有效记录是 `cost-experiment/*-v2-results`、`receipts-v2.json` 和 `summary-v2.json`，不是第一次共享target目录。两变体采用独立冷target且二进制摘要不同；1k/5k文件 × 5档cap × 3类查询 × 30次重复 × 2变体，共1800条。输入清单和测试配置一致，只切换预选是否错误地缩成硬过滤。每次清除LRU并绕过结果缓存，同一次查询的grep→hydrate文本复用仍保留；release而非debug，OS缓存未清，不取best-of。

下面是5k文件、目标在错误软预选之外的关键字查询，每格30次：

| 行为 | grep cap | 找到目标 | grep正文读取 | 已计量总正文读取 | 已计量外层SQL VM步骤 | 覆盖状态 |
|---|---:|---:|---:|---:|---:|---|
| 旧隐式硬过滤 | 1 | 0/30 | 1 | 1 | 63 | complete仅针对错误缩小的范围 |
| 正确硬范围+软优先 | 1 | 30/30 | 1 | 2 | 99 | partial |
| 正确硬范围+软优先 | 0 | 30/30 | 0 | 1 | 89 | partial |
| 正确硬范围+软优先 | 8192 | 30/30 | 5000 | 5000 | 40121 | complete |

cap=0仍可由FTS候选正文批取产生1次读取，证明grep cap不是整次查询解压上限。所有1800条grep读取不超过cap。旧实现便宜但漏答，不能将错误少搜报成性能优势。mid-token/无答案场景、所有预算和原始时延均保留，未只选有收益的格子。

成本覆盖仅词法/批取/grep外层SQL，排除预选、精确查找、图SQL、FTS内部操作、I/O和峰值内存；缺失或无效不是0。计数器有32位范围边界，不用它认证超大工作量。实验采用独立冻结P1-D共同框架，之后的stats复用连接和测试/注释差异有 `final-source-difference.patch` 与binding说明；不能把机制实验二进制冒充最终产品性能。实测核心路径不调用stats；此实验只支持机制成本结论，不支持最终MCP尾延迟/100k发行认证。

## 6. G1与交接

`P1-GATE.json` / `P1-D-GATE.json` 为 `passed_local_scoped`：P1范围/缓存/读取退化/兼容合同与可比较的质量成本证据完成；不是所有原始benchmark gates绿色，也不是M1或产品发行认证。权威状态在tasks.json，05-TODO由脚本派生，当前40done/152todo。下一批P2-A（P2-001–005）：PublicSurface模型、规范指纹/存储与JS/TS/Rust/Python公共接口。

默认离线行为、Rust核心、七crate、14工具和旧输入参数保留。无DB schema迁移，无模型/OCE服务调用，无Linux/Windows/远端Actions/holdout/100k/连续峰值内存认证。未修改P2解析算法。WebCodex工作流卡片最后的gateway状态同步/读取被工具层拦截，未绕过也不声称远端Goal已完成；代码、任务文件和本地验收证据独立完成。未提交/推送/PR。
