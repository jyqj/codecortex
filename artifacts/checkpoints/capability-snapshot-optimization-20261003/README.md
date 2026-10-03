# Capability snapshot：契约问题上报，未实施生产优化

指定 final 基线 `6db4d396e5d994388ada1e97c3d28947ffdb9c81`，生产基线 `9ebdb155c64e094b3d774be41dbe7ab8c4e222c1`，诊断 `de4981c0727c82cb02096c705f6428bcfe4649a6`。本交付执行用户的备用路径：先带最小 diff 设计问题报父，保留 outer checks。没有修改生产文件、现有测试、根 ledger/TODO；不是完成的性能优化，也不是独立 review。

## 必须先明确的选择

现有 status 的契约比“所有字段同一时点”更强。`capability_status.rs:480` 的实际 FakeProvider 发布交错用例在 observer 内提交新 publication 后，要求返回新 generation、dense ready、observer 恰好调用两次（第一轮被丢弃）；`:649` 在每轮变更后要求恰好三轮，retryable error、null generation、disabled dense。独立 `tests/support/p7_status_review/checks.rs:186` 与 `:265` 分别保留第二轮最新结果和 churn 三轮失败。冻结 `tests/support/p7_status_review/candidate.rs` 直接 include checks；顶层独立用例编译这个副本，不会自动验证未来生产改动。不能修改副本来获得绿色，也不能仅跑副本宣称候选通过。

一笔短 read transaction 能在第一次 metadata SELECT 固定 SQLite snapshot：即使后来普通 semantic publish 提交，全部 counts、freshness、active space、pending 都仍属于完整旧 snapshot。若直接返回它，数据没有混代，但在上述交错下得到 old generation 和 backfilling，observer 一次；因此现有 latest 判据不成立。这与旧缺陷“旧 root + 新 coverage + ready”不同，需要独立新增 as-of oracle，但不能据此放宽原 oracle。

父集成方需要确定以下哪个交付是当前任务的目标：

1. **保持现有 latest status。** typed transaction 完全可做；server 继续比较 transaction generation 与事务结束后的 latest generation，并保留三轮上限。删除六个无用 COUNT、coverage 内层重试与跨连接投影，但每次跨 generation 仍需重新统计，持续 churn 仍会返回现有契约指定的错误。可以减少成本，不能保证 churn 下成功，也不能宣称移除了重复全扫。
2. **新增显式 as-of 消费契约。** 新 typed API 返回完整旧或新 snapshot，字段显式声明 `as_of_generation`/数据库 incarnation；普通 semantic publish 不触发全扫重试。现有 latest 路径及全部 oracle 保留，新增独立 oracle 验证 as-of 路径。如何由 public status 消费或选择这个路径，需父方明确契约/API 边界，不能默认把 `retrieval-capabilities-v1` 的现有字段降为 as-of。

如果“持续 churn 不饥饿”只要求现有三轮有界错误而非成功返回 snapshot，则方案1可在现有生产写权内继续；这不是本交付已验证的优化。本次不自行把这句话解释成较弱验收。

## 最小 diff

`PROPOSED-DB-ONLY.patch` 是未应用、未编译的数据库设计草案，不是可投产补丁：

- 新 `CapabilityReadSnapshot` / `CapabilitySemanticSnapshot` 和 `ReadOps::capability_snapshot_as_of(include_semantic)`。
- 一次 read pool checkout，一笔 deferred read transaction，以严格 `read_generation::read_on` 固定时点；仅统计 files/symbols 和实际消费的 eligible/published/pending/failed，不统计其他六表、不做 stale/reason 的额外全扫。
- 从现有 freshness 读取中提取同连接 helper，snapshot 使用严格 generation 的 index_epoch；legacy helper 保持原事务行为。
- `active_space` 与全部 semantic 数值来自同一事务；未配置时返回真实 None/零，所有 SQL 错误向上传播，不用 unwrap_or(0)。语义读取可按实际 wiring 跳过。
- 不提供 raw connection；不改现有 `semantic_coverage()` 的 latest/fence 契约；不接入 server，不删 outer checks。

