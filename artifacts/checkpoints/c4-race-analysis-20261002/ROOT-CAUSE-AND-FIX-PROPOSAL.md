# c4 混合负载系统性竞态：根因深挖与修复设计提案

- 日期：2026-10-02
- 性质：只读分析 + 设计提案。本文档不修改任何 `crates/` 源码。
- 证据基线：`artifacts/benchmarks/p5e-formal-runs-20261001-v4-rerun-r1/R1-RERUN-RECEIPT.json`（R1 复跑回执，c4 2/2 复现、c16 瞬态对照、零数据损坏），以及 `artifacts/benchmarks/p5e-formal-runs-20261001-v4/` 对应 mixed stage 原始记录。
- 实施前置：**P5-020 收口之前不得改动 `crates/`**。本提案仅为设计输入。

---

## 1. 症状回顾

- 错误签名：`Mcp error: -32603 "index changed during retrieval after N attempts; retry the query"`，`data.retryable=true`，`response_classification=rpc_or_transport_error`（非超时：service+transport 仅 7.4ms/18.1ms）。
- 触发条件：concurrency≥4 且读与 `full:true` 全量重建交错。约 1–4/300 读落在代变更上。
- 两类出口：
  - **硬 error 尾部**：内部重试耗尽（或 fence 外单次检查命中）→ `-32603`。v4 出现 2 次（c4-baseline seq154 `attempts=1`；c16-candidate seq85 `attempts=3`）。
  - **降级出口**：fence 内部重试成功恢复 → 响应正常，但 dispatch seam 的 freshness 对比（dispatch 前后 `index_epoch`）判定代变更 → `resolution_freshness.status=changed_during_query`、`resolution.complete=false` → harness known-red 判定失败（要求 `resolution.complete=true`）。v4/rerun 共 7 次（c4: seq306/149/167/327、seq26/147；c16: seq85/173/294）。
- 零数据损坏：全部受影响 stage 完成 330 job，count lock 一致（回执 `real_finding_for_product_record`）。

---

## 2. 守卫机制完整定位（文件:行号）

### 2.1 代（generation）的定义与持久化

| 机制 | 位置 | 说明 |
|---|---|---|
| `ReadGeneration` 结构 | `crates/cc-model/src/generation.rs:4-26` | `{incarnation:[u8;16], index_epoch, evidence_epoch, semantic_epoch}`；`local_key`/`graph_key` 是缓存键的代成分 |
| 持久化读取 | `crates/cc-db/src/read_generation.rs:50-89`（`read_on` / `ReadOps::read_generation`） | 每次**新从读池 checkout 一条连接**读 metadata 四键；无进程内快照 |
| incarnation 写入 | `crates/cc-db/src/read_generation.rs:32-48` | `ensure`（打开时）/`renew`（重建发布前，staging db 上调用） |
| epoch bump 规则声明 | `crates/cc-db/src/epoch_rules.rs:40-166`（`EPOCH_RULES`） | 表→时钟单一声明；写方法内显式调用 |
| bump 实现 | `crates/cc-db/src/index_db.rs:612-624,675-683`（`bump_index_epoch_on`/`bump_evidence_epoch_on`/`bump_epoch_on`） | 与数据写入**同一事务** |
| 重建收尾代 | `crates/cc-db/src/index_db.rs:635-663`（`finalize_rebuild_generation`） | `max(floor,live)+1` 双 epoch + `renew` 新 incarnation，写入 staging 文件 |

### 2.2 谁 bump 代（本竞态中的写入侧）

- **全量重建**（harness 每 10 读发一次 `index full:true`）：
  - 入口 `crates/cc-server/src/engine.rs:318-326`（`build_index`，build gate 串行化，并发第二次请求得 `BuildBusy`）。
  - 协议 `crates/cc-db/src/index_db_rebuild.rs:278-286`（`run_rebuild_protocol` = `build_rebuild_staging` + `swap_rebuild_staging`）。
  - **提交瞬间**：`swap_rebuild_staging`（`index_db_rebuild.rs:91-160`）——持写锁（:109）→ `finalize_rebuild_generation`（:114）→ 删旧 WAL/SHM（:117-120）→ **原子 rename temp→main**（:123）→ 重开写连接（:138-142）→ **整池重建读池**（:145-152，`build_read_pool`）。
  - 每次全量重建 = **恰好一次**代变更（双 epoch+1 且 incarnation 全新）。旧文件被 rename 后由仍在使用中的旧池连接以 unlinked inode 形式继续可读（数据完整，这是零损坏的结构原因之一）。
