# 语义持久化单库边界修订：权威 `index.sqlite3` 之外只允许可丢弃的派生 artifact cache，不引入第二权威库或隐式服务

- Status: accepted
- Date: 2026-10-01
- Supersedes（限定修订）: `DESIGN.md`「单一数据库」原则与 `docs/internals/STORAGE.md`
  开篇"所有状态存于单一数据库文件 `index.sqlite3`，没有第二个数据库"的**无限定
  表述**——权威状态仍严格单库，新增一个显式例外：内容寻址、可丢弃、可校验重建的
  派生 artifact cache 不属于权威状态。本文档即为该修订的正式决策记录；两份文档的
  文本同步按约定落在后续文档轮（见 Consequences 与"验收对齐"）。
- 关系: 不取代 ADR-0001（Cypher fast path）与 ADR-0002（commit 三段化）——语义
  持久化的 epoch 行为扩展（P6-004）以 ADR-0002 确立的 commit 分段为前提，方向一致。
- 依据: `artifacts/checkpoints/todolist-p6-owner-preparation-20261001/`
  （`ADR-0003-PREPARATION.md` + `REQUIREMENTS-PREPARATION.json`）与独立一致性评审
  `artifacts/checkpoints/todolist-completion-audit/round03/P6-READONLY-CONSISTENCY-REVIEW.md`。

## Context and Problem Statement

现行章程（`DESIGN.md` 设计原则）规定"所有状态在 `index.sqlite3`"且"没有
`runtime.sqlite3`、没有会话存储"；`docs/internals/STORAGE.md` 同样声明所有状态
存于单一数据库文件。这条边界在 P5 及以前是完整自洽的：schema 21 的所有表都是
可从源码重建的索引内容，`UnitOfWork` commit 总是 bump `index_epoch`，epoch 键控
缓存随提交自然失效。

P6（语义持久化与发布底座）引入四类此前不存在的持久化职责，逐条冲击该边界的
无限定形式：

1. **desired/outbox 与 claim/lease fencing**（P6-006/P6-007）：嵌入任务的期望
   状态必须与源码事务原子写、可跨进程认领。它们的 commit/heartbeat/retry 若沿用
   "commit 必 bump `index_epoch`"的现有规则，辅助重试会持续冲刷检索内容缓存
   （epoch 键控的搜索结果缓存、GraphReadModel 缓存）——这是把队列可靠性数据
   误当作检索状态的直接后果。
2. **昂贵向量产物**（P6-008）：嵌入向量是付费/耗时换来的产物，"索引可重建"
   假设对它不成立——按旧边界塞进 `index.sqlite3`，则索引重建（换 staging 库、
   `incarnation` 变更）会连带丢弃已付费产物，并放大主库写锁与 WAL 压力。
3. **发布（manifest CAS）与恢复**（P6-011/P6-014/P6-015）：发布需要
   incarnation/lease/doc/input/space 五重 fencing，且与崩溃恢复要求每个持久化
   边界可 kill/restart 重放。
4. **GC 与 model space 切换**（P6-016/P6-017）：GC 必须与发布共享同步点，
   空间切换要求新旧空间分数永不混排。

同时存在一条反向约束：不允许把 outbox/lease 这套内部可靠性队列**泛化**成
agent runtime、会话或工作流存储——那正是 DESIGN 非目标清单禁止的方向。
因此问题不是"要不要放弃单库"，而是"单库边界应该精确划在哪里、例外如何限定、
谁拥有跨边界一致性"。

## Decision Drivers

- 默认（不开 semantic）行为零变化：不新增网络实现、不创建第二存储、不改变
  本地可用性；默认构建产物无第二库、无网络依赖。
- 一个权威 `index.sqlite3`：当前文档、incarnation/generation、desired outbox、
  active space、manifest 可见集合及其引用关系都在这里；权威读路径只走已有
  strict `ReadGeneration`（语义 epoch key 缺失即 `None` = 未就绪，绝不当作 0）。
- artifact cache 仅是**派生产物**：可丢弃、可隔离、可校验、可重建；不得成为
  源码/manifest 权威；不存秘密；namespace 与实际输入字节/spec/space/checksum
  分开验证，跨项目默认隔离。
