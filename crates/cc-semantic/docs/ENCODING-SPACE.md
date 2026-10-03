# 编码空间冻结规格（spec v1）

- 状态：**冻结**（spec v1，`ENCODING_SPEC_VERSION = 1`）
- 边界权威：`docs/adr/0003-semantic-persistence-single-db-boundary.md`（ADR-0003 约束表
  P6-003 行："VectorSpace/DocumentEncoding/QueryEncoding 三分与完整 digest 是 cache
  namespace 验证的前提；同维度不同模型不可混用"）
- 实现落点：`crates/cc-semantic/src/spec.rs`（常量、类型、校验、digest 公式）、
  `crates/cc-semantic/src/types.rs`（不透明 digest 新类型，构造已收口）
- 任务依据：`artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md` P6-003

## 1. 冻结面清单

### 1.1 常量

| 常量 | 值 | 含义 |
|---|---|---|
| `ENCODING_SPEC_VERSION` | `1` | 规格版本；进入 `SpaceDigest` |
| `MIN_DIMENSION` / `MAX_DIMENSION` | `1` / `65_536` | 合法维度闭区间 |
| `MAX_MODEL_ID_BYTES` | `512` | `provider/model` 全名长度上界（字节） |
| `MAX_TOKENIZER_BYTES` | `256` | tokenizer 标识长度上界 |
| `MAX_INSTRUCTION_BYTES` | `4_096` | instruction 存在时的长度上界 |
| `MIN_MAX_TOKENS` / `MAX_MAX_TOKENS` | `1` / `32_768` | `max_tokens` 合法闭区间 |
| `MAX_INPUT_BYTES` | `1_048_576` | 嵌入输入字节上界（1 MiB） |

### 1.2 类型

- `DistanceMetric`：封闭枚举，v1 仅 `Cosine`。新增变体必须与版本 bump 同轮落地
  （`VectorSpace::validate` 的 `match` 是编译期准入门）。
- `VectorSpace`：`{model_id, dimension, distance, spec_version}`，字段私有。
  **四字段全部进入身份**：`SpaceDigest` 对域标签与完整冻结 tuple 进行哈希。
  维度相同不足以判定空间相同；同维度不同 `model_id` 或同模型不同维度均
  产生不同 `SpaceDigest`，不能混用。这遵循 ADR-0003 的完整 digest 约束。
  此处订正旧文字“dimension 不进”；既有 v1 序列化、digest 公式和缓存布局不变。
- `DocumentEncodingSpec` / `QueryEncodingSpec`：同形四字段
  `{space, instruction, max_tokens, tokenizer}`，**刻意为两个不同类型**：
  query 侧任何变化只改 `QuerySpecDigest`，永不触碰 `DocSpecDigest`
  → 文档不重嵌。
- 不透明 digest 新类型：`SpaceDigest` / `InputDigest` / `DocSpecDigest` /
  `QueryDigest` / `QuerySpecDigest` / `ArtifactRef`。

### 1.3 三分 digest 语义（含输入字节 digest 共五类）

| digest | 标识对象 | 唯一公开构造路径 |
|---|---|---|
| `SpaceDigest` | 冻结向量空间身份 | `VectorSpace::digest()` |
| `DocSpecDigest` | 文档编码 spec | `DocumentEncodingSpec::digest()` |
| `QuerySpecDigest` | 查询编码 spec | `QueryEncodingSpec::digest()` |
| `InputDigest` | 文档路径实际嵌入输入字节 | `InputDigest::of_input(bytes)` |
| `QueryDigest` | 查询路径实际嵌入输入字节 | `QueryDigest::of_input(bytes)` |

`QueryDigest` 与 `QuerySpecDigest` 是两个不同概念：前者描述"这次嵌了什么字节"，
后者描述"查询侧编码规格是什么身份"。P6-002 骨架期 `QueryDigest` 的文档注释曾误标
为 query encoding spec digest，已按本三分语义修正（评审必改项 1）。

