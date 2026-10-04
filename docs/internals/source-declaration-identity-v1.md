# 源码声明地址 v1：契约与纯模型原型

Status: proposed（产品设计；不是发布或评测修复）

Date: 2026-10-04

Base: `37dd042eaa1209a86e0cafdcd92ae77e036e76f5`
诊断依据：`bc4e5602e10841258fb5583138419335bc9098dd`，PR137 的
`artifacts/diagnostics/requests-native-zero-20261004/README.md`。

## 问题与决策

`SymbolRecord.qname` 保留现有 parser 的 dotted lexical scope 语义；
`StableId::symbol_uid(file_path, qname, kind, normalized_signature)` 保留原算法与输入。
Python 顶层 `pulse` 与类方法 `Beacon.pulse` 不加路径前缀；局部函数的词法名也不改成
带 `<locals>` 的 runtime 名。诊断证明字段命名空间存在缺口，不证明声明具有 runtime
可导入性，也不关闭现有质量门禁。不得改 gold/scorer、同时接受两个命名空间或用 query 特判。

新类型 `DeclarationAddress` 描述一个 owner/source-root 范围内的模块与词法声明地址。
它不是 Python `__qualname__`、不是模块属性查找路径、不是导出别名，也不是 stable UID。
模块和词法链分别存为数组；词法段保留 Class/Function 类型，避免把拼接字符串当唯一身份。
`IdentitySchema::PythonDeclarationV1` 序列化为 `python_declaration_v1`，语言与版本明确。
同一逻辑地址允许多个条件声明、重复定义或 overload occurrence；不能在 DB 建唯一地址约束。
`BoundDeclaration` 用源码与范围证据区分 occurrence，其 fingerprint 是不可变绑定的 equality guard，
不是跨版本稳定 UID。owner 是调用方分配的 index/repository 范围标识，不能用机器绝对目录代替。

## 先检查现有 resolver，复用证据而不继承推断

base 的 `cc-index/src/project_model/python.rs` 从 pyproject 的 setuptools package-dir、
packages.find.where、Poetry from 收集 roots，未配置时仍回退到 `.`/`src`。
`cc-model::module_inputs::PythonProject` 只有 roots/diagnostics，没有每个 root 的显式来源标记。
`cc-index/src/module_resolution/python.rs` 复用 immutable FileCatalog、逐祖先探测
`__init__.py`，区分普通包和 namespace，并处理相对 import 的 dots 与 ordered roots。
这些配置 bytes、canonical file catalog、package marker probes、diagnostics 是可复用的证据。
默认 roots、最长前缀和 import search 顺序不能直接升级为声明身份契约。当前 resolver 无改动。

原型复用 `repo_path::normalize_relative` / `is_canonical_file`、`source::ByteSpan` 及
`identity::hash` 的 deterministic JSON+BLAKE3。它不调用 import resolver：声明推导与 import
定位的目标不同。未来配置适配器应补 provenance，而不是复制一套配置 parser 或修改 resolver 输出。

## 精确首切片及真实结果

纯 API 位于 `cc-model/src/declaration_identity.rs`；仅在 lib.rs 注册模块。
`DeclarationSnapshot::new(owner, files, roots)` 接收完整 immutable regular-file inventory、
显式 `ConfiguredRoot` 及原配置 bytes digest；`resolve(&DeclarationInput)` 返回
`Derived(BoundDeclaration)` 或 `Unavailable(IdentityReason)`。非法路径/证据构造返回 `CcError`。
不读取文件系统，不执行 Python，不解析配置或 AST，不写缓存/DB。
`is_current(&BoundDeclaration)` 在当前 capture 重新推导并精确比较全部绑定，不只检查源码 hash。