- 增量写（本负载不出现，但同一机制）：`bump_index_epoch_on` 随 `replace_files_batch` 等写事务提交。

### 2.3 读路径在哪检查（读侧 fence 与检查点）

读侧核心是 `SearchEngine::with_stable_generation`（乐观代 fence）：

```rust
// crates/cc-search/src/engine_cache.rs:185-214
pub fn with_stable_generation<T>(&self, mut work) -> CcResult<(IndexGeneration, T)> {
    const MAX_ATTEMPTS: u32 = 3;                       // :189 重试上限
    for _ in 0..MAX_ATTEMPTS {
        let before = self.observe_epochs()?;           // :191 work 前读代（并做缓存失配清理 :154-179）
        let result = work(before);
        if matches!(&result, Err(QueryCancelled|QueryTimedOut|QueryInvalidated)) {
            return result...;                          // :193-200 取消类不重试
        }
        let after = self.db.reads().read_generation()?;// :201 work 后再读代
        if before == after {
            return result...;                          // :202-206 代稳定 → 接受（含 Err 原样上抛）
        }
        std::thread::yield_now();                      // :209 丢弃混合代尝试；唯一"退避"=让出 CPU，无 sleep
    }
    Err(CcError::RetrievalChanged { attempts: MAX_ATTEMPTS })  // :211-213 硬 error
}
```

fence 的嵌套结构（harness `search` 工具 mode=hybrid 实际路径）：

1. MCP dispatch seam：`crates/cc-server/src/mcp.rs:80-108`（`spawn_handler!`）—— `:86` 捕获 dispatch 前 freshness，`:88` 响应后再读并 attach。
2. handler：`crates/cc-server/src/handlers/context.rs:29-63`（`search_async`）→ `crates/cc-server/src/query_handle.rs:59-123`（`search_async`）→ `:116` `search_in_context_with`。
3. **外层 fence**：`crates/cc-server/src/engine.rs:624-628`（`search_in_context_with` 包 `assemble_context_once`）。
4. `assemble_context_once` 内：
   - **内层 fence**：`engine.search_context_candidates` → `crates/cc-search/src/engine_graph.rs:106-108`（`search_with_graph_context` 也是 `with_stable_generation`）；plain `search` 同理在 `crates/cc-search/src/engine.rs:198-231`（`:202`）。即 **3×3 = 最多 9 次尝试**。
   - hydrator 代检查：`crates/cc-server/src/engine.rs:676-682` 用**外层** `generation` 构造 `EvidenceHydrator`；`EvidenceHydrator::finish` 在 `crates/cc-search/src/evidence_hydrator.rs:193-198`——`read_generation()? != self.generation` → `Err(RetrievalChanged{attempts:1})`（构造时 ：69 与 hydrate 前后 ：75/:162 都会触发）。
   - 组装尾部再查一次：`crates/cc-server/src/engine.rs:862`（`verifier.finish()?`）。
5. **fence 外的单次检查（硬 error 主源）**：`crates/cc-server/src/handlers/context.rs:44-56`——fence 已接受 envelope 后，在 `run_cpu` 闭包内 `serde_json::to_value(envelope)` 后调用 `cc_search::evidence_hydrator::validate_envelope_generation(&db, &value)?`（`:55`）。该函数（`evidence_hydrator.rs:19-37`）重读当前代并与 envelope 内嵌的 accepted generation 比较，**不等则直接 `Err(RetrievalChanged{attempts:1})`，无任何重试**。
6. 语义 lane 代检查（fence 内）：`crates/cc-search/src/lanes.rs:271-274`（`append_semantic_outcome`，`attempts:1`，语义端口未启用时不可达）。
7. 无 fence 的读路径：`crates/cc-server/src/handlers/context.rs:65-94`（`symbol_search_async`，直接 `find_symbol_in_scope`，仅靠 dispatch freshness 前后对比降级，本身不产生硬 error）。

### 2.4 硬 error 与 Partial 降级的分支条件

