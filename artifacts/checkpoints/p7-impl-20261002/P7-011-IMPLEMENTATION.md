# P7-011 实施记录：dense 范围与 hydrate 守卫

日期：2026-10-03
任务：`docs/roadmap/code-index-v2/tasks.json` P7-011「dense范围与hydrate守卫」（批次 3 首任务，depends_on P6-020/P7-009/P7-010）+ 规划简报 TASK-BRIEFS.md「P7-011 dense 范围与 hydrate 守卫」节 + 接线待办 12 消纳。
红线遵守：schema 零改动（全部只读消费）；ports/spec 冻结面零改动；P6/P7 冻结交付零修改（P6-010 `vector/exact.rs` 零 diff，守卫全部为新增模块 + additive 只读原语，逐条申报见 §6）；既有检索行为零回归（lexical/grep/exact/graph 路径零 diff，未接线 semantic 路径不变，cc-eval p5c_hydration 7 项回归绿，复跑见 §4）；V18 disabled 口径零漂移（默认构建无 semantic 代码，workspace check exit 0）；`tasks.json` status 未改；未 `git commit`。

## 1. 做了什么

### 1.1 文件与关键位置

| 文件 | 内容 | 关键行 |
|---|---|---|
| `crates/cc-server/src/semantic_scope_guard.rs` | **本任务核心新增模块**（cfg(feature="semantic")，待办 12 消纳）：dense recall 范围声明守卫（§1.2） | 全文件 |
| — `DenseScopeDeclaration` | 守卫裁决三元组（status / coverage / truncation_reason），recall 原样消费 | :78 |
| — `declare` / `declare_with` | 范围判定入口：空间 fence → P6-012 uncovered 清单 × hard scope 交集 → Complete/Partial 裁决（§1.2 口径原文） | :90 / :107 |
| — 常量 | `PARTIAL_COVERAGE_REASON="semantic_coverage_uncovered"`（:67）、`SPACE_NOT_ACTIVE_REASON="semantic_space_not_active"`（:69）、`UNCOVERED_SCAN_PAGE_ROWS=256`（:71）、`UNCOVERED_SCAN_MAX_PAGES=64`（:72 附近，≈16k 文档预算） | :67-73 |
| `crates/cc-server/src/semantic_wiring.rs` | recall 接线（additive）：扫描连接释放后调 `declare`；非 fusable 短路（Unavailable 空receipt）；最终 LaneOutcome 由裁决置 status/coverage/reason（原硬编码 Complete 三行替换） | :354 / :363 / :431 |
| `crates/cc-server/src/lib.rs` | `#[cfg(feature = "semantic")] pub mod semantic_scope_guard;` | :13-14 |
| `crates/cc-search/src/semantic_hydrate_guard.rs` | **本任务核心新增模块**：hydrate 阶段 dense 命中的 P6-011 manifest fence 读侧守卫（§1.3） | 全文件 |
| — `is_dense_hit` | dense 命中识别：`semantic` / `semantic@{rank}` reason（plan.rs 既有 lane 注记约定，唯一 sanctioned 标记） | :56 |
| — `DenseFenceGuard` + `manifest_current` | 按 doc_key memoized 的 fence 裁决；`check` 四条规则（§1.3）；DB 错误上抛（strict reads，fail-stop），仅 fence 裁决允许 skip | :66 / :87 / :100 |
| `crates/cc-search/src/evidence_hydrator.rs` | hydrate 接线（additive）：身份校验（硬失败段）之后、`path_current` 之前插入 fence 检查，违例 skip；diagnostics 新增 `dense_manifest_fence` 块（additive JSON 键） | :46 / :64 / :117 / :218 |
| `crates/cc-search/src/lib.rs` | `pub mod semantic_hydrate_guard;` | :30 |
| `crates/cc-db/src/semantic_manifest_reads.rs` | **新增 additive 只读**（P7-010 `chunk_ids_by_doc_keys` 同一先例）：`SemanticPublicationRow`（fence 三元组 doc_version/encoding_key/space_id，:68）+ `SemanticManifestReads::published_row(doc_key)`（:97，doc_key 主键单行投影，None=无发布行）。P6-010 冻结的 `SemanticManifestRow`/`scan_space` 零 diff | :68 / :97 |

