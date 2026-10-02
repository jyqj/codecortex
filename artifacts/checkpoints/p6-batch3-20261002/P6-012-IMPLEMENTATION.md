# P6-012 实施记录：覆盖率与 semantic epoch（读侧）

- 日期：2026-10-02
- 依据：`artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`
  P6-012 节（分母口径 eligible/published/failed/stale、零 eligible 有原因、单连接一致读）、
  `docs/adr/0003-semantic-persistence-single-db-boundary.md`（第 148 行 P6-012 约束；
  第 52 行"语义 epoch key 缺失即 None = 未就绪，绝不当作 0"）、批次交付记录
  （P6-004 semantic_epoch bump 语义 + Q4、P6-011 publish CAS 与
  `visible_set_changed`、P6-010 `SemanticManifestReads` keyset 先例、
  capability_status.rs:42-52 的 before/after 一致读模式）。
- 改动范围：`crates/cc-db` 1 个新模块 + lib.rs 一行注册 + 1 个新集成测试文件。
  **schema v22 零变更**；P6-006/007/008/010/011 交付物**只调用未改动**（零文件触碰）；
  cc-server `capability_status.rs` 本轮未动（见偏差 1）；`tasks.json` status 未改；
  未 git commit。

## 1. 文件:行号改动清单

| 文件 | 变更 | 内容 |
|---|---|---|
| `crates/cc-db/src/semantic_coverage.rs` | 新增（全文） | 模块文档（覆盖率口径原文 + epoch 一致性读纪律）+ `ZeroEligibleReason`(:64，三变体) + `SemanticCoverage`(:82) + `SemanticUncovered`(:103) + `SemanticCoverageSnapshot`(:113) + `coverage_on`(:128) + `uncovered_on`(:193) + `impl ReadOps::semantic_coverage`(:239) / `semantic_uncovered`(:259) + 8 个模块级测试(:344-568) |
| `crates/cc-db/src/lib.rs:47` | 修改 | `pub mod semantic_coverage;`（一行注册） |
| `crates/cc-db/tests/semantic_coverage.rs` | 新增 | 3 个门面级集成测试（§5） |

## 2. 覆盖率口径定义（简报原文与落点）

简报 P6-012 结构体草案与四条口径原文：

> `pub struct SemanticCoverage { pub eligible: u64, pub published: u64, pub failed: u64,
> pub stale: u64, pub reason: ZeroEligibleReason }`
> `// eligible   = document_manifest 中 encoding_key 非空且属 active space 的行数`
> `// published  = semantic_manifest 中 active space 行数`
> `// failed     = semantic_outbox state='failed' 活跃计数`
> `// stale      = pending/claimed 中 doc_version 已落后 document_manifest 的计数`

落点（`crates/cc-db/src/semantic_coverage.rs:128-191`，全部单连接 SELECT）：

| 口径 | 实现定义 | SQL 落点 |
|---|---|---|
| `eligible` | active space 存在时，`document_manifest` 中 `encoding_key IS NOT NULL` 的行数；**无 active space 时恒为 0**（`SemanticNotConfigured`，连 document 表都不查——不为未被任何空间认领的文档虚报分母） | `SELECT count(*) FROM document_manifest WHERE encoding_key IS NOT NULL` |
| `published` | `semantic_manifest` 中 **active space** 行数（可见集合）；`backfilling`/`revoked` 空间的行永不计入（P6-017"不同空间分数永不混排"，异空间向量不可返回故也不构成覆盖） | `SELECT count(*) FROM semantic_manifest WHERE space_id=?` |
| `uncovered`（新增字段） | `eligible - published`（saturating）：**manifest 可见集合 vs document_manifest 全集的差集口径**。子集不变式由 P6-011 CAS 结构性保证（fence 4 拒绝 NULL encoding_key；FK 保基行在场），故差值非负 | Rust 侧计算（:180） |
| `failed` | active space 的 outbox 终态 `failed` 行数（`superseded`/`done` 天然被 state 过滤排除） | `SELECT count(*) FROM semantic_outbox WHERE space_id=? AND state='failed'` |
| `stale` | active space 的 **live（pending/claimed）** 任务中，`(doc_key, doc_version)` 与 `document_manifest` 不再匹配（版本落后**或**基行已消失）的计数。写路径 eager supersede（P6-006）下正常恒为 0；该计数器把任何滞留变为可观测而非假设 | `NOT EXISTS (SELECT 1 FROM document_manifest d WHERE d.doc_key=t.doc_key AND d.doc_version=t.doc_version)` |

零 eligible 有原因（简报"零 eligible 有原因"原文）：`ZeroEligibleReason`
{ `SemanticNotConfigured`, `NoDocuments`, `EncodingUnsupported` }，优先级
未配置 > 无文档 > 编码不支持；`eligible > 0` 时 `reason: None`（结构体字段
为 `Option<ZeroEligibleReason>`，偏差 3）。

