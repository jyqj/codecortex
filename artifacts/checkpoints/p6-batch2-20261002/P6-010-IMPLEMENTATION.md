# P6-010 实施记录：实现 filtered exact 向量 backend

- 日期：2026-10-02
- 依据：`artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`
  P6-010 节（算法不变式 1-5、接口草案、验收对照 V16）、ADR-0003 约束表 P6-010 行
  （"过滤先于 exact top-k、bounded batch、稳定 ties、空间隔离；删除/异空间向量
  不可返回"）与"ports 冻结后可委托"条款（`cc-semantic/src/vector/exact.rs` 为
  排他可委托件）、C09（`02-CONTRACTS.md`："向量 scope 筛选应发生在 top-k 之前；
  Some(empty) 永不退化全仓"）、C10（"固定 tie-break；结果顺序不是线程完成顺序；
  NaN/Inf 拒收"）、P6-006/007 的 `semantic_manifest` 表（cc-db，只读调用）。
- 改动范围：cc-db 1 个新模块 + 1 行 lib.rs 声明 + 5 个内联测试；cc-semantic
  2 个新文件 + 16 个内联测试 + 1 个新集成测试文件 + Cargo.toml（加 `cc-db` 生产
  依赖与 `rusqlite` dev 依赖）+ lib.rs 模块声明与边界文档更新 + `Cargo.lock`
  重生成（`cc-semantic → cc-db` 依赖边落盘，+14 行）。
- 红线对照：`ports.rs`/`cache.rs`/`providers.rs`/`spec.rs`/`types.rs` 零触碰；
  `semantic_outbox.rs` 及全部 P6-006/007 交付物零触碰（只读调用）；零 schema 变更；
  `tasks.json` status 未改；未 git commit；无 ANN/索引结构（V22 可选轨道，模块
  文档明示 exact 是小规模 oracle backend，不得当生产大库 backend）。

## 1. 文件:行号改动清单

| 文件 | 变更 | 内容 |
|---|---|---|
| `crates/cc-db/src/semantic_manifest_reads.rs` | 新增（240 行，只读） | `SemanticManifestRow`(:47，候选行七字段：doc_key/doc_version/file_path/input_digest/space_id/artifact_ref/language)、`SemanticManifestReads`(:64) + `on`(:70) + `scan_space`(:80，keyset 分页：`WHERE space_id=? AND doc_key > ? ORDER BY doc_key ASC LIMIT ?`，`LEFT JOIN files` 取 language，`batch_rows==0` 类型化拒绝) + 5 个内联测试(:157-240) |
| `crates/cc-db/src/lib.rs:47` | 修改（+1 行） | `pub mod semantic_manifest_reads;`（新模块；P6-006/007 交付物内容零改动） |
| `crates/cc-semantic/src/vector.rs` | 新增（8 行） | 模块文档（P6-010 归属、本轮仅 exact、ANN 归 V22）+ `pub mod exact;` |
| `crates/cc-semantic/src/vector/exact.rs` | 新增（1240 行） | 模块文档(:1-72，范围声明/组合语义/五条不变式/降级语义) + 端口 `ManifestCandidates`(:80) + 生产适配器 `SpaceScopedManifestReads`/`space_manifest_reads`(:88-111) + `ScoredDoc`(:114) + `ExactSearch`(:123) + `search`(:136) + `score_candidate`(:182) + `row_passes_scope`(:231) + `cosine_f64`(:246) + `parse_artifact_ref`(:278) + `TopK`(:302)/`Candidate::cmp`(:316)/`offer`(:340)/`finish`(:347) + 16 个内联测试(:361-1240) |
| `crates/cc-semantic/src/lib.rs` | 修改 | `pub mod vector;`(:34)；边界文档更新（P6-010 落位、依赖口径从 "cc-model only" 改为 "cc-model + cc-db only"，ADR-0003 明文允许） |
| `crates/cc-semantic/Cargo.toml` | 修改 | `[dependencies]` 加 `cc-db = { path = "../cc-db" }`；新增 `[dev-dependencies] rusqlite.workspace = true`（集成测试建 v22 内存库用） |
| `crates/cc-semantic/tests/manifest_exact_integration.rs` | 新增（281 行） | 真实 cc-db 读路径 × 真实 cache 的 4 个集成测试(:128-281) |
| `Cargo.lock` | 变更 | `cargo update -p cc-semantic --offline` 重生成 cc-semantic 依赖边 |

