# 待父亲审：单一 point-in-time status 契约及最小接口

用户最新指令将本单优化块从方案1改为方案2。以下是拟实施设计，不是已落地的 v2 产品。本检查点保全了方案1代码与测试；不继续其性能验收，也不宣称优化已完成。

## 当前安全检查点

生产改动：`capability_status.rs`、cc-db 新 `capability_read.rs`、`lib.rs` 一个导出、`freshness_store.rs` 同连接 helper 提取。capability 的 files/symbols、resolution freshness、active space、pending/failed/eligible/published 全部来自同一个短 read transaction。方案1 server 事务与 lease 结束后仍有两次原 fresh generation 检查和最多三轮；配置 space 投影改用事务内字段；worker failed 和 degradation 覆盖保留。当前 spec 仍 v1。

实际生产模块原测试 8/8 通过，inline tests 区块与 final 基线逐字节相同；冻结独立 oracle 文件未改。新目标直接编译当前生产前缀（有源码相等断言，移除的是内部私有 fixture 测试区块）并应用原独立检查：5/5 通过。这是当前生产源码测试，不能把原冻结副本通过替代它。DB 2/2（单连接、事务内提交与配置 space 改变时保持完整旧 snapshot，后续新 snapshot/freshness 正确）。

有限 AB driver 已创建：单 binary 同编译配置，用模式选择原 final status 与当前 status；实际 CodeIndex / SemanticRuntime / Gated FakeProvider；固定 generator 1k/5k、read pool 1、batch 16、dim 2、poll gap 200ms、120s deadline、独立 cold cache。第一次 release 构建因调用 crate-private attach 失败，已改为已有公共 services setters；尚未最终构建通过、没有运行 AB、没有性能数字。不是 MCP RPC transport，测量层会明确标为 in-process actual production status 与实际 worker。

## 拟批准的公共字段（单一路径，不兼容 v1 分支）

```json
{
  "retrieval": {
    "spec": "retrieval-capabilities-v2",
    "consistency": "point_in_time",
    "generation_scope": "observed_database_snapshot",
    "generation": {"incarnation": [1], "index_epoch": 6, "evidence_epoch": 0, "semantic_epoch": 2},
    "identity_validation": "checked_at_observation_boundary",
    "service_state_scope": "process_observed_separately",
    "semantic_active_space": "digest"
  }
}
```

incarnation 示例只示意字段，实际保持原 ReadGeneration 的完整 16-byte 数组。generation 字段继续保留原结构，**明确改变语义为观察 generation**，不额外复制 observed_generation 对象，不承诺响应时 latest。没有数据库时 generation/active_space 为 null、identity_validation 为 `not_observed`；不确定身份时走保守 error/unavailable/unverified，不能保留 ready 或旧 counts。

所有 DB 字段（顶层 indexed_files/indexed_symbols、retrieval generation/freshness/active space/pending/failed/dense_desired/dense_published、由这些值推导的状态）来自一笔事务。普通 index/evidence/semantic publish 提交后，不按 epoch变化重扫；返回完整旧或新 snapshot 均合法。`dense_state=ready` 只意味着该 observed generation 的 eligible>0 且 published 覆盖完成；不是 provider健康或任意查询代际正确性承诺。

`semantic_state` 保留现有 worker failed / degradation 的保守优先级，这些运行态不是事务的一部分，`service_state_scope` 显式说明。query_pins/execution 原 scope 字段保留。query严格 generation fence 一行不改。

## 最小 typed 接口及身份处理

保留单一 `ReadOps::capability_snapshot(include_semantic) -> CcResult<CapabilityReadSnapshot>`；不向 server 给 raw connection、SQL或VFS handle。新模块私有 helper 在 snapshot 前后检查**SQLite 自己打开的 main 文件**是否仍对应其 pathname。提案采用 `SQLITE_FCNTL_HAS_MOVED`，只读调用（不是 checkpoint、pragma修复或schema初始化）。

增加一个便宜的 typed 控制检查：

```rust
pub fn validate_capability_identity(&self, expected_incarnation: [u8; 16]) -> CcResult<bool>;
```

它独立获取一次 lease（snapshot 的事务和 lease 已结束），先检查该连接 main 文件的 HAS_MOVED，再用严格 metadata SELECT 取 incarnation，比较 expected，最后再检查 HAS_MOVED。不比较 index/evidence/semantic epochs，不投影这笔控制读取的其他数据；只有数值 snapshot 的 generation 对外公开。普通发布不触发统计重试；替库/in-place incarnation变化才允许最多三次重取，耗尽保守 error。unsupported VFS、SQLite错误或无法证明身份直接保守错误，不把 `SQLITE_NOTFOUND` 默认为未移动。

