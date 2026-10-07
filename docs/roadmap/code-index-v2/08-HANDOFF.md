# 08｜执行交接与下一轮入口

## 1. 当前状态

<!-- code-index-progress:start -->
Code Index V2 共 **192 项任务：152 done / 16 in_progress / 23 todo / 1 blocked**。

当前阶段：**P7｜provider与dense端到端**；计划状态：`in_progress`；更新日期：`2026-10-08`。
下一任务：**P7-013｜查询总deadline和模型故障退化**（硬依赖已完成）。

| 当前下一项、进行中任务及其未完成前置 | 状态 | 硬依赖（任务状态） |
|---|---|---|
| P7-013｜查询总deadline和模型故障退化 | `in_progress` | P7-012 (done) |
| P7-014｜配置/status/MCP全链贯通 | `in_progress` | P7-013 (in_progress) |
| P7-015｜后台回填与前台查询竞争测试 | `todo` | P7-014 (in_progress) |
| P7-016｜fake全故障矩阵回归 | `todo` | P6-020 (done)、P7-015 (todo) |
| P7-017｜离线默认包和未启用测试 | `todo` | P7-016 (todo) |
| P7-019｜本地加dense的质量/成本消融 | `todo` | P7-017 (todo) |
| P7-020｜P7语义闭环与发布范围验收 | `todo` | P7-001 (done)、P7-002 (done)、P7-003 (done)、P7-004 (done)、P7-005 (done)、P7-006 (done)、P7-007 (done)、P7-008 (done)、P7-009 (done)、P7-010 (done)、P7-011 (done)、P7-012 (done)、P7-013 (in_progress)、P7-014 (in_progress)、P7-015 (todo)、P7-016 (todo)、P7-017 (todo)、P7-019 (todo) |
| P8-001｜锁定release候选与证据输入 | `in_progress` | P7-020 (todo) |
| P8-002｜完成真实多仓native语料认证 | `in_progress` | P8-001 (in_progress) |
| P8-003｜运行外部兼容套件 | `todo` | P8-002 (in_progress) |
| P8-004｜封存holdout与反过拟合检查 | `in_progress` | P8-003 (todo) |
| P8-005｜完整规模1k到100k | `in_progress` | P8-004 (in_progress) |
| P8-006｜增量规模与fanout曲线 | `in_progress` | P7-020 (todo)、P8-001 (in_progress)、P8-005 (in_progress) |
| P8-007｜多并发与混合负载 | `in_progress` | P8-006 (in_progress) |
| P8-008｜冷建/重开/热查分层 | `in_progress` | P8-007 (in_progress) |
| P8-009｜内存/磁盘/费用总账 | `in_progress` | P8-008 (in_progress) |
| P8-010｜长时soak与连续修改 | `in_progress` | P8-009 (in_progress) |
| P8-011｜端到端故障与恢复认证 | `todo` | P7-020 (todo)、P8-007 (in_progress)、P8-010 (in_progress) |
| P8-012｜MSRV与平台冷构建矩阵 | `in_progress` | P8-011 (todo) |
| P8-013｜指标/门槛与失败退出最终认证 | `in_progress` | P8-012 (in_progress) |
| P8-016｜数据库/配置/包回滚演练 | `in_progress` | P7-020 (todo)、P8-012 (in_progress)、P8-013 (in_progress) |
| P8-017｜删除临时兼容和重复模块 | `todo` | P8-016 (in_progress) |
| P8-018｜文档事实与安装契约同步 | `in_progress` | P8-017 (todo) |
| P8-019｜发布工件与完整报告归档 | `in_progress` | P8-018 (in_progress) |

进度入口：[重构总览](README.md) · [逐项 TODO](05-TODO.md) · [唯一任务状态源](tasks.json) · [执行交接](08-HANDOFF.md)。
任务完成数不等同发布认证；以各任务证据和适用验证范围为准。

> 本块由 `scripts/code_index_plan.py --write` 从 `tasks.json` 生成；无参运行校验全部进度入口。
> 源文件 SHA-256：`700c50afdb100f2f77beea94534837aa4fa5784dcfb651b46ca4ad8d95d26f40`。
<!-- code-index-progress:end -->

## 本轮继续推进（2026-10-08）

本批选择此前P8批次未涉及的十个原始任务，逐轮提交与剩余数见 [后续十项进度](P8-NEXT-PROGRESS.md)。#144/#145按任务ID和固定源码证据合流；两套旧source guards原字节保留，由v10审查已固定的新顺序delta。未把局部工具交付算成完整发布验收。

## 2. 下一步与依赖

按 `tasks.json` 的 `next_task` 推进；上方生成区同时列出进行中任务尚未完成的硬依赖。已有实现和历史通过记录应先与当前源码核对，再按原验收条件补齐独立复核。任务状态不因入口更新或 PR 整合自动改变。