## 2. 过滤组合语义（定案原文）

**过滤先于 top-k**（模块文档 `exact.rs:24-32`）：

> **过滤先于 top-k** ("filter before top-k"): the [`HardScope`] intersection
> (C09: repo namespace + caller `file_paths`/`path_prefix`/`languages`) is
> applied to every candidate row *before* it is scored; the top-k selection
> therefore runs over the filtered stream only. `Some(empty)` scopes short
> -circuit to an empty result without loading a single candidate (C09:
> "Some(empty) 表示空集合，永不退化全仓"). Because exact scoring evaluates
> every filtered candidate, filter-before-top-k and filter-after-top-k
> coincide *for the result set* here; the ordering is still load-bearing for
> cost and for the ANN successor, where filtering after top-k would silently
> under-report.

落地要点：

- `search` 入口先做两个短路：`k == 0 || filter.is_empty() → Ok(vec![])`
  （`exact.rs:148-151`），`Some(empty)` 连一批候选都不装载（测试
  `some_empty_scope_short_circuits_without_loading_candidates` 断言零批次请求）。
- 谓词求值复用 `HardScope::passes` 单一权威（`row_passes_scope`，exact.rs:231），
  唯一补充是 "language 缺失" 语义：`files` 行缺失（FK 链下实际不可达，防御性
  LEFT JOIN）→ 匹配**任何** languages 过滤器失败、无 languages 过滤时照常放行
  —— "匹配无语言"，绝不解释为 "匹配所有语言"。
- 空间隔离在**装载层**两道闸：SQL `WHERE space_id = ?`（cc-db scan）+
  `score_candidate` 对 `row.space_id` 与 `artifact_ref` 内嵌 space digest 的
  双重校验（exact.rs:190-193, 210-216）——异空间向量结构性不可装载，不是低分。
- 删除不可返回：候选只来自 `semantic_manifest`；删除文档走同事务撤销或
  `ON DELETE CASCADE`，无行即无候选（`score_candidate` 无从看到它）。

**artifact_ref → cache 寻址**：`semantic_manifest` 不单列存 doc_spec_digest，
由 `parse_artifact_ref`（exact.rs:278，`cas.v1:<namespace>:<space_id>:
<input_digest>:<spec_digest>:<checksum>` 五段解析）取寻址三元组，并在读 cache
前校验 namespace == `cache.namespace()`、space == 查询空间 digest、input ==
行内 `input_digest`，任一不符（含 ref 不可解析）→ 降级跳过，不报错、不隔离
（隔离动作归 P6-018，ADR "损坏可检测且只降级不污染"）。cache `Miss`/`Corrupt`
同样跳过（`score_candidate` 尾部分支）。

## 3. tie-break 规则（定案原文）

**总序 = `(score desc, doc_key asc)`**（模块文档 `exact.rs:52-57` + 实现
`Candidate::cmp` exact.rs:316-323）：

> 4. **稳定 ties**: scores accumulate in `f64` with a fixed operand order;
>    the total order is `(score desc, doc_key asc)` via
>    [`f64::total_cmp`] — independent of thread and batch order (C10:
>    "固定 tie-break；结果顺序不是线程完成顺序").

实现三件套：

1. `cosine_f64`（exact.rs:246）：dot/两范数全部 `f64` 顺序累加（逐分量
   `dot += x*y; norm_a += x*x; norm_b += y*y`，一次除法），同输入逐位同分。
2. `Candidate::cmp`（exact.rs:316）：`self.score.total_cmp(&other.score)`
   （f64 全序，NaN 不可能产生仍用 total_cmp 防御）`.then_with(|| other.doc_key
   .cmp(&self.doc_key))` —— 同分时 **doc_key 字典序小者为更优幸存者**。
3. `TopK`（exact.rs:302）：`BinaryHeap<Reverse<Candidate>>` 容量 k 的最小堆，
   `offer` 超容量弹掉最劣者；`finish` 按 desirability 降序输出——幸存集合与
   顺序均与候选到达顺序（批切分/线程序）无关。

