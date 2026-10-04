# 声明地址纯模型：有限资源契约

基线：`e4a8df4cbc6dfae29af8cb0eac9ee83fbba4d696`。参考独立审查
`8d5282acbc0ff630b37429c258968bb07090ba65`。仅修改 cc-model 的声明地址模块、
资源测试和本文；无 production capture、AST/config 语义验证、DB/MCP 或查询接线。

## API 与上游适配

显式入口为 `DeclarationSnapshot::with_limits(owner, files, roots, limits)`，返回
`DeclarationResult<DeclarationSnapshot<F>>`。`F` 仅允许
`BTreeMap<String, Vec<u8>>` 或 `&BTreeMap<String, Vec<u8>>`，trait 已封闭：
移入 owned inventory 不复制 bytes；shared borrow 不复制 bytes，也使原 map 在 snapshot
存活期间不能被安全 Rust 修改。模型保留不可变 digest map 与规范化 roots。不能使用会随调用
改变内容的自定义 provider。借用 snapshot 的类型为
`DeclarationSnapshot<&BTreeMap<String, Vec<u8>>>`；原 owned 类型名保持默认参数兼容。

```rust
let limits = DeclarationLimits {
    max_files: 200,
    max_total_bytes: 2 * 1024 * 1024,
    max_file_bytes: 256 * 1024,
    max_roots: 1,
    max_evidence: 16,
    max_ancestry_depth: 32,
    max_identifier_bytes: 128,
};
let snapshot = DeclarationSnapshot::with_limits(owner, &inventory, roots, limits)?;
let outcome = snapshot.resolve_with_limits(&input)?;
```

以上数字仅为调用示例，不是生产容量建议。每个调用方必须选择全部有限上限，没有 unlimited
哨兵或缺省 production admission。零是有效上限，只允许相应维度为空。
`limits.check_inventory_sizes(file_count, sizes)` 可在 capture 分配 bytes 前做长度预检；它要求
sizes 数量与 file_count 完全一致，返回 checked 总数。这不是 capture 实现，也不能证明完整性。
实际构造仍对收到的 map 重检，不能信任预检凭据。BTreeMap 上游覆盖重复 key 的问题不变。

`DeclarationInput`、`DeclarationSegment`、`ConfiguredRoot` 字段全部不变；AST adapter 继续产生
原 input，模型仅验证 byte/range assertion。`resolve_with_limits(&DeclarationInput)` 和
`is_current_with_limits(&BoundDeclaration)` 使用 snapshot 已选择的 limits，返回 typed error。
这些名字表示遵循 snapshot policy，不能逐调用放宽 policy；更换 policy 必须重新 admission。

兼容入口 `new(owner, owned_files, roots)` 仍返回 `CcResult`，明确采用有限
`DeclarationLimits::PROTOTYPE`：4096 files、64 MiB 总 logical bytes、8 MiB 每文件、64 roots、
256 package evidence、256 ancestry 段、4096 bytes 每 identifier。旧 `resolve` / `is_current`
保留 `CcResult`，resource refusal 映射到 `CcError::InvalidParams` 文本；新适配器要用 typed 入口。
兼容入口只供当前未接线原型，生产适配器必须显式选择 limits，不能把这些数字当认证容量。

## 维度、次序与拒绝

| 维度 | 精确计量与检查 |
|---|---|
| max_files | 完整 inventory 的 map.len()，空文件也计数；先检查，超出不遍历 sizes。 |
| max_total_bytes | 所有 inventory bytes.len() 的 checked_add 总和，包含无关文件/config/marker。 |
| max_file_bytes | 每个 inventory bytes.len()；不检查 Vec capacity，绝不使用 wrapping/saturating。 |
| max_roots | supplied ConfiguredRoot 条数，重复 roots 也计数；在大小遍历前检查。 |
| max_evidence | 单次 resolve 的 source-root 下 package 祖先 marker 条数；预先按规范化 relative path 计数，在分配 module 和访问 package evidence 前检查。root/config 另由 max_roots 控制，target/source 不计作 package marker。 |
| max_ancestry_depth | input.ancestry.len()；在归一化路径、遍历名字或 ranges、克隆输出前检查。 |
| max_identifier_bytes | 每个 lexical name、package component 和目标 module stem 的 UTF-8 bytes.len()；不是字符数，不是所有 identifier 的总和。initializer 的 __init__.py 不产生模块 stem，不计该固定哨兵。 |
| 固定 metadata ceiling | owner、inventory path、raw root directory/config_path/directive/config_digest、raw input path/source_digest 每项至多 4096 bytes；归一化前检查，避免巨大的 alias spelling 引起中间分配。 |