### 1.2 范围守卫口径（原文）

依据（P6-018 口径延伸到召回层）：**「不得把未覆盖文档伪装成完整结果」——"绝不把缺向量当完整空结果"在召回层的对偶**：exact 扫描对可见集（active space 的 `semantic_manifest`）完成 ≠ 对 eligible 集完成；补嵌/重编码/预算耗尽期间，任何召回结果（含空结果）都基于部分知识，声明 Complete 即伪装。

`declare`（semantic_scope_guard.rs:107-161）三条规则：

1. **空间 fence**：`active_space_on(conn) != Some(space_digest)` → `Unavailable + "semantic_space_not_active"` + `LaneCoverage::not_run()` + 零候选——P6-017「不同空间分数永不混排」：非活跃空间的向量永不返回，也永不构成 Complete+0 的假象。
2. **uncovered 清单消费（待办 12）**：P6-012 `uncovered_on` 按文档差集清单 keyset 分页（256/页，64 页预算），逐行与请求 hard scope 求交——只有 scope 内的未覆盖文档才使本次召回部分覆盖。语言规则镜像 `vector/exact.rs::row_passes_scope` 的保守口径：`languages` 约束的 scope 拒绝无语言未覆盖行（清单不带语言），无约束 scope 只评 path 维度。这保持 `LaneCoverage`「scoped to the lane's declared hard-scope query」的 cc-model 既有语义，避免简报警告的「守卫过严把合法命中打成 partial」。
3. **裁决**：scope 内无未覆盖文档 → 既有 `Complete / LaneCoverage::complete(None, n) / truncation=None`（与前守卫行为逐字节一致）；否则 `Partial / LaneCoverage::partial(None, n) / "semantic_coverage_uncovered"`——候选保留（Partial 可融合），receipt 点名缺口。页预算耗尽（剩余清单可能含 scope 内未覆盖文档）**保守裁 Partial**，疑问永不利 Complete。

### 1.3 hydrate 守卫规则（P6-011 fence 读侧）

现有 hydrate 已证明文档侧事实（identity/span/proof 硬校验 + `path_current` 磁盘时效，skip 语义既有）；本守卫补齐**向量基础**半边：dense lane 命中的排名依据是已发布 embedding，发布可在文档侧事实不变时移动/消失（revoke、space switch、supersede 已入 manifest）。`DenseFenceGuard::check`（semantic_hydrate_guard.rs:100）四条全过才算 current：

1. `semantic_manifest` 存在该文档的发布行（revoked → 排名基础消失）；
2. 行 `space_id` == 单一活跃空间（P6-017：异空间向量永不作证）；
3. 行 `doc_version` == 命中 `DocumentRef.doc_version`；
4. 行 `encoding_key` == 命中 `DocumentRef.encoding_key`（publish CAS fence 4 从 `document_manifest.encoding_key` 拷贝的嵌入输入摘要句柄，消费侧 DocumentRef 携带同源字段——即「doc_version/input_digest 与 manifest 一致性」的载体）。

**陈旧候选处置**：skip + 计数（`diagnostics()["dense_manifest_fence"] = {checked, skipped, skip_disposition:"stale_publication_skipped"}`），镜像 `path_current` 的 skip 语义；绝不折进更小但"完整"的结果，也绝不报错（身份损坏仍走既有硬失败契约）。检查位置在身份硬校验之后、`path_current` 磁盘读之前（一次索引 SELECT，先廉后贵）；按 doc_key memoize（同文档 chunk 共享发布行）。

### 1.4 与简报的对照

