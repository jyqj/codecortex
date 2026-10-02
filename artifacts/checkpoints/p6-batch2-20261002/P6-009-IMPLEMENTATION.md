# P6-009 实施记录：实现 deterministic fake provider

- 日期：2026-10-02
- 依据：`artifacts/checkpoints/p6-implementation-planning-20261002/TASK-BRIEFS.md`
  P6-009 节（确定性向量、脚本化故障注入、输出门禁、V15 对照）、ADR-0003 约束表
  P6-009 行（"worker 侧实现，永不触网；端口冻结前不得开工"）、
  `crates/cc-semantic/src/ports.rs`（`EmbeddingProvider` 冻结面 + `ProviderError`
  六变体）、`crates/cc-semantic/src/spec.rs`（`VectorSpace` 冻结构造/校验）、
  `crates/cc-semantic/src/cache.rs`（P6-008 交付，只调用不改）。
- 改动范围：`crates/cc-semantic` 2 个新文件 + 2 个既有文件修改 + `Cargo.toml`
  （cc-semantic）加 `blake3.workspace = true` + `Cargo.lock` 重生成 cc-semantic 依赖边。
  `ports.rs`/`cache.rs`/`spec.rs`/`types.rs` 零触碰；零 schema 变更；`cc-db` 零触碰；
  `tasks.json` status 未改；未 git commit。workspace 根 `Cargo.toml` 已含
  `blake3 = "1"`（先前会话改动），本轮未改它，只复用 workspace 依赖。

## 1. 文件:行号改动清单

| 文件 | 变更 | 内容 |
|---|---|---|
| `crates/cc-semantic/src/providers.rs` | 新增（17 行） | 模块文档（实现容器定位、永不触网声明、真实 provider 归 P7 线）+ `pub mod fake;` |
| `crates/cc-semantic/src/providers/fake.rs` | 新增（549 行） | 模块文档（确定性算法全文 + 故障脚本说明）+ `FAKE_PROVIDER_ALGORITHM_VERSION`(:63) + `FakeProviderConfig`(:78) + `FakeProvider::{new,call_count,embed}`(:118-176) + `EmbeddingProvider` 实现(:181-194) + `deterministic_vector`(:200) + `validate_output`(:231) + 16 个内联测试(:262-549) |
| `crates/cc-semantic/src/lib.rs:31` | 修改 | `pub mod providers;`；模块边界文档更新：fake provider 已落位（"its vectors carry no semantic quality claim"），vector 搜索逻辑（P6-010）与组合根接线仍缺位 |
| `crates/cc-semantic/Cargo.toml` | 修改 | 增加 `blake3.workspace = true`（workspace 已有 `blake3 = "1"`，无新外部依赖引入） |
| `Cargo.lock` | 变更 | `cargo update -p cc-semantic --offline` 重生成：`cc-semantic → blake3` 依赖边落盘 |

## 2. 确定性向量算法（模块文档原文，`fake.rs:27-48`）

> ## Deterministic vector algorithm (`FAKE_PROVIDER_ALGORITHM_VERSION = 1`)
>
> Same input digest under the same frozen space always yields the bitwise
> identical vector, on every machine, for the lifetime of algorithm version 1:
>
> 1. **Key material** (UTF-8, ASCII only, every field delimited):
>    `cc-semantic.fake-provider.v1/alg=1/kind={document|query}/model={model_id}/dim={dimension}/digest={digest}`.
>    The `kind` tag domain-separates the document and query paths (real
>    models embed asymmetric pairs differently too); `model_id` is part of
>    the space identity, so a different model never reproduces another
>    model's vectors.
> 2. **Keystream**: `blake3::Hasher::update(key_material)` then
>    `finalize_xof()`, `fill()`ing exactly `dimension * 4` bytes — the
>    official extendable-output function, not a homemade PRNG.
> 3. **Component mapping**: each little-endian `u32` is mapped to
>    `((u as f64) * (2 / 2^32) - 1)` — an *exact* f64 intermediate, then a
>    single IEEE-754 round-to-nearest cast to `f32`.
> 4. **Normalization**: the L2 norm is accumulated in `f64` by sequential
>    fold (`sqrt(sum(v*v))`) and each component divided by it (f64 divide,
>    one cast back to `f32`), placing the vector on the unit sphere as the
>    frozen `DistanceMetric::Cosine` requires. A degenerate all-zero
>    pre-normalization vector (probability ~0) is left as zero and caught by
>    the output gate below.
>
> Every arithmetic step is basic IEEE-754 (`*`, `-`, `/`, `sqrt`, defined
> integer→float casts) with fixed operand order — Rust performs no
> reassociation — so the result is bitwise reproducible cross-machine. Any
> change to this pipeline bumps [`FAKE_PROVIDER_ALGORITHM_VERSION`] and is a
> breaking change for recorded fixtures.

