# Commit 三段化：postprocess/analysis 计算移出写锁，接受最终一致窗口

- Status: accepted
- Date: 2026-06-12

## Context and Problem Statement

索引构建的 commit 半程此前是一个整体：在调用方的 `CodeIndex` 写锁内依次执行
write → postprocess（test edges、dispatch synthesis、Louvain 社区检测）→
analysis（git co-change、infra、ADR）。其中 postprocess/analysis 的计算部分
（签名扫描、合成 pass、Louvain、git log、infra 遍历）只读已提交快照，却同样
占用写锁——在中等规模仓库上这段计算阻塞所有 MCP 读工具 300–800ms（合成 10k
基准实测：单文件增量构建 postprocess p50 674ms + analysis p50 79ms，见
`docs/benchmarks/synthetic_10k_latest.md`）。prepare（scan → parse →
resolve）此前已移出锁外，commit 内的 postprocess 计算成为剩余的最大锁内
开销。

## Decision Drivers

- 读可用性：搜索/图查询是 agent 的主路径，索引构建期间不应被秒级阻塞。
- 正确性边界已经存在：R1 引入的 per-project build gate 串行化了所有构建
  入口（MCP `index`、watcher tick、auto-index），prepare 侧已有
  `index_epoch` generation guard。
- postprocess 各 pass 的 compute/apply 拆分在 dispatch synthesis
  （`synthesis_pipeline.rs`）已有先例：对已提交快照纯读计算 → 单事务原子
  apply。

## Considered Options

1. Commit 三段化：write（写锁）→ postprocess/analysis COMPUTE（无锁，产出
   类型化 delta）→ APPLY（短写锁），接受 write 与 apply 之间的最终一致窗口
2. 维持单段 commit，仅靠 PassGate 跳过缩短锁内时间
3. 把 postprocess 整体改为后台异步任务（构建返回后再补全）

## Decision Outcome

选择方案 1。实现见 `crates/cc-index/src/build_plan.rs`（模块文档即锁域契约）
与 `crates/cc-server/src/handlers/core.rs::run_split_build`：

1. `commit_write` —— generation guard + `phase_write`，在调用方写锁内；
2. `compute_postprocess` —— 无锁：通过读池读取刚提交的快照，产出类型化
   delta（`PostprocessPlan` / `AnalysisPlan`）；签名门在此阶段决策并产出
   `DeferredSignatureRecord`，由 apply 阶段在写入落地后才持久化；
3. `apply_postprocess` —— 短写锁内以短事务应用 delta 并产出 `IndexReport`。

正确性依赖两层：进程内由 per-project build gate 跨三段持有（stage 2 期间
无并发构建可改写 DB）；stage 3 在 apply 前廉价复查 `index_epoch`，跨进程
写入方表现为 `CcError::StalePreparedBuild`，`run_split_build` 整体重跑一次
prepare+commit（进程内因 build gate 永不触发）。捆绑式 `commit`（`&mut self`
构建入口）内联组合同样三个阶段函数，每个阶段只有一份实现。

方案 2 无法消除"输入确实变化"时的锁内计算；方案 3 失去"构建返回即
postprocess 完成"的报告语义，且错误处理/重试复杂度显著更高。

### Consequences

- 最终一致窗口：stage 1 提交后、stage 3 落地前，读方可能短暂看到索引内容
  已更新而 postprocess 产物（合成边、社区、test edges、co-change/infra/ADR）
  仍是旧值。这是接受的行为——每次 stage-3 apply 照常 bump `index_epoch`，
  epoch-keyed 缓存（搜索结果缓存、GraphReadModel 邻接缓存）随 delta 落地
  自然收敛。
- stage-2 的正确性前提（build gate 跨三段持有）成为调用方义务：新增构建
  入口必须先取 gate 再走 split build；`build_plan.rs` 模块文档与
  `run_split_build` 的注释声明了该契约，epoch 复查兜底跨进程场景。
- 锁内残留工作只剩 `phase_write` 与 delta apply；chunk zstd 压缩已随本轮
  一并移入 prepare（`PreparedBuild::chunk_blobs`），事务侧仅绑定预压缩
  blob（保留缺失回退）。
