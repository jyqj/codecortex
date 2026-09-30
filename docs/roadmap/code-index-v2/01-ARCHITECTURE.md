# 01｜目标架构、文件树与职责迁移

> 全部是拟实施结构，不是当前已有文件。树列出本设计影响的模块；未列出的 framework/infra/Cypher/installer 等现有模块继续保留。目标是能力边界清楚，不是一次性照树搬家。

## 1. 两条派生路径、一个事实来源

```text
源码文件 + 项目配置 + scanner 事件
          │
          ▼
FileCatalog + SourceSnapshot + ProjectModel
          │
          ▼
ParseFacts（符号/import/export/关系/边界，含能力与不确定性）
          │
          ├── PublicSurface + ResolutionDependencies → DirtyPlanner
          │                  │
          ▼                  ▼
ModuleResolver → NameResolver → ResolvedFacts → PreparedBuild
          │
          ├── SQLite 结构图 / FTS / 实际源码 chunk
          └── RetrievalDocument + ActiveManifest + outbox（同一索引事务）
                               │
                               ▼
                     可选 SemanticWorker
                               │
                Embedder → SemanticArtifactCache
                               │
                     publish CAS + semantic_epoch
                               │
                               ▼
查询 → HardScope / SoftHints / QueryPolicy
     → lexical / exact-symbol / path / grep / graph / optional dense
     → RRF → deterministic rank → optional rerank → CoverageSelector
     → EvidenceHydrator → BudgetPacker → ContextEnvelope / MCP
```

结构事实可以来自语法、静态解析或用户摄入的运行证据，均需来源说明。向量结果只指向可验证的 RetrievalDocument，不产出事实边。P9 可选 LSP 给出单独的 verified observation；与静态结论冲突时同时标示，不无条件覆盖。

## 2. 依赖与组合

`A -> B` 表示 A 编译依赖 B：

```text
cc-parsers -> cc-model
cc-db      -> cc-model
cc-index   -> cc-db + cc-parsers + cc-model
cc-search  -> cc-db + cc-model
cc-semantic (optional) -> cc-db + cc-model
cc-server  -> cc-index + cc-search + cc-db + optional cc-semantic
cc-eval    -> cc-server + public model/testing seams
```

外部能力接口放 `cc-model` 中只在多 crate 需要时使用；不把网络客户端、Tokio Handle、SQLite Connection 放进领域类型。P5 的 `SemanticRecall` 是查询端口，P6 的 `VectorIndex`/`Embedder` 是实现端口，不能让 cc-search 依赖 cc-semantic。cc-server 是唯一生产组合根。

同步本地 lane 与异步 optional lane 使用同一候选/状态协议，不要求对整套同步 DB API 做 async 改造。生产网络工作在 Tokio 任务内，CPU/DB 工作进入有界执行器，deadline 传播，不为每个候选开线程。

## 3. 目标文件树

