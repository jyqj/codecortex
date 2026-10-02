# P6-003 实施记录：冻结编码空间与输入规范（含 P6-002 评审必改两处）

- 日期：2026-10-02
- 依据：`artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md` P6-003、
  `docs/adr/0003-semantic-persistence-single-db-boundary.md`（ADR-0003）、
  `artifacts/checkpoints/p6-batch1-20261002/P6-002-IMPLEMENTATION.md` 及其评审结论。
- 规格文档（权威）：`crates/cc-semantic/docs/ENCODING-SPACE.md`。

## 0. P6-002 评审必改（两处，均落地）

1. **`QueryDigest` 语义修正**（原 `types.rs` 误标 "query encoding spec digest"，与
   `ports.rs` 的输入字节用法冲突）：按简报三分语义理顺为五类不透明 digest——
   `SpaceDigest`（空间身份）/ `DocSpecDigest`（文档 spec）/ **新增 `QuerySpecDigest`**
   （查询 spec）/ `InputDigest`（文档输入字节）/ `QueryDigest`（查询输入字节）。
   `QueryDigest` 文档注释改为"query-path analog of InputDigest，不是 spec digest"，
   并交叉引用 `QuerySpecDigest`。冻结面签名（`QueryInput.digest: QueryDigest`）不变。
2. **vacuous assert 替换**（原 `types.rs:129` `assert_ne!(format!("{s}"), format!("{:?}", i))`
   Display vs Debug 近乎恒真）：重写为有意义的域分离断言——同一段文本经不同 digest
   域（`VectorSpace::digest()` vs `InputDigest::of_input`）必须得到不同值，测试名
   `digests_are_opaque_and_distinct_by_construction` 保留，移入 `spec.rs`。

## 1. 文件清单

| 文件 | 变更 | 内容 |
|---|---|---|
| `crates/cc-semantic/src/spec.rs` | 新增 | 冻结面主体：常量、`DistanceMetric`/`VectorSpace`/`DocumentEncodingSpec`/`QueryEncodingSpec`（私有字段 + 校验构造器）、`validate_input_bytes`/`input_bytes_digest`、三分 digest 公式、14 个单测 |
| `crates/cc-semantic/src/types.rs` | 重写 | 不透明 digest 宏构造器收口（`new` → `pub(crate)`）；`QueryDigest` 语义修正；新增 `QuerySpecDigest`；`InputDigest::of_input`/`QueryDigest::of_input`；`VectorSpace`/`DistanceMetric` 移入 spec.rs 并在此 re-export（`crate::types::` 路径不变）；vacuous assert 替换 |
| `crates/cc-semantic/src/ports.rs` | 修改 | `DocumentInput::from_bytes`/`QueryInput::from_bytes`/`verify()` 收口构造器；测试改走收口路径；新增红绿测试 `input_constructors_bind_digest_to_exact_bytes`；trait 签名零改动 |
| `crates/cc-semantic/src/lib.rs` | 修改 | 注册 `pub mod spec`；crate 文档同步（P6-003 已冻结） |
| `crates/cc-semantic/Cargo.toml` | 修改 | 追加 `serde.workspace = true`（serde 本就在 lock 与依赖图内，零新外部包版本） |
| `Cargo.lock` | 自动再生 | `cc-semantic` 节点追加 `serde` 依赖（+1 行）；零新外部包 |
| `crates/cc-semantic/docs/ENCODING-SPACE.md` | 新增 | 冻结规格文档（冻结面清单、digest 公式、版本化策略、偏差记录） |

冻结面变更自查（红线）：`types`/`ports` 签名层面仅 `QueryDigest` 文档语义理顺 +
`VectorSpace` 按简报移入 `spec.rs`（原路径 re-export 保持）；无新能力引入——
无 provider、无存储、无 IO、无 `try_init`（延续 P6-002 边界）。

## 2. 冻结面清单（实质内容）

常量：`ENCODING_SPEC_VERSION=1`、维度 `[1,65536]`、`MAX_MODEL_ID_BYTES=512`、
`MAX_TOKENIZER_BYTES=256`、`MAX_INSTRUCTION_BYTES=4096`、`max_tokens ∈ [1,32768]`、
`MAX_INPUT_BYTES=1MiB`。

校验规则：`VectorSpace::validate`（model_id 非空/≤512/无首尾空白、维度区间、
metric 编译期准入、版本等于冻结版本）；spec 字段共用校验（tokenizer、max_tokens、
instruction）；`validate_input_bytes`（非空、≤1MiB、合法 UTF-8）。失败统一
`SemanticError::InvalidInput` → `CcError::InvalidParams`。

digest 公式：规格类 `blake3(canonical json (DOMAIN_TAG, spec))`（复用
`cc_model::identity::hash`）；输入类 `blake3(bytes)` 经输入门（复用
`cc_model::identity::bytes_hash`）。三个 domain tag 使跨域碰撞结构性不可能。

## 3. digest 收紧前后对照

