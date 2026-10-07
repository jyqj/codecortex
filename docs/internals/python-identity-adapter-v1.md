# Python 声明身份 AST 适配器 v1

实现基线：生产 source `e4a8df4cbc6dfae29af8cb0eac9ee83fbba4d696`，
docs head `6b8f21a664c7b965ff99a12b3541fff98f7312eb`（其相对 source 只有文档/证据变化）。
本切片仅新增 `cc-parsers::python_identity`、自编测试/byte witnesses、本文及一行模块注册。
不改 cc-model resource/capture；模型资源工作可独立组合。

## API 与信任边界

`declaration_inputs(file_path, original_bytes, PythonIdentityLimits)` 返回
`CcResult<PythonIdentityOutcome>`。成功为 `Inputs(Vec<DeclarationInput>)`，空文件/无声明
可返回空 Vec。错误/不支持为 typed `Unavailable(PythonIdentityReason)`；无效 budgets/path
长度返回 InvalidParams，tree-sitter 初始化或 timeout/failure 返回原 parse_tree 的 Parse error。
任何失败均丢弃全部临时输出，不交付部分列表。

复用现有 `parse_common::parse_tree` 与锁定的 tree-sitter-python grammar；UTF-8 校验后 `&str`
仍借用同一原始 byte slice。无解码重写、CRLF normalization、源码片段重解析、qname split、
SymbolRecord 行号反推或 Python 执行。digest 直接覆盖传入 original bytes。
锁定的 tree-sitter-python 0.23.6 可让 module span 从 leading blank/comment trivia 或 UTF-8
BOM 后开始，纯 whitespace/BOM 的空 module 可位于 source.len()。root 仅要求 module kind
与 0 <= start <= end <= source.len()，不要求覆盖完整 source。完整原始 bytes 仍送入 parser，
全文件 has_error/MISSING 检查和 digest 覆盖前后 trivia/BOM 均保留。BOM 接受依据是该锁定
语法的 recovery-free tree，不声明 CPython 编译合法性或 importability。R1 正向回归与原失败
证据见 `docs/reviews/python-identity-r1-fix-20261004/README.md`。
本独立 opt-in API 创建自己的 tree，不在现有 extraction 中增加解析或更改结果；未来若接线
复用 extraction tree，必须另行设计原始 bytes/tree ownership，不能接受外部任意 tree assertion。

迭代 TreeCursor 遍历所有节点（包含匿名节点），只在真实 `class_definition` /
`function_definition` 入栈、离开时出栈。async 使用同一 function node。读取 name/body/parameters
field，检查 identifier/block/parameters 节点类型及非空 body。decorated_definition 的 definition
field 必须指向该节点，canonical declaration span 从 wrapper 的第一个 decorator 开始，到
完整 definition 结束；每个声明只输出一次。name span 始终来自 definition 的 name field。
if/else/try/except/for/with/match/block 等容器不创建 lexical segment，但遍历不会丢掉祖先。
重复/条件声明保留多个 occurrence，不去重。所有祖先 Function 都保留，交由纯模型明确返回
LocalDeclaration（包括局部 class 和其 method），不得提升到模块范围。

这是锁定 grammar 的**语法声明地址证据**，不是 CPython 完整语义/编译合法性验证，也不是
runtime importability、exports、alias、UID 或生产检索 namespace。测试包 marker 故意含
`raise RuntimeError`，纯模型仍可派生语法地址；没有运行这个包。
Unicode name bytes 原样保留，让模型 v1 返回 UnsupportedIdentifier。hard Python keywords
另行拒绝为 UnsupportedAst：grammar 实测可在无 ERROR 树中把 `def if()` 标为 identifier；
match/case/type soft keywords 保留合法 identifier 用法。

## 错误所有权与资源预算

任意 ERROR/MISSING 拒绝**整个文件**为 SyntaxError，正常 sibling 也不能输出。root.has_error
先拒绝，遍历再检查 error/missing。空 body（grammar 实测不一定 has_error）、错误必需 field
或 canonical wrapper、hard keyword 为 UnsupportedAst。该策略宁可缺失身份，也不穿过 recovery
猜完整祖先链。原 extraction 的容错与 SymbolRecord 行为无变化。

