# Python 完整捕获收据与重新推导 v1

本切片落实 `python-inventory-capture-v1.md` 记录的版本化 ingestion 缺口：
调用方可以保存一个捕获结果的比较收据，并在之后显式要求重新读取、解析和派生，
确认当前内容仍与收据一致。它是既有完整 capture 的独立 opt-in API；
`CapturedDeclarations`、`BoundDeclaration` 的私有构造和既有准入规则保持原样。

## API 和实际行为

```rust,ignore
use cc_index::project_model::python_inventory::{
    capture_python_declarations, revalidate_python_declarations,
};

// root、request、policies 由调用方显式提供。receipt 必须保存到 root 之外，
// 或由调用方在内存中保留，因为任何 scope 内的新文件都会改变完整 inventory。
let captured = capture_python_declarations(root, request, policies)?;
let receipt_json = serde_json::to_vec(&captured.receipt()?)?;

// 重新提供本次 owner/scope 授权与 capture、AST、模型三组预算。
let fresh = revalidate_python_declarations(root, new_request, new_policies, &receipt_json)?;
// 只消费 fresh.outcomes()；不恢复或采用外部存储的旧声明对象。
```

`receipt()` 只读取既有对象的 immutable bytes、完整配置/provenance 和 outcomes，
不访问文件系统。`revalidate_python_declarations` 先检查 wire 边界与版本，再调用
原 `capture_python_declarations`，完整执行 Linux native inventory、配置 Loader、
AST adapter、模型准入和最终 inventory/bytes 验证。全部收据字段比对成功后，
返回该次重新构建的 `CapturedDeclarations`。缺文件、语法错误、配置不完整、native
alias 或资源拒绝均按原路径返回，不会因曾有有效收据而跳过检查。

收据可以反序列化为 `CaptureReceipt`，其字段始终只是外部断言；它没有访问权限、
签名或真实性保证，也不能创建 `BoundDeclaration`。真正重新准入的入口只接收
原始 JSON bytes，自己执行长度和格式检查。它不接受外部 inventory、ConfigInput、
AST、policies 或声明对象来替代重新捕获。owner 仍是调用方的授权断言；
收据不会为调用方发放或扩大授权。

## Wire 与版本

`CaptureReceipt` 有七个必需字段：

| 字段 | v1 含义 |
| --- | --- |
| `version` | wire 形状版本，当前仅接受整数 `1` |
| `derivation_version` | 完整 capture/provenance/AST/model 推导契约版本，当前仅接受整数 `1` |
| `owner` | 与本次 `CaptureRequest.owner` 逐字节相等 |
| `scope` | 只接受 `entire_project`；本次 request 仍必须是 EntireProject 且无 exclusions |
| `inventory_digest` | 全部 regular-file 的原路径和原 bytes；含配置、marker、隐藏/忽略及无关文件 |
| `configuration_digest` | 完整 ConfigInput 与 PythonRootProvenance，包括原配置 digest 和全部原始 directive/evidence |
| `outcomes_digest` | 全部 Python 文件的有序 Derived/Unavailable 输出，包括空数组和重复声明 occurrence |

JSON 在反序列化前以 `CAPTURE_RECEIPT_MAX_BYTES = 16384` 限制原始 bytes；精确等于
上限可接受，超出即 typed refusal。此固定 wire ceiling 不替代调用方的三组准入预算。
原 owner 最多 4096 bytes，即使需要 JSON quote/backslash 转义，合法生成收据也能容纳。
serde 的严格 struct 解析拒绝 duplicate、missing、unknown fields、错误类型、非法 JSON
和尾随非空白；三个 digest 必须是 64 个小写十六进制字符。

旧/未知 wire 或推导版本不作兼容回退，也不由缺字段推断版本。推导版本 1 固定既有
完整 Linux inventory v1、显式 root provenance v1、当前锁定 tree-sitter-python 0.23.6
的 AST adapter 和 PythonDeclarationV1 模型语义。后续改变这些准入/推导规则必须显式
升级 `derivation_version` 并另行审查；wire 变化升级 `version`。版本号不代表任意
Git SHA、release、全仓 source guard 或质量评测认证。

## Digest 编码

使用 BLAKE3 `Hasher::new_derive_key` 分离三个上下文，输出小写十六进制：