未覆盖清单（任务书"读方法暴露覆盖率/未覆盖清单（供 status/查询守卫用，如 dense
召回的范围声明）"）：`uncovered_on`(:193) 按 `doc_key` keyset 分页
（`doc_key > after ORDER BY doc_key ASC LIMIT`，与 P6-010 `scan_space` 同纪律），
每行携带 `doc_key/file_path/doc_version`；只在 revoked/backfilling 空间发布过的
文档**保持未覆盖**（异空间不覆盖任何东西）。调用方据此声明
"published over N of M eligible documents" 的 dense 召回范围。

## 3. semantic epoch 读侧一致性口径（任务书第 2 项）

读路径既有锚点 `read_generation.rs:52`（SELECT 已覆盖 `semantic_epoch` 键，
缺失→`None`）本轮**零改动**；补齐的是一致读纪律并固化为读侧规则：

- **规则**：每次 semantic 诊断读在同一池化连接上，先取 strict
  `ReadGeneration`（before）→ 执行计数 SELECT → 再取（after）；前后相等才
  返回，否则重试；3 次耗尽报 `RetrievalChanged { attempts: 3 }`，**绝不返回
  混代的计数**。这是 capability 快照既有模式
  （`crates/cc-server/src/capability_status.rs:42-52`）在语义读侧的固化，
  与 P6-011 写侧五重 fencing 构成读/写对偶（均为 strict ReadGeneration，
  绝不走 legacy 双钟）。
- **快照配对**：`SemanticCoverageSnapshot { coverage, generation }`(:113) 把
  计数与它们所对应的代快照绑定返回；status/守卫消费方缓存时应以该
  generation 为键，而非假设覆盖率与 epoch 无关。
- **None ≠ 0 纪律**：`semantic_epoch == None`（键缺失 = 未就绪）原样进快照
  （`Option<u64>` 类型保证），与 `published == 0` 共存且语义不同——
  `unwired_facade_reports_honest_zero_and_none_epoch` 显式断言。
- **epoch 作用域声明**（模块文档明文）：`semantic_epoch` 只跟踪可见集合
  （P6-004 Q4），eligibility 变化不推进它——消费方不得用 epoch 键控覆盖率
  缓存而不看快照本身。
- 读纯净性：`coverage_on`/`uncovered_on` 不触碰任何钟
  （`coverage_reads_are_pure_and_zero_limit_is_rejected` 断言）。

## 4. 与既有 status/capability 面（任务书第 3 项）

只加只读方法（`ReadOps` 面两个新方法），不改任何既有行为；semantic 未接线时
口径如实为 0 + `SemanticNotConfigured`。MCP 响应面本轮未暴露（红线"本轮可只
在库层"）：简报草案中 `capability_status.rs` 的 `semantic_coverage`/`provider`
字段留待接线轮走 additive（库层 `SemanticCoverageSnapshot` 已携带所需全部字段，
含 generation，可直接 json 化）。

## 5. 测试清单（11 个，全绿）

**模块级（`crates/cc-db/src/semantic_coverage.rs`，8 个）**

1. `unwired_semantic_reports_zero_coverage_without_inflating`(:344)——未接线
   正反例：embeddable 文档在场但无 active space → 全零 + `SemanticNotConfigured`、
   未覆盖清单空；仅 `backfilling` 空间仍算未接线。
2. `zero_eligible_is_explained_by_documents_and_encoding`(:372)——零 eligible
   三态：无文档→`NoDocuments`；文档全 NULL encoding→`EncodingUnsupported`；
   有一行 embeddable→`reason: None`。
3. `partial_coverage_reports_the_gap_and_the_uncovered_list`(:394)——部分覆盖：
   eligible=3/published=1/uncovered=2；清单逐项（doc_key/file_path/version）+
   keyset 分页与游标穷尽。
4. `full_coverage_reports_no_gap_and_no_reason`(:429)——全覆盖：差集为 0、
   清单空、无 reason。
5. `foreign_space_rows_never_count_as_published`(:445)——多空间：revoked 空间
   的两行不构成 published；仅 d1 回填进 active space → published=1、
   uncovered 恰为 d2（异空间向量不覆盖任何东西）。
6. `failed_counts_only_active_space_failures`(:475)——真实 outbox 原语入队/
   claim/retry 至 `failed`：active space 计 1，reowned 到 revoked 空间的失败
   不计。
7. `stale_counts_live_tasks_lagging_the_base_manifest`(:522)——四任务矩阵：
   pending 匹配（不算）、版本落后（算）、基行消失（算）、claimed 匹配（不算）
   → stale=2；且 stale 任务不是覆盖（uncovered 仍按差集口径）。
