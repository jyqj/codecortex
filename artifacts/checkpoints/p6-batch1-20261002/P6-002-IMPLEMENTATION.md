# P6-002 实施记录：新增可选 cc-semantic 骨架

- 日期：2026-10-02
- 依据：`artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`（P6-002 简报）、
  `artifacts/checkpoints/p6-implementation-planning-20261002/IMPLEMENTATION-ORDER.md`（批次 1）、
  边界权威 `docs/adr/0003-semantic-persistence-single-db-boundary.md`（ADR-0003）。
- **F0 后首批声明**：本批是批次 0（P5-020 收口 + F0 冻结闭包）之后第一批合法 `crates/` 改动；
  批次 1 结束需在新 SHA 重冻结（F1），批内所有提交共享一次 F1 重认证。

## 1. 文件清单与模块结构

| 文件 | 变更 | 内容 |
|---|---|---|
| `crates/cc-semantic/Cargo.toml` | 新增 | 依赖仅 `cc-model` + `thiserror`（workspace dep，无新外部包） |
| `crates/cc-semantic/src/lib.rs` | 新增 | crate 边界文档；`SemanticHandle` 零尺寸占位 + 契约测试 |
| `crates/cc-semantic/src/types.rs` | 新增 | 不透明 digest 新类型 `SpaceDigest`/`InputDigest`/`DocSpecDigest`/`QueryDigest`/`ArtifactRef`；`VectorSpace`/`DistanceMetric` 骨架 |
| `crates/cc-semantic/src/ports.rs` | 新增 | `EmbeddingProvider` trait（照 P6-009 冻结面草案签名）、`DocumentInput`/`QueryInput`/`ProviderError` |
| `crates/cc-semantic/src/error.rs` | 新增 | `SemanticError::InvalidInput` → `cc_model::CcError::InvalidParams` 映射 |
| `Cargo.toml`（根） | 修改 | workspace members 追加 `crates/cc-semantic` |
| `Cargo.lock` | 自动再生 | 仅新增 `cc-semantic` 包节点（9 行），零新外部依赖 |
| `crates/cc-server/Cargo.toml` | 修改 | optional dep `cc-semantic` + `[features] semantic = ["dep:cc-semantic"]`，默认不启用 |

模块划分对照简报：`spec/queue/cache/providers/vector` 分别由 P6-003/007/008/009/010 落位，
本轮不建空占位模块（避免"存在的空模块"被误读为已实现）；`ports/types/error` 为本轮骨架面。

## 2. 与简报接口草案的偏差（以现有代码/验收事实为准）

1. **`try_init` 未定义**。简报草案含 `pub fn try_init(&ProjectConfig) -> Option<SemanticHandle>`，
   但简报同节验收明文"try_init 与任何 cache 路径在 P6-008 前不存在"。取验收文本为准：
   `SemanticHandle` 保留为零尺寸占位（含真实构造将随 P6-008 cache 落地的说明），`try_init`
   推迟到 P6-008 + 组合根接线轮。此偏差已记录于 `lib.rs` 模块文档。
2. **依赖面收窄为仅 `cc-model`**。ADR 约束"只依赖 cc-model/cc-db"是上界；骨架无任何存储/队列
   逻辑，引入 `cc-db` 为未用依赖。`cc-db` 由 P6-007/008（queue/cache）按需加入。
3. **`VectorSpace` 落在 `types.rs` 而非 `spec.rs`**：仅为让 `EmbeddingProvider::space()` 按草案
   签名返回真实类型；P6-003 冻结时将移入/固化到 `spec.rs`（`types.rs` 文档已注明）。
4. `cc-server` 侧本轮零接线（P6-004 才接 write effects；`service_factory.rs` 的
   `set_semantic` 组合根挂接点不动）。

## 3. 与 ADR-0003 约束逐条对照

