# 08｜执行交接与下一轮入口

## 1. 当前状态

**115 done / 3 in_progress / 74 todo；P5为15/20。P5-016～018保持进行中，P5-019/020保持todo。** 最新P5-D final-v3验收failed：Rust 1.95 workspace退出101，`project_session::tests::close_idle_instances_closes_cached_non_active_projects`预期关闭2实例、实际1；独立audit.json不存在。先定位并复验此问题，不重复开发已完成的P5-C，也不把stable的通过替代最低工具链验收。见[当前快照](CHECKPOINT-2026-09-30.md)与[P5-D-PROGRESS.md](P5-D-PROGRESS.md)。

**下面两段为P5-C历史验收，不能用于宣称当前P5-D通过。** 其完整记录见[P5-C-COMPLETION.md](P5-C-COMPLETION.md)与[P5-C-GATE.json](P5-C-GATE.json)。

冻结616文件、6605476字节，摘要`471919b1d73828db2938d314e3319dd34827a7c1828388925c45b1db7372f473`。38条命令收据，源码/归档/日志/不可变二进制均核验；最终接受版本对当前源码重新运行全套验证；中间版本曾复用的旧收据只保留历史，不代替最终结果。证据目录`artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930`。两工具链各workspace 1767 passed/57 ignored、HTTP 255 passed/50 ignored、专项 71 passed/2 ignored、真实stdio 24 passed、取消协议1 passed、watcher17 passed。各组内部失败0，忽略项不计通过；测试组有重叠，不相加为唯一测试总数。

固定51题306请求的逐题Top-1/nDCG无负差分，无效源码命中为0，raw回放一致。原Partial/S11继续失败，本次新增18次真实预算省略Partial，涉及6个问题；完整检索Gate保持not_passed，不能称G5/M2通过。gold与评分公式未改。一次默认并行HTTP测试的debug索引计时超过原500毫秒门限，原失败保留；源码不变的独立复验通过，后续计时相关测试组串行执行，内部并发测试不变。未放宽阈值，不把隔离通过当成共享负载性能认证。

本次Git快照以`4514630dcd26481cf6dbc2aff38824ed71ef06da`为父基线，保存当前源码与全部任务文档；提交号以仓库Git历史为准。轻量原始收据位于`artifacts/checkpoints/20260930-git-sync/p5d-final-v3/`，完整benchmark工作区、二进制和导出包保留本地而不入Git。历史收据的旧HEAD/digest不改写成新提交证据。

## 2. 当前批次：先收口P5-D的P5-016～018，再推进P5-019～020

先调查最低工具链下空闲项目回收测试为何只关闭一个实例；目前只有失败现象，不能先断言是计时抖动或产品缺陷。保留原失败收据，冻结修复源码后执行所需复验和独立审计，满足验收再勾选P5-016～018。随后按依赖推进P5-019的质量/成本/并发消融及P5-020整体验收；保留原题库、评分和失败门禁。

先读P5-C-COMPLETION/GATE、EVIDENCE_ASSEMBLY.md与QUERY_EXECUTION.md，复用ReadGeneration、拥有资源的QueryHandle、有界执行、取消栅栏和rank-only融合。最终校验必须在selector/packing之前；异步后置freshness仍需完整字节计数和接受代际复核，不能换用另一活动项目的正文。

预算的正文/引用优先级已修复旧五题退化；文件引用保留原digest，召回理由是诊断，不是准确函数正文。新增packing.partial不是旧失败消失，P5-019/020必须复核完整性、无答案及实际代价。不得改gold、按题目编号特判或压掉Partial修绿。

三次外层装配尝试和三次内部检索共用原deadline，不是无限重试；每文件磁盘检查不是原子快照。配置取得前600秒临时准入、不可中断SQL、冷路由等旧边界保留。需要P5-D后才能判断G5/M2，不能凭P5-C完成提前宣布。

schema21不变，public adapter7、retrieval policy16、packing spec2。预算针对code_index_context结果对象，不包含JSON-RPC帧；其他旧工具形状保留原预算。没有真实provider/vector、持久化semantic epoch、原子文件系统快照、公开holdout、跨平台或发行认证。

## 3. 每次开始

读取README、04对应阶段、05对应任务、相关02/03契约和06/09验收。核对HEAD/dirty/nested规则；只对授权批次修改；判断哪些依赖done且证据覆盖当前目标。使用现有本地工具读实际实现，不因设计树中有文件名就假设已经存在。

变更比计划更早落地时，以实际代码/测试证明并更新任务状态，不重复实现；发现计划依赖不合理时修订tasks.json及说明，不暗中绕过。原始参考固定SHA，不随手把主分支新行为套到旧benchmark分数上。

## 4. 每次结束

冻结被测源码后跑本批V编号和相关旧回归；留命令、退出码、日志、run-id、diff摘要、源码SHA/dirty digest。更新tasks.json：完成必须有evidence；阻塞列出准确原因和受影响的发布范围；conditional未授权不执行，不当成功。由JSON重新生成05-TODO，校验source hash；更新08下一批入口与PLAN-CHECK。

需要提交PR时在用户授权后使用可用GitHub连接器，明确分支/基线/变更范围，不自动合main。只落本地规划不是已提交到Git。

## 5. 任务状态更新契约

唯一状态源tasks.json。每项保留id/phase/batch/title/scope/depends_on/steps/deliverables/acceptance/validations/rollback/conditional/required_for/evidence。id不复用，删除任务改deferred或superseded note并保留可追踪关系，不重新给后续所有任务编号。

完成证据最少：target_sha、worktree_digest、commands、exit codes、run_id/artifact paths、review、rollback_status。done需要校验对应V全部适用项已通过；依赖中的conditional任务可以明确deferred，但依赖gate只能对不要求该live能力的scope给结论，不能将此转换成全量认证。

Markdown派生规则：按phase_order和任务原顺序；状态todo/blocked/deferred显示未完成，done显示勾选；每项输出现有字段与证据；首段记录tasks.json SHA-256。不得手工只勾05不改JSON。P0-020会把这套维护收口到文档工具；本轮生成使用一次性文档生成脚本，没有向生产包加入维护代码。

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

校验13个必需文件存在；tasks总数/phase数/batch数；唯一ID；依赖都存在且DAG无环；phase与batch编号；引用V编号均在06登记；重要字段非空；当前状态计数、done项证据、根current_phase/next_task提示与硬依赖一致；05包含全部ID且source hash一致；相对Markdown链接可解析；新增实现、测试、文档和证据均在本批授权范围；HEAD不变。

PLAN-CHECK.json记录实际检查结果、文件字节/行数/hash（不包含自身hash以避免递归）、检查范围和非目标。这不是业务代码测试；不得把PLAN-CHECK的passed用作G0–G9通过证据。