- **错误→wire 映射**：`crates/cc-server/src/mcp.rs:50-60`（`handler_error_data`）：`CcError::is_retryable()` → `-32603` + `data.retryable=true`；`RetrievalChanged` 在可重试清单 `crates/cc-model/src/error.rs:87`；消息模板 `error.rs:45-48`。`attempts` 值即上抛的 `RetrievalChanged{attempts}`：
  - `attempts=3`：外层 fence 三次尝试全部跨越代变更（或内层 fence 耗尽后恰逢外层代稳定原样上抛）。
  - `attempts=1`：fence 外 `validate_envelope_generation`（context.rs:55）单次命中——**无重试，一次即硬 error**。历史旁证：`artifacts/checkpoints/20260930-git-sync/p5d-final-v3/stable-real-mcp.log:163-196` 一次全量重建提交后约 0.1s 内 14 个并发读 id 连续报 `after 1 attempts`。
- **Partial 降级**：fence 成功（含内部重试恢复）后，dispatch seam 前后 `resolution_freshness` 的 `index_epoch` 不等 → `attach_observed`（`crates/cc-server/src/handlers/freshness.rs:43-68`，分支 `:48-56`）置 `status=changed_during_query`、`complete=false`、`reason=index_generation_changed_during_query`。freshness 读取实现：`crates/cc-db/src/freshness_store.rs:55-86`（与 epoch 同一 SQLite 快照）。
  - **注意语义错位**：fence 内部重试成功时返回的是**新代上的自洽新鲜结果**，但 dispatch 前后 epoch 仍不等 → 依旧被打成 `changed_during_query`/`complete=false`。这是"恢复成功仍标红"的直接原因（seq306/seq26/seq147 一类）。

---

## 3. 复现机制图

```
worker A(读)                                worker B(全量重建)
────────────────────────                    ─────────────────────────────
search_async (context.rs:29)
 └ capture freshness before=epoch G0        Indexer full rebuild
 └ 外层 fence before=G0                        (staging 写 temp，不动 live)
    └ 内层 fence before=G0
       └ lanes/plan 各自 checkout 连接
         （此刻仍属旧池 → 旧 inode → G0）
                                            swap_rebuild_staging (index_db_rebuild.rs:91-160)
                                              finalize G1=max(floor,live)+1 + renew incarnation
                                              rename tmp→main  ← 提交瞬间
                                              读池整体替换为新池(:145-152)
       └ 内层 fence after=G1 ≠ G0 → 重试
          第二次尝试：observe G1，work 全新池 → 接受
    └ hydrator.finish：current=G1，但 self.generation=外层 before=G0
       → Err(RetrievalChanged{1})            （若外层 after==before 才会上抛，见 §4.3）
 └ 外层 fence after=G1 ≠ G0 → 第 2 次尝试成功
 └ 【分支 α：恢复成功】
    attach_observed：dispatch before=G0 ≠ after=G1
    → status=changed_during_query, complete=false（数据其实新鲜自洽）
 └ 【分支 β：硬 error】context.rs:55 validate_envelope_generation
    在 fence 接受之后、序列化窗口内又一次提交落地
    → Err(RetrievalChanged{1}) → -32603，无重试
 └ 【分支 γ：硬 error】外层 fence 3 次尝试各跨越一次提交（重建排队密集时）
    → Err(RetrievalChanged{3}) → -32603
```

关键放大器：**一次查询在一次 handler 生命期内要读代 6–9 次**（外层 fence 前后、内层 fence 前后 × 重试、hydrator 2–3 次、fence 外 validate 1 次、dispatch freshness 2 次），代变更瞬间附近的"暴露窗口"是整段 handler 时长，而非单条 SQL。

---

## 4. 为什么并发 4 是经验阈值（结构解释）

1. **c1 结构性为零**：harness 用单一共享有序 job 队列（`artifacts/benchmarks/p5e-harness-20261001/final-v4/driver/src/mixed_stdio.rs:204-216`），`concurrency` 个 worker 并发领取（:244-261）。c1 时读与 full build 由同一 worker 严格串行执行，build RPC 返回即 swap 已完成，下一条读必然在新代上发起——**不存在跨越提交的读**。这与普查 c1=0/0 完全一致。
2. **c≥2 起竞态在结构上可能**：读（worker A）与 build（worker B）并行，swap 瞬间可落在任一在途读的 handler 时窗内。
3. **为什么恰好 c4 2/2 确定性复现**：
   - 读池大小按 repo tier 取值：Tiny=4（`crates/cc-model/src/config.rs:715-722`），server 侧入口 `crates/cc-server/src/engine.rs:185-190`。基准项目为 Tiny → **读池恰好 4**。concurrency=4 时读 worker 与池 1:1，每个 swap 瞬间几乎总有满额在途读处于暴露窗口；c>4 时多出的 worker 排队（`client_queue_us` 增大），在途读数被池钳制在 4，暴露面不再随并发线性增长，反而队列化改变了 swap 相对读的相位——c8/c16 的事件数呈间歇性（v4 c8: 0/1，c16: 3+1/0），而非像 c4 那样在固定 seed 调度（seed=1905，`build_every=10`）下 2/2 复现。
   - 换言之：**机制上 c≥2 全部暴露；c4 是"暴露概率 × 固定调度相位"在该 seed 下最稳定的采样点**。回执将 c4 判为 SYSTEMATIC、c16 判为 TRANSIENT，与该结构一致。