- 换库改变 incarnation：旧 claim/lease/doc/input/space 一律不能 publish；
  旧 cache 在完整校验后可复用（付费产物保留是修订边界的核心动机之一）。
- 文档事务原子性：源码事务原子更新文档 + 撤旧可见 manifest + 写 desired
  outbox；删除只撤销、永不产生 embedding 请求。
- 无跨两库/网络原子提交保证：artifact durable 在先、manifest CAS 在后；
  任何 crash 点都以幂等 recovery 重试兜底，不宣称跨两库 exactly-once。
- 所有者串行集成：发布/GC/worker/reconcile 的共同状态与全部 rollback 由
  owner 统一集成；worker 可分件的实现（fake provider、filtered exact）在
  ports 冻结后才可委托。

## Considered Options

1. **所有语义产物（含向量 cache）全部放进 `index.sqlite3`**：单文件最简单，
   但（a）索引重建/换 staging 库会连带丢弃已付费向量，与"重建不误删已付费
   向量"（P6-014 验收）直接冲突；（b）向量 blob 的批量读写加剧主库写锁与 WAL
   压力，侵蚀 ADR-0002 辛苦移出写锁的收益；（c）cache 的 GC 节奏与索引 GC
   完全耦合。**否决**——除非未来有基准与故障证据表明独立文件的成本更高。
2. **权威索引（单库）+ 可选派生 artifact cache**：权威状态严格留在
   `index.sqlite3`，只有可丢弃、可校验的派生产物放在独立内容寻址存储；代价是
   必须显式解决发布/GC 共享同步与 durability 顺序（artifact 先于 manifest）。
   **采纳**——与 P6 任务定义及准备材料一致，例外范围最小且可验证。
3. **独立队列服务 / 通用 runtime 存储**：把 outbox/lease 抽成独立服务或通用
   队列设施。**否决**——超出产品边界（DESIGN 非目标：不做会话/任务管理、不做
   工作流引擎），引入隐式常驻服务违反"默认确定性、离线"原则；本决策中
   outbox/lease 是 `index.sqlite3` 内的表 + 进程内 worker，不是服务。

## Decision Outcome

选择方案 2。边界线与所有权如下。

### 留在权威库 `index.sqlite3`（cc-db 拥有 schema 与写路径）

- `document` 当前态、`semantic_manifest`（可见集合）、`semantic_outbox`
  （desired 任务）、claim/lease fencing 状态（attempt token、lease 到期、
  supersede 标记）、active space 指针、incarnation/generation、语义 epoch 键。
- 写入经 P6-004 扩展的封闭枚举 `Index/Evidence/Semantic/Auxiliary` 四类 typed
  write effects：`Index`/`Evidence` 保持现行"commit 必 bump 对应 epoch"规则；
  `Semantic`（manifest/outbox 相关提交）按语义 epoch 规则推进；`Auxiliary`
  （heartbeat/retry/claim 续约）**不推进任何检索 epoch**——队列可靠性数据不是
  检索内容，这是边界修订在 epoch 协议上的直接体现。
- `semantic_edges` 静态图语义边留在 Index 侧，不进入稠密 manifest（防止把
  全量稠密结构塞进可见集合判定的热路径）。

### 权威库之外（cc-semantic 拥有，仍是本地文件、非服务）

- 内容寻址 artifact cache：按 `namespace + input digest + spec digest +
  space + checksum` 存 validated 向量；可丢弃、可逐条校验、损坏可检测且
  只降级不污染（P6-018）；跨项目默认隔离；无秘密字段。
- durability 顺序不变式：**artifact 持久化成功在先，manifest CAS 在后**；
  两存储间不存在原子提交，任一 crash 点由幂等 recovery 重放（P6-015），
  重复 ack 被 fencing 吸收。不宣称跨两库/网络 exactly-once 或零重复收费
  （与 P6-019 文档验收一致）。
- GC 与发布共享同一同步点（活跃引用 + 活跃 lease + 最短保留期 mark/sweep），
  消除"刚发布的 artifact 被 GC 删除"竞态（P6-016）。

### 所有权与集成（照准备材料的 owner 计划）