`ArtifactRef` 唯一公开构造路径是 P6-008 的 `ArtifactCache::put`
（`cas.v1:<namespace>:<space_id>:<input>:<spec>:<checksum>`，见 `src/cache.rs`）。

### 1.4 输入字节规范（v1）

`validate_input_bytes`：嵌入输入必须同时满足

1. 非空；
2. 长度 ≤ `MAX_INPUT_BYTES`；
3. 合法 UTF-8（v1 输入是规范化文本；超界是调用方 bug，不静默截断）。

`InputDigest`/`QueryDigest` 只能从通过该门的字节构造：
`digest = blake3(bytes)`。`DocumentInput::from_bytes` / `QueryInput::from_bytes`
（ports 侧收口构造器）保证 `digest ↔ bytes` 绑定在构造期不可漂移，`verify()`
可复检。

## 2. 校验规则汇总

- `VectorSpace::validate`：`model_id` 非空、≤512 字节、无首尾空白；`dimension ∈
  [1, 65536]`；metric 经编译期准入；`spec_version == ENCODING_SPEC_VERSION`。
- spec 字段校验（两类 spec 共用）：`tokenizer` 非空 ≤256 字节；
  `max_tokens ∈ [1, 32768]`；`instruction` 存在时非空 ≤4096 字节；space 先过
  `VectorSpace::validate`。
- 所有校验失败映射 `SemanticError::InvalidInput` → `cc_model::CcError::InvalidParams`。

## 3. digest 公式与 canonical 序列化

- 规格类：`blake3(serde_json(to_vec((DOMAIN_TAG, spec))))`，即
  `cc_model::identity::hash`；输入字节类：`blake3(bytes)`，即
  `cc_model::identity::bytes_hash`。
- domain tag 使每个公式自描述：`cc-semantic.vector-space.v1` /
  `cc-semantic.document-encoding-spec.v1` / `cc-semantic.query-encoding-spec.v1`。
  同一段文本走不同 digest 域必然得到不同值。
- canonical 化规则（照 C03 `02-CONTRACTS.md:21`）：serde 对 struct 按声明字段序
  序列化、无任何 map 类型参与；域标签 + tuple 序列化顺序固定。digest 对同一输入
  恒定（`digest_is_stable_across_reconstruction` 守护）。

## 4. 版本化策略（冻结后如何 bump）

1. `ENCODING_SPEC_VERSION` 是 `SpaceDigest` 的组成字段：bump → 所有新空间身份
   变化 → 新 `SpaceDigest` → 新 cache 命名空间分支，旧空间产物在其旧 space id 下
   依旧可校验、可复用（付费产物不因版本 bump 失效，回收走 P6-016 GC 保留期）。
2. 允许 bump 的最小集合：新增 `DistanceMetric` 变体、放宽/收紧任一常量区间、
   变更输入字节规范、变更任一 digest 公式或字段集合。
3. 不允许的事情：在 bump 之外原地修改上述任何冻结面；让旧实现继续产生新版本的
   digest（版本字段进 digest 使这不可行）；把"校验放行"当兼容手段。
4. P6-017 空间切换是**运行时**多空间并存机制（`semantic_spaces`
   backfilling→active→revoked），与本节的**规格**版本 bump 正交：切空间不 bump
   规格，bump 规格必然产生新空间。

## 5. 与简报/ADR 的偏差记录

- `digest()` 返回 `CcResult<SpaceDigest>` 而非裸值：digest 公式经 serde_json，
  失败路径存在，照工作区错误约定走 `CcError`（简报草案为裸返回）。
- `VectorSpace` 等字段私有 + 校验构造器：简报草案为 pub 字段；冻结面的完整性
  （构造后不可漂移）要求收口，P6-009 provider 经 `VectorSpace::new` 构造不受影响。
- P6-002 偏差延续：`try_init`/cache 路径仍不存在（P6-008 落地），本规格不改该边界。