4. **为什么 full-build 提交瞬间读会连续落在变更代**：
   - 每次全量重建提交 = 恰好一次代变更，但提交窗口内读池被整体替换（`index_db_rebuild.rs:145-152`），替换前已 checkout 的旧池连接继续读旧 inode（旧代），替换后的新 checkout 读新文件（新代）——同一毫秒内不同读请求天然看到不同代。
   - fence 的重试是 `thread::yield_now()`（`engine_cache.rs:209`），零退避立即重跑整个 work（p50 10–30ms）。重建按 `build_every=10`、50ms  offered interval 排队，饱和时重建近乎背靠背；重试的 10–30ms work 再次跨越下一次 swap（或 `BuildBusy` 之外的排队重建）即消耗第 2、3 次尝试 → 耗尽走分支 γ。
   - 分支 β 的窗口虽小（fence 接受 → 序列化 → validate，亚毫秒级），但一次 stage 有 30 次提交 × 300 读，命中一次即整 stage 判红；且它**一次命中就没有重试**。

---

## 5. 候选修复方向评估

约束（任务红线）：不得掩盖 freshness 语义（stale 读必须显式标注）；不得破坏 lease/epoch 正确性；零数据损坏现状必须保持。另注：`error.rs:45-46` 的既有契约——**绝不返回或缓存混合代结果**——是零损坏的语义支柱，任何方案不得放松。

### 方案 a：读路径持快照/代引用而非重查（retry 改 pin）

把一次查询的全部读固定在同一条连接（SQLite 读事务/快照）上，从根上消除"一次 handler 内多代混读"。

- 利：单次尝试内天然代稳定，fence 退化为形式校验；`changed_during_query` 只会出现在真·stale（pin 到旧代）场景，语义最干净。
- 弊与改动面：
  - 与全量重建的**文件替换架构直接冲突**：pin 住的连接属于被替换的旧池（`index_db_rebuild.rs:145-152`），查询全程读 unlinked 旧 inode → 结果必然是旧代，必须显式 stale 标注；而池替换会丢弃旧池，pin 语义需要绕过 r2d2 生命周期自管理连接。
  - 现有代码多处明示"无连接跨阶段"不变量（`engine_cache.rs:181-184` 注释、`engine.rs`（cc-search）`:252-254` "各 lane/batch-fetch 各自 checkout，1 连接池也不会嵌套"）。pin 需要把快照句柄穿透 plan/lane/hydrator/enrich 全链路，涉及 cc-search + cc-server 大量读模型调用点。
  - 读池=4 且查询长持连接 → c4 场景满池 pin，后续 checkout 阻塞（r2d2 等待），尾延迟恶化。
- 风险：高。属于架构级改造，P5-020 之前不可行，ROI 低。

### 方案 b：重试窗口内读旧代并如实标注（把硬 error 尾部变成有界 stale 读）

fence 耗尽或 fence 外校验命中时，不再硬 error，而是返回"最后一次自洽尝试"的结果并显式标注 stale。

- 利：消灭硬 error 尾部；改动集中在错误出口。
- 弊与风险：
  - **直接违反既有契约**：fence 丢弃某次尝试的原因恰是该尝试可能混合代（lanes 之间、hydrator 前后各自 checkout）——"最后一次尝试自洽"无法在不重放校验的前提下证明。放松它就是放松零混合代保证（离零数据损坏的语义边界最近的一步）。
  - 与 `freshness.rs:59-67` 的既有原则冲突：响应体无位置承载警告时宁可报错，不得静默 serve 旧事实。
- 结论：**按原样不采纳**。但其"有界"精神可以拆成两个安全子项，并入方案 d 的 P1/P2。

