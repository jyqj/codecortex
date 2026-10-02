# P7-010 实施记录：dense 召回端口接线 + 组合根最小装配腿

日期：2026-10-03
任务：`docs/roadmap/code-index-v2/tasks.json` P7-010「dense召回端口接线」（批次 2 收尾，depends_on P7-009）+ IMPLEMENTATION-ORDER 第 2 节「P7-010 最小装配腿（接线待办 1 部分/4/5）」。
红线遵守：schema 零改动；ports/spec 冻结面零改动（`ports.rs`/`spec.rs` 零 diff）；P6/P7 既有交付物零修改（装配全部走新增 + 消费槽就位点最小填充，逐条申报见 §6）；默认构建零依赖（cargo tree 双口径 §4）；V18 disabled 口径零漂移（复跑 §4）；`tasks.json` status 未改；未 `git commit`。

## 1. 做了什么

### 1.1 文件与关键位置

| 文件 | 内容 | 关键行 |
|---|---|---|
| `crates/cc-server/src/semantic_wiring.rs` | **本任务核心新增模块**（cfg(feature="semantic")，默认构建整体编出） | 全文件 |
| — `SemanticSubsystem` | 装配产物聚合（recall 端口 + worker 侧 ledger/cache/query_cache + namespace + 冻结 space/query spec），P7-014 全量接线的直接输入 | :75 |
| — `assemble` / `assemble_with` | 子系统构造器（散落组件装配集中点）：`enabled=false` → `Ok(None)`（零构造零落盘）→ `resolve_provider`（P7-002 配置解析，缺键/能力不匹配 = 点名键的拒启 Config 错误）→ 共享 gate/breaker 显式 init（P7-005/006 单例，first-wins 既有语义未改）→ `resolve_cache_root_with` + `namespace_key(项目身份)` + `ArtifactCache::open`（side-effect free，待办 5）→ `QueryVectorCache`（4096 entries / 64 MiB 双硬上限，程序化界，P7-009 偏差 4 同口径）→ `DegradationLedger::new(None)`（预算键归 P7-014 待办 6 模式）→ `QueryEncodingSpec(space, None, max_input_tokens, TOKEN_ESTIMATOR)` | :101 / :123 |
| — `wire` / `wire_with` | 注入/摘除：`Some` → `set_semantic` + 快照桥接；`None` → 双槽对称清空（回到 `not_configured`，不残留半开态）；装配 Err 先于任何槽写入（拒绝的配置绝不留半接线态）。`wire_with` 注入 cache-root lookup，测试零环境变量接触 | :212 / :235 |
| — `From<DegradationSnapshot> for SemanticDegradation` | 待办 4 一行桥接的纯数据转写（七字段一一对应，零 cc-semantic 类型过槽） | :265 |
| — `ExactRecallService` + `impl SemanticRecall` | **生产 recall 供给**（§1.2） | :285 / :313 |
| `crates/cc-server/src/lib.rs` | `#[cfg(feature = "semantic")] pub mod semantic_wiring;`（+4 行） | :13 |
| `crates/cc-server/src/engine.rs` | `set_project` 内 cfg-gated 接线调用（+10 行，消费槽就位点最小填充）：`wire(&self.query_services, 项目规范路径, config, db)`；`semantic` 节配置错误拒绝 `set_project`（fail-closed，self 字段未被污染——wire 在字段赋值之后、返回 Err 即整体失败） | :213-222 |
| `crates/cc-db/src/index_db_retrieval.rs` | **新增 additive 只读方法** `chunk_ids_by_doc_keys`（doc_key → chunk_id，IN 分批，沿 `sql_in_placeholders`/`IN_BATCH_SIZE` 惯例）：exact 后端讲 doc_key、既有候选读讲 chunk_id 的唯一缺口；零既有 diff，偏差申报见 §6.1 | :657 |

### 1.2 SemanticRecall 供给链路（ExactRecallService::recall，semantic_wiring.rs:313）