HAS_MOVED 所需 unsafe FFI只封装在 cc-db 私有函数；conn lease 保证sqlite handle在调用期间存活、无并发共享，C int输出缓冲正确初始化，main为静态NUL字符串。上层只消费bool/typed snapshot。Unix VFS本地bundled SQLite明确实现该控制，检查实际打开的设备/inode，避免“在当前pathname stat两次但lease早已属于旧inode”的漏洞。它覆盖rename已发生而pool尚未刷新时的可检测旧lease：返回false/错误，不会标新库ready。不改rebuild、pool替换协议、WAL/SHM处理。

这里的身份保证是**读/投影验证边界**，不是阻止响应生成后再次替库；与诊断point-in-time契约一致。若父要求身份锁一直覆盖序列化/发回响应，必须扩展rebuild与status共同的read-side publish guard，超当前写权；本提案不作该保证。

跨平台边界需要亲审：Unix有原实现；其他/自定义VFS未支持HAS_MOVED时，保守不可用。若这导致已支持平台的能力回归，不能静默放宽为“assume unmoved”；可另设计平台read-only file identity helper，但需确认支持范围后再实施。当前方案不要求修改rebuild或泄漏rawconn。

## 实现预览（尚未应用）

`POINT-IN-TIME-SERVER.patch` 对安全检查点只改字段及server控制流：将两个全generation fence改为一次typed incarnation/file identity validation；只对检测到identity变化重取。snapshot DB模块的HAS_MOVED实现尚未写入生产，草案也不能编译当作完成产品。批准后补helper、身份回归和全部v2 oracle。

## 消费者审计与迁移计划

全部生产入口共用一条capability路径：`engine.rs:595` 的 capabilities_info；`engine.rs:651` diagnostics_info 嵌retrieval；`handlers/core.rs:183` 与 `handlers/facade.rs` status capabilities/schema/all。它们透传Value，没有解析spec或缓存generation；无需并存v1/v2实现，也不需要改query读侧。

`consumer-inventory.json` 列出读取status/readiness/generation或spec的当前 Rust、Python、文档消费者（不含heldout与历史artifact树）。显式v1常量只出现在生产capability和冻结历史status副本。原公共V11 ready epoch用例使用稳定ready后generation检查实际查询的严格fence；不把它们迁移为宽松query oracle。原parameter/lifecycle default/ready/failure/permission/generation shape字段断言保留，增v2 spec/consistency/scope判据。性能脚本的ready字段入口不需要兼容分支，但此次专属harness会断言v2消费含义；不借旧脚本历史输出当新门。

新增ADR（提案编号待选）与 MCP_TOOLS.md、CONFIGURATION.md迁移说明：消费者若要当前查询保证，应依赖query自己的generation/receipts与fence，不能把status ready当实时授权。历史roadmap/ledger/TODO不改，不改伪造既有验收。

## 新 oracle，禁止仅删旧断言

生产inline发布交错用例改成：在snapshot完成后真实worker提交，允许status为完整before或after；generation与published/uncovered/pending必须匹配同一组，旧generation+新ready必须失败。新持续churn用例不再强制error，普通发布不得迫使统计三轮；每个返回snapshot在固定原时间预算内保持自洽，并最终stable ready。保留旧反例及原oracle字节在Git与本检查点证据（历史冻结target标为v1历史验证，不冒称v2）。

增加正常只读身份回归：已链接旧连接、替换后文件identity变化、in-place incarnation变化、identityunsupported fail-closed、同一连接pool、不误将配置space切换/workerfailed/degradation报告ready。文件身份用独立synthetic DELETE-journal fixture的正常rename检测，不调用rebuild、GC、kill或WAL fault；不重试曾遭RO拒绝动作。service运行态按现有scope保守处理，并明确snapshot read之后的状态变化不等价于响应时一致性。

父批准具体API/身份方案后才落最终v2测试与有限AB；1k/5k相同generator/frequency/deadline，报告status成本、错误率、ready时间、结构性count减少，结果不足也如实交付。100k remains not_run/后续独立任务。

## 身份检查依据

[SQLite官方File Control文档](https://www.sqlite.org/c3ref/c_fcntl_begin_atomic_write.html)明确HAS_MOVED可检测已打开文件的rename/move/delete；本环境 `libsqlite3-sys-0.38.1/sqlite3/sqlite3.c:44390` 的Unix实现调用fileHasMoved。SQLite内部databaseIsUnmoved在不支持该控制时历史上假设未移动，本capability提案**不沿用该弱fallback**。这是只读源码审计，尚未运行新身份测试或声称跨平台认证。
