# P7 实施前开放问题（需用户/owner 裁决）

> 只读规划发现，未擅自裁决。按对实施顺序的影响排序。Q1/Q2 阻塞批次 1/4 开工，
> 其余可在对应批次收口前裁决。

## Q1｜HTTP 客户端 crate 的放置（阻塞 P7-001 落码）

`crates/cc-semantic` 依赖地板是 cc-model+cc-db only（lib.rs:66-67，ADR-0003 口径），
而 P7-001 要建 OpenAI-compatible 适配层。本规划取 **transport seam** 方案
（`EmbeddingTransport` trait 定义在 cc-semantic、HTTP 实现注入自 cc-server 或
测试 stub，见 TASK-BRIEFS P7-001），保持依赖地板不变。**需裁决**：
① 同意 transport seam，还是接受在 cc-semantic 上开 `provider-http` cargo
feature（依赖地板条款需 ADR 增补）？
② 若走 seam，HTTP 客户端 crate 选型（如 ureq / reqwest blocking）落在 cc-server，
`--features semantic` 依赖树将新增网络 crate——G6 式"闭包零网络客户端 crate"
结论对 feature 树不再成立，需确认新的 GATE 表述口径（默认包零网络 + 未
opt-in 零连接尝试）。

## Q2｜两条 depends_on 边是否放宽（影响批次排期弹性）

`P7-006 depends_on P7-005` 与 `P7-016 depends_on P7-015` 是链上耦合最弱的两条边
（理由见 IMPLEMENTATION-ORDER 第 1 节）。放宽需 owner 修改 `tasks.json`（本规划
不代行）。默认不放宽：收益有限，且 P7 全链已按主题分段、单批工作量可控。

## Q3｜接线待办 9 与 11 的批次归属（round12 移交项落位确认）

本规划建议：待办 9（机会性 reclaim 收窄）→ P7-016；待办 11（有界 desired 投影
回接 reconcile）→ P7-015。两者也可归 P7-008/P7-014（备选见 IMPLEMENTATION-ORDER
第 2 节表格）。**需确认**或改派；无论落哪，13 项对账义务（P7-014 收口时双向
出账）不变。

## Q4｜同步 EmbeddingProvider trait 的线程模型（影响 P7-001/005/006/013）

`EmbeddingProvider` 是同步 trait（ports.rs:99-106，P6 冻结面），网络调用会阻塞
调用方线程；C11 禁止"简单在 scoped-thread join 上等待网络"、要求锁外网络
（02-CONTRACTS.md:87-91）。P6 的 worker 侧是显式 drain、无常驻线程
（lib.rs:26-31），drain 由组合根调度。**需裁决**：
① worker drain 的阻塞 provider 调用跑在哪个执行上下文——专用有界阻塞线程池
（新引入）还是调度线程直跑（简单但占死线程）；
② 查询路径（P7-013）是否允许查询内联触发 query 向量编码（本规划默认：不允许，
miss 即该查询 dense lane 记 Unavailable，编码只发生在 worker 侧），还是允许
`ExecutionPool.run_async` 内桥接一次阻塞编码。

## Q5｜query cache 键是否纳入 semantic_epoch（影响 P7-009 V11 证据口径）

C12 缓存依赖表的 dense 行要求缓存键含 semantic epoch（02-CONTRACTS.md:93-107）。
但 query 向量与文档集合无关，纳入 epoch 意味着每次回填推进清空 query 缓存
（保守正确、有浪费）。本规划默认**按 C12 表保守入键**；若 owner 同意"query
向量缓存键只含 namespace+QuerySpecDigest+QueryDigest"，需在 C12 表补一行
豁免口径再实施。

## Q6｜语义配置键的定名与层级（影响 P7-002/P7-007/P7-014）

`.codecortex.json` 目前没有任何语义键（CONFIGURATION.md:207-208），预算/租约
参数仍由调用方按库层 API 传入。P7-002 要开第一组键（provider/model/dimensions/
metric/endpoint/api_key_ref/enabled），P7-014 还要加 GC 宽限、cache 根、deadline
等键。**需裁决**：键命名空间（建议 `semantic.*` 单节，还是 provider/model/
limits 分节）、以及既有"调用方参数"类键（re-embed 预算、min_retention_secs、
lease 时长）是否一并迁入配置面（round12 待办 6 只点名 GC 宽限）。

## Q7｜P7-018 是否创建 manifest 占位（建议：不创建）

P7-018 scope 含 `crates/cc-eval/benchmarks/manifests/`。本规划建议本轮**不创建
任何 live manifest 文件**（blocked 记账只落在 P7-020 报告与 tasks.json），避免
出现"声明了 live 运行但没有运行"的虚假 manifest。若 owner 希望留占位骨架，
必须带 `"status": "blocked"` 字段且不进入任何运行清单——请裁决。

## Q8｜tasks.json `current_phase` 记账（P7-020 收口前处理）

`current_phase` 仍为 `"P5"`（tasks.json:11549），而 P6-020/G6 已 done、
`next_task=P7-001`（tasks.json:45 尾段，且明示 next_task"仅为依赖图指针"）。
round13 收口未推进 phase 字段。**需确认**：phase 字段推进时机是"gate 通过即推"
还是"下一阶段首个任务开工时推"，由 owner 在 P7 批次 1 开工前统一，避免 P7-020
收口时再补历史口径。

## Q9｜P7-004 整体拒绝 vs 部分接受（低优先，可在 P7-018 授权后重审）

本规划取批级整体拒绝（一坏俱拒 + 批级重试），理由见 TASK-BRIEFS P7-004。
真实供应商若普遍部分成功，整体拒绝会放大费用——该项依赖 live 数据才能裁决，
本轮不阻塞，仅登记为 P7-018 授权后的复审项。