```
QueryHandle::search_async (query_handle.rs:76-110，既有消费侧，零改动)
  → services.semantic() → policy.effective != Local 才进入（"local 策略不调用端口"，
    既有短路 query_handle.rs:78 + engine.rs:206/:224 缓存排除，本任务零触碰）
  → semantic_adapter::recall (cc-search，panic 捕获/取消/超时/容量分类/weight 归一，零改动)
    → ExactRecallService::recall：
      1. QueryInput::from_bytes(query) → QueryCacheKey::new(namespace, query_spec, input)
         → QueryVectorCache::get ——查询路径只消费 P7-009 缓存，无 provider、无 transport、
         零 HTTP 形状类型；miss → LaneStatus::Unavailable + truncation
         "query_vector_not_encoded"（C10：没执行 ≠ 没结果；查询内联编码裁决归 P7-012/013）
      2. control.check() → 短读连接内：SemanticManifestReads::scan_space（space 隔离）
         × exact::search（C09 filter-before-top-k、(score desc, doc_key asc) 全序、
         Corrupt/Miss 候选跳过不污染）——代际新鲜度由调用方既有机制管
         （lanes.rs:271-274 回读 generation 比对）
      3. retrieval().chunk_ids_by_doc_keys → chunk_candidate_rows_by_ids（既有镜像校验全复用）
         → CandidateRef{document(doc_key/doc_version), source_span, lane_rank, raw_score,
         scoring_spec="cosine-exact-v1", exact_identity=false}
      4. LaneOutcome{Complete, coverage.complete(None, n), truncation=None}——exact 扫描
         全 hard scope 后取 top-k，Complete 语义成立；receipt 由 adapter validate +
         append_semantic_outcome 身份/跨度/hard scope 二次复验（既有，零改动）
```

**验收对照**：「搜索不依赖具体HTTP客户端」——recall 实现闭包零传输类型，cargo tree
断言见 §4；「返回 doc 版本/空间/coverage」——CandidateRef.document.doc_version +
coverage.complete，集成测试断言固化；「local 策略不调用端口」——既有短路 +
`local_strategy_never_reaches_the_semantic_port` 断言（QueryPolicy resolve Local）。

## 2. 接线待办消纳表（编号 = round12 审计 `wiring_round_pending_table.items[].no`）

| # | 待办项 | 本轮状态 | 说明 |
|---|---|---|---|
| 1 | try_init 组合根接线与语义子系统初始化 | **部分消纳** | 最小装配已落：feature 开 + `semantic.enabled` 才构造（`assemble`），`set_project` 接线（engine.rs:217），产出可注入实例。**剩归 P7-014**：配置键驱动启停的完整 try_init、全量初始化语义（capability probe 触发点、运行时关闭路径） |
| 4 | 降级快照转写（DegradationLedger::snapshot → set_semantic_degradation） | **已消纳** | `wire_with` 桥接（semantic_wiring.rs:252-256）+ `From` 转写（:265）。边界：快照为装配时点照面；ledger 后续变化的**重转写**随 worker drain 调度点（待办 2/3）归 P7-014 |
| 5 | cache 根目录与项目身份传入 | **已消纳** | `resolve_cache_root_with` + `namespace_key(项目规范路径)`（semantic_wiring.rs:160-170）；根不可导出 = 点名 `CODECORTEX_SEMANTIC_CACHE_ROOT` 的拒启错误 |
| 2 | drain/reconcile/recovery/GC 调度时机 | 归 P7-014（+P7-015 竞争测试验收腿） | 本腿零调度：无 drain、无常驻线程、无定时器 |
| 3 | worker drain 外层挂 degrade 门面 | 归 P7-014 | 与 2 同一装配点 |
| 6 | GC 宽限配置面 / 7 GC 审计计数 / 10 页帽化 / 13 space switch 触发面 | 归 P7-014 | C14 配置面与 V18 贯穿链 |
| 8 | worker 公平化 ORDER BY | 归 P7-005 | 与本轮无交集 |
| 9 | 机会性 reclaim 收窄 / 11 desired 投影回接 | 归 P7-016 / P7-015 | owner 确认项 |
| 12 | dense lane 对覆盖率的消费 | 归 P7-011（守卫）+ P7-012（coverage 透出） | `LaneCoverage.complete` 已由 recall 产出 |

## 3. TDD 流程与测试清单（原样，`semantic_wiring` 10 项全 ok）

红绿两轮：红轮桩 `todo!()` 先行 → `cargo test -p cc-server --features semantic --lib`
**72 failed**（全部 panic 于桩，含波及 set_project 的既有测试）→ 实现后全绿。绿轮
3 次测试种子修正（files.document_spec 非空、镜像一致性 content_hash/encoding_key、
span 端点=byte_len），实现侧零改动。

