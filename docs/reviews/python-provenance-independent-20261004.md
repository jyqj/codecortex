# Python provenance 独立审查（2026-10-04）

审查对象：`a65c655f7822bc8d025d6c01b1aa86371bbd9de9`。
对照 base：`37dd042eaa1209a86e0cafdcd92ae77e036e76f5`。
独立测试提交：`bc0d0a25`（报告与测试分开提交）。

结论：输入 evidence 实现获有界接受；未发现需要修改生产代码的缺陷。
发现一处可复现的文档事实错误，需更正文档后再声称其 prototype 兼容性说明准确。
没有开启 production identity conversion，也没有 merge/deploy。

## 发现：空根兼容性说明错误

位置：`docs/internals/python-root-provenance-v1.md:67-68`。
文档声称 pure prototype 构造器拒绝生产根 `.` 对应的 normalized `""`。
但文档明确引用的 prototype `866cbed73f303463a80cee07462824d1816f2a44`
在 `DeclarationSnapshot::new` 中调用 `repo_path::normalize_relative`，该函数接受
`""` 并返回 `""`，也把 `"."` 归一化为 `""`。后续检查并不拒绝空 directory。

独立 probe 使用原始配置 bytes、正确 BLAKE3、有效 config path、非空 owner 与 directive，
直接调用构造器。最初按文档断言 `root("").is_err()`，失败，因构造成功。
将测试改为真实契约后，`root("")` 与 `root(".")` 均成功，probe 通过。
建议仅更正文档该句；不存在本次需要启用的转换，未来仍须决定完整 evidence 的保留策略。
这属于文档缺陷，不是 production identity 路径的运行时回归。

probe 保存在 `crates/cc-index/tests/provenance_review_support/prototype_compatibility.rs`，
不被 cc-index 自动编译；复现实验时复制到应用 prototype 的临时 worktree 的
`crates/cc-model/tests/provenance_compatibility.rs`。

## 独立比较与范围

通读全部 provenance 文档、Python adapter delta、模型新增类型及原始 Loader、discover、
publication verify、ProjectInputs 校验/摘要/持久化与 Python resolver。
未找到 AGENTS.md 或 `.agents/skills/*/SKILL.md`；workspace `.agents` 为空。
遵守 CONTRIBUTING 的官方 Rust 1.95 要求，用户限定的 scoped checks 优先于 broad suite。

独立 integration test 通过 path module 引用审查 SHA 的真实 adapter，同时执行原样复制的
base adapter（support/legacy.rs）。不是把新实现重写成第二个预期实现。
每个 compare 对 roots vector（含顺序）、diagnostics vector 作完整相等断言，并为
4 个 source path × 10 个 import spec 比较完整 ModuleResolution（含状态、原因、
probes、dependencies 与 resolved path）。

- 111 个自编 JSON document × repository/nested 两个 scope，共 222 个主矩阵组合。
  包含父 table/array/null/number/bool/string shape、空指令、named mappings、Poetry
  include/from 缺失或非法、duplicate normalized roots、多根、outside/absolute/drive/
  separator/Unicode spelling，where 与 Poetry 的 0/1/31/32/33/40 项。
- 21 个自编真实 TOML × 两个 scope，共 42 个 Loader/discover 矩阵组合；合法配置、
  defaults、空根、named mapping、mixed array、invalid shapes、语法错误、duplicate key。
  逐项与 base adapter 比较并确认 captured bytes 的精确 BLAKE3。
- 多个 nested scopes、同 scope 多 document、非标准 suffix、missing digest、非法摘要、
  parsed 缺失/不匹配、capture error。未知旧 PythonProject wire 和空 provenance record
  均未升级为 explicit。
- `./src`、`src`、`src/./` 和 Poetry `from` 合并后保留四份 evidence，逐个 pointer
  在原 parsed document 中找到原 value；包括 package-dir 空 key 的末尾 slash。
- 原始 Loader cache reuse、bytes-only comment mutation、ProjectInputs digest 变化、
  publication verify 拒绝旧捕获。原提交的 focused tests 补充实际 UTF-8、size、missing、
  duplicate TOML、symlink/parent symlink、hardlink、rename 与路径 policy 验证。