| 场景 | v1 结果 |
|---|---|
| 一个显式 collection root，顶层 `standalone.py` | 模块 `standalone`；无 package marker 要求 |
| `src/beacon/signals.py`，root=`src`，每个包祖先有 `__init__.py` | 模块 `beacon.signals`，词法链另存 |
| `src/beacon/sub/__init__.py`，普通包祖先完整 | 模块 `beacon.sub`，不追加 `__init__` |
| source root 自己的 `__init__.py` | `RootInitializer`；未声明 root 外的 package 名 |
| root 未显式配置 | `NoConfiguredRoot`；无 `.`/`src` 默认推断 |
| 两个或更多 roots，包括重叠、重复、互不相交 | `MultipleRoots`；v1 整体不支持，不按顺序/最长前缀挑选 |
| 文件不在 root 下 | `OutsideRoot`；以组件边界检查，`sr` 不匹配 `src` |
| 缺少任一祖先包 marker，包括 namespace package | `NamespaceAncestry`；不猜名称，不拼接 namespace portions |
| `pkg.py` 与 `pkg/__init__.py` 共存 | `ModulePackageCollision`；不借 import 优先级裁定声明地址 |
| `a.b.py`、数字开头、keyword 文件名、非 `.py` | `UnsupportedFile` |
| Unicode/非法/keyword package 或词法 identifier | `UnsupportedIdentifier`；v1 只支持 ASCII Python identifier |
| 自由函数、类、类内函数或嵌套类 | 支持 typed lexical ancestry；末段为 Class/Function |
| 任何祖先是 Function，包括 local class/inner method | `LocalDeclaration`；不制造 runtime 或局部可导入名 |
| 缺少文件、digest 不匹配、名字/range 错误 | `MissingSource` / `StaleSource` / `InvalidDeclaration` |
| relative imports / reexports / alias / star import | 本 API 不接收这些输入；真实声明地址不变，不生成 alias 身份 |

配置 root 的语义是 collection root，不是包含名称的 package root；显式 root=`src/beacon`
不能隐式恢复 `beacon`。named package-dir 映射、动态路径、namespace roots、多个配置 scope、
stub/native modules、非 Python 等待独立版本/适配器设计。成功也不声明 importability：模块可能
有条件定义、运行时异常、动态 rebinding、私有嵌套类或 runtime import hooks。

## 证据与信任边界

`ConfiguredRoot` 的 directory、config_path、config_digest、directive 绑定到 inventory 中
原始配置 bytes。这里不验证 directive 在 TOML 的语义；必须由将来的配置适配器解释并断言，
不能把任意文本/默认 root 自动标为 explicit。原型 tests 使用自编配置和直接 assertion。
`DeclarationInput` 的 ancestry 是 parser assertion，包含每段 kind/name、声明范围和 name 范围。
模型验证 source digest、非空范围、包含关系、name bytes；它不证明该 token 的 AST 意义、完整
祖先链、声明 kind 或 importability。未来 parser adapter 必须从同一源码 snapshot 的真实节点
产生完整链；不得仅从 qname split 或 SymbolRecord 的行号重构这条证据。

snapshot 的 files 必须是一次完整、已授权、禁止 symlink 的 regular-file capture。
纯模型无法识别磁盘 symlink、目录 race 或遗漏文件；传入 map 是上游 capture 的 assertion。
任何未完成扫描应使适配器拒绝创建 snapshot，而不能让 marker absence 产生看似确证的结果。
现有 ProjectModel capture 的 admitted file policy 应在集成前逐项审查是否满足此要求。

路径沿用 repository portable、case-sensitive spelling。用户输入的 `./`、重复 slash、portable
backslash 在边界归一化；拒绝绝对、drive、控制字符与任意 `..`。inventory 和 config_path 必须
已经 canonical，拒绝别名而不合并。native Unix backslash 文件名与非 UTF-8 由 capture adapter
通过 `repo_path::from_native_relative` 拒绝；不能用 portable normalize 冒充 native canonicalize。
不做 realpath、case-fold 或 Unicode normalization，避免机器相关身份变化。

## 失效、版本与向后兼容