零范数（全零 query 或全零 doc）cosine 无定义 → `cosine_f64` 返回 `None` → 该
候选跳过，NaN 分数结构性不可产生（C10 "NaN/Inf 拒收"）。

## 4. 算法与 bounded batch

`search`（exact.rs:136）：validate space → 校验 `query.len() == dimension` 与
`batch_rows >= 1`（类型化拒绝）→ C09 短路 → keyset 循环
`manifest.next_batch(cursor, batch_rows)`（cursor = 已消费的最大 doc_key）→
逐行 `scope 闸 → ref 寻址校验 → cache.get → metric 打分 → TopK.offer`。
峰值内存 = `O(batch_rows·dim + k)`（一批向量 + 大小 k 选择堆）；每批请求数
`≤ batch_rows` 由测试 `batch_loads_never_exceed_the_bounded_batch_knob` 固化
（11 行 / batch 4 → 请求序列 4,4,4,0 终止）。

**metric dispatch（按 spec 的三分裁决）**：`DistanceMetric` 是封闭枚举，spec v1
唯一 admitted 变体为 `Cosine`（`spec.rs:80-83` 冻结面，加变体必须伴随
`ENCODING_SPEC_VERSION` bump）。`score_candidate` 内 dispatch 为穷尽 `match`
（exact.rs:222-227）——"Cosine/L2/内积按 spec" 在当前冻结 spec 下裁决为
**Cosine-only**：L2/内积未实现，未来新增变体在此处**编译失败**，强制显式写
分支（与 `spec.rs` 的编译期准入门同构）。测试
`metric_dispatch_is_cosine_per_the_frozen_spec` 用正交向量断言 cosine 恰为
`0.0`（L2/内积 backend 会给出不同结果，dispatch 接错即翻转）。

## 5. 测试清单（25 个新增，全绿）

### cc-semantic 内联（`vector/exact.rs`，16 个）

1. `hand_computed_cosine_gold_and_ordering`(:547)——手算 gold（brief 不变式 5，
   dim=3 无 fake provider 数值）：q=(1,0,0)，a/b/c/d 四 doc 手算 cos
   `1, 1/√2, 0, -1`，分值+排序双断言（batch=2 强制多批）。
2. `knn_matches_naive_reference_on_deterministic_corpus`(:587)——**对拍**：40
   docs × dim 8 的 LCG 确定语料，独立朴素参考实现（先归一再点积，公式形态与
   实现刻意不同）排序截断，k=7 / batch=3 下 `Vec<ScoredDoc>` 全等（doc_key 与
   score 双 tolerance 1e-12）。
3. `result_is_independent_of_batch_splitting`(:622)——batch ∈ {1,2,16,17,100}
   五种切分结果与 baseline 全等（批序无关性）。
4. `metric_dispatch_is_cosine_per_the_frozen_spec`(:662)——见 §4。
5. `filter_none_admits_every_candidate`(:696)——None scope 全放行。
6. `some_empty_scope_short_circuits_without_loading_candidates`(:723)——
   `Some(empty)` 空结果 + **零批次装载**（C09 永不退化全仓）。
7. `prefix_file_paths_and_languages_intersect_before_top_k`(:757)——三级交集：
   prefix → prefix∩languages → prefix∩languages∩file_paths，逐级集合精确断言。
8. `missing_language_row_matches_no_language_filter`(:819)——language None 行
   过 languages 过滤必拒、无过滤可过。
9. `empty_and_single_element_boundaries`(:855)——空候选集 / 单元素 k 超语料
   （(3,4)·(4,3) 手算恰 24/25=0.96，f32 精确表示逐位断言）/ 全过滤空结果。
10. `degenerate_inputs_are_rejected_or_yield_empty`(:907)——k=0 空、batch_rows=0
    与维度不符类型化拒绝、零 query 空结果、零 doc 被跳过（非 NaN）。
11. `equal_scores_tie_break_on_doc_key_ascending`(:967)——三同分 doc 对每个
    k∈{1,2,3} 取字典序前缀 `["aa","mm","zz"][..k]`，倒序插入不改变结果。
12. `repeated_runs_are_bitwise_deterministic`(:1011)——两次运行 doc_key 相等且
    `score.to_bits()` 逐位相等。