- 「过滤在 topk 前且最终二次检验 manifest/source」：前者 P6-010 已交付（exact.rs filter-before-top-k，本任务零改动），后者 = SourceVerifier 既有磁盘/证明校验（零改动复用）+ 本任务 manifest 半边（§1.3）。
- 「删除和 scope 测试」：删除→revoked publication skip 测试（cc-search 7 项）+ V05「删除复活」语义在 hydrate 层闭合；scope 正反例矩阵见 §3。
- 「semantic 找回结果也不会被 softscope 误删或越过 hard 范围」：soft scope 在既有链路只作预选/排序（plan.rs:338 `role:"ranking_and_scan_priority_only"`），无删除点，semantic 候选不经 preselect；hard 范围生产侧 exact filter（P7-010 已测）+ 消费侧 `append_semantic_outcome` passes_filters 硬拒（lanes.rs:297-304 既有）双保险保持零改动。
- 模块归属偏差：简报草案把二次校验放在 lane 阶段 `SourceVerifier::hit_current`——`SourceVerifier` 需要 project root 而 LaneContext 无此参数；任务指令（以 hydrate 阶段为准）与 P7-010「recall 落 semantic_wiring」的既成格局下，落地为 cc-search hydrate 守卫模块 + cc-server 范围守卫模块，复用面（`HardScope::passes` 口径、P6-012 读面、SourceVerifier 既有校验）全部兑现。

## 2. 接线待办 12 消纳声明

**待办 12（「dense lane 对覆盖率的消费（范围声明/查询守卫）」，P6-012 收口移交）本轮已消纳**：`semantic_coverage`/`semantic_uncovered` P6-012 读面进入召回范围判定（`declare`，semantic_scope_guard.rs:90），裁决进 `ExactRecallService::recall` 的 LaneOutcome（semantic_wiring.rs:354/:363/:431）。消纳范围 = recall 供给侧的范围声明与守卫；`semantic_coverage` 快照在 explain/状态面的更丰富透出（`LaneCoverageExplain` 等）归 P7-012（简报明确 P7-011 只做召回层守卫，coverage 口径与 P7-012 统一由其 explain 面承接）。13 项待办表账目不变更（权威位置 round12 审计文件）。

## 3. 测试

| 位置 | 测试 | 覆盖 |
|---|---|---|
| cc-server `semantic_scope_guard.rs` tests（8 项） | full/partial/外 scope/语言约束/页预算保守/非活跃空间/无活跃空间/validate 耦合 | 范围守卫正反例矩阵（覆盖/未覆盖/部分覆盖 + hard scope 交集不过严 + 预算诚实） |
| cc-server `semantic_wiring.rs` tests（新增 2 项） | `partial_publication_coverage_is_declared_never_masqueraded`（FakeProvider 全链：seeded_world + 未发布 d2 → Partial + 缺口命名 + 候选仅来自已发布文档 + adapter 收据门放行 Partial）；`recall_from_a_non_active_space_is_unavailable_never_complete_zero` | recall 全链集成（FakeProvider），范围外排除 + 空间 fence |
| cc-search `semantic_hydrate_guard.rs` tests（7 项） | current 放行/陈旧版本 skip/删除（revoke）skip/异空间 skip/非 dense 命中不受 fence/未配置语义 skip 不报错/注记约定谓词 | hydrate 一致性守卫 + 陈旧候选处置（真实引擎 fixture：production document 路径写入 + 磁盘字节 + 真实 search hit） |
| cc-db `semantic_manifest_reads.rs` tests（新增 1 项） | `published_row_reads_the_single_current_publication_whatever_space` | additive 原语 |

## 4. 验证（命令与结果原样）