binding 包含 owner 范围的 snapshot digest、target digest、完整词法 ranges、root evidence、全部
普通包 marker paths/digests。snapshot digest 覆盖所有 files 的 paths/digests 与全部配置 roots，
因而还绑定 marker 的缺席、竞争 module 的缺席及文件 rename。添加/移除 marker、package rename、
source/root/config rename、配置内容修改、source bytes/span 漂移、竞争文件新增都需要重新推导。
v1 保守使用整 scope inventory 失效；即使不相关文件变化也会改变 fingerprint。不得持久化后
只比较 declaration digest 就复用，逻辑 address 相等也不能证明 binding 当前有效。

输入/输出只提供 Serialize，不提供盲目 Deserialize。输出字段私有且只由 resolve 构造；
外部 serialized output 的入口必须检查版本并以当前 captured inputs 重新推导。未知 schema 不做
字符串 fallback；未来 wire decoder 应有版本分派和明确 UnsupportedVersion 状态。
版本变更不会修改 qname/UID。旧 SymbolRecord、DB row、MCP hit 仍按原行为；此提交无新增字段、
查询条件、schema migration 或生产调用。将来 identity 缺失必须保持 typed unavailable/absent，
不能用 qname 伪造 module identity。旧消费者继续读 qname；新消费者明确 opt-in identity schema。

## 自编验证与后续接线

`crates/cc-model/tests/declaration_identity_v1.rs` 覆盖 regular/init/top-level 模块、root evidence、
多 roots/namespace、冲突、路径攻击、dotted filename、local declaration、byte witnesses、
relative/reexport 不改声明地址、config/marker/source/rename 失效、owner/重复 occurrence、
旧 SymbolRecord wire 与 lexical UID 兼容。无 public DEV body、holdout、published gold 或 scorer。

只运行新 scoped test、cc-model library check/lint、新增 Rust 文件的 rustfmt 与 diff check；不跑
包含旧排除项的测试 suite、正式评测、100k 或生产 runtime 测试。命令：

```sh
CARGO_HOME=/workspace/.cargo RUSTUP_HOME=/workspace/.rustup \
  /workspace/.cargo/bin/cargo test --locked -p cc-model --test declaration_identity_v1
```

本次验证：官方 `1.95.0-x86_64-unknown-linux-gnu`，原 Cargo.lock；以上 scoped tests 全部通过。
`cargo clippy --locked -p cc-model --lib --test declaration_identity_v1 -- -D warnings` 通过；
对本模块、测试、lib.rs 的 `rustfmt --edition 2021 --check --config skip_children=true` 与
`git diff --check` 通过。与 base 比较，symbol/id、index、parsers、DB、search、eval 均无改动。
仓库及 workspace `.agents`/`.codex` 未找到适用 AGENTS.md/SKILL.md；遵循 CONTRIBUTING 的
中文文档与 scoped test 习惯，用户禁止全套/排除项优先于通用全仓验证建议。

最小下一项可独立审查的生产集成：**仅补 Python project capture 的显式 root provenance**。
让现有配置解析输出每个 root 的 config path/digest、directive、explicit/inferred 标记，验证
capture completeness/native path policy，并用自编配置测试默认 roots 不具备 identity admission。
不在该项改 parser/qname/UID、DB/retrieval/evaluator，也不启用身份输出。此项先解决当前
PythonProject 无法区分显式与回退 roots 的信息缺口。

随后独立 PR 顺序：parser 从原 bytes AST 生成 typed ancestry/name-span assertion；组合 capture
adapter 与此模型做纯派生（保留 unavailable reasons）；单独设计 DB additive optional identity
record、版本分派和 config/package/inventory invalidation；再单独 opt-in MCP/retrieval 输出字段。
identity record 必须通过已有单库/source-bound publication guard 与当前 snapshot 一起提交，
不是第二权威库。旧行无 identity 时仍可读取，启用时重建而不改 UID。最后才设计 evaluator 的
显式 identity namespace/schema 迁移方案，由独立作者/审查决定新协议，不放宽旧 gold 的精确比较，
不重写已发表 gold，不把原质量 FAIL 变成认证。