- **owner 串行集成**：`cc-semantic` 的 Cargo/lib/spec/ports、`cc-model`
  identity、`cc-db` 的 generation/schema/migration/outbox/claim/CAS/rebuild、
  `cc-index` 原子文档 delta、`cc-server` 组合根与 capability status、以及
  cache/publish/GC/reconcile 的协调与全部 rollback。
- **ports 冻结后可委托**：`cc-semantic/src/providers/fake.rs`（deterministic
  fake provider 及其故障测试）与 `cc-semantic/src/vector/exact.rs`（filtered
  exact 及其 oracle 测试）；两者永不触网，结果不计为真实语义质量。
- fencing 身份链：publish CAS 校验 `incarnation + lease token + doc version +
  input digest + space`；换库（staging 重建后 rename）改变 incarnation，
  必须围栏住仍持有旧连接的**另一进程**——进程内 mutex 不足以覆盖该场景；
  权威判定只认 strict `ReadGeneration`，legacy 双时钟读路径不得用于 publish
  fencing。

### 被否决备选方案的遗留义务

方案 1 未被永久排除：若未来实测表明独立 cache 文件在真实负载下成本高于
合库（例如单文件场景 I/O 放大），可凭基准与故障证据重启评估；届时本 ADR 的
边界定义与 durability 顺序仍适用于合库形态。

### Consequences

**对 P6 各任务的约束**（本 ADR 生效后，下列任务实现不得偏离）：

| 任务 | 边界约束 |
|------|----------|
| P6-002 `cc-semantic` 骨架 | 只依赖 `cc-model`/`cc-db`；feature + 组合根延迟初始化；默认编译/启动不生成空 cache 目录（cache 属派生产物，不预创建权威性空结构） |
| P6-003 编码空间冻结 | `VectorSpace/DocumentEncoding/QueryEncoding` 三分与完整 digest 是 cache namespace 验证的前提；同维度不同模型不可混用 |
| P6-004 typed write effects | `Index/Evidence/Semantic/Auxiliary` 封闭枚举即"epoch 边界"的机制化：Auxiliary 永不刷 `index_epoch`/检索缓存 |
| P6-005 新表 schema | document/manifest/outbox 表与索引全部进主库并按发布节点合并 schema 版本；新旧 DB 有明确重建路径，FTS 旧数据不半升级 |
| P6-006 原子写 outbox | outbox 写在源码事务内原子完成（撤旧 manifest + desired 任务 + 删除不发 embedding）；无"半个任务"泄露 |
| P6-007 claim/lease | 短事务 claim/renew/retry、每 attempt 独立 token；两进程不能同时持有同一 lease；过期 worker 无法 ack 新 lease |
| P6-008 artifact cache | 严格按"派生、可丢弃、可校验"实现；namespace 与 input/spec/checksum 分开验证；无秘密 |
| P6-009 fake provider | worker 侧实现，永不触网；端口冻结前不得开工 |
| P6-010 filtered exact | 过滤先于 exact top-k、bounded batch、稳定 ties、空间隔离；删除/异空间向量不可返回 |
| P6-011 发布 CAS | 必须 artifact-before-manifest；五重 fencing 校验；慢旧结果不能挂到同路径新版本；发布幂等 |
| P6-012 覆盖率/semantic epoch | 分母明确（eligible/published/failed/stale）；Aux 重试不冲刷完整查询缓存；零 eligible 有原因 |
| P6-013 worker 合并 | pending 合并 + 队列上限 + 公平批次 + 有界关闭；旧任务 supersede；任何时刻本地查询可用 |
| P6-014 换库 incarnation | 重建换 incarnation 后从 artifact cache 补 manifest（付费产物保留），旧 DB 时代回包被 fencing 拒绝 |
| P6-015 崩溃恢复 | 对每个持久化边界真实 kill/restart 验证；恢复有界且可复算；已存 artifact 优先复用 |
| P6-016 GC 与发布 | 共享同步点；无"manifest 引用刚被 GC 删除产物"竞态；孤儿最终可回收 |
| P6-017 model space 切换 | 新空间回填/切 active/撤销三段；不同空间分数永不混排；旧 cache 经校验可回滚复用 |
| P6-018 cache 降级 | 缺失/损坏 → 隔离坏记录、语义 degraded、本地继续；补嵌受费用策略控制，不静默无界重费 |
| P6-019 文档更新 | STORAGE/CONCURRENCY/TROUBLESHOOTING 落"at-least-once、两存储顺序、恢复步骤、namespace"事实，含本 ADR 的边界表述 |
| P6-020 验收 | 依赖图与默认包检查须证明：默认包无第二库、无网络、无隐式服务 |

