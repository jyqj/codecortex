# 声明地址资源准入独立审查

审查对象：`08733fa26504e9792586513ebe81075f058b586e`，直接父提交
`e4a8df4cbc6dfae29af8cb0eac9ee83fbba4d696`。完整阅读
[资源契约](declaration-identity-resource-contract.md)、目标 diff、纯模型、既有三组 scoped tests
及 CONTRIBUTING；当前 workspace 未发现 AGENTS.md 或适用本地 skill。

结论：在下述有限范围内通过，未发现具体实现缺陷。不是生产适配、AST/config 真实性、性能、
hard RSS 或容量认证。未改实现、parser adapter 或 Cargo.lock。

## 独立证据

新增 `crates/cc-model/tests/declaration_resource_independent.rs`，7 项测试：

- 全部七个可配置维度使用自编 fixture 的 exact policy 成功派生，然后逐项减少 1 验证
  `LimitExceeded` 的 resource、limit、actual；同时核对全部 PROTOTYPE 常数。
- size preflight 在 `[usize::MAX, 0]` 精确通过，在 `[usize::MAX, 1]` 返回 typed
  `ArithmeticOverflow(TotalBytes)`，无巨量 byte 分配。检查少/多 sizes、先 count 后 iterator，
  file bytes 优先于 total。实际 inventory 的余量 capacity 不进入 logical bytes 计量。
- thread-local allocator 计数证明：128 段绑定在 depth=4 的 snapshot 上调用 typed
  `is_current` 拒绝时分配数为 0；超深且 oversized/非法路径的 resolve 也是 0。
  构造 roots refusal、raw root metadata refusal 和 resolve raw input metadata refusal 为 0。
  这验证这些具体路径在拒绝前不克隆 ancestry、不做路径归一化分配或 hash 输出图，
  不能推广成所有失败/成功路径都无分配。
- owner、inventory path、raw root directory/config_path/directive/config_digest、raw input
  file_path/source_digest 每项独立验证 4096/4097 UTF-8 byte ceiling。4096 的伪 digest
  到达既有 Model/stale 检查；不把通过预算误报成有效 digest。长 alias spelling 在归一化前拒绝。
- package component 与 module stem 的多字节长度按 UTF-8 bytes 拒绝；package evidence
  预算先于 missing/stale。4096 raw metadata ceiling 独立于 identifier policy。
- whole-inventory、无关空文件、无关 marker bytes、使用中的 package marker、config bytes+digest、
  directive 和 owner 变化使旧 binding 不 current；stale source 与 bad config digest 正确拒绝。
  失败后原 shared snapshot 仍 current。
- 保留 baseline 模块作为 test-only oracle；内容来自指定父提交，仅将首个 `use crate` 改成
  `use cc_model` 并加 provenance 注释。对 root-level module、regular package、两层 package、
  package initializer 四组合法 fixture，包含 nested Class/Function、alias normalization 和
  会抛异常的 package bytes，逐一比较完整序列化 address+binding 与 fingerprint。
  两种足够的 policy、owned/shared 和兼容 wrapper 全部相等；未执行 Python 或 import。

`tests/support/resource_api_probes/check.py` 另外执行 4 个外部 Rust API probe：

| Probe | 官方 Rust 1.95 结果 |
|---|---|
| owned/shared 与原 owned 默认类型 | 编译并运行成功 |
| 显式 `&mut BTreeMap` | E0277：不满足 DeclarationInventory |
| 外部自定义 provider | E0277：不满足私有 sealed::Inventory |
| shared snapshot 后修改原 map、随后使用 snapshot | E0502：仍有 immutable borrow |

不依赖只有“编译失败”而不知道原因的检查：脚本要求完整 error-code 集合等于预期；positive
probe 使用同一 rlib 防止选错库造成伪阳性。最初误选历史 rlib 和缺少 deps search path 的 harness
尝试已修正；上表来自 Cargo 当前 build artifact 的最终成功检查。

## 次序与范围判断

代码检查确认构造时 file count → roots count → canonical BTreeMap 顺序的长度/checked total →
owner/path/root raw metadata → root normalization → 内容 hashing → config digest → snapshot digest。
resolve 与 is_current 共用 borrowed-slice `resolve_parts`，depth 在路径、名字、ranges、输出 clone
前检查；lexical bytes、relative package count/module bytes 也在输出构造与 package probes 前检查。
package count 来源于 size-bounded normalized input path；递增/总字节使用 checked arithmetic。

sealed trait、私有 snapshot 字段及 shared borrow 确保安全 Rust 无法在准入后替换 bytes；
BoundDeclaration 没有公开 mutable evidence API。限额不参与 identity digest。
资源错误与 Model/Unavailable 区分清晰；兼容 API 按契约映射到 InvalidParams。
超预算 missing/stale/unsupported 输入可先收到 Resource，不要求保持旧 unavailable 优先级。

资源契约明确只保证单次 logical admission：不计 caller Vec capacity、此前 AST/capture 分配、
allocator OOM、hard RSS、wall-clock 时间或重复调用/保留输出的累计成本。
4096 metadata ceiling 加有限 count/identifier/depth/evidence 控制模型内部逻辑规模，
JSON+BLAKE3 临时分配及全 inventory rehash 仍存在；未声称 hard memory quota 或速度提升。

## 可复现验证

官方 `rustc 1.95.0 (59807616e 2026-04-14)`，`--locked`，Cargo.lock 不变。
此环境需要 `CARGO_HOME=/workspace/.cargo`、`RUSTUP_HOME=/workspace/.rustup`，
并使用 `/workspace/.cargo/bin/cargo`。

```sh
cargo +1.95.0 test --locked -p cc-model \
  --test declaration_resource_independent --test declaration_identity_resources \
  --test declaration_identity_v1 --test declaration_identity_independent_review
cargo +1.95.0 clippy --locked -p cc-model --lib \
  --test declaration_resource_independent --test declaration_identity_resources \
  --test declaration_identity_v1 --test declaration_identity_independent_review -- -D warnings
cargo +1.95.0 build --locked -p cc-model --lib
python3 crates/cc-model/tests/support/resource_api_probes/check.py \
  --rustc /workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/rustc \
  --rlib target/debug/libcc_model.rlib
rustfmt +1.95.0 --edition 2021 --check \
  crates/cc-model/tests/declaration_resource_independent.rs \
  crates/cc-model/tests/support/declaration_resource_baseline.rs \
  crates/cc-model/tests/support/resource_api_probes/*.rs
git diff --check
```

结果：42 tests passed（独立新增 7 + 原有 35），0 failed/ignored；4 个编译 probe 满足预期；
scoped Clippy `-D warnings`、rustfmt、diff check 通过。最后扩充 allocation assertions 后新增 7 项
重新通过。未跑全仓、production capture/DB/MCP、formal DEV、gold/scorer、100k、old runtime、
private42、GC/WAL faults 或任何排除套件；没有 merge/deploy。

剩余 blocker：本地技术验证无。Git push / draft PR 的实际结果见审查提交的交付消息，
本文不预先宣称外部发布成功。