### 方案 c：full-build 提交与读的公平调度（提交点让路）

swap 前等待在途读排空（RCU 式 grace period）：server 侧维护在途读计数（dispatch 进入 +1/退出 -1，AtomicU64），`swap_rebuild_staging` 在 rename 前有界等待计数归零（如 ≤50ms，超时放行）。

- 利：在源头消灭"swap 瞬间有读在窗"，对 c≥2 全并发生效；不动任何读语义与 freshness 判定。
- 弊与改动面：需要 cc-server 建立读准入计数并贯穿 `spawn_handler!`/fence；swap 路径（cc-db）要新增有界等待钩子——**触碰写路径时序**，需要谨慎论证不引入写饥饿；grace 期不是气密的（grace 中新起的读仍可能跨越 swap），必须配合读侧重试才能闭环。
- 风险：中。建议作为 P0-P2 之后的可观测性驱动选项（若残差仍不可接受再做）。

### 方案 d（推荐）：从代码结构看到的三点组合修复（全部在读路径，零写路径/零 epoch 语义改动）

- **P0 · 拆除 fence 外单次硬 error 源（消灭 attempts:1 尾部）**
  `crates/cc-server/src/handlers/context.rs:55` 的 `validate_envelope_generation` 是 fence 接受之后的重复校验：envelope 已内嵌 accepted generation（`evidence_summary/source_freshness/generation`），而同一时窗的代变更已由 dispatch seam 的 `attach_observed`（`freshness.rs:43-68`，mcp.rs:88）以 `changed_during_query` 如实标注。该检查把"响应序列化窗口内恰有提交"这一良性事件升级为无重试硬 error，**不提供任何 attach_observed 之外的保证**。
  改动：将 `:55` 的 `?` 硬失败改为把比较结果并入 freshness 标注（或直接移除该调用，交由 attach_observed 覆盖）。改动面：1 个文件 ≤10 行。风险：低——freshness 语义不被掩盖（变更仍显式标注），契约 `error.rs:45-46` 不受影响（结果本身是单代自洽的）。
- **P1 · fence 重试加 deadline 感知短退避（把 attempts:3 耗尽压到 ≈0）**
  `engine_cache.rs:209` 的 `yield_now()` 改为有界退避（如 5–20ms，受 QueryControl 剩余预算钳制）。重建提交间隔 ≥500ms、单次尝试 10–30ms，退避后"三次尝试各自跨越提交"需要 ≥3 次提交精确落窗，概率塌缩到可忽略。不改变"耗尽即硬 error"的终态语义——只是让耗尽真正只属于极端情形。改动面：1 处。风险：极低；耗尽终态保留，不构成对竞态的掩盖。
- **P2 · dispatch freshness 精化：用 fence 接受代消解假阳性 Partial（消灭 known-red 误标）**
  现状：fence 内部重试成功返回的**新代自洽新鲜结果**，因 dispatch 前后 epoch 不等被打成 `complete=false`（§2.4 末）——seq306/seq26 这类红与数据质量无关。改法：让 fence 的接受代（返回元组第一个元素，`engine_cache.rs:205`/`engine.rs:628` 已有）随结果透出，`attach_observed` 增加一个分支：`accepted_generation == after` 时 `complete=true`，同时保留显式的 `dispatch_observed_generation_change` 观测字段（含 from/to epoch）。**真 stale 场景（accepted≠after 或 fence 降级路径）仍严格 `changed_during_query`**——标注语义不减弱，只是把"结果新鲜"与"dispatch 时窗内有提交"两个事实分开表达。
  改动面：cc-search fence 返回值透传 + cc-server freshness 分支，~2 文件 30 行内。风险：低-中（harness known-red 清单与该字段的对应关系需在 P5-020 后单独核对）。

### 候选方案对比表