13. `batch_loads_never_exceed_the_bounded_batch_knob`(:1051)——每次请求 ≤
    batch_rows、keyset 步数精确。
14. `foreign_space_and_foreign_ref_rows_never_score`(:1103)——异 space_id 行 /
    ref 内嵌异空间 / 不可解析 ref / ref input 与行 input 不符，四类行全部不可
    返回，仅合法行得分（空间隔离双闸+寻址校验）。
15. `deleted_or_corrupt_cache_objects_degrade_without_failing`(:1147)——cache
    discard（Miss）与 payload 篡改（checksum→Corrupt）均降级跳过，健康行照常
    返回。
16. `fake_provider_embed_cache_search_hits_its_own_documents`(:1194)——
    **FakeProvider 集成回路**：embed 3 docs → `cache.put` → 构造 manifest 行
    （input_digest 与发布 tuple 一致）→ `embed_queries` → search(k=3) 结果与
    参考实现对拍全等；doc/query kind 域分离保证 query 自身不在候选中。

### cc-db 内联（`semantic_manifest_reads.rs`，5 个）

17. `scan_is_keyset_paginated_and_ordered`(:164)——乱序插入、三段 keyset
    （`""→d1,d2→d3→∅`）有序分页。
18. `scan_is_space_isolated_at_the_load_layer`(:189)——SQL 层异空间行不进候选。
19. `scan_joins_language_and_carries_provenance_columns`(:202)——language
    join 与 doc_version/input_digest/artifact_ref 逐字段断言。
20. `deleted_document_cascades_out_of_the_candidate_stream`(:214)——删
    `document_manifest` 行 → FK CASCADE 撤 `semantic_manifest` 行 → 扫描为空
    （brief 验收第 5 条 "FK 保证不会发生，测试证明 CASCADE 同步"）。
21. `empty_space_scans_empty_and_zero_batch_is_rejected`(:229)——空空间空结果、
    `batch_rows=0` 类型化拒绝。

### cc-semantic 集成（`tests/manifest_exact_integration.rs`，4 个，真实 sqlite v22 + 真实 cache）

22. `published_rows_round_trip_through_manifest_scan_and_cache`(:128)——发布
    三行（同向量异 key）经真实扫描+cache 读回，结果即 tie-break 序
    `[doc-a, doc-b, doc-c]` 且分值恰 1.0。
23. `sql_scan_is_space_isolated_and_keyset_paginated`(:161)——异空间行在库内、
    k=2/batch=1 的 keyset 走查只覆盖本空间按序前 2 行。
24. `cascaded_deletion_removes_the_candidate_structurally`(:206)——真实 CASCADE
    删除后其 cache 向量虽仍在盘上但结构性不可返回（"删除不可返回"的库级证明）。
25. `hard_scope_language_filter_applies_on_the_real_read_path`(:248)——
    `Language::from_name` 回读 + `HardScope::passes` 在真实读路径生效。

## 6. 对拍测试证据（原样）

`knn_matches_naive_reference_on_deterministic_corpus`（exact.rs:587）绿：

```
test vector::exact::tests::knn_matches_naive_reference_on_deterministic_corpus ... ok
```

参考实现（测试内独立成形，exact.rs:480-492）：

```rust
/// Independent naive reference (deliberately formulated differently from
/// the implementation: normalize both vectors first, then dot).
fn reference_cosine(a: &[f32], b: &[f32]) -> f64 {
    let norm = |v: &[f32]| -> Vec<f64> {
        let length = v
            .iter()
            .fold(0.0_f64, |acc, x| acc + (*x as f64) * (*x as f64))
            .sqrt();
        v.iter().map(|x| *x as f64 / length).collect()
    };
    let (na, nb) = (norm(a), norm(b));
    na.iter()
        .zip(nb.iter())
        .fold(0.0_f64, |acc, (x, y)| acc + x * y)
}
```

对拍断言（exact.rs:510-520，tolerance 1e-12，含排序全等）：

```rust
fn assert_docs_eq(actual: &[ScoredDoc], expected: &[(String, f64)]) {
    assert_eq!(actual.len(), expected.len());
    for (got, (key, score)) in actual.iter().zip(expected) {
        assert_eq!(got.doc_key, *key);
        assert!(
            (got.score - score).abs() < 1e-12,
            "score for {key}: got {} expected {score}",
            got.score
        );
    }
}
```