1. `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo test -p cc-server --features semantic -p cc-semantic -p cc-index --locked --offline` — **exit 0，全部套件 0 failed**。关键套件：cc-server lib `273 passed; 0 failed`（P7-010 收口 263 + 本任务 10）；cc-semantic lib `212 passed; 0 failed`；cc-index lib `379 passed; 0 failed; 1 ignored`（既有）。新测试全部具名通过（§3 清单，逐条 ok）。
2. `cargo check --workspace --locked --offline` — **exit 0**（`Finished dev profile`）。
3. 零回归补充：`cargo test -p cc-eval --test p5c_hydration --locked --offline` — `7 passed; 0 failed`（hydrate 既有消费面回归）；`cargo test -p cc-search --locked --offline` — `288 passed; 0 failed`（+7）；`cargo test -p cc-db --locked --offline` — 全绿（+1）。
4. 唯一编译 warning 为 `cc-semantic/tests/reconcile_rebuild.rs` 既有 `digest_table` dead_code（本轮零触碰，历史在案）。

## 5. 偏差清单

1. **cc-db 触及超出 tasks.json scope**（scope 仅列 exact.rs/evidence.rs）：`semantic_manifest_reads.rs` 新增 additive 只读 `published_row`/`SemanticPublicationRow`——fence 读侧需要按 doc_key 取发布行的唯一缺口，P7-010 `chunk_ids_by_doc_keys` 同一先例（additive、只读、冻结结构零 diff），按惯例申报。
2. **exact.rs / evidence.rs 零 diff**（scope 所列）：红线「P6/P7 冻结交付零修改」优先——exact.rs 是 P6-010 冻结交付（「范围过滤断言强化」由既有 filter-before-top-k 生产侧断言 + 消费侧 lanes.rs:302 双保险 + 本轮正反例测试兑现，无需改动）；evidence.rs 的 `SourceVerifier` 按简报复用未改，守卫落新增模块 `semantic_hydrate_guard.rs`（红线「消费与新增守卫模块」）。
3. **`SemanticPublicationRow` 新结构而非复用 `SemanticManifestRow`**：后者是 P6-010 冻结 scan 面（加字段即改冻结结构）；fence 只需三元组投影。
4. **声明读取改为 `&IndexDb` 独立 checkout**（简报草案为"同一连接"）：cc-server 对 rusqlite 仅有 dev-dependency（`--locked` 不可加正式依赖），且引擎既定惯例禁止嵌套 checkout（1 连接读池历史 bug）；改为扫描连接释放后的第二次 short checkout，代际交错由 adapter 既有 generation 复验覆盖（模块文档已声明）。
5. **V05/V16 验证证据**：tasks.json deliverables 要求 `artifacts/benchmarks/<run-id>/` 的 V05/V16 证据「实施时生成」——本轮未生成：V05/V16 正式验证矩阵证据按批次 1/2 既有口径归验收轮（not_run 不推断），本记录以 §4 命令级测试结果为证据。
6. **空间 fence 的行为新增**：非活跃空间召回由「Complete+0」（P7-010 硬编码）改为 `Unavailable("semantic_space_not_active")`——这是本任务范围守卫的预期修正（P6-017 口径），非回归；既有 P7-010 测试（活跃空间匹配）零改动通过。

## 6. 移交与边界

- **P7-012**：coverage 口径的 explain 透出（`LaneCoverageExplain`、covered/uncovered 文档级清单面）、Partial receipt 的融合语义深化；`PARTIAL_COVERAGE_REASON` 字符串为两任务共享契约。
- **P7-014**：全链贯通时 hydrate 守卫的 `dense_manifest_fence` 诊断可接入 MCP 响应面（本轮为 hydrator diagnostics 内的 additive JSON 键，无 MCP 字段改动）。
- 未接线语义路径（默认构建、`semantic.enabled=false`）零行为变化；`semantic_space_not_active` 与 `semantic_coverage_uncovered` 两个 truncation_reason 均为 Partial/Unavailable 必带 reason 的 validate 合法值，且使 LaneOutcome `is_cacheable=false`（Partial 非既有限定原因集），语义召回本就被排除在结果缓存外，无缓存语义变化。
