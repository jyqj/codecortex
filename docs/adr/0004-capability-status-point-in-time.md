# ADR-0004：Capability status 使用明确的 point-in-time 诊断快照

- 状态：接受（2026-10-03，本单优化块；独立 review 后续另开）
- 范围：`status(aspect="capabilities")` 及嵌套该 retrieval 投影的 status schema/all。普通 query 的严格 generation fence、publication CAS 和原子替库协议不变。

## 问题与取舍

v1 在外层最多三次读取 generation/stats/freshness/semantic counts 并比较 generation；semantic coverage 还有内层重试。通用 stats 统计八张表，capability 只消费 files/symbols。普通 semantic publication 持续改变 epoch 时，诊断轮询可能重复统计后几乎总报错，增加回填本身的成本。

原防线解决了“旧 root generation + 新 coverage + ready”缺陷；这个实质不变量必须保留。要求任何并发提交后必须拿第二轮 latest，以及普通 churn 必须三轮报错，是旧诊断接口的时序选择，不是 query 正确性的必要条件。本次以明确的新契约取代它们；不并存长期 v1/v2 生产分支，不静默改变原 spec 的含义。历史反例、独立冻结 oracle 与方案1安全检查点留在 Git 和本任务证据中；旧冻结 target 通过只能说明 v1 历史行为，不算 v2 验收。

## 公共 v2 契约

`retrieval.spec = retrieval-capabilities-v2`，新增：

- `consistency = point_in_time`
- `generation_scope = observed_database_snapshot`
- `identity_validation = checked_at_observation_boundary`，无有效观察时 `not_observed`
- `service_state_scope = process_observed_separately`
- `semantic_active_space`（读取 semantic 时的 active-space；未读取/未配置为 null）

完整 `generation` 对象结构保留。它表示同一 SQLite read transaction 观察到的 generation/incarnation，不承诺响应时 latest。indexed_files/indexed_symbols、resolution_freshness、active-space 与 pending/failed/eligible/published 都从同一连接、同一短只读事务读取；派生状态只消费这些值。配置期望 space 与这笔事务的 active-space 比较，配置不匹配保守 partial/backfilling。

`dense_state=ready` 仅说明 observed generation 内 eligible>0 且覆盖完成；不证明真实 provider 健康、语义质量、响应时数据库未变化或随后 query 一定成功。普通 index/evidence/semantic publication 不触发全统计重试：完整旧或新快照都合法，旧 generation + 新 coverage 仍非法。

worker failed/degradation 在独立运行态观察中保守覆盖 semantic ready，原 priority 不变；query_pins/execution 保留原 scope。运行态与数据库不是跨域原子快照，不能把这些诊断作为 query 的实时授权。query 仍使用自身严格 generation/freshness/receipts 与代际 fence。

## 替库与身份边界

只比较事务内 incarnation 或在 pathname 上 stat 两次，不足以确认池中连接没有指向旧 inode。cc-db 私有只读 `SQLITE_FCNTL_HAS_MOVED` 检查 SQLite 实际打开的 main 文件，在统计事务前后执行。transaction/lease 释放后，上层通过 typed `validate_capability_identity(expected_incarnation)` 独立 checkout，读取 incarnation 并再次验证 main-file 身份；不比较普通 epochs，不从这笔控制读取投影数值。单连接 pool 不嵌套 checkout。

可检测的替库/in-place incarnation变化最多重取三次，耗尽返回保守 error/null generation/null counts，无 ready；不修复池、schema 或 WAL，不改 rebuild。SQLITE_NOTFOUND、SQLite错误或非定义的布尔结果均报 identity unverified，不采用 SQLite 某些内部路径的“unsupported就假设未移动”历史 fallback。这只限制 capability 诊断，不能据此断言普通 query 不可用。

身份保证止于观察/投影验证边界，不承诺阻止响应生成后再次替库。本次实测 Linux bundled SQLite Unix VFS；macOS、Windows、自定义 VFS 未测，不声称跨平台认证。不支持 HAS_MOVED 的 VFS 上该诊断保守不可用；未新增平台 fallback。已有公开源码未发现 Windows capability 可用性的明确承诺，后续若发现具体平台回归应单独评估。

[SQLite 官方 File Control 文档](https://www.sqlite.org/c3ref/c_fcntl_begin_atomic_write.html)定义 HAS_MOVED 的 opened-file 身份检查。只读 FFI 封装位于 cc-db，无 raw connection/handle 向 server 导出。

## 消费者迁移与验证

入口共用 `capabilities_info`，status capabilities/schema/all 透传相同 retrieval 对象；无需兼容双实现。generation 字段读取者必须同时消费 v2 scope；等待 ready 的调用方只获得一次覆盖观察，下一条 query 仍须进行自身校验。MCP 工具名称/输入 schema 不变。

新 oracle 验证完整旧/新而非强制第二轮最新；持续普通 publish 不重复统计；稳定 ready 正确；旧 root + 新 coverage 反例继续拒绝。正常 incarnation/rename 身份检查、配置 space、运行态降级、单连接 pool 另有回归。禁止以移除旧失败断言冒充通过：原历史 source/oracle 保留，当前 source 专门编译/运行新 oracle。此次不执行 kill/GC/WAL 故障或曾遭 RO 拒绝动作。

有限固定 1k/5k FakeProvider AB 单独报告读成本、错误率、ready 观察时间和边界；不降低 poll、不提高阈值，不把历史 PR101 或本次小规模结果当100k门通过。100k与独立 review 都是后续独立任务。