每项 `actual == limit` 通过，`actual > limit` 返回
`DeclarationError::Resource(ResourceRefusal::LimitExceeded { resource, limit, actual })`。
整数溢出返回 `ArithmeticOverflow { resource }`。原路径/证据错误包装为 `DeclarationError::Model`；
原 `IdentityOutcome::Unavailable` 语义保留。资源检查可以先于 unavailable，因此同时超预算且
stale/missing/unsupported 的输入先被拒绝，不承诺超预算输入的旧 reason 优先级。

构造次序：file count → roots count → BTreeMap canonical 次序的每文件 bytes 与 checked total →
owner、inventory paths、root 元数据 → root 归一化 → 完整内容 hash → config digest 验证 →
root 排序及 snapshot digest。任何失败不产生部分 snapshot；borrowed inventory 不被修改。
resolve 次序：ancestry count → input 元数据 → lexical name 长度 → 路径规范化/root selection →
package evidence count 与 module identifier 长度 → source/range/identifier 语义验证 → 派生。
后续 partial failure 的 module/package 临时值只在本调用内销毁；不发布部分绑定、无缓存写入。

## 能保证什么与成本

这是有限 logical input/work admission，不是 hard RSS、allocator quota、capture 内存上限或
运行时间认证。调用者在入参 map、Vec capacity、root/input strings 上已经完成的分配不受模型
限制；by-value 拒绝可能销毁调用者移入的数据，若需重试/保留原 inventory 应选 shared borrow。
模型不能在分配前验证自己没有参与的 AST/capture 作业。Rust allocator OOM 不转成 typed resource
refusal；typed refusal 只针对这里列出的逻辑预算和算术。

构造仍对所有 admitted 文件求 BLAKE3：内容工作与总 logical bytes 成比例。还分配每个 path/digest
的 BTreeMap、规范化 roots、JSON snapshot hash 临时 bytes；路径有固定 4096 ceiling，条目受
file/root count 限制。与原型相比持久保留 digest map，交换的是 resolve 不再重 hash source/marker。
因此不能据此宣称速度、100k 容量或 peak RSS。resolve 的 ancestry、package probes、output clones
受相应计数及单项长度约束；重复调用和调用方保留任意数量 BoundDeclaration 不受单次契约限制。
fingerprint 的 JSON serialization 仍按已 admitted 输出大小分配；并未加入 streaming hash。

限额不进入 identity digest：同一 inventory/root/owner/input 在两套均足够的 policy 下产生完全
相同的 address、binding 和 fingerprint。原 domain-separated JSON+BLAKE3、root 排序、map 次序、
lexical qname/UID、ASCII identifier 和 adapter trust boundary 均保持。没有跨版本 UID 或 runtime
importability 保证。snapshot digest 仍覆盖完整 inventory 的所有 paths/content digests；无关空文件
新增或无关 bytes 改动都使 owner scope 所有旧 binding 失效。重建 snapshot 要重 hash 全 inventory，
重新检查旧声明须逐项重新派生；不是增量 capture 或 dependency graph，缓存/DB 接线须承担该
保守 whole-inventory invalidation 成本，不能只比较 target digest。

## 自编验证

`declaration_identity_resources.rs` 覆盖所有显式维度 exact / one-over、usize::MAX 合成 size overflow
（不分配巨大 bytes）、UTF-8 多字节计量与原 ASCII refusal、1024 层 Class assertion 的迭代处理、
小 policy 下 is_current 在克隆前拒绝深 ancestry、partial admission/resolve failure 后复用、borrowed /
owned /兼容入口 fingerprint 相等、whole-inventory invalidation、零预算和 4096 metadata 边界、兼容构造器 4097 个空文件的有限拒绝。
它不证明 AST semantic validity，也不测性能。

验证使用官方 `rustc 1.95.0 (59807616e 2026-04-14)`，保持原 Cargo.lock：

```sh
cargo +1.95.0 test --locked -p cc-model --test declaration_identity_resources \
  --test declaration_identity_v1 --test declaration_identity_independent_review
cargo +1.95.0 clippy --locked -p cc-model --lib \
  --test declaration_identity_resources --test declaration_identity_v1 \
  --test declaration_identity_independent_review -- -D warnings
rustfmt +1.95.0 --edition 2021 --check crates/cc-model/src/declaration_identity.rs \
  crates/cc-model/tests/declaration_identity_resources.rs
git diff --check
```

未发现适用 AGENTS.md、relevant.agents 或本地 skill；遵循 CONTRIBUTING 的中文文档与官方 MSRV。
只执行上述 scoped 验证，不跑全仓/排除项/public gold/scorer/formal DEV/100k；不 merge/deploy。

结果：9 个资源 tests、15 个原型 tests、11 个独立 review tests 全部通过（35 passed，0 failed / ignored）；
上述 scoped clippy、rustfmt 与 diff check 通过。Cargo.lock 未改。