要点：向量以 **digest**（而非 bytes）为键，故"同 input 同 digest 必得同向量"由构造
成立；blake3 官方 XOF（`finalize_xof().fill`）做确定性展开，无自制 PRNG、无随机
种子、无时钟、无环境读取。golden fixture（`v1_pipeline_is_bitwise_reproducible_golden`）
钉死 `"fake/golden-model"`/dim=8/`"golden fixture input"` 的前四个 `f32::to_bits`
（`[3169967527, 1032920341, 3201138986, 1039597886]`），任何流水线漂移都会炸出并
强制走版本 bump。

## 3. 故障注入模式清单（`FakeProviderConfig`，供 P6-016 全故障矩阵复用）

| 字段 | 语义（精确规则） |
|---|---|
| `fail_after_n_calls: Option<usize>` | 调用序数（1 起，跨 `embed_documents`/`embed_queries` **两方法合并计数**，一批 = 一次调用，**失败调用也计数**）超过 n 后每次调用都失败；`None` = 永不触发 |
| `fail_with: Option<ProviderError>` | 触发时注入的错误；故障已武装但本字段 `None` 时默认 `ProviderError::ServerError` |
| `delay_per_call: Duration` | 每次调用的人为延迟，成功/失败路径均生效（默认 `ZERO`） |
| `zero_vector_digests: Vec<String>` | 命中 digest 的向量被故意产出为全零 → 输出门禁拒绝为 `InvalidInput`（digest 存 hex 字符串，同时覆盖 document(`InputDigest`)/query(`QueryDigest`) 两条路径） |
| `call_count()` | 累计调用计数器（P6-014 断言"provider 调用 0 次"用；`AtomicUsize`，线程安全） |

`ProviderError` 六变体（`RateLimited{retry_after}`/`ServerError`/`AuthError`/
`Timeout`/`Cancelled`/`InvalidInput`）全部可经 `fail_with` 注入且原样返回
（测试 11 逐一验证）。输出门禁 `validate_output`（fake.rs:231）：维度不符 /
NaN/Inf（报 index）/全零（报 digest）→ `InvalidInput`，坏向量到不了 cache（V15
门禁语义，与 P6-008 `ArtifactCache::put` 的 cache 侧校验双层设防）。

## 4. 测试清单（`fake.rs` 内联 `#[cfg(test)]`，16 个，全绿）

1. `same_digest_yields_bit_identical_vectors_across_calls_and_instances`——同 provider
   两次调用 + 独立实例，`f32::to_bits` 逐位相等。
2. `different_digests_yield_different_vectors`——异输入异向量。
3. `vectors_are_unit_length_for_the_frozen_cosine_metric`——dim=64 范数≈1（f64 判）。
4. `v1_pipeline_is_bitwise_reproducible_golden`——算法 v1 golden fixture
   （前四分量 to_bits 钉死；漂移即强制版本 bump）。
5. `space_contract_dimension_and_identity_hold`——`space()` 回构型一致、向量长=维度。
6. `model_id_and_dimension_are_part_of_the_derivation`——同维度异模型不串（V16 第一层）。
7. `batch_order_is_preserved_on_both_paths`——batch 输出 i == 单输入调用（逐位），
   两路径各验；不拆批。
8. `document_and_query_paths_are_domain_separated`——同字节 doc≠query（kind tag）。
9. `empty_batch_returns_empty_success`——空批空成功。
10. `fault_fires_after_n_calls_with_the_configured_error`——第 1 次成功、第 2 次起
    `RateLimited` 精确相等、跨方法持续、失败也计数。