Defaults、explicit、partial/invalid 的 provenance 不改变 legacy fallback 时点。
只采到 outside root 时 normalized roots 仍为空；不会新增二次 defaults。
同 scope 多 document 仍按 legacy BTreeMap 顺序覆盖 roots；provenance 标记 partial，
不是把不同配置文档无条件合并成完整来源。Poetry 截断只记录采纳的前 32 项；where
超过 32 项整组拒绝。新增 shape limitation 未改变 legacy diagnostics 或 resolver 状态。

## 绑定、版本与诚实边界

生产 discover 把同一 Loader 的 `documents` 与 `loader.inputs` 传入 adapter，未读第二次磁盘
或重解析；digest 绑定 Loader 读到的完整原始 bytes，path 保留实际捕获 key。
`captured_document` 的信任边界是既有 Loader，不是从任意构造的 ConfigInput 重新证明磁盘内容。
测试中的 synthetic digest 只用于隔离 adapter shape/绑定检查，真实字节保证由另一组
Loader/discover fixtures 覆盖。旧 cache 仍以 bytes digest 复用已有 parsed，不提供额外
防伪证明；publication verify 重新读文件摘要，但不从摘要重新验证任意注入的 parsed。

`PROJECT_MODEL_VERSION=3`、PROJECT_INPUT_KEY、ConfigInput/ProjectInputs wire、payload/digest
算法和持久化 StoredInputs 均未变。snapshot 不存 PythonProject，因此无现存 input 迁移需求。
PythonProject/ProjectModel 的派生 JSON 增加字段：不是字节兼容，整份派生 JSON 的 hash 会变。
新 version 1 只标识 evidence 格式；旧 wire 缺失字段为 Unknown/version 0，不能作为授权。
Rust struct literal 的下游用户也需补 provenance 或使用 Default；additive serde 不意味着
所有 Rust 源码消费者无须适配。该项目内部 scoped targets 编译通过。

没有把 ConfiguredRoot 或 identity prototype 接入生产。临时独立 worktree 在审查 SHA 上
仅应用 `866cbed` 的 cc-model 补丁：15 个原型测试通过，独立空根 probe 通过。
配置 capture 不证明完整 source inventory、package-marker absence、collision、owner、
AST ancestry 或 native alias uniqueness；source root 不做 realpath/inode/全平台唯一性验证。
Linux 测试不能替代 Windows；未验证规模性能，也不对未来 identity admission 背书。

## 执行回执

官方 `rustc 1.95.0 (59807616e 2026-04-14)`。
命令环境：PATH 加 `/workspace/.cargo/bin`，RUSTUP_HOME=`/workspace/.rustup`，
CARGO_HOME=`/workspace/.cargo`；所有 test 使用 `--locked`。

```sh
cargo +1.95.0 test --locked -p cc-index --test python_provenance_independent_review
# 4 passed; 0 failed; 0 ignored; 0 filtered out
cargo +1.95.0 test --locked -p cc-index --lib python_provenance
# 12 passed; 0 failed; 0 ignored; 390 unrelated tests filtered out
# 临时 prototype worktree：
cargo +1.95.0 test --locked -p cc-model --test declaration_identity_v1 --test provenance_compatibility
# prototype: 15 passed；最初遵循错误文档的 is_err probe: 1 failed
cargo +1.95.0 test --locked -p cc-model --test provenance_compatibility
# 改为实际接受契约后：1 passed; 0 ignored
cargo +1.95.0 fmt --all -- --check
# exit 0（新增测试先按 rustfmt 格式化；未修改原实现）
git diff --check
# exit 0
```

原 Cargo.lock 不变。生产源码、query-target、benchmark/gold/scorer 均未修改。
未运行 excluded post_index 或包含它的 broad suite、formal DEV、100k、private42、GCWAL faults。
正常 origin fetch 成功；push 与唯一 draft attempt 的结果另作交付回执。
