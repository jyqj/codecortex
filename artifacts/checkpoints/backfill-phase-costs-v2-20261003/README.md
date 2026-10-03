# Backfill phase cost diagnosis — fixed v2, 2026-10-03

结论：正式 100k 的主要时间确定在合成 HTTP 服务段之外；有限正常流水线中，durable cache put 是最大已测阶段（1k 62.9%、5k 59.2% worker 轮次时间），FIFO claim 存在随 ready 队列规模增长的排序成本。**不能把小样本占比移植为 100k 精确归因，也不能宣称 ready gate 已改善。** 不推荐先增并发、去掉 readback/fsync，或把 reclaim 从每条移走。

已结束实验。所有写入均在本目录，原 checkout 生产树无修改。无新正式 100k、真实 provider、heldout、GC/WAL/kill/fault、锁/权限改动、schema bump、merge/deploy。诊断后依用户追加交付指令，仅提交本证据目录、push 新自有分支并创建 draft PR。

## 固定身份和正式失败

- production `11af963c33cfa68cc9497e355464c1d6d058adac`；source `29b03a0fec6bfde92aac9c41dce996ebb2d08cc7`。
- PR108 head `99973e7d6faf3809add4a12cd444f8e69f04d93c`，开始和结束都由 GitHub App 元数据核对。production→source、source→PR108 的 crates/Cargo 差异均为空，见 `identity.json`。
- 唯一正式 100k：cold 29.935471s，300.000821s 观察 manifest 29082，cleanup 29696；1347 status 全 backfilling/error0，p50/p95 20.525/33.200ms。仍为 failed。
- `a8452be` 原失败和 `de4981c` 成本诊断保留在取得的 git 历史中；未改写或重新归类。status 降成本不是 ready 改善。
- 工作区找不到 AGENTS.md；根 `.agents` 空；固定 checkout 无 `.agents/skills`/SKILL.md。使用官方 `/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu`。只读使用现有 cargo registry，缺 reqwest 的 offline lookup 后，在本目录 cargo 副本正常取得依赖。没有本次 Username 错误或明确 Forbidden，未重试历史 gh Forbidden/RO 拒写操作。

## 先给出的原始日志结论

离线复用 PR108 压缩原始 HTTP/RPC。`offline.py`、`offline-http-rpc.json` 可复算：

- 排除首个原协议 fixture 等待，29081 个 HTTP handler 服务段累计 **9.803475s**，均值 0.337109ms。
- 29081 个 returned→next entered 间隔累计 **290.183813s**，均值 9.978467ms，约窗口 96.7%；相邻请求重叠为 0。
- 前 50s gap 均值 6.158858ms，最后 50s 11.606657ms；合成 HTTP 服务均值约 0.32–0.39ms，增长主要在服务段外。
- 按每 16 个输入推测轮次边界：1817 个边界 gap 累计 50.383707s/均值 27.729063ms；其他 27264 个 gap 累计 239.800106s/均值 8.795485ms。边界 gap 包含每条工作的成本，不能全部叫 reconcile。
- status 客户端 span 累计 29.318165s，可能和 worker 重叠，**不能与 HTTP/gap 累计简单相加或扣掉**。
- handler 时钟不包括完整客户端传输、响应解码、provider gate；gap 包含这些边缘成本、reclaim/claim/renew/input/cache/CAS、调度、轮次与 status 竞争。原正式运行无逐阶段 timer，100k 各阶段精确份额不可恢复。

## 有限正常流水线阶段计时

同一生成器 `source(i,value=0)`，1k/5k 独立冷语料、128 维 loopback fake HTTP，生产 MCP stdio index→后台 runtime→queue→input resolve→真实 HTTP provider→durable cache→readback→publish CAS。配置沿用 PR108（claim16、concurrency4/per-project2、parse4、poll .2s）。正常服务从首请求即返回，没有新设置的 provider hold。不是直接调用 SQL 的 pipeline 替代品。