```
test semantic_wiring::tests::disabled_config_keeps_the_port_unattached_and_v18_wording_unchanged ... ok
test semantic_wiring::tests::missing_required_keys_are_config_errors_that_name_the_key ... ok
test semantic_wiring::tests::capability_mismatch_is_a_config_error_never_a_silent_default ... ok
test semantic_wiring::tests::enabled_config_assembles_attaches_and_creates_no_cache_path ... ok
test semantic_wiring::tests::switching_the_switch_off_detaches_again ... ok
test semantic_wiring::tests::degradation_ledger_snapshot_bridges_into_the_capability_probe ... ok
test semantic_wiring::tests::fake_provider_full_chain_publish_encode_recall_hit ... ok
test semantic_wiring::tests::uncached_query_vector_is_unavailable_never_an_inline_encoding ... ok
test semantic_wiring::tests::hard_scope_is_applied_before_top_k_on_the_wired_path ... ok
test semantic_wiring::tests::local_strategy_never_reaches_the_semantic_port ... ok
```

- **装配正例**：`enabled_config_assembles_...`（attach + cache 根零落盘断言 + 探针
  `port_attached_unverified`/`dense_state: "disabled"` 双断言）。
- **装配反例**：缺 model_id / dimensions（点名键）、metric 不匹配（"支持差异不能吞"），
  三者均断言槽位未被污染。
- **降级路径**：`degradation_ledger_snapshot_...`（note_corrupt → 桥接 → 探针
  `degraded` + reasons）；`switching_the_switch_off_...`（enabled→disabled 对称摘除）。
- **FakeProvider 全链集成**（`fake_provider_full_chain_...`）：文档嵌入
  （FakeProvider.embed_documents）→ **真实 P6 publish CAS 发布**（outbox enqueue →
  claim → `Publisher::publish_embedding`，五围栏真实走通）→ 查询编码（P7-009
  `encode_queries` 写入子系统 query cache）→ dense 召回经生产 `SemanticRecall` 端口
  + `semantic_adapter::recall` 收据门命中（doc_version/coverage/raw_score 断言）。
- **disabled/降级回归**：V18 口径由 `assert_v18_disabled`（not_configured +
  dense disabled + 原因原文）固化，且 cc-eval 探针复跑（§4）。
- **local 短路**：`local_strategy_never_reaches_...`（端口 attached 下
  `QueryPolicy::resolve` 仍解析 Local）+ 既有 query_handle/engine 短路零触碰。

## 4. 验证命令与结果（原样）

1. 任务指定主命令：
```
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-server --features semantic -p cc-semantic --locked --offline
→ cc-semantic: lib 212 passed + 集成套件 17/4/4/17/9/3/5/5/8/6/2 passed（query_cache/
  artifact_cache/manifest_exact/publish_cas/queue_worker/reconcile/recovery/degrade/gc/
  space_switch/retry_layering 全绿零回归）；
  cc-server: lib 263 passed（= 默认 251 + 本任务 10 项 semantic_wiring + 既有 2 项
  provider_gate/breaker 测试），0 failed。
  唯一 warning = cc-semantic reconcile_rebuild 的 digest_table never-used
  （先前轮遗留，P7-009 记录在案，未触碰）。
```
2. workspace check：
```
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo check --workspace --locked --offline
→ Finished `dev` profile [unoptimized + debuginfo] target(s) in 5.17s（零 error 零 warning）
```
3. cargo tree 双口径（默认构建零依赖复验）：
```
cargo tree -p cc-server --locked --offline -e normal | grep -c cc-semantic   → 0
cargo tree -p cc-server --features semantic --locked --offline -e normal | grep -c cc-semantic → 1
cargo tree -p cc-server --locked --offline（默认全 edges）grep cc-semantic   → 无输出
```
4. V18 探针复跑（disabled 口径零漂移）+ 默认构建回归：
```
SDKROOT=... cargo test -p cc-eval --test p5d_contract --test p5d_runtime --test p5b_execution --locked --offline
→ 10 passed / 1 passed / 10 passed（含 capability_readiness_distinguishes_absence_empty_error_and_semantic_port），0 failed
SDKROOT=... cargo test -p cc-server --locked --offline（默认构建）
→ lib 251 passed，0 failed（与 feature 构建 263 的差 = 12 项 cfg(semantic) 测试）
SDKROOT=... cargo test -p cc-db --locked --offline
→ 173 passed + 集成套件全绿（新增 chunk_ids_by_doc_keys 零回归）
```

## 5. tasks.json 条目对照

