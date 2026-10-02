# P7-009 实施记录：查询编码与缓存

日期：2026-10-03
任务：`docs/roadmap/code-index-v2/tasks.json` P7-009「查询编码与缓存」（批次 2 第四任务，depends_on P7-008）
红线遵守：schema 零改动（cc-db 零触碰）；ports/spec 冻结面零改动（`spec.rs` 本任务零修改，键复用既有 `QuerySpecDigest`/`QueryDigest`）；`cache.rs` 既有方法零修改（全部为新增段，:477 起）；零真实网络（FakeProvider/RecordingProvider，offline 锁定运行）；查询文本零泄漏（有专项测试固化）；`tasks.json` status 未改；未 `git commit`。

## 0. Q5 裁决：query cache 键是否纳入 semantic_epoch

**裁决（本轮实施口径）：键 = `namespace + QuerySpecDigest + QueryDigest`，不含 `semantic_epoch`。** 采纳任务指令给出的裁决方向；简报的「默认按 C12 表保守入键」原文即自标注为待 owner 裁决的默认值（`OPEN-QUESTIONS.md` Q5），非既定口径。

理由：

1. **查询向量是 (spec, 文本) 的纯函数。** 回填（backfill）推进 semantic epoch 改变的是可见文档集合与 manifest；它不改变查询文本的编码结果——epoch 前后同键对应的向量逐位相同。入键的正确性收益为零。
2. **C12 dense 行不覆盖本缓存。** C12 表（`02-CONTRACTS.md:93-107`）dense 行「上述 + semantic epoch/vector-space/query encoding spec」约束的是 dense 召回结果缓存——其候选是文档数据，必须随 epoch 失效。查询编码缓存不缓存任何文档派生内容，不在该行语义射程内。豁免口径表述（供回写 C12，本任务不改文档红线内的契约文件）：「C12 dense 行的 semantic epoch 依赖适用于 dense 召回结果缓存；provider 查询编码缓存（query text → vector，P7-009）的键只含 namespace/QueryEncodingSpec digest/QueryDigest——查询向量与文档集合无关，semantic epoch 推进不使其失效」。
3. **消费侧 freshness 由既有机制管。** dense 召回（P7-010 `ExactSearch`）每次查询经 manifest × artifact cache 现算 + C12 第一版读前/读后 generation 验证，混代拒绝不依赖编码缓存键。
4. **保守入键的代价是纯浪费**（简报风险①原文）：每次回填推进清空全部 query 缓存，全部重编码付费。

**仍保有的全量失效路径**：instruction/tokenizer/max_tokens/space 变更 ⇒ `QuerySpecDigest` 变 ⇒ 新键（space 是 spec 字段，跨空间复用结构性不可达，有测试固化）；`ENCODING_SPEC_VERSION` bump 同理。**逃生通道**：本缓存是进程内可丢弃导数；若 owner 推翻裁决，给 `QueryCacheKey` 加 `semantic_epoch` 字段即全量失效旧条目，零迁移负担（`QueryCacheKey` 文档已预写此通道，cache.rs:490-505）。

## 1. 做了什么

### 1.1 文件与关键位置

| 文件 | 内容 | 关键行 |
|---|---|---|
| `crates/cc-semantic/src/cache.rs` | **本任务全部实现**（新增 P7-009 段，既有方法零触碰） | :477 起 |
| — `QueryCacheKey` | 缓存键（Q5 裁决原文见其文档注释 ：490-505）：`namespace`（`namespace_key` 输出，与文档 artifact cache 同一隔离根）+ `spec: QuerySpecDigest`（覆盖 space/instruction/max_tokens/tokenizer——instruction 变更天然失效，文档路径永不重嵌）+ `query: QueryDigest`（查询文本 digest；键结构性不含原文） | :514 |
| — `QueryCacheKey::new` | 唯一认可构造器：namespace 过 `validate_namespace`（:105 既有私有函数，同模块复用）、spec digest 由已验证冻结 spec 铸造、query digest 来自 digest 绑定的 `QueryInput` | :527 |
| — `QueryVector` | 编码结果载体（dimension + data）；`validate_intrinsic`（维数非零/长度匹配/有限/非全零）与 `validate_for`（+ 空间维数匹配）两级校验 | :547 |
| — `QueryVectorCache` | 有界进程内 LRU：**容量 + 字节双上限**（简报「有界硬要求」）；`max_entries == 0` = 全不存（get 恒 miss）；超字节预算的单条不存（`Ok(())` 非错误，编码器仍返回向量，仅缓存跳过）；`Mutex` 线程安全；`#[derive(Debug)]` 合法——内部只有 digest 键与向量值 | :610 |
| — `get`/`put` | get 命中刷新 recency；put 校验在前（错误向量结构性到不了缓存，与 `ArtifactCache::put` 同门）、覆盖更新改字节账、逐出只从队首（刚插入条目永不为逐出对象，map.len()>1 守卫） | :633 / :648 |
| — `QueryEncodeOutcome<K>` | `Encoded { key, vector }` / `Skipped { key, reason: OversizeReason }`——admission 拒绝显式透出，不静默 | :706 |
| — `encode_queries` | **查询编码全链**（见 §1.2） | :758 |
| — `query_provider_failure` | `ProviderError` → `CcError` 查询路径映射：`Cancelled → QueryCancelled`、`Timeout → QueryTimedOut`（CcError 既有变体，retryability 语义正确）；其余 → `Other("query embedding provider call failed: {queue.rs::provider_reason 同型理由}")`（理由文案镜像 queue.rs:326 私有映射，不改 queue.rs） | :732 区段 |
| `crates/cc-semantic/tests/query_cache.rs` | **新集成测试套件 17 项**（沿 P6-008 `tests/artifact_cache.rs` 惯例：只测公共面） | 全文件 |
| `crates/cc-semantic/src/lib.rs` | crate 文档 P7-009 段（仅文档） | :130-140 |