临时诊断只加 `Instant` span；总轮次包含初始空间准备/64 desired 页协调，leaf `round_reconcile` 在 drain 前截止。provider span 包括真实 FencedProvider cache lookup + gate/HTTP/解码，内部 lookup 单独作为 nested span保留。publish CAS span包括 IndexDb write lock、BEGIN IMMEDIATE、fences、manifest/ack/epoch 和 COMMIT；durable put 完整包含校验、目录创建、两个 atomic_write 和同步，不是假 fsync。CAS、FIFO、available_at、space、readback 均未改语义。

主要采用无并行诊断构建的 `isolated-n1000`/`isolated-n5000`，全部任务完成、HTTP 每次 1 input、无请求重叠、pending/future/other-space 0，完整性/FK正常、正常 EOF exit0。cold 分别 0.195026/0.946279s；index 响应后的 observed ready 3.025091/17.192220s，worker round累计 2.893184/17.119945s。小样本完成不构成正式 100k 或 full V20 的通过。

| 阶段 | 1k 累计 s | 5k 累计 s | 5k 均值 ms |
|---|---:|---:|---:|
| `reclaim` | 0.009674 | 0.055564 | 0.011111 |
| `claim` | 0.085986 | 1.250092 | 0.249968 |
| `renew` | 0.011915 | 0.068271 | 0.013654 |
| `input_resolve` | 0.047158 | 0.294095 | 0.058819 |
| `provider_including_cache_lookup` | 0.588902 | 3.351780 | 0.670356 |
| `cache_durable_put` | 1.821009 | 10.130282 | 2.026056 |
| `cache_readback` | 0.017921 | 0.098574 | 0.019715 |
| `publish_cas` | 0.183103 | 1.155410 | 0.231082 |
| `round_reconcile` | 0.114518 | 0.638192 | 2.038953 |


`publish_total_inclusive` 为 2.023038/11.390629s；provider cache lookup为 0.005285/0.026969s，均为 nested，不能重复相加。5k 分解余量约 0.077685s，未计量的独立循环/校验/admission/计时 bookkeeping；逐轮 JSON 写 stderr 发生在 round timer 后，不包含在 round累计里。

探针扰动：每次使用 Instant+互斥 Vec 记录 wall time，16 claims一轮后输出原始 ns JSON，额外分配、时钟和日志会扰动缓存与调度。原 .2s RPC polling和20ms资源采样仍存在，报告不是无探针反事实性能。未做 uninstrumented A/B，因此不能声称探针开销为零或给精确减法校正。首轮 `pipeline-n*` 保留：worker 3.050995/15.360471s，5k部分与独立 SQL example 构建重叠；复核消除该已知并行构建，阶段排序不变，绝对时间有环境波动。正常无hold使少量 worker可能在 index 完成前开始，round累计和 post-index ready时钟不同于原正式 gated drain。

5k claim第一/末四分位均值 0.248639/0.137561ms（队列随完成变短）；这和原100k gap随时间增大方向不同。因此 FIFO排序是确定的规模成本，但它**单独不能解释原日志后半段变慢**。cache put内究竟文件 sync、目录 sync还是目录/元数据操作占主导，当前 aggregate探针尚未单独分拆；不把全put时间叫fsync时间。

## FIFO SQL A/B：独立 synthetic DB

`sql_probe.py` 为 Python SQLite {version 见文件} 的先期结果，36组1k/5k/10k、0/50/90%已完成，保存 states、ready active/future active/other pending、EQP、排序opcode和VMsteps。权威复核 `fifo_probe.rs` 真正调用 `cc_db::semantic_outbox::claim_next_fair_on`，链接生产 rusqlite **SQLite 3.53.2**，并测同一完整 UPDATE RETURNING 的 VmStep/Sort。只在独立 synthetic DB 加索引；timing 为 rollback事务里的 claim，不含commit，不能等价成pipeline。

现有：`semantic_outbox_ready(state,available_at,space_id)`，actual FIFO SELECT WHERE space/pending/available_at ORDER BY task_id → ready covering index + **USE TEMP B-TREE FOR ORDER BY**。旧 ready_tasks探针的 ORDER BY available_at,task_id不能代替这个真实计划。

