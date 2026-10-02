> 共同集成：`b738a6a3aff23dbda1ceec0b5180a9a016b03b09`；限定格式源码 draft [PR #8](https://github.com/jyqj/codecortex/pull/8) `c4dfa324143a8406ee7f54e1337c54c1e1700118`，四crate lib975 passed/0 failed/1 ignored。011范围修复与013两取消提交均按序集成；正式验收缺口见云 checkpoint integration-receipt.json；011/013仍todo，014仍in_progress。013单独PR被其执行器拒绝，未代开。Cargo.lock/依赖未改，归父对话安全任务。

> P7-014 生命周期子块：draft [PR #7](https://github.com/jyqj/codecortex/pull/7)，源码 `ca5326a627e52bd1ec75ce84a73d95e1f8ac6970`（叠加 #5）；engine 保留 subsystem，初始化错误保原项目，close/reopen 对称清理。默认 lib253/253、semantic lib280/280通过；证据见云 checkpoint 的 lifecycle-receipt.json。尚缺生产 worker 调度与 wired-state stdio；整项保持 in_progress。011 languages 与013取消问题交独立修复，不在此分支收口。

> 2026-10-02 云端最新入口：基线 `ff458bc591b4e7e444af4464d6eef2513cdb335c`，恢复提交 `ddd4f7e38ffff2d1bdd0193d5ccf4054deef82fa`，draft [PR #5](https://github.com/jyqj/codecortex/pull/5)。150 done / 1 in_progress / 41 todo；P7-011～013 独立复核待收口，P7-014 进行中。旧 P5 交接正文仅为历史；云端实测与未通过项见 `artifacts/checkpoints/cloud-p7-014-20261002/receipt.json`。默认/semantic 构建恢复，状态4/4、semantic lib278/278、默认真实stdio1/1通过；全仓2250/4/60，lint/format失败；生产worker调度和wired lifecycle stdio尚未完成。

# 08｜执行交接与下一轮入口

## 1. 当前状态

**118 done / 1 in_progress / 73 todo，P5 为 18/20；P5-016～018 已验收，P5-019 正在实施通用查询质量修复与独立消融。P5-D 整批与 G5/M2 尚未完成。** 已接入能力状态、查询视图租约、LRU 弱登记与取消安全的冷初始化、非阻塞空闲清理，以及 search/context 的显式策略参数；dense 仍明确 disabled。最新见 [P5-D-RUNTIME-IMPLEMENTATION.md](P5-D-RUNTIME-IMPLEMENTATION.md) 与 [P5-D-RUNTIME-GATE.json](P5-D-RUNTIME-GATE.json)。

冻结 623 文件、6675050 字节，摘要 `44b30ae15be8c0ab1cb1fe71c8cb3c5027d4d0085af17678565af89c9483c5a0`；38 条命令收据、源码归档、日志和不可变二进制一致。证据目录 `artifacts/benchmarks/p5d-20260930-resume/final-v3`。stable：workspace 1786 passed/60 ignored, http 270 passed/53 ignored, focused 82 passed/3 ignored, real-mcp 25 passed/0 ignored, watcher 17 passed/0 ignored；1.95.0：workspace 1786 passed/60 ignored, http 270 passed/53 ignored, focused 82 passed/3 ignored, real-mcp 25 passed/0 ignored, watcher 17 passed/0 ignored。各组失败 0，忽略项不计通过，重叠组不相加为唯一测试总数。

固定 51 题/306 请求无排序负差分、无效源码或新增完整性失败；原 source/intent Partial 和 S11 仍失败，完整检索 gate 保持 not_passed。两个可选 retrieval_strategy 字段以外，14 工具的旧输入属性和必填项保持一致。新增 12 组 release 项目清理/租约观测，复跑 90 次旧查询成本和 192 次准入请求；可选 ps 进程树采样在本机停滞后显式关闭，对应 RSS 为 null、原生自身 RSS 单列。核心验证未跳过；不是 P5-019 的完整消融、100k、尾延迟或发行认证。

Git HEAD=`0a56a257f9a92c54d06ea5be0ce1d1763917a527`；未提交、推送、PR 或合并，既有工作与失败证据保留。

## 2. 下一步：P5-019，再进入 P5-020

先读本轮 RUNTIME-IMPLEMENTATION/GATE、[QUERY_LIFECYCLE.md](../../internals/QUERY_LIFECYCLE.md) 与既有 EVIDENCE_ASSEMBLY/QUERY_EXECUTION。P5-016～018 已验收，不重复实现或把三个任务的通过升级为整批通过。

P5-019 必须完成独立 exact/path/selector 等查询质量、成本、并发消融，记录真实混合构建与查询的延迟、线程/资源归属。当前 51 题配对只证明排序和完整性状态未回退，不替代独立消融。保留旧 Partial、S11 与真实预算省略，不能改 gold、按问题编号特判或压掉状态来修绿。

冷路径登记锁属于工作线程直到缓存发布，已打开缓存走不受冷锁影响的快路径；查询 clone 共用租约，在取消后仍运行的 blocking 工作结束前不能释放。保持这些负例，不为吞吐牺牲实例一致性。监听器启动/原生析构不可强制抢占；20 秒启动看门狗不是性能指标，既有 500 毫秒 debug 索引门限未修改。

P5-020/G5 需在当前实现上完成本地增强版整体验收后再判断 M2；真实 provider/vector、持久化 semantic epoch、公开 holdout、100k 和跨平台发行仍未完成。


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