`result_is_independent_of_batch_splitting`（批序无关）绿：

```
test vector::exact::tests::result_is_independent_of_batch_splitting ... ok
```

## 7. 偏差清单

1. **`ExactSearch.space` 用 `&VectorSpace` 而非草案的 `&SpaceDigest`**
   （exact.rs:123）：`ArtifactCache::get` 需要完整冻结空间做 dimension/model
   校验（P6-008 冻结面），digest 由 `search` 内部派生。草案是"接口草案"，
   该改不破坏简报任何不变式。
2. **`search` 第二参从具体 `&SemanticManifestReads` 改为 `&dyn
   ManifestCandidates` 端口**（exact.rs:80/136）：端口落在新模块
   （红线"新增端口走新模块"，冻结面 `ports.rs` 未动）；生产侧经
   `space_manifest_reads(&SemanticManifestReads, &SpaceDigest)` 适配器把
   space 钉死进 keyset 扫描（防误扫异空间），oracle 测试用内存候选源。类型名
   `SemanticManifestReads` 保留自简报草案，落位 cc-db 新模块
   `semantic_manifest_reads.rs`（`semantic_outbox.rs` 等既有交付物零触碰）。
3. **"metric 三分支"裁决为 Cosine-only**：冻结 spec v1 只 admit Cosine
   （`spec.rs` 冻结面 + 编译期准入门），L2/内积**未实现**；dispatch 穷尽
   match 使未来变体成为本处编译错误。测试以正交向量钉住 dispatch 正确性。
   若后续轮要求 L2/内积，必须先走 spec version bump（简报/ADR 一致）。
4. **doc_spec_digest 从 artifact_ref 解析**而非新列：schema 冻结（红线不改
   schema），`cas.v1` 五段格式为 P6-008 已冻结格式，解析含 namespace/space/
   input 三重一致性闸，不实即降级跳过。
5. **零范数候选/查询 → 跳过**：简报未显式规定；依 C10 "NaN/Inf 拒收" 取
   "无 cosine 即不可分" 语义（不记 0 分——0 分会伪造一个排序位置）。
6. **languages 过滤的 None-language 语义**：LEFT JOIN 防御位（FK 链下实际
   不可达），定义为"匹配无语言"，测试 8 固化。
7. **cc-semantic 新增 `cc-db` 生产依赖**：ADR-0002/0003 明文允许（"depends on
   `cc-model`/`cc-db` only"）；`lib.rs` 依赖口径文档同步更新；`Cargo.lock`
   相应重生成。rusqlite 仅入 dev-dependencies（集成测试建库），生产依赖面
   不含。
8. **简报验收"bounded memory（大 batch_rows 断言峰值）"以结构性断言替代**：
   断言"每批请求数 ≤ batch_rows 且 keyset 步数精确"（测试 13）+ 实现仅持有
   单批向量与大小 k 堆（代码审阅事实），未做真实内存峰值测量——oracle backend
   的内存上界由结构直接给出，测量型断言在本轮为噪音。

## 8. 验证

```
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-semantic --locked --offline
  → 54 lib + 17 (artifact_cache) + 4 (manifest_exact_integration) 全绿，0 警告
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-db --locked --offline
  → 162 lib（含新增 5）+ 集成套件全绿（cc-db 既有 157 测试零回归）
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo check --workspace --locked --offline   → Finished，无错误无警告
```

TDD 过程记录：先写测试后补实现；期间修出两个真缺陷——
① `TopK::finish` 升序 `sort()` + `Candidate::cmp` 双分量写反导致 tie-break 与
最终排序全反（tie 测试红：`left: ["mm","aa"] right: ["aa","mm"]`），修正为
desirability 序 + `sort_by(b.cmp(a))`；② 手算值 `cos((0.8,0.6),(0.6,0.8))=0.96`
在 f32 分量下不逐位成立（f32 十进制表示误差 ~2.9e-8 > 断言 1e-12），改用 f32
精确表示的 (3,4)/(4,3) 使 24/25 逐位成立。两处均为测试暴露实现/断言缺陷，非
绕过。