候选：`CREATE INDEX diagnostic_fifo_pending ON semantic_outbox(space_id,task_id,available_at) WHERE state='pending'`。保留原 ready/doc/live-per-doc 索引、原 task_id FIFO、space/available_at、原 UPDATE RETURNING原子CAS；只取消排序。

| 10k 分布 | 当前 VM steps | 候选 VM steps | 候选+ready guard VM steps | 选中 task | 
|---|---:|---:|---:|---:|
| 10000 ready active | 90162 | 174 | 194 | 1 |
| 4000 ready / 2000 future / 2000 other pending / 1000 claimed / 1000 done | 44163 | 184 | 204 | 7 |
| 最老5000 future、后5000 ready | 45163 | 25174 | 25194 | 5001 |
| 全10000 future、无 ready | 36 | 50032 | 37 | null |

所有候选FIFO输出与独立 min(task_id) 参考相同；当前Sort=1，候选和guard Sort=0。全pending1k/5k现有9162/45162steps，候选均174。输入/剩余量分布和完整计划见 `rust-sql-probe.json`、`guarded-sql-probe.json`；终点pipeline均done，但SQL探针也保留future/other-space场景。

## 给父线程的最小后续改动

1. **一个窄的 FIFO选取改动包**：保留ready索引，后续单独设计/审核添加partial FIFO索引的正式迁移；在原CAS scalar候选处用同语句 `CASE WHEN EXISTS(ready(space,available_at)) THEN (原FIFO SELECT) ELSE NULL END` 保护无ready路径。独立synthetic已验证 guard，SQL原文见 `candidate-synthetic-only.sql`。本次不迁移生产，不bump schema，不改claim批次/锁/token/CAS。
2. 预期：减少全ready队列每次claim扫描排序，VM工作量由随剩余队列增长到接近常数；**不是pipeline 500倍加速**。5k claim占round约7.3%，即便消除全部claim成本，其余阶段保持相同时上限也只有约7.3%；100k收益未知。风险：最老future前缀仍可线性扫描，混合空间分布和统计信息会影响EXISTS与planner，新增索引增加状态转换写放大/存储，migration需在后续完成验证。
3. durable cache put是有限pipeline的主要成本，但现在没有证据支持删readback/file-sync或增并发。若后续优化cache，只先对 `cache.rs::put/atomic_write` 的mkdir、bin/meta file sync、rename、parent directory sync做窄计时，再决定是否能减少重复操作且保持“整个artifact完成持久化和readback才CAS”的边界。当前源码每个artifact写bin/meta两次file sync和两次best-effort目录sync；这仅是下一处具体调查点，不是本次已经验证的优化方案。

## 复现与保留

- 核心结论/原始计时：`offline-http-rpc.json`，`isolated-n*/phase-summary.json`、`phases-raw.json`和压缩HTTP/RPC/resource日志；完整度 `verification.json`。
- `identity.json`绑定源blob、官方rustc、二进制SHA和完整 `instrumentation.patch`。`instrument.py`在独立PR108副本加探针；新helper完整收在patch及 `phase_cost.rs`。`pipeline.py`可在新case前缀下复现有限正常流程。
- 复现构建：沿用本目录cargo/target/tmp，官方cargo `build --manifest-path source/Cargo.toml --release -p cc-server --bin codecortex --no-default-features --features semantic-http --locked`；日志 `build.log`。该二进制为诊断构建，不是原正式binary。
- 复现SQL：把 `fifo_probe.rs`作为副本 `cc-db/examples/phase_fifo_probe.rs`，官方cargo release编译，再执行 `target/release/examples/phase_fifo_probe <本目录>`（新DB目录），随后同命令加 `guard`。初始/复核日志 `sql-build.log`、`guarded-sql-build.log`。
- `source/`、`cargo/`、`target/`、语料/DB/cache都只本地保留；deliverable校验清单不把编译cache/生成语料变成提交。诊断结束时 main repo状态只增加本目录。后续证据交付以固定 source 为父提交，使用目录内独立 index/staging 排除 instrumented生产源码、runtime、语料/DB/cache；remote SHA/PR URL另存本地 delivery receipt 并在最终回复核对。没有新实验或生产更改。