| 方案 | 消灭硬 error(attempts:1) | 消灭硬 error(attempts:3) | 消灭假阳性 Partial | 改动面 | 语义/正确性风险 | 与约束相容 |
|---|---|---|---|---|---|---|
| a 读路径 pin 快照 | 是 | 是 | 是（stale 显式标注） | 极大（cc-search+cc-server 全链路 + 绕过 r2d2/文件替换） | 高：破坏"无连接跨阶段"不变量，与文件替换架构冲突 | 相容但成本失配 |
| b 耗尽后返回旧代+标注 | 部分 | 部分 | 否 | 中 | 高：需放松"不返回混合代"契约 | **不相容**（原样） |
| c swap 前读排空 grace | 是 | 大部分 | 否（恢复成功仍标红，需配 P2） | 中（读准入计数 + 写路径时序钩子） | 中：写饥饿/气密性需论证 | 相容 |
| d-P0 拆 fence 外硬校验 | **是** | 否 | 否 | 极小（1 文件） | 低 | 相容 |
| d-P1 fence 短退避 | 否 | **≈是** | 否 | 极小（1 处） | 极低 | 相容 |
| d-P2 接受代精化 freshness | 否 | 否 | **是** | 小（2 文件） | 低-中（需对账 known-red 清单） | 相容（标注不减弱） |

**推荐**：P0 + P1 + P2 组合（方案 d）。理由：三者分别精确对准三类出口的根因（fence 外无重试校验、零退避重试、freshness 粒度错位），合计改动 ≤3 文件 ~50 行，不动 cc-db 的 epoch bump/finalize/池替换等任何写路径与 lease 机制，不放松任何既有契约；组合后硬 error 尾部（两类）与假阳性 Partial（c4 判红的直接原因）同时消除。方案 a 在当前"temp-db + rename + 换池"架构下成本收益失配，标记 NO-GO；方案 c 保留为 P0-P2 落地后若仍有残差时的后续选项。

---

## 6. 实施前置与验证建议

1. **前置门**：P5-020 收口前不动 `crates/`；本提案仅作为该轮的设计输入。
2. 落地顺序建议：P0 → P1 → P2（每步独立可回退）。
3. 验证路径：
   - 单测：`crates/cc-eval/tests/p1d_concurrency.rs`（现有 `RetrievalChanged{attempts:3}` 契约）、`engine_cache.rs` 内 fence 四个测试（:376-458）、`freshness.rs:71-97`。P2 需新增"接受代==当前代 → complete=true 且带观测字段"与"真 stale → 仍 changed_during_query"两侧用例。
   - 复测：P5-E mixed-c4 stage（同 seed 1905 / 同 manifest）复跑，判据：0 硬 error、0 假阳性 Partial；`changed_during_query` 观测字段计数应与历史普查量级一致（作为竞态仍被如实观测的证据，而非竞态消失的伪证）。
4. 明确不做：不改 `epoch_rules.rs` 声明、不改 `swap_rebuild_staging` 时序、不改 harness 阈值与 known-red 判定逻辑。

---

## 附：本文引用的关键位置速查

| 主题 | 位置 |
|---|---|
| fence 主体/重试/耗尽 | `crates/cc-search/src/engine_cache.rs:185-214`（MAX_ATTEMPTS :189，yield :209，Err :211） |
| 外层 fence（context） | `crates/cc-server/src/engine.rs:624-628` |
| 内层 fence（search/graph） | `crates/cc-search/src/engine.rs:202`、`crates/cc-search/src/engine_graph.rs:107` |
| hydrator 代检查 | `crates/cc-search/src/evidence_hydrator.rs:193-198`（构造即查 :69） |
| fence 外硬 error 源 | `crates/cc-server/src/handlers/context.rs:55` + `evidence_hydrator.rs:19-37` |
| Partial 降级分支 | `crates/cc-server/src/handlers/freshness.rs:43-68` |
| freshness 读取 | `crates/cc-db/src/freshness_store.rs:55-86` |
| wire 错误映射 | `crates/cc-server/src/mcp.rs:50-60`；可重试清单 `crates/cc-model/src/error.rs:87`；消息 `error.rs:45-48` |
| 全量重建提交瞬间 | `crates/cc-db/src/index_db_rebuild.rs:91-160`（rename :123，换池 :145-152）；`crates/cc-db/src/index_db.rs:635-663` |
| 代定义/读取/bump | `crates/cc-model/src/generation.rs:4-26`；`crates/cc-db/src/read_generation.rs:50-89`；`crates/cc-db/src/index_db.rs:612-624,675-683` |
| 读池大小 | `crates/cc-model/src/config.rs:715-722`（Tiny=4）；`crates/cc-server/src/engine.rs:185-190`；`crates/cc-db/src/index_db.rs:256-283` |
| 无 fence 读路径 | `crates/cc-server/src/handlers/context.rs:65-94` |
| harness 调度 | `artifacts/benchmarks/p5e-harness-20261001/final-v4/driver/src/mixed_stdio.rs:204-216,244-261` |