**spec.rs 零修改**（tasks.json scope 列有 `spec.rs` 的偏差说明见 §6.1）：键类型放 `cache.rs` 而非简报草案的 `spec.rs`——简报草案接口本就把 `QueryCacheKey` 画在 `cache.rs`（TASK-BRIEFS P7-009 草案代码块标注 `// cache.rs（草案）`），且键不需要任何新 digest 类型（`QuerySpecDigest`/`QueryDigest` 皆为 P6-003 冻结既有），动冻结文件零收益。

### 1.2 查询编码全链（encode_queries，cache.rs:758）

```
查询文本 (K, Vec<u8>)
  → validate_namespace(namespace)                    隔离根合法性
  → spec.digest() → QuerySpecDigest                  冻结面校验+铸造（instruction/space 变更 ⇒ 新键）
  → plan_query_batches(queries, budget, spec.tokenizer())   P7-003 query 批次口径（admission.rs:363）：
        tokenizer_gate 强制声明估算器（utf8-bytes-div-ceil-4-v1）；超限项显式 Skipped(OversizeReason)
  → 逐 planned batch：
      每 item 铸 QueryCacheKey{namespace, spec, input.digest}
      cache.get → 命中直接入槽（零 provider 接触）
      misses 重批 → provider.embed_queries(&miss_inputs)   只送未命中项，一次调用不超 planned batch
        Err → query_provider_failure → 整调用失败（该批零缓存，P7-004 整批拒绝口径；已成功的更早批次条目保留）
        数量不符 → Err
        逐向量 validate_for(space)（维数/有限/非全零）→ 错误向量在 put 之前被拒（错误向量不缓存）
      cache.put（Q5 键）→ 全部槽位按 input 顺序组装 Encoded
  → Vec<QueryEncodeOutcome<K>>（输入顺序）
```

线程模型守约：本函数是被动库函数，阻塞编码发生在调用方线程（P7-005 简报 Q4 定案：查询线程不内联阻塞编码，编码在 worker 侧——消费归 P7-013/P7-014）；零线程、零定时器、零时钟读取（put/get 无时间参数，纯 recency 序）。

### 1.3 与 P6-008 文档缓存的关系

与文档 artifact cache **同根（`namespace_key` 同一隔离根）不同区（进程内 LRU vs 磁盘 CAS）**。文档路径不经过本缓存：`QueryCacheKey` 不含 `DocSpecDigest`/`InputDigest`，文档对象寻址不经过 `QueryVectorCache`——查询侧 spec/instruction 变更「不需无谓重嵌文档」由三层结构性保证（三向 digest 分离 + 键不相交 + §3 专项测试的字节级证据）。

## 2. TDD 流程

红绿两轮，无跳步：

1. **红轮**：17 项测试 + `get`/`put`/`encode_queries` 三个 `todo!()` 桩先行。3 次测试文件自身编译修正（空切片 K 类型不可推断、`k3` move、`QueryVectorCache` 缺 `Debug`/辅助函数 `K: Debug` 约束）后跑通编译：**15 项 panic 于 `todo!()`，2 项键恒等测试直接绿**（`QueryCacheKey::new` 属纯构造器，随类型一并落地）。
2. **绿轮**：实现三个函数体。17 项中 16 项一次绿；`encode_queries_rebatches_only_misses_inside_one_planned_batch` 失败为**测试自身期望错误**（第一遍已把 `cold` 一并缓存，第二遍未命中仅 `fresh`，provider 批应为 `[2, 1]` 而非 `[2, 2]`）——实现行为正确，修正测试期望后全绿。实现侧零改动。