| ADR 约束（约束表第 138 行及 Decision Drivers） | 本轮落实 | 证据 |
|---|---|---|
| 只依赖 `cc-model`/`cc-db` | `cc-semantic/Cargo.toml` 仅 `cc-model` + `thiserror`；`Cargo.lock` 零新外部包 | lock diff 9 行 |
| feature + 组合根延迟初始化 | cc-server 声明 optional dep + `semantic` feature，默认不启用；无任何组合根改动 | `crates/cc-server/Cargo.toml` |
| 默认编译不含语义实现（V21 默认/semantic 两包口径） | `cargo tree -p cc-server -e normal`（默认 feature）不含 `cc-semantic`（grep 计 0）；`--features semantic` 计 1 | 本记录 §4 |
| 默认编译/启动不生成空 cache 目录 | crate 内 0 处 `std::fs`/`mkdir`/文件系统调用（grep 计 0）；无 `try_init`，启动路径零改动 | grep 证据 |
| artifact cache 属派生产物，不在库外成为第二权威存储 | 骨架未实现任何存储；`ArtifactRef` 仅是占位新类型 | §1 |
| 不含秘密、不含网络调用 | 无网络依赖 crate、无 IO、无配置读取；ports 文档明示"never touch the network" | §1 |
| `capability_status.rs` 现有 `semantic_state` 行为不变（V18） | `capability_status.rs` 零触碰 | git diff |
| cc-eval/cc-search 零触碰（批次 1 冻结面最小化） | 两 crate 本轮零改动 | git status |
| 不动 formal-v4/final-v8/v5/v6 冻结链与 p5e-g5-freeze | 仅新增文件 + workspace 清单，未触碰任何冻结证据目录 | §1 |

## 4. 验证结果（原样）

1. `cargo check --workspace --locked --offline`
   → `Finished \`dev\` profile [unoptimized + debuginfo] target(s) in 4.27s`（通过）。
2. `cargo test -p cc-semantic --locked --offline`
   → `10 passed; 0 failed; 0 ignored`（doc-tests 0 passed）。测试清单：
   `handle_is_a_zero_sized_marker`、`same_model_same_dimension_is_same_space`、
   `same_dimension_different_model_is_a_different_space`（"同维度不同模型不可混用"第一层契约）、
   `digests_are_opaque_and_distinct_by_construction`、`cosine_is_the_only_admitted_metric`、
   `embed_documents_preserves_batch_order_and_count`、`embed_queries_maps_one_output_per_query`、
   `provider_error_variants_are_distinguishable`、`port_is_object_safe_and_send_sync`、
   `invalid_input_maps_to_invalid_params`。
3. 默认包依赖图：`cargo tree -p cc-server -e normal --offline | grep -c cc-semantic` → `0`；
   `cargo tree -p cc-server --features semantic -e normal --offline | grep -c cc-semantic` → `1`。
4. `cargo check -p cc-server --features semantic --locked --offline`
   → `Finished \`dev\` profile ... in 10.52s`（feature 开启可编译）。

**环境注意事项（非本任务代码问题）**：本机默认 SDK 为 `MacOSX27.0.sdk`，其 tbd 文件含
`arm64e.x1` 架构字段，当前 CLT 链接器无法解析，导致任何 Rust **测试二进制**链接失败。
本轮以 `SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk` 前缀运行测试规避；
未修改仓库任何构建配置。后续收口轮跑全量测试需沿用该规避或先修复 CLT/SDK 配套。

## 5. 未做与剩余风险

- P6-003 规格文档（编码空间冻结）属下一任务，本轮不做。
- P6-002 的 V18/V21 验证证据（`artifacts/benchmarks/<run-id>/`）属实施期/收口轮产物，
  本轮仅产出上列命令级证据，未标任何 tasks.json status。
- `tasks.json` status 未改（红线）：P6-001 裁决写入其 `implementation_notes`；
  P6-002 亦未翻状态，留审计收口轮。
- `SemanticHandle` 与 digest 新类型构造器当前不做内容校验（骨架诚实边界），P6-003 冻结
  canonical 编码时必须收紧，否则将在 V10/V16 验证中暴露。