默认独立预算：source 1 MiB（parse 前检查）、200000 visited nodes、tree depth 128（root=1，
包含匿名节点）、4096 declarations、32768 累计输出 ancestry segments、8 MiB 累计复制的
path/digest/name text bytes、100000 µs tree-sitter timeout。调用方可显式调整为非零有限值；
0 不解释为 unlimited。文本计数使用 checked arithmetic，输出 clone 前检查；metadata 数量
由 declaration/segment counts 单独控制。声明名称/临时 ancestry 受 source bytes 限制。
path 长度最多 4096；路径与 configured-root/capture admission 仍由模型/独立 capture 负责。

source limit 限制 parser 输入，timeout 是现有 tree-sitter 的 best-effort 控制；node/depth/output
预算在 tree 已建立后才作用，不证明 parser peak memory 上限，也不提供 OS 硬超时或 RSS
隔离。未测 peak RSS/吞吐；不能把模型资源预算当作 tree-sitter allocation 证明。

## 自编验证与实际结果

先写 `tests/fixtures/python_identity/WITNESSES.md` 与 witness.py，再实现适配器。预期固定
每个 kind/完整 ancestry/name span/装饰 wrapper span。首轮纠正一个 CRLF 手算 end off-by-one，
并发现 grammar 无 ERROR 空 body；下一轮发现 hard keyword identifier，均有回归测试。
最终 scoped integration tests 对真实 adapter 与同 bytes 的纯模型进行比较，configured root
来自自编合法 setuptools package-dir fixture（此处是模型明确允许的 configuration assertion，
不声称测试已调用生产 config/capture adapter）。

6 个测试覆盖：decorated class/function/async、嵌套 class/function/local class、重复条件声明、
UTF-8/CRLF 字节、Unicode/soft/hard keywords、错误 name/parameters/superclass/decorator/body、
error 后不得保留正常 sibling、非声明文本/alias/lambda、词法容器、digest stale、伪造 name field、
各独立预算拒绝与精确 output/text/source budget 边界。测试所有 fixture 为自编，未用 public gold。

检查 workspace/repo AGENTS.md、.agents/.codex、SKILL.md：未找到适用文件；读取 CONTRIBUTING
及 source identity contract/独立审查。中文文档、官方 Rust 1.95 与用户 scoped 验证约束优先。

验证环境：官方 `rustc 1.95.0 (59807616e 2026-04-14)`，原 Cargo.lock，无新依赖。
使用 `PATH=/workspace/.cargo/bin:$PATH RUSTUP_HOME=/workspace/.rustup CARGO_HOME=/workspace/.cargo`：

```sh
cargo +1.95.0 test --locked -p cc-parsers --test python_identity_adapter
cargo +1.95.0 clippy --locked -p cc-parsers --lib --test python_identity_adapter -- -D warnings
cargo +1.95.0 build --locked -p cc-parsers --lib
rustfmt +1.95.0 --edition 2021 --check --config skip_children=true crates/cc-parsers/src/python_identity.rs crates/cc-parsers/src/lib.rs crates/cc-parsers/tests/python_identity_adapter.rs
git diff --check
```

最终结果：6 passed / 0 failed / 0 ignored；上述 lint/build/format/diff checks 均通过。
没有运行旧 post_index、broad/private42/GCWALfaults、formalDEV、100k、全仓套件，未改
DB/MCP/retrieval/qname/UID/抽取语义、gold/scorer/query。身份输出仍未启用，无 merge/deploy。

## 待独立审查的模型资源 API 组合

模型资源作者 source `08733fa26504e9792586513ebe81075f058b586e` 尚未独立审查；
本 branch 没有导入/覆盖/合并该 source，也未在兼容 worktree 验证组合。DeclarationInput
字段保持不变，所以 AST adapter 的生产输出无需依赖 snapshot constructor。

后续组合方应在模型资源 API 独立审查通过后显式选择模型 budgets，使用
`DeclarationSnapshot::with_limits(owner, owned_map_or_shared_borrow, roots, limits)` 与
`resolve_with_limits(&input)`。共享借用形式为
`DeclarationSnapshot<&BTreeMap<String, Vec<u8>>>`；通用 helper 可使用
`F: DeclarationInventory`。这些 API 信息来自作者交接，本文不作为代码组合验证。
本切片的模型 fixture 仍使用当前基线 `new`/`resolve`，其成功不证明新 API 的兼容性。

作者交接说明兼容 `new` 默认有限：4096 files、64 MiB total、8 MiB/file、64 roots、
256 package evidence、256 ancestry、4096 identifier bytes。未来 adapter/model 组合必须
分别显式配置预算并保留各自拒绝结果，不能让模型预算替代 AST parser 的 source/timeout/
traversal/output 预算，也不能把 snapshot capture/config assertion 升级为已验证生产接线。