8. `coverage_reads_are_pure_and_zero_limit_is_rejected`(:568)——读纯净
   （前后 ReadGeneration 相等）+ `limit==0` 拒绝。

**门面级（`crates/cc-db/tests/semantic_coverage.rs`，3 个）**

9. `unwired_facade_reports_honest_zero_and_none_epoch`(:75)——真实 `IndexDb`
   未接线：全零 + `SemanticNotConfigured`，快照 `generation.semantic_epoch ==
   None`（None ≠ 0 纪律）且等于 fresh strict 读；未覆盖清单空。
10. `coverage_facade_pairs_a_strict_generation_snapshot_with_publication`(:110)——
    发布前后各读一次：发布前 eligible=1/published=0/epoch `None`；经真实
    P6-011 门面 `publish_semantic` 发布后 published=1/uncovered=0/
    `semantic_epoch == Some(1)`，快照 generation 与 fresh `read_generation()`
    全等（一致读成立），未覆盖清单收敛为空。
11. `primitive_counts_agree_with_facade_on_a_stable_database`(:167)——稳定库上
    `*_on` 原语与 generation 守卫门面结果逐字段相等（重试纪律只过滤跨写竞态）。

## 6. 验证命令与结果（原样）

```
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-db -p cc-index --locked --offline
→ 全部套件 "test result: ok."，0 failed，0 warning；关键计数：
  cc-db lib 170 passed (+1 ignored，本轮 +8)；
  semantic_coverage 集成 3 passed；
  semantic_publish 11、semantic_outbox 17、semantic_lease 9、semantic_schema 6、
  semantic_manifest_reads(嵌入 lib) 等既有套件零回归；
  cc-index lib 377 passed (+1 ignored) 及全部集成套件绿

SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo check --workspace --locked --offline
→ Finished `dev` profile [unoptimized + debuginfo] target(s) in 30.16s（零 error）

cargo clippy -p cc-db --locked --offline     → 零 warning
rustfmt --check（本轮新增 2 文件）            → 干净
```

## 7. 偏差清单（与简报草案）

1. **cc-server `capability_status.rs` 未加字段**：简报归属含
   "capability_status.rs 新增 semantic_coverage 与 provider 字段"；本轮红线
   "不改 MCP 响应契约，新增只读面若需暴露走 additive，本轮可只在库层"——
   库层 `SemanticCoverageSnapshot`（自带 generation）已备好，暴露留接线轮。
   同理 `provider: "fake"|"none"` 字段未落（P6-002/009 交付记录已把它挂在本
   任务，属 status 面 additive，随上项同轮处理）。
2. **`SemanticCoverage` 增加 `uncovered` 字段**（草案五字段外）：任务书明文
   "manifest 可见集合 vs document_manifest 全集的差集口径"，差值显式可观测
   优于调用方自行相减；`SemanticUncovered` 清单即该差集的逐文档投影。
3. **`reason` 为 `Option<ZeroEligibleReason>`**（草案为必填）：`eligible > 0`
   时无"零原因"可报，`Option` 让"没有要解释的事"显式化；草案末尾 `…` 本就
   留了枚举演化空间。
4. **stale 定义含"基行消失"**（草案措辞"doc_version 已落后"）：删除路径在
   P6-006 下会 eager supersede，基行消失的 live 任务只能来自异常窗口——
   正是该计数器要暴露的对象；与"落后"合并为一个 `NOT EXISTS` 谓词，测试 7
   两分支独立可观测。
5. **一致性重试纪律落在门面（ReadOps）而非原语**：`*_on` 原语保持单连接单
   时刻语义（与 P6-006/007/010/011 的 `*_on` 家族一致）；before/after 比对
   属编排关注，固化在 `semantic_coverage`/`semantic_uncovered` 两个门面方法。
   强制重试路径（读写交错窗口）无确定性注入点，未做直接测试——以测试 10
   （快照与 fresh 读全等）+ 测试 11（原语/门面等价）作行为等价证据；这与
   capability 快照既有代码（同样无交错注入测试）口径一致。

## 8. 未做与剩余风险

- `stale` 在当前生产写路径下恒为 0（eager supersede），计数器为守卫面；
  P6-013 worker/P6-017 切换窗口引入新写序后应复跑测试 7。
- `eligible` 未做 50k+ 规模基准（简报风险段：capability 快照为用户触发路径
  可接受；如需优化走 `document_manifest_encoding`/`semantic_manifest_space`
  索引 count，SQL 已按可走索引形态书写）。
- dense lane 对覆盖率的消费（范围声明、查询守卫）属后续接线轮；本轮交付
  库层读面与固化口径。
- cc-semantic 3 个既有 clippy warning 与仓内既有 fmt 漂移未处理（红线：交付
  文件不触碰）。
- 未改 `tasks.json` status，未 git commit，artifacts 冻结链未触碰。