## 3. 测试清单（原样，17 项全 ok）

```
test query_cache_key_distinguishes_namespace_spec_and_text ... ok
test query_cache_key_debug_carries_no_query_text ... ok
test query_cache_round_trips_and_reports_bounds ... ok
test query_cache_evicts_lru_within_both_bounds ... ok
test query_cache_skips_entries_that_cannot_fit ... ok
test query_cache_rejects_invalid_vectors_on_put ... ok
test query_cache_concurrent_access_stays_bounded ... ok
test encode_queries_returns_vectors_in_input_order_across_batches ... ok
test encode_queries_second_pass_hits_cache_without_provider_contact ... ok
test encode_queries_never_reuses_vectors_across_specs_or_spaces ... ok
test encode_queries_rebatches_only_misses_inside_one_planned_batch ... ok
test encode_queries_skips_oversized_queries_and_never_sends_them ... ok
test encode_queries_rejects_invalid_vectors_and_caches_nothing ... ok
test encode_queries_maps_provider_failures_and_caches_nothing ... ok
test encode_queries_leaks_no_query_text_anywhere ... ok
test encode_queries_empty_input_is_a_provider_free_no_op ... ok
test query_path_never_touches_the_document_cache ... ok
```

任务 TDD 五面对照：

- **编码路径全链（FakeProvider/适配层互换）**：`encode_queries_returns_vectors_in_input_order_across_batches`（FakeProvider，3 查询/max_items=2 → 2 次 provider 调用、顺序保持、向量形状）+ `encode_queries_rebatches_only_misses_inside_one_planned_batch`（RecordingProvider 换入，批尺寸 `[2, 1]` 证明只重批 miss 项）；`EmbeddingProvider` 端口对象安全，FakeProvider/RecordingProvider/EchoProvider 三实现互换即「适配层可替换」。
- **缓存命中/未命中/版本区分**：`encode_queries_second_pass_hits_cache_without_provider_contact`（第二遍零 provider 调用、向量相同）；`encode_queries_never_reuses_vectors_across_specs_or_spaces`（instruction 变更重编码、跨 model 空间重编码且向量不同——「不跨空间复用」）；`query_cache_key_distinguishes_namespace_spec_and_text`（键级五维区分）；`query_cache_evicts_lru_within_both_bounds`（双上限 + recency 刷新）。
- **digest 稳定性**：键重构相等断言（同输入 ⇒ 同键）；`QueryDigest`/`QuerySpecDigest` 的构造即绑定与稳定性由 P6-003 既有测试背书，本套件键级复验。
- **零泄漏**：`query_cache_key_debug_carries_no_query_text` + `encode_queries_leaks_no_query_text_anywhere`（含密钥样式标记串的查询文本经失败路径（错误 Debug+Display）与成功路径（outcome/key/cache 三者 Debug）渲染后全文断言无标记；键结构性只含 digest）。
- **显式 skip / 错误向量不缓存 / 文档路径无扰**：`encode_queries_skips_oversized_queries_and_never_sends_them`（Skipped+BytesTooLarge、超限文本零 provider 接触）；`encode_queries_rejects_invalid_vectors_and_caches_nothing`（NaN 注入）+ `query_cache_rejects_invalid_vectors_on_put`（五类坏向量全拒、缓存恒空）；`query_path_never_touches_the_document_cache`（同 namespace 下编码查询后 artifact cache 根下文件数逐字节不变 + 文档对象仍 Hit——「不需无谓重嵌文档」的机制级证据）。

## 4. 验证命令与结果（原样）

```
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-semantic --locked --offline
→ 13 × "test result: ok."，0 failed：
  lib 212 passed（零回归，本任务 17 项全在新集成套件）；
  集成套件 query_cache 17 passed（本任务新增）+ artifact_cache/manifest_exact_integration/
  publish_cas/queue_worker/reconcile_rebuild/retry_worker_layering/semantic_degrade/
  semantic_gc/semantic_recovery/space_switch 全绿零回归。
  新增代码零 warning（cache.rs 既有 3 条 clippy 维持原位：namespace_key enclosing-Ok、
  resolve_cache_root_with 文档缩进 ×2，P7-008 记录在案，行号因新增 import 平移）。

SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo check --workspace --locked --offline
→ Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.89s（零 error 零 warning）。
```

## 5. 验收对照（acceptance）