`git apply --check` 成功只证明补丁适用于基线。草案尚未经过编译/并发/性能验证，不能用作生产 ready 权威。

## identity 与运行态必须单独保守处理

事务中两次 generation 一样只证明同一 SQLite snapshot，不证明池中连接仍对应 live pathname。`index_db_rebuild.rs:91` 在 write lock 下 rename，随后重开 writer，再更换 pool；`index_db.rs:708` checkout 只短持 pool RwLock，返回的 lease 不阻止 rename。旧 lease 在替库窗口中可以两次读到旧 incarnation；只在旧连接事务内读 incarnation 或持 pool read lock均不足以与 rename 协调。不得让此草案直接把旧库报告为新库 ready。

现有 outer fresh checkout 是既有防线，不能删，也不能把它声称为全面消除了 rename→pool-refresh 窗口。后续需在允许范围内设计只读 live-path identity validation：控制检查与投影数值分开；释放首 lease 后再验证，不在一连接 pool 内嵌套 checkout；检测替库、缺失或 identity 不确定时保守失败。若要求严格与 rename 原子协调，现有 rebuild 写权之外的协调变更需要父方明确允许，本次没有动 rebuild 或运行替库故障测试。

配置 space 期望 digest 与同事务 active_space 比较，不能再跨连接取 active-space。worker failure/degradation/attachment 是进程运行态，不属于 SQLite snapshot；必须显式区分 DB as-of 观察与 service 运行态，保留 failed/degraded 覆盖 ready 的优先级，并补 detach/配置切换正常并发测试。

## 验证与后续有限测量

已读 CONTRIBUTING.md、相关 generation/coverage/freshness/rebuild 源码、ADR-0003 与原 status 修复收据；`/AGENTS.md`、`/workspace/AGENTS.md`、指定 tree 的 AGENTS.md/SKILL.md 均未发现。当前个人 skills 中没有适用于此数据库优化的专项 skill；没有触发小说、Agent 架构或 Library 工作流。

`prepare_design.py` 重生成草案与 `source-evidence.json`，核验十个相关文件在 final 基线逐字节不变，保存 SHA256，检查独立 oracle 的关键断言存在。没有执行 runtime tests。没有执行固定 1k/5k fake/statuspoll AB：尚未选定可接受的 server 消费契约，没有候选产品，不能制造等价 AB 或给出性能结论。100k 明确 not_run，留给后续独立任务，旧 PR101 不构成本候选结果。

确定契约并实现后才进行以下正常并发验证与测量：

| 验证 | 必须保留的判据 |
|---|---|
| 既有 latest 发布交错/churn | 原独立及生产 oracle 不删不放宽；候选生产须单独运行 |
| 新 as-of 发布交错 | 完整 old 或 new；generation、coverage、pending、计数不混时点 |
| 稳定完成 | 最终 ready 与 publication generation/count 正确 |
| 持续普通 publish churn | 明确选定成功 as-of 或三轮 latest error；记录扫描次数与错误率 |
| incarnation/swap/config-space | 无旧 inode 新库 ready、配置 mismatch 保守；不以事务内 generation 代替身份验证 |
| service failure/degradation | 运行态覆盖 ready；单独标明观察范围 |
| 单连接 pool | checkout 不嵌套、无死锁；沿用原超时 |
| 固定 1k/5k fake +有限 statuspoll AB | 同 generator、相同 batch/config、频率和超时、隔离 cache；报告统计成本、RPC、ready耗时、错误率；不调阈值或降低 poll |

真实 provider=0；heldout/账号/私源未读，未外传；没有 kill/fault injection、GC/WAL 测试，也没有重试已拒绝的 RO 动作。没有 schema/migration/parser/queue/cache/publish/rebuild/gc 改动。