```text
crates/
├── cc-model/src/
│   ├── source.rs                  # SourceSnapshotId/SourceSpan/encoding
│   ├── identity.rs                # DocKey/ContentDigest/InputHash/VectorSpaceId
│   ├── public_surface.rs          # ObservableSurface/visibility/reexports
│   ├── project_model.rs           # packages/config versions/module targets
│   ├── resolution.rs              # outcomes/candidate sets/dependency reasons
│   ├── retrieval.rs               # documents/HardScope/SoftHints/LaneOutcome
│   ├── generation.rs              # persisted incarnation + generation vector
│   ├── semantic.rs                # optional port/types, no provider implementation
│   ├── search.rs                  # existing wire-compatible SearchRequest/Hit
│   ├── context.rs                 # source-backed evidence + additive diagnostics
│   └── config.rs                  # existing config; validated typed extensions
├── cc-parsers/src/
│   ├── traits.rs                  # parse outputs capability + chunk boundaries
│   ├── chunker/
│   │   ├── mod.rs                 # replace chunker.rs only when behavior is frozen
│   │   ├── boundaries.rs          # compact AST span projection, no retained trees
│   │   ├── split.rs               # recursive/symbol/statement splitting
│   │   ├── merge.rs               # small fragments, dedup, constrained adjacency
│   │   ├── fallback.rs            # UTF-8/long-line/generic fallback
│   │   └── source_slice.rs        # original bytes -> exact contiguous spans
│   ├── exports/                   # language surface extraction helpers
│   │   ├── mod.rs
│   │   ├── jsts.rs
│   │   ├── rust.rs
│   │   ├── python.rs
│   │   ├── go.rs
│   │   └── conservative.rs        # Java/C/C++/generic capability-limited surface
│   └── existing language parsers # call helpers; one AST pass per file
├── cc-db/src/
│   ├── epoch_rules.rs             # Index/Evidence/Semantic/Auxiliary write effects
│   ├── unit_of_work.rs            # typed explicit commit effects
│   ├── sql/                      # schema changes consolidated at release gates
│   ├── public_surface_store.rs
│   ├── resolution_dependency_store.rs
│   ├── document_store.rs          # current docs/manifests/version lookup
│   ├── semantic_outbox.rs         # desired work, leases, publish CAS
│   ├── freshness_store.rs         # incomplete closure / reconciliation frontier
│   └── existing index_db_*        # keep tested query/write/rebuild implementations
├── cc-index/src/
│   ├── project_model/
│   │   ├── mod.rs
│   │   ├── discover.rs            # consume WalkManifest/FileCatalog
│   │   ├── config_cache.rs        # content-aware cache + config dependency DAG
│   │   ├── typescript.rs          # config scopes, extends, workspace entries
│   │   ├── rust.rs                # reuse cargo_workspace + module visibility
│   │   ├── python.rs              # roots/packages/relative/__all__ capability
│   │   ├── go.rs                  # go.mod/go.work/package member sets
│   │   └── generic.rs             # typed unsupported/heuristic outcomes
│   ├── module_resolution/
│   │   ├── mod.rs                 # pure resolution against immutable project model
│   │   ├── typescript.rs
│   │   ├── rust.rs
│   │   ├── python.rs
│   │   └── go.rs
│   ├── incremental/
│   │   ├── mod.rs
│   │   ├── change_kind.rs         # text/API/config/inventory/feature changes
│   │   ├── surface.rs             # canonical fingerprints
│   │   ├── dependencies.rs        # positive and negative resolution dependencies
│   │   ├── planner.rs             # reuse dirty closure and reload policies
│   │   └── reconcile.rs           # durable partial closure continuation
│   ├── documents/
│   │   ├── mod.rs
│   │   ├── project.rs             # AST spans -> source-backed retrieval docs
│   │   ├── render.rs              # source text vs embedding input metadata
│   │   └── delta.rs               # document upsert/remove/reuse + outbox plan
│   ├── build_plan.rs              # authoritative phase ordering, not duplicated
│   └── indexer_phases/*           # integrate added contracts by vertical slice
├── cc-search/src/
│   ├── scope.rs                   # hard intersection vs soft hints
│   ├── query_policy.rs            # budget, intent routing, requested capability
│   ├── lanes/
│   │   ├── mod.rs
│   │   ├── lexical.rs
│   │   ├── exact_symbol.rs
│   │   ├── path.rs
│   │   ├── grep.rs
│   │   ├── graph.rs
│   │   └── semantic_adapter.rs    # abstract port only; no HTTP/provider client
│   ├── execution.rs               # bounded scheduling/cancel/deadline/status
│   ├── fusion.rs                  # existing RRF extended, stable ties
│   ├── selection/
│   │   ├── mod.rs
│   │   ├── coverage.rs
│   │   ├── overlap.rs
│   │   └── budget.rs
│   ├── evidence.rs                # hydrate + manifest/span validation
│   ├── engine_cache.rs            # generation/spec/policy-sensitive cache keys
│   ├── plan.rs                    # normalize requests; never mutate hard scope
│   └── existing cypher/*          # no rewrite in main plan
├── cc-semantic/                   # added in P6; optional dependency only
│   ├── Cargo.toml
│   └── src/
│       ├── lib.rs                 # explicit service construction
│       ├── ports.rs               # Embedder/VectorIndex contracts
│       ├── spec.rs                # document vs query encoding and vector spaces
│       ├── cache.rs               # content-addressed derived SQLite cache
│       ├── worker.rs              # index-maintenance worker, not general task runtime
│       ├── queue.rs               # cc-db outbox adapter, lease/claim handling
│       ├── publish.rs             # active manifest validation, no cross-store fiction
│       ├── admission.rs           # batch/token/byte/rate/concurrency limits
│       ├── policy.rs              # opt-in/endpoint/egress/budget policy
│       ├── providers/
│       │   ├── mod.rs
│       │   ├── openai_compatible.rs
│       │   └── fake.rs            # deterministic test provider, no network
│       ├── vector/
│       │   ├── mod.rs
│       │   └── exact.rs           # filtered bounded exact-search baseline
│       └── reconcile.rs           # restart/orphan/missing artifact recovery
├── cc-server/src/
│   ├── service_factory.rs         # composition root, lazy optional dependencies
│   ├── query_handle.rs            # Arc-owned snapshot inputs, no network under lock
│   ├── capability_status.rs       # advertised vs enabled vs ready states
│   ├── handlers/*                 # preserve 14 tools; additive strategy/diagnostics
│   ├── project_session.rs         # pin active query; idle close cooperates with worker
│   └── existing graph_*           # remain here per ADR-0001
└── cc-eval/
    ├── src/bin/cc-eval.rs         # development benchmark CLI, not product CLI
    ├── src/benchmark/            # one authoritative benchmark implementation
    │   ├── manifest.rs           # source/corpus/model/config/scoring locks
    │   ├── schema.rs             # queries, gold groups, result/error states
    │   ├── importer_oce.rs       # externally supplied OCE JSONL and metadata
    │   ├── adapters/             # real MCP stdio / optional OCE HTTP / rg
    │   ├── metrics.rs            # compatible + native scoring profiles
    │   ├── oracle.rs             # canonical full-vs-incremental comparison
    │   ├── mutations.rs          # isolated reproducible edit sequences
    │   ├── sampler.rs            # process-attributed resource/timing samples
    │   ├── statistics.rs         # paired deltas, macro/micro, confidence
    │   ├── report.rs             # raw -> JSON/Markdown, one result source
    │   ├── gate.rs               # valid/failed/invalid/cancelled exit states
    │   └── ablation.rs           # independent lanes/chunker/model comparisons
    ├── fixtures/index-v2/         # tiny multilingual & adversarial fixtures
    ├── benchmarks/               # schema/manifests/native/mutations/goldens
    └── tests/
        ├── incremental_equivalence.rs
        ├── module_resolution_matrix.rs
        ├── document_identity.rs
        ├── semantic_lifecycle.rs
        ├── mcp_v2_contract.rs
        └── index_v2_scale.rs
```

