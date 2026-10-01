# ADR-0003 只读准备：可选语义产物与单权威索引边界

Status: preparation，未修改正式 ADR/设计章程，未实现，未验收。
Date: 2026-10-01
前置：独立完成 P5-020/G5；当前 source-v2 冻结期间不得改 runtime/spec。

## Context and Problem Statement

DESIGN 的原则是所有状态在 index.sqlite3；STORAGE 当前 schema21 的 UoW commit 总是 bump index_epoch。P6 已批准任务要求需要持久化的 desired/outbox、claim/lease fencing、可复用昂贵向量产物以及重建后 recovery。必须正式说明限定修订理由，不能把内部可靠性队列泛化成 agent runtime、会话或工作流存储。

## Decision Drivers

- 默认不开 semantic 时不新增网络实现、不创建第二存储、不改本地可用性。
- 一个权威 index.sqlite3：当前文档、incarnation/generation、desired outbox、active space、manifest 可见集合与其 CAS 都在这里。
- 可选内容寻址 artifact cache 仅是可丢弃/隔离/校验/重建的派生产物；不得成为源码/manifest 权威，不存秘密。namespace 与实际输入字节/spec/space/checksum 分开验证。
- 换库改变 incarnation，旧 claim/lease/doc/input/space 均不能 publish；旧 cache 可在完整校验后复用。
- 文件事务原子更新文档+撤旧可见 manifest+desired outbox；删除只撤销，不产生 embedding 请求。
- 无跨两库/网络原子提交保证：artifact durable 在先，manifest CAS 在后；任一 crash 点以幂等 recovery 重试。

## Considered Options

1. 所有大产物塞进 index.sqlite3：单文件简单，但重建会丢复用产物、增加索引写锁/WAL压力；应基准和故障证据再决策。
2. 权威索引 + 可选派生 artifact cache：与 P6 原任务一致，但必须解决发布/GC共享同步与 durability 顺序；拟选，尚待实现审阅。
3. 独立队列/通用 runtime 服务：超出产品边界，不选，不引入隐式服务。

## Owner integration plan

先固定编码与身份 ports，类型化 Index/Evidence/Semantic/Auxiliary 写 effects、schema和 desired/claim/CAS；worker 仅在 stable ports 后独占 deterministic fake、filtered exact 及其 oracle。发布/GC/worker/reconcile 的共同状态与所有 rollback 由 owner 串行集成。

## Required proof before acceptance

V13：旧默认 commit 行为保持；辅助 heartbeat/retry 不刷检索内容；rollback不推进；换库 incarnation。
V14：短事务 claim，attempt token，expired/superseded/delete/two-process fencing，artifact-before-manifest，重复 ack。
V16：过滤先于 exact top-k，手算分数/稳定 ties/空间隔离/坏向量/批上界。
V17：每个持久化边界真实 kill/restart、cache缺失/损坏、GC/publish竞争、空间切换/撤销，默认不自动无界收费。
V21：正式ADR/设计/存储更新，默认包无第二库/网络，semantic可选包、旧schema隔离重建、SDK/MSRV/回滚全部新证据。

不使用本准备文件将任何 P6 项标 done。