### 本轮交付（2026-10-07）

[PR #142](https://github.com/jyqj/codecortex/pull/142) 将固定整合源合入 main，
新一轮 check/MSRV/security 全部通过。原 139 项开放 PR 已逐项审计，93 项归并关闭、46 项保留独立范围；
固定判定、操作快照及保留理由见 [PR 管理检查点](../../../artifacts/checkpoints/pr-management-20261007/README.md)。

[PR #143](https://github.com/jyqj/codecortex/pull/143) 增加当前最终组装的 SQL 计量并同步四个进度入口，
P7-011 的局部实现与 21 项定向通过证据见 [validation work](../../../artifacts/checkpoints/p7-validation-work-20261007/README.md)。
P7-014 增加版本化 Python capture 凭据与完整重新校验入口，26 项定向通过及独立审查见
[revalidation](../../../artifacts/checkpoints/python-inventory-revalidation-20261007/README.md)。
早期 767 输入组合的 27 项记录保留在 [历史组合证据](../../../artifacts/checkpoints/combined-validation-20261007/README.md)。
新增计量收据曾在原预算内挤掉正文，CI run37623178087 的失败与修复过程见 [packing 修复证据](../../../artifacts/checkpoints/validation-work-packing-budget-fix-20261007/README.md)。
第一轮修复后的 768 输入组合为 48 passed / 0 failed / 1 个既有显式 stdio ignored，fmt 通过，见 [该轮组合证据](../../../artifacts/checkpoints/combined-validation-fix-20261007/README.md)。
后续 CI run37629938552 在更早的 metadata 投影阶段暴露公开 BM25 score receipt 丢失，现增加早期可回滚探测：先压缩重复正文，再判断整体省略可选计量是否足以保住全部 hit/retrieval metadata。
原预算、原 oracle 及正文边界断言不变；新组合的 12 项窄测和 13 项 V05/V16 feature 测试通过。默认完整续跑仍有 4 个失败，进程归属、socket 权限、性能阈值及 generation fixture 原证据均保留；见 [metadata 修复与当前回归证据](../../../artifacts/checkpoints/validation-work-bm25-metadata-fix-20261007/README.md)。
各组有重叠，不相加为唯一案例数；该历史P7收口固定源码选择为 `p7-capture-revalidation-20261007-v7`，旧 v4/v5/v6 记录保留历史范围。完整最终 CI 另行绑定实际 head，不由本地窄测或源码准入推定通过。
P7-011 与 P7-014 的整项状态及正式质量/规模门保持上方权威状态，后续按原验收条件补齐。

用户要求继续多轮 workspace multi-subagent，至少推进 P7-011～020 这 10 个原始 TODO；每轮报告已完成与剩余数量，并分别记录有实质交付、待验收和受阻状态。本轮起点为 150 done / 1 in_progress / 41 todo，42 项未完成；内部子步骤不计为多个 TODO。真实 provider 认证仍按任务原有的明确授权及预算条件执行。

### 并行 P8 本地工程批（2026-10-07）

本批以 main `6d02d77f018a5965a6f289b0b43558ed4b9f8322` 为基线，分3轮推进10个原始P8 TODO，避开另一会话的P7实现。逐轮计数、具体交付和剩余验收见 [P8本地推进](P8-LOCAL-PROGRESS.md)。当前192项为150 done / 11 in_progress / 31 todo，42项未完成；10项均保持in_progress，原硬依赖和验收条件不变。

当前显式源码选择为 `p8-local-engineering-20261007-v8`：固定远端Rust source `18499879a8ba3197e599cfd2e9ae96bcb657d65b`（其777输入与已测本地source `853385b7ccb2780818f9f8e8a83791f1c197efcf`逐字节一致，独立审阅实际远端source后重新绑定准入），独立审阅17路径后，将其作为base `6d02d77`上的精确delta加入原接受链，共777项crate/Cargo/lock输入。源码准入只验证来源和字节，不能继承旧质量、100k或发布结论。历史v1/v2/v3与既有两组review pins原样保留；CI只增加两项本地Python检查并更新准确selector。最终测试/CI单独绑定实际源码与head，见 `artifacts/checkpoints/p8-local-waves-20261007/integration/`。

## 3. 每次开始

读取README、04对应阶段、05对应任务、相关02/03契约和06/09验收。核对HEAD/dirty/nested规则；只对授权批次修改；判断哪些依赖done且证据覆盖当前目标。使用现有本地工具读实际实现，不因设计树中有文件名就假设已经存在。

变更比计划更早落地时，以实际代码/测试证明并更新任务状态，不重复实现；发现计划依赖不合理时修订tasks.json及说明，不暗中绕过。原始参考固定SHA，不随手把主分支新行为套到旧benchmark分数上。

## 4. 每次结束

冻结被测源码后跑本批V编号和相关旧回归；留命令、退出码、日志、run-id、diff摘要、源码SHA/dirty digest。更新tasks.json：完成必须有evidence；阻塞列出准确原因和受影响的发布范围；conditional未授权不执行，不当成功。更新JSON中的current_phase/next_task后运行 `python3 scripts/code_index_plan.py --write`，同步05-TODO与三个入口进度块；再无参运行校验source hash和视图一致性，仅在退出码为0后将实际JSON stdout保存到PLAN-CHECK.json。

需要提交PR时在用户授权后使用可用GitHub连接器，明确分支/基线/变更范围，不自动合main。只落本地规划不是已提交到Git。

## 5. 任务状态更新契约

唯一状态源tasks.json。每项保留id/phase/batch/title/scope/depends_on/steps/deliverables/acceptance/validations/rollback/conditional/required_for/evidence。id不复用，删除任务改deferred或superseded note并保留可追踪关系，不重新给后续所有任务编号。

完成证据最少：target_sha、worktree_digest、commands、exit codes、run_id/artifact paths、review、rollback_status。done需要校验对应V全部适用项已通过；依赖中的conditional任务可以明确deferred，但依赖gate只能对不要求该live能力的scope给结论，不能将此转换成全量认证。

Markdown派生规则：05-TODO按phase_order和任务原顺序输出；状态todo/blocked/deferred显示未完成，done显示勾选；每项输出现有字段与证据。根README、roadmap README和本页的 `code-index-progress` 标记块同样由 `scripts/code_index_plan.py` 生成并校验tasks.json SHA-256；标记外的历史和说明保留。不得手工只勾05或修改入口计数而不改JSON。`--write`只生成视图，不改变任务状态或自动选择下一任务。

## 6. 实施提示词模板

```text
在 WebCodex 的 codecortex-rust 项目执行 Code Index V2 的 <Batch ID>。
先读 docs/roadmap/code-index-v2/README.md、04-PHASES.md、tasks.json 中该批任务，
以及02-CONTRACTS/03-SEMANTIC和06-VALIDATION/09-BENCHMARK对应要求。
核对当前HEAD、dirty与实际源码；不覆盖其他修改、不假设规划文件树已实现。
先补会暴露问题的fixture/测试，再贯通本批代码、存储、查询/MCP、配置和文档。
保持Rust核心与默认离线；benchmark通过真实公开协议，不读取gold作为检索输入。
按V编号保存当前目标SHA的真实证据，区分fake、真实provider、白盒与stdio。
完成后更新tasks.json、派生TODO和交接入口，汇报通过/失败/阻塞及下一批。
提交/PR/合并仅按本次用户明确授权执行。
```

模板是后续执行入口，不是已经发出的任务，也不建立自动后台执行。

## 7. 规划完整性检查规则

完整规划审查应覆盖：13个必需文件存在；tasks总数/phase数/batch数；唯一ID；依赖都存在且DAG无环；phase与batch编号；引用V编号均在06登记；重要字段非空；当前状态计数、done项证据、根current_phase/next_task提示与硬依赖一致；四个进度视图及source hash一致；相对Markdown链接可解析；新增实现、测试、文档和证据均在本批授权范围；HEAD不变。

PLAN-CHECK.json是无参生成器当前成功输出的滚动收据，实际包含任务总数、状态计数、tasks.json SHA-256和四个视图数量。生成器检查任务字段与状态、总数及阶段计数、引用V编号、依赖存在与无环、done项证据和硬依赖、导航与视图一致性；尚未实现的完整文件清单、phase/batch编号、链接解析、文件字节/行数及HEAD检查应另留真实收据，不能由该passed推定。
这是规划检查，不是业务代码测试；不得把PLAN-CHECK的passed用作G0–G9通过证据。替换的2026-10-01原始收据保存在 [历史维护证据](../../../artifacts/checkpoints/roadmap-maintenance-20261007/README.md)。

## 8. 历史交接快照（2026-09-30 至 2026-10-02）

以下为原交接正文，保留当时的下一步、测量与未通过项。当前下一任务和依赖见本页顶部生成区。

> 共同集成：`b738a6a3aff23dbda1ceec0b5180a9a016b03b09`；限定格式源码 draft [PR #8](https://github.com/jyqj/codecortex/pull/8) `c4dfa324143a8406ee7f54e1337c54c1e1700118`，四crate lib975 passed/0 failed/1 ignored。011范围修复与013两取消提交均按序集成；正式验收缺口见云 checkpoint integration-receipt.json；011/013仍todo，014仍in_progress。013单独PR被其执行器拒绝，未代开。Cargo.lock/依赖未改，归父对话安全任务。

> P7-014 生命周期子块：draft [PR #7](https://github.com/jyqj/codecortex/pull/7)，源码 `ca5326a627e52bd1ec75ce84a73d95e1f8ac6970`（叠加 #5）；engine 保留 subsystem，初始化错误保原项目，close/reopen 对称清理。默认 lib253/253、semantic lib280/280通过；证据见云 checkpoint 的 lifecycle-receipt.json。尚缺生产 worker 调度与 wired-state stdio；整项保持 in_progress。011 languages 与013取消问题交独立修复，不在此分支收口。

> 2026-10-02 云端最新入口：基线 `ff458bc591b4e7e444af4464d6eef2513cdb335c`，恢复提交 `ddd4f7e38ffff2d1bdd0193d5ccf4054deef82fa`，draft [PR #5](https://github.com/jyqj/codecortex/pull/5)。150 done / 1 in_progress / 41 todo；P7-011～013 独立复核待收口，P7-014 进行中。旧 P5 交接正文仅为历史；云端实测与未通过项见 `artifacts/checkpoints/cloud-p7-014-20261002/receipt.json`。默认/semantic 构建恢复，状态4/4、semantic lib278/278、默认真实stdio1/1通过；全仓2250/4/60，lint/format失败；生产worker调度和wired lifecycle stdio尚未完成。

### 当时状态（2026-09-30）

**118 done / 1 in_progress / 73 todo，P5 为 18/20；P5-016～018 已验收，P5-019 正在实施通用查询质量修复与独立消融。P5-D 整批与 G5/M2 尚未完成。** 已接入能力状态、查询视图租约、LRU 弱登记与取消安全的冷初始化、非阻塞空闲清理，以及 search/context 的显式策略参数；dense 仍明确 disabled。最新见 [P5-D-RUNTIME-IMPLEMENTATION.md](P5-D-RUNTIME-IMPLEMENTATION.md) 与 [P5-D-RUNTIME-GATE.json](P5-D-RUNTIME-GATE.json)。

冻结 623 文件、6675050 字节，摘要 `44b30ae15be8c0ab1cb1fe71c8cb3c5027d4d0085af17678565af89c9483c5a0`；38 条命令收据、源码归档、日志和不可变二进制一致。证据目录 `artifacts/benchmarks/p5d-20260930-resume/final-v3`。stable：workspace 1786 passed/60 ignored, http 270 passed/53 ignored, focused 82 passed/3 ignored, real-mcp 25 passed/0 ignored, watcher 17 passed/0 ignored；1.95.0：workspace 1786 passed/60 ignored, http 270 passed/53 ignored, focused 82 passed/3 ignored, real-mcp 25 passed/0 ignored, watcher 17 passed/0 ignored。各组失败 0，忽略项不计通过，重叠组不相加为唯一测试总数。

固定 51 题/306 请求无排序负差分、无效源码或新增完整性失败；原 source/intent Partial 和 S11 仍失败，完整检索 gate 保持 not_passed。两个可选 retrieval_strategy 字段以外，14 工具的旧输入属性和必填项保持一致。新增 12 组 release 项目清理/租约观测，复跑 90 次旧查询成本和 192 次准入请求；可选 ps 进程树采样在本机停滞后显式关闭，对应 RSS 为 null、原生自身 RSS 单列。核心验证未跳过；不是 P5-019 的完整消融、100k、尾延迟或发行认证。

Git HEAD=`0a56a257f9a92c54d06ea5be0ce1d1763917a527`；未提交、推送、PR 或合并，既有工作与失败证据保留。

### 当时下一步：P5-019，再进入 P5-020

先读本轮 RUNTIME-IMPLEMENTATION/GATE、[QUERY_LIFECYCLE.md](../../internals/QUERY_LIFECYCLE.md) 与既有 EVIDENCE_ASSEMBLY/QUERY_EXECUTION。P5-016～018 已验收，不重复实现或把三个任务的通过升级为整批通过。

P5-019 必须完成独立 exact/path/selector 等查询质量、成本、并发消融，记录真实混合构建与查询的延迟、线程/资源归属。当前 51 题配对只证明排序和完整性状态未回退，不替代独立消融。保留旧 Partial、S11 与真实预算省略，不能改 gold、按问题编号特判或压掉状态来修绿。

冷路径登记锁属于工作线程直到缓存发布，已打开缓存走不受冷锁影响的快路径；查询 clone 共用租约，在取消后仍运行的 blocking 工作结束前不能释放。保持这些负例，不为吞吐牺牲实例一致性。监听器启动/原生析构不可强制抢占；20 秒启动看门狗不是性能指标，既有 500 毫秒 debug 索引门限未修改。

P5-020/G5 需在当前实现上完成本地增强版整体验收后再判断 M2；真实 provider/vector、持久化 semantic epoch、公开 holdout、100k 和跨平台发行仍未完成。