P9 仅在分项批准后增加：`cc-semantic/src/vector/<selected_backend>.rs`、`cc-server/src/precise_queries/*`、`cc-search/src/rerank/*`。没有“先建空目录以表示预留”的任务。

Benchmark 在 P0 即启动，完整文件树和接口规格以 [09-BENCHMARK.md](09-BENCHMARK.md) 为准。现有 eval runner/断言继续复用，新增评分原语统一归 `src/benchmark/metrics.rs`，不再另建平行 retrieval_metrics 实现。正式检索评测只走公开协议，白盒 oracle 仅验证增量事实，不向被测检索器提供 gold。

## 4. 当前 → 目标迁移映射

| 当前位置 | 迁移方式 | 不允许的回退 |
|---|---|---|
| SearchPlan 把 preselect 写入 file_paths | P1 先就地拆语义；P5 再组织 scope/query policy 模块 | 删除过滤导致显式范围泄漏 |
| preselect 中 BM25 转换 | P1 就地单调修复与回归 | 大改所有排序权重掩盖局部错误 |
| dirty_closure/dirty_reload_policy | P2 复用算法，增加 surface/dependency 输入；后续再归并到 incremental | 用全量 parse 替代所有 DirtyResolveOnly |
| import_resolver + cargo_workspace | P3 抽出项目模型，Rust 先保留现有通过场景 | 把简单路径拼接声称完整语言解析 |
| chunker.rs | P4 先增加边界产物与 SourceSpan，分步切换策略再拆文件 | 同时更换 AST 库、身份和检索算法 |
| StableId::chunk_id | 保留 legacy wire id，新增文档键/版本；分期弃用旧内部键 | 用序号作为异步结果发布依据 |
| lanes.rs/run_lanes | P5 候选/状态协议先改，本地实现继续同步 | 将现有同步 DB 全面 async 化 |
| UnitOfWork/epoch_rules | P6 显式 effect 类型扩展，默认旧路径仍 Index | raw SQL 绕开写接口，任务 heartbeat 刷全部搜索缓存 |
| index rebuild/schema mismatch | P6 外部派生缓存保留，索引 manifest 重建后对账 | 以索引重建为由自动清空已付费向量 |
| context/output_budget | P5 pack 前置、P7 status/additive options | 直接截断序列化 JSON 当成合法证据 |
| eval runner | P0 建适配/锁/scorer/oracle，每阶段扩充，P8 规模认证；legacy corpus 继续跑 | 延后 benchmark 到 P8，或仅测试名/数量通过就宣称语义正确 |

## 5. 资源所有权

SourceSnapshot 是本次读取的不可变字节与哈希，不承诺文件系统原子快照。FileCatalog 描述准入文件集合，跨文件变化以 generation 与复核收敛。语法树在单文件解析任务结束后释放；紧凑跨度可持久化。ProjectModel 按内容/配置指纹缓存；不在每个 import 上 stat。

构建仍每项目串行，解析内部有界并行。QueryHandle 只拷贝 Arc 配置/服务/DB身份及 generation，不携带 RwLock guard；跨网络等待前释放锁和读连接。语义服务有项目级共享的并发/费用限制；关闭或驱逐时停止接收新工作、取消有界等待、保留 outbox，不永久阻塞 MCP。

## 6. 明确不做的架构扩展

不建第二套全文/结构数据库、不增加 OCE Python 服务依赖、不把 CodeCortex 变为通用多模态 RAG、不新增 UI/租户/RBAC、不接管 Relay 会话和记忆、不让大模型生成不可校验的依赖边。将来 Relay 通过现有 MCP 或公共接口消费，不在本轮跨仓修改 Relay。
