# 声明地址原型独立审查（2026-10-04）

审查对象：`866cbed73f303463a80cee07462824d1816f2a44`，基线：
`37dd042eaa1209a86e0cafdcd92ae77e036e76f5`（已核对 merge-base）。

结论：**在明确的 adapter assertion 信任边界内，纯 opt-in 模型可接受；不据此批准生产接线。**
未发现需要修改当前模型的可复现正确性缺陷。独立新增 11 个测试全部通过，原型的 15 个
scoped 测试也全部通过。没有更改生产或模型实现、依赖锁文件、现有测试或原型契约文档。
新增测试位于 `crates/cc-model/tests/declaration_identity_independent_review.rs`。

完整阅读 `declaration_identity.rs` 和 `docs/internals/source-declaration-identity-v1.md`，
并核对 repo_path、ByteSpan、identity::hash、StableId 和 SymbolRecord。检查 workspace/repo
中的 AGENTS.md、.agents/.codex 和 SKILL.md：未发现适用文件。遵循 CONTRIBUTING 的中文
文档与 Rust 1.95 要求；用户限定的 scoped 验证优先于全仓测试建议。本审查不需要额外技能。

## 证据与边界

| 项目 | 独立观察与验证 |
|---|---|
| logical module 与 runtime importability | 自编包 marker 含 `raise RuntimeError`；嵌套类方法仍得到 harbor/bells 和 typed lexical chain。没有执行 Python；这是 logical derivation，不能证明该包实际运行时可导入。 |
| 显式 root/config | 无 roots 不回退；重复/互不相交 roots 都返回 MultipleRoots。collection root 改成 code/harbor 后模块仅为 bells。config bytes/digest/path/directive 改动使旧 binding 失效；错 digest、非 canonical config path 被构造器拒绝。 |
| occurrence | 条件分支两个 chime 拥有相同 address、不同完整 ranges 与 fingerprint；重复调用稳定。地址不能作为 occurrence 唯一约束。 |
| ancestry/ranges | 三层 Tower/Room/ring 合法；祖先 Function 返回 LocalDeclaration。空、反向、usize::MAX、越界、child 逃出 parent、name 逃出 span 被拒绝；逐一枚举末段 name-range 的小范围 start/end 组合，只有正确 bytes/range 成功，未 panic。 |
| namespace/collision | marker 缺席为 NamespaceAncestry；祖先 module 和目标 module/package 竞争均为 ModulePackageCollision；旧 binding 不再 current。原 scoped tests 另覆盖 root initializer、nested package initializer、Unicode/keyword/dotted filename。 |
| 完整 immutable inventory | map 移入 snapshot 且字段私有；调用者没有公开修改入口。加入无关二进制文件、改变其 bytes 也使 binding 失效。遗漏竞争文件无法被模型检测：构造器没有 scanner/symlink/race 证据，完整性必须由 capture adapter 保证。 |
| rename/invalidation | config rename、target rename、config bytes/directive、marker bytes、source bytes、owner 变化均拒绝旧 binding；原 scoped tests 补充 source-root/package rename。 |
| deterministic fingerprints | 逆序 map 插入、portable declaration/root 路径归一化得到完全相同 binding/fingerprint；独立重算 domain-separated JSON+BLAKE3 与 fingerprint 一致。没有跨版本稳定 UID 或密码学无碰撞证明的主张。 |
| malformed paths/duplicates | absolute、UNC/drive、任意 traversal、control chars 被拒绝；inventory 的 empty/dot/repeated slash/backslash/trailing slash aliases 被拒绝；owner/file/directive 长度上限有负例。BTreeMap 的同 key 覆盖发生在调用前，模型无法追溯重复输入记录；capture 必须先检查 native alias/duplicate。 |
| 旧 wire/qname/UID | 自编 legacy SymbolRecord wire 可反序列化，原有字段保持一致，输出无 identity 字段；独立 BLAKE3 输入重算原 lexical UID。base→prototype 仅增加模型模块/测试/文档与一行模块注册；rg 检查生产代码无 declaration_identity 调用。没有运行生产端到端兼容性验证。 |

配置与 AST 的语义真实性刻意不由模型证明。独立负信任边界 fixture 使用仅含注释的
config/source，匹配的 name bytes 可被声明为 Class 或 Function；Class 等跨度祖先重复也
可 derive。这符合契约，而非把 token bytes 等价误当作 AST kind、完整祖先链或显式 directive。
未来 parser adapter 必须从同一源码 AST 产生完整链，并保证 kind/range 的语义真实性；
config adapter 必须只标记真实显式配置；capture adapter 必须拒绝 incomplete inventory。

## 最小实际接线缺口与成本

当前纯模型没有必须修复的缺陷；以下为生产接线前可独立处理的具体工作：

1. 现有 PythonProject roots 没有足够显式来源证据；先补 config path/digest/directive 与
   explicit/inferred provenance，以及 capture completeness/native path policy。
2. snapshot 拥有所有文件 `Vec<u8>`；new 对完整 inventory 求 digest，resolve 对目标 bytes
   再求 digest，fingerprint 序列化完整 ancestry/binding。资源策略应限制 bytes、file count、
   ancestry depth、identifier length，并优先复用已验证 capture。已有 4096 路径/owner/directive
   限制不等于总资源预算。256 层重复 Class assertion 被接受是有界实测；不作为真实 AST 控制。
3. 一次无关文件变化也改变 snapshot_digest，使整个 owner scope 的旧 binding 失效。
   这保守且正确，但 DB/cache 接线需要完整 dependency/invalidation 方案，不可只看 source digest。
4. 版本化 ingestion 必须重新推导，不能盲信 serialized BoundDeclaration；之后单独设计
   optional DB/MCP identity，与 source-bound publication guard 一起发布，保持 qname/UID 原行为。

资源控制测试仅使用一个 1 MiB 无关文件、256 个小 inventory 文件及 256 ancestry 段；
成功/失效行为有证据，没有测 peak RSS、吞吐、扫描速度、100k 或真实仓库规模，不能据此
宣称性能足够或发现生产级 DoS。测试输出的耗时也不是 benchmark。

## 验证记录

官方安装的 `1.95.0-x86_64-unknown-linux-gnu`：
`rustc 1.95.0 (59807616e 2026-04-14)`，LLVM 22.1.2；Cargo.lock 未改。
以下命令均使用 `PATH=/workspace/.cargo/bin:$PATH`、
`RUSTUP_HOME=/workspace/.rustup`、`CARGO_HOME=/workspace/.cargo`：

```sh
cargo +1.95.0 test --locked -p cc-model --test declaration_identity_independent_review
# 11 passed; 0 failed; 0 ignored
cargo +1.95.0 test --locked -p cc-model --test declaration_identity_v1
# 15 passed; 0 failed; 0 ignored
cargo +1.95.0 clippy --locked -p cc-model --lib --test declaration_identity_independent_review -- -D warnings
# exit 0
rustfmt --edition 2021 --check crates/cc-model/tests/declaration_identity_independent_review.rs
# exit 0
git diff --check
# exit 0
```

未运行 public DEV/gold/scorer/formal eval/100k、旧 post_index、broad/private42/GCWALfaults、
全仓测试或 merge/deploy。发布只使用请求的 review branch；实际拒绝后不改身份、不换路由重试。