11. `armed_fault_without_error_defaults_to_server_error`——默认错误 ServerError。
12. `full_provider_error_matrix_is_injectable_and_distinguishable`——六变体逐一
    注入→原样返回（P6-016 复用面）。
13. `zero_vector_injection_is_rejected_by_the_output_gate`——单毒输入
    `InvalidInput`（点名 digest）；含毒批次整体失败不回部分向量；query 路径同覆盖。
14. `delay_is_applied_per_call`——5ms 延迟实测下界。
15. `config_defaults_are_naturally_successful`——默认脚本零故障/零延迟/零注入。
16. `embed_put_get_roundtrip_is_bitwise_lossless`——cache 回路：embed→`put`→`get`
    Hit→数据逐位相等 + ref/dimension 一致（临时目录，自清理）。

无效 `VectorSpace` 在类型层不可构造（字段私有 + sanctioned 构造器校验），故构造器
内 `validate()` 为 belt-and-braces，未为其写不可构造场景的测试（spec.rs 已覆盖
`VectorSpace::new` 拒绝逻辑）。

## 5. 验证结果（原样）

```
SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo test -p cc-semantic --locked --offline
running 38 tests  ... test result: ok. 38 passed; 0 failed   （lib：P6-002/003/008/009 既有 22 + 本轮 16）
running 17 tests  ... test result: ok. 17 passed; 0 failed   （artifact_cache.rs，零改动回归）
running 0 tests   ... test result: ok. 0 passed              （doc-tests）

SDKROOT=/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk \
  cargo check --workspace --locked --offline
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 0.55s   （零 warning）
```

默认依赖图复查（2026-10-02 收口轮订正：原命令 flag 无效，结论经有效命令复验成立；
`--no-dev-deps` 非 `cargo tree` 有效 flag，而 `--no-dev-dependencies` 与 `-e normal`
互斥，故采用 `-e normal` 口径）：真实输出 `cargo tree -p cc-server -e normal |
grep -c cc-semantic` = 0；`cargo tree --workspace -e normal` 中 cc-semantic 仅以
workspace 成员根行出现 1 次，0 条依赖边指向它（cc-semantic 仅经未启用的
`semantic` feature 可达，blake3 只进 cc-semantic 自身依赖）。

## 6. 偏差清单（对简报接口草案）

1. **`zero_vector_digests: Vec<String>`**（草案：`Vec<InputDigest>`）：草案类型只能
   覆盖 document 路径，query 路径的 `QueryDigest` 是不同新类型，无法装入同一
   `Vec<InputDigest>`；存 digest hex 字符串天然覆盖两条路径，语义不变，已在类型
   文档写明。
2. **`fail_with: None` 且故障已武装时默认 `ServerError`**：草案未定义两字段组合的
   语义缺口，取文档化默认而非 panic；精确规则（序数 1 起、跨方法合并、失败也计数）
   写入字段文档与测试 10。
3. **新增 `call_count()` 公开计数器**：草案没有，但 P6-014 验收明确需要"fake 计数器
   验证 provider 调用 0 次"，属简报下游任务的既有需求前移，非冻结面改动（不触碰
   ports.rs）。
4. **模块落位 `providers.rs` + `providers/fake.rs`**（2021 edition 无 mod.rs 布局），
   与 P6-002 草案 `mod providers { pub mod fake; }` 结构一致。
5. **向量以 digest 为键而非草案的 `blake3(input_bytes + salt)`**：digest 即
   `blake3(bytes)`（P6-003 已把 digest↔bytes 绑定在构造路径收口），以 digest 为键
   使"同 digest 同向量"由构造成立且少一次冗余哈希；key material 同时纳入
   `model_id`/`dimension`/`kind`/算法版本（域分隔 + 空间身份），比裸 `bytes+salt`
   更强——`model_id` 是空间身份（ADR"同维度不同模型不可混用"）的直接体现。
6. **未实现 P6-015 需要的 crash 断点钩子**（简报 P6-015 风险段"`crash_after_persist`
   之类"）：简报明确该钩子属 P6-015 对 `FakeProviderConfig` 的扩展且涉及 ports 冻结
   时机，本轮不预做。