| 面 | P6-002 骨架（收紧前） | P6-003（收紧后） |
|---|---|---|
| `opaque_digest!::new` | `pub`，任意字符串可造任意 digest | `pub(crate)` 逃逸口；公开面只有下列验证路径 |
| `InputDigest`/`QueryDigest` | 无构造约束（诚实边界） | `of_input(bytes)`：过 `validate_input_bytes` 后 `blake3(bytes)` |
| `SpaceDigest` | 无构造约束 | 仅 `VectorSpace::digest()`，构造前强制 `validate()`（含版本门） |
| `DocSpecDigest`/`QuerySpecDigest` | 仅 `DocSpecDigest` 存在、无约束 | 仅各自 spec 的 `digest()`，先过全字段校验；二者为不同类型，跨类型比较是编译错误 |
| ports 输入 | `DocumentInput`/`QueryInput` 字面量可拼出 digest↔bytes 漂移 | `from_bytes`/`verify()`：绑定在构造期成立、可复检 |
| `ArtifactRef` | `pub new` | 无公开构造器（P6-008 cache 构造） |
| `VectorSpace` | pub 字段字面量可造任意值 | 私有字段 + `new` 校验构造器（stamp 冻结版本与唯一 metric） |

## 4. 测试结果（原样）

`SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk cargo test -p cc-semantic --locked --offline`
→ `22 passed; 0 failed; 0 ignored`（doc-tests `0 passed`），零 warning。

P6-002 既有 10 测试共存情况（名称全部保留，部分随类型归属移至 `spec.rs`，两处
按简报更新构造路径）：`handle_is_a_zero_sized_marker`、
`same_model_same_dimension_is_same_space`、
`same_dimension_different_model_is_a_different_space`（新增 digest 层断言）、
`digests_are_opaque_and_distinct_by_construction`（vacuous assert 替换）、
`cosine_is_the_only_admitted_metric`、
`embed_documents_preserves_batch_order_and_count`、
`embed_queries_maps_one_output_per_query`（改 `QueryInput::from_bytes`）、
`provider_error_variants_are_distinguishable`、`port_is_object_safe_and_send_sync`、
`invalid_input_maps_to_invalid_params`。

新增 12 测试：正例 `valid_boundary_values_pass`、
`input_digests_are_deterministic_and_distinct`、`digest_is_stable_across_reconstruction`、
`digest_display_matches_inner_string`、`input_constructors_bind_digest_to_exact_bytes`
（红绿：字面量构造路径已删除，绑定不可漂移）；反例 `zero_and_oversized_dimensions_are_rejected`、
`bad_model_ids_are_rejected`、`foreign_spec_versions_are_rejected`、
`bad_encoding_spec_fields_are_rejected`、`invalid_input_bytes_are_rejected`、
`input_digests_reject_invalid_bytes`；三分语义
`query_only_change_never_touches_the_document_spec_digest`。

## 5. 验证命令（原样）

1. `cargo check -p cc-semantic --locked --offline`
   → `Finished \`dev\` profile ... in 0.25s`（零 warning）。
2. `cargo check --workspace --locked --offline`
   → `Finished \`dev\` profile ... in 0.53s`（通过）。
3. `cargo check -p cc-server --features semantic --locked --offline`
   → `Finished \`dev\` profile ... in 3.09s`（feature 开启可编译）。
4. `cargo test -p cc-semantic --locked --offline`（SDKROOT 前缀规避 CLT/SDK 链接问题，
   同 P6-002 记录 §4）→ 22 passed / 0 failed。

红线自查：`git status` 中 `crates/` 下除 `cc-semantic/` 外的 modified 文件均为本
会话开始前已存在的改动，本轮零新增；`tasks.json` status 未改；未 git commit；冻结链
artifacts 未触碰。简报与 ADR 无冲突点（简报 P6-003 即 ADR 约束表的直接展开）。

## 6. 与简报偏差清单

1. `digest()` 返回 `CcResult<_>`（简报草案为裸返回）：serde_json 序列化存在失败路径，
   照工作区错误约定走 `CcError`。
2. `VectorSpace`/两类 spec 字段私有 + 校验构造器（简报草案 pub 字段）：冻结面构造后
   不可漂移要求收口；P6-009 经 `VectorSpace::new` 构造不受影响。
3. `VectorSpace` 落 `spec.rs` 并在 `types.rs` re-export：简报归属如此，P6-002 偏差
   记录预告过该动作；原 `crate::types::` 路径经 re-export 保持，ports 冻结签名零变化。
4. 新增 `serde` 依赖（workspace 内既有包）：digest 的 canonical JSON 公式需要
   `Serialize` derive；`Cargo.lock` 零新外部包版本。
5. 简报草案中的 `QuerySpecDigest` 落为独立新类型而非改名 `QueryDigest`：`QueryDigest`
   在 ports 冻结面（P6-009 草案 `QueryInput.digest`）中承担查询输入字节 digest 角色，
   改名会动冻结面；三分语义由新类型 + 文档修正达成。

## 7. 未做与剩余风险

- P6-008 落地时 `ArtifactRef` 的构造器在 cache 内启用（当前无公开构造器是刻意留白）。
- `ENCODING_SPEC_VERSION` 的首次 bump 流程未经实战（v1 即首个冻结版本）；策略见
  `ENCODING-SPACE.md` §4，P6-017 空间切换实施时如与 bump 交互需回读该节。
- V10/V16 的正式验证证据（`artifacts/benchmarks/<run-id>/`）属实施期/收口轮产物；
  本轮只产出命令级测试证据，未标任何 tasks.json status。