| 条目要求 | 落地 |
|---|---|
| step「将语义服务注入SemanticRecall」 | `wire`/`wire_with` → `set_semantic(Some(ExactRecallService))`（service_factory.rs:171 既有槽零改动） |
| step「返回doc版本/空间/coverage」 | CandidateRef.document.doc_version + LaneCoverage.complete（§1.2 第 3/4 步），测试断言固化 |
| acceptance「搜索不依赖具体HTTP客户端」 | recall 闭包零传输类型；`cc-server` 生产面无任何 HTTP crate（默认与 feature 构建同表，cargo tree §4） |
| acceptance「local策略不调用端口」 | 既有 query_handle.rs:78 短路 + engine.rs:206/:224 缓存排除零触碰 + 新增策略断言 |
| deliverable「实现/配置或规格变更」 | 新 `semantic_wiring.rs` 模块 + cc-db additive 读方法 + engine/lib 最小填充 |
| deliverable「artifacts/benchmarks/<run-id>/ V11, V16 证据」 | **not_run/blocked**：V11/V16 正式矩阵依赖查询执行全链与 P7-012/013（coverage 透出/守卫）及 live provider（D1/D2 不授权）；mock 腿证据 = §3 全部测试 + §4 回归。benchmark run-id 未生成，tasks.json evidence 不回填 |
| validation V18 | disabled/not_configured 口径零漂移（cc-eval p5d 复跑 + 单测固化）；五态全量化归 P7-014 |

## 6. 偏差清单

1. **`crates/cc-db/src/index_db_retrieval.rs` 新增 `chunk_ids_by_doc_keys`（scope 未列 cc-db）**：exact 后端产物是 doc_key、既有候选读 `chunk_candidate_rows_by_ids` 只收 chunk_id，二者缺一映射；按「SQL 归 cc-db 读模型」的 C2 系列约定下沉为 additive 只读方法（新增非修改，既有方法零 diff），同时避免在 cc-server 生产面引入 rusqlite（其仅为 cc-server dev-dep）。申报为最小越界。
2. **查询路径不内联编码（本腿裁决口径）**：recall 只消费 P7-009 缓存，miss → `Unavailable("query_vector_not_encoded")`——取 TASK-BRIEFS P7-012 风险注记的默认裁决（「默认：不允许，查询路径只消费缓存」）。**诚实边界**：生产侧当前没有任何查询编码调用方（编码归 worker/回填侧或 P7-012/013 的查询内联决策），故 enabled+attached 下每条查询的 dense lane 都是 Unavailable 直至该决策落地；全链 FakeProvider 语义命中由测试内预编码证明机制可用。
3. **接线点 = `set_project`**：配置错误拒绝 set_project（fail-closed，自愈面无半开态）；不含运行时热切换（配置变更需重 set/reopen），运行时关闭路径归 P7-014（IMPLEMENTATION-ORDER 第 6 节口径）。
4. **gate/breaker 单例 first-wins 未改**：若 lazy permissive/breaker 默认先被取用，本轮显式 limits 不替换实例——P7-005/006 既有单例语义与已记录 caveat，未新增偏差。
5. **DegradationLedger 预算 `None` + 快照为装配时点**：re-embed 预算无配置键（待办 6 模式归 P7-014）；ledger 变化的重转写随调度点（待办 2/3）归 P7-014，本腿只落"一行桥接"的最小版。
6. **QueryVectorCache 界限程序化常量**（4096 entries / 64 MiB）：P7-009 偏差 4 同口径，P7-014 若需操作者可调再加键。
7. **query_spec 取值**：无 instruction（无配置键）、`max_tokens = capability.max_input_tokens`、tokenizer = `TOKEN_ESTIMATOR`（encode_queries 的 tokenizer gate 唯一承认值）；instruction 配置键若后续引入，仅改 assemble 一处。
8. **P7-011 守卫未含**：dense 范围二次检验/hydrate 守卫是 P7-011 任务本体，本腿 recall 依赖 exact::search 的 C09 filter-before-top-k 与 append_semantic_outcome 的既有身份/scope 复验，不预支 011 交付。
9. 红线确认：schema 零改动；ports.rs/spec.rs 零 diff；P6/P7 既有交付物零修改（本次触碰的既有文件仅 `lib.rs`/`engine.rs` 消费槽就位点最小填充 + cc-db 新增一法，逐条已申报）；默认构建零依赖（§4.3）；V18 disabled 口径零漂移（§4.4）；`tasks.json` status 未改；未 `git commit`。