**回滚策略**（沿 tasks.json P6 通用 rollback）：禁用语义 worker 并撤销新
manifest；保留派生 cache；索引按 incarnation 重建。边界修订本身可回滚：
关掉 semantic feature 后系统回到"无限定单库"的 P5 行为，cache 目录成为无害
孤儿（可整目录删除），主库语义表按 P6-005 的重建路径处理。

**明确不做（否决方向，后续提案不再重复论证）**：

- 不把 outbox/claim/lease 泛化为 agent runtime、会话、工作流或通用任务
  队列存储——它们只服务于语义产出的可靠性。
- 不引入独立队列服务、常驻进程或网络端点；默认构建产物内无第二数据库文件。
- 不宣称跨两库/网络的原子提交、exactly-once 或零重复收费。
- 不让 artifact cache 承担源码/manifest 权威、不存秘密、不做跨项目共享
  （默认隔离）。
- 不以本 ADR 将任何 P6 任务标 done——本文件是决策记录，不是实施证据。

**文档同步义务**：`DESIGN.md` 设计原则第 3 条与 `docs/internals/STORAGE.md`
开篇需按本 ADR 落限定性文本（"权威状态单库；显式例外：可丢弃派生 artifact
cache"）。本次执行委托限定只写 `docs/adr/` 新文件，故该文本同步为待办，归
P6-019 文档轮或 owner 收口轮完成；在此之前以本 ADR 为边界权威。

### 验收对齐（P6-001）

照 `tasks.json` P6-001 的 acceptance/validations，无证据项标 `not_run/blocked`：

| 验收项 | 状态 | 说明 |
|--------|------|------|
| 修改章程有明确理由、默认仍单库且无隐式新服务 | **blocked（部分）** | 理由与边界已由本 ADR 正式确定（含否决备选与不做范围）；但 `DESIGN.md`/`STORAGE.md` 章程文本同步未做（本次执行限定只写 ADR），须由后续轮落文本方算闭环 |
| 相关旧功能回归通过 | **not_run** | 本任务为纯设计文档，未运行任何回归；证据须在实施/收口轮生成 |
| V21（回滚/发行/文档：新旧 schema 降级重建、cache 不误读、默认/semantic 两包、SDK/MSRV、文档事实漂移检测） | **not_run** | 属实施期验证，当前无任何证据 |

tasks.json 状态回填（`todo` → 实际状态）按约定留给收口轮，本 ADR 不改
`docs/roadmap/`。

## 冲突与开放问题（如实记录，不擅自裁决）

1. **依赖未满足**：P6-001 `depends_on: ["P5-020"]`，且准备材料声称前置
   "独立完成 P5-020/G5"；但 `tasks.json` 当前 `current_phase=P5`、
   `next_task=P5-019`、`P5-020 status=todo`。本 ADR 按委托先行交付决策文本，
   但"前置已满足"这一前提在 `tasks.json` 中尚不成立——是否放行 P6-001 置
   done 属收口轮裁决范围。
2. **scope 与执行红线的偏差**：P6-001 scope 含 `DESIGN.md` 与
   `docs/internals/STORAGE.md`，本次执行红线限定只写 `docs/adr/` 新文件；
   章程文本同步因此顺延（见上文文档同步义务）。
3. **准备材料的验收面更宽**：`ADR-0003-PREPARATION.md` 列出 V13/V14/V16/
   V17/V21 五组"required proof before acceptance"，而 P6-001 的 validations
   仅 V21——其余四组属于 P6 后续任务的验收面，本文档只承接 V21 的"正式
   ADR/设计/存储更新"子项，不代领其他任务的证据义务。