| 上下文字符串 | 被 hash 的内容 |
| --- | --- |
| `CodeCortex Python capture receipt v1 inventory` | regular-file 数量的 u64 little-endian；随后按 BTreeMap 路径顺序，对每文件写入路径 UTF-8 长度 u64 little-endian、原路径 bytes、文件长度 u64 little-endian、原文件 bytes |
| `CodeCortex Python capture receipt v1 configuration` | `serde_json::to_writer` 的紧凑 JSON tuple `(ConfigInput, PythonRootProvenance)` |
| `CodeCortex Python capture receipt v1 outcomes` | `serde_json::to_writer` 的完整 outcomes BTreeMap，Vec 顺序不变 |

路径/文件长度 framing 保留边界，文件 map 按路径排序。JSON 内的对象、数组、空结果、
Unavailable 原因及 declaration binding 均参与 hash；只保存 Derived fingerprint 的方案
无法覆盖无声明文件和全 Unavailable 情形，本 API 没有依赖这种捷径。

原配置注释/空白虽不改变解析值，仍通过 ConfigInput 的完整原 bytes digest 参与
configuration hash，并再次参与 inventory hash。比较按 configuration、inventory、
outcomes 顺序返回第一个 mismatch，优先指出配置变化。重新推导采用当前预算；
预算缩小可以拒绝旧收据，即使实际文件仍未变化。

hash 直接向 BLAKE3 writer 流式写入已有数据；不复制整个 inventory，不额外生成
完整 outcomes JSON Vec。工作量仍随全部捕获 bytes 和全部输出增长，并且重新准入
必须再次扫描与解析。它没有增量缓存/存储失效或性能加速保证，也不提供 parser
峰值内存、RSS、OS deadline 或跨文件原子快照保证。

## 拒绝结果

`RevalidationRefusal` 保留以下可区分结果：

| 结果 | 含义 |
| --- | --- |
| `ReceiptBytes { actual, limit }` | wire bytes 超过固定上限；解析前拒绝 |
| `MalformedReceipt` | JSON 形状、字段、类型或 digest 编码非法 |
| `UnsupportedVersion` | wire/推导版本不受支持 |
| `OwnerMismatch` / `ScopeMismatch` | 外部 owner 不等于本次 request，或外部 scope 不受支持 |
| `Mismatch(Configuration / Inventory / Outcomes)` | 完整重新捕获成功，但相应比较字段不同 |
| `Capture(CaptureRefusal)` | 原授权、平台、native、配置、AST、模型或预算拒绝，原原因保留 |
| `Encoding` | 已准入内存对象生成 digest 时的序列化错误；不返回部分结果 |

格式、版本及 receipt owner/scope 检查不读取项目；正确收据仍会在不存在的 root 上
得到原 `Io` 拒绝。仅有收据无法离线恢复声明，也无法把外部声明断言升级成可信对象。

## 内容一致性和保留边界

收据绑定显式 owner/scope 下的 regular-file 内容，不持久化 native inode、mount、
绝对 root 路径或空目录的连续身份。同一 owner 显式授权的另一个 root 可以重新捕获
相同内容；可信 root 的 canonical symlink alias 仍遵循原规则。每次 capture 内仍完整
检查目录、native alias、link、mount 和前后漂移，但这不是历史物理 root 连续性证明。
空目录变化不会改变 v1 receipt 的内容 digest，和现有模型的内容 binding 口径相同。

完整 inventory 包含 `.git`、缓存、ignore 文件和被忽略的 marker。保存收据、提交 Git
或生成任意 scope 内文件都可能改变下一次 inventory，必须据实拒绝旧收据。API 不会
偷偷排除这些文件，也不在项目目录写入收据或缓存。收据由调用方保存在授权 root 外。

保留 Linux + `statx(STATX_MNT_ID)` 条件；其他平台经原入口返回 UnsupportedPlatform。
没有 DB/MCP/retrieval 接线、production UID/qname 改动、publication guard、自动缓存恢复、
runtime importability 或 Python 执行。P7-014 及后续正式验证仍由父任务管理，本子项
不能把 parent gate、V19、public quality 或 100k 规模评测自动转为完成。

## 本切片验证

新增 `crates/cc-index/tests/python_inventory_revalidation.rs` 使用自编临时项目，覆盖
round-trip、原 bytes/provenance、配置与全部 inventory 变化、marker absence/collision、
digest/version/owner/scope 错配、严格 wire、16 KiB exact/one-over、三组当前预算、
配置/AST/native refusal、零声明、合法 root alias 和显式授权的内容等价 root。
实际命令、源码摘要、日志和最终结果随本切片的 validation receipt 交付；不修改旧
capture 文档/证据，也不沿用旧 source registry 的认证声明。