- **「不需无谓重嵌文档」**：文档路径结构性不经过本缓存（§1.3 三层保证），`query_path_never_touches_the_document_cache` 字节级固化；文档缓存回归 = `tests/artifact_cache.rs` 17 项零回归。
- **「不跨空间复用query向量」**：`VectorSpace` 是 `QueryEncodingSpec` 字段 → space 变 ⇒ `QuerySpecDigest` 变 ⇒ 键变（spec.rs:319 冻结 digest）；键级 + 行为级双测固化（`query_cache_key_distinguishes_...` / `encode_queries_never_reuses_vectors_across_specs_or_spaces`）。
- **steps「实现QueryEncodingSpec键和有界query cache」**：`QueryCacheKey`（spec digest 进键）+ `QueryVectorCache`（容量+字节双硬上限，§3 有界性三测）。
- **steps「instruction变更失效」**：instruction 变 ⇒ spec digest 变 ⇒ miss 重编码（`encode_queries_never_reuses_...` 的 instruction 腿：provider_a 调用数 1→2）；文档侧 `DocSpecDigest` 不受影响（spec.rs 既有 `query_only_change_never_touches_the_document_spec_digest` 背书）。
- **deliverable「artifacts/benchmarks/<run-id>/ V11, V15 证据」**：**not_run/blocked**——V11（查询执行/缓存端到端）与 V15 的 live 腿依赖真实 provider（D1/D2 本轮不授权，与 P7-001~008 同口径）与查询执行接线（P7-010/P7-013 未落地）；mock 腿证据 = §3 全部测试。benchmark run-id 未生成，tasks.json evidence 不回填。
- **rollback 口径可用性**：查询缓存是进程内可丢弃导数，关闭远程语义即整体失效，无任何持久状态需回滚。

## 6. 偏差清单

1. **键类型落 `cache.rs` 而非 tasks.json scope 所列 `spec.rs`**：简报草案接口本就画在 cache.rs；键无需任何新 digest 类型（复用 P6-003 冻结既有）；`spec.rs` 是冻结面，动它零收益且违反「冻结面零改动」红线——scope 行按实质（「实现QueryEncodingSpec键」）而非文件名兑现。spec.rs 本任务零 diff。
2. **`semantic_epoch` 未入键（Q5 裁决，§0）**：与简报「默认保守入键」草案相左，按任务指令的裁决方向实施；裁决原文与 C12 豁免口径、逃生通道见 §0，owner 可一键推翻（加字段即全量失效，零迁移负担）。
3. **内存 LRU 而非 artifact cache 磁盘落位**：简报接口草案明写 `QueryVectorCache { /* LRU：容量 + bytes 双上限（有界硬要求） */ }`，查询向量重编码成本低、会话内重复查询才是主要命中面，磁盘持久化无对应简报要求；「同根」体现为共用 `namespace_key` 隔离根（跨项目复用不可能，有测试）。若后续需磁盘落位，沿 P6-008 布局加新方法，不动既有（任务指令预留的扩展路径）。
4. **零配置键新增**：`QueryVectorCache::new(max_entries, max_bytes)` 为程序化装配（组合根/测试构造），简报风险②「内存上限需配置化（P7-002 配置节扩展）」本轮不落键——与 P7-008 偏差 5 同口径：P7-002 冻结的 `semantic.*` 键面不扩散，接线轮（P7-014）若需操作者可调再加键（届时配套 cc-model 测试与 CONFIGURATION.md）。
5. **provider 错误映射新增 `Other/QueryCancelled/QueryTimedOut` 三形态**：冻结 `ProviderError` 六变体无 CcError 对应物，queue.rs 用 retry-reason 字符串、degrade.rs 用 dead-letter 理由，均不适合查询侧调用方（P7-013 需要 lane 级降级判据）；`Cancelled/Timeout` 映射到 CcError 既有同名语义变体（retryability 正确），其余归 `Other` 带镜像自 queue.rs:326 的理由文案（零文本泄漏）。
6. **`encode_queries` 空输入即空计划**：空 `queries` 切片 → planner 返回空 → 零 provider 接触返回空 outcomes（有测试）；不做「空即错误」——调用方（P7-013）的 dense lane 用「零查询」与「不可用」区分是它自己的职责（C10：没有结果与没有执行不是同一状态）。
7. 红线确认：schema 零改动；ports.rs/spec.rs 零 diff；cache.rs 既有方法零修改（新增段 :477 起，`ArtifactCache` 既有 get/put/discard/object_dir/reference 均未触碰）；零真实网络；查询文本零泄漏（§3 专项双测）；`tasks.json` status 未改；未 `git commit`。
