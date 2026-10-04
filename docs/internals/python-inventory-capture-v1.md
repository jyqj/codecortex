# Python 完整 inventory 捕获与声明准入 v1

基线是 PR138 delivery `ba6197771da5425166fb0386d46c74f4a05e8273`，
采纳的 reviewed product 是 `50a4933e48ef20b16401ac8f75c386a660aa8e5c`。
已完整读取该 delivery 的 `artifacts/checkpoints/python-resource-integration-20261004/README.md`。
本切片补上其中的完整 capture→admission；仅 opt-in 内存 API，不构成 release 或原子文件系统认证。

## 实际 API 和授权

`cc_index::project_model::python_inventory::capture_python_declarations(root, request, policies)`
返回 `Result<CapturedDeclarations, CaptureRefusal>`。
调用方必须指定 `CaptureRequest.owner`、`AuthorizedScope::EntireProject`、空 `exclusions`，
并提供 `AdmissionPolicies` 的 capture、AST 和模型三组预算。没有默认准入预算或隐含 owner。
`Subtree` 和任何 exclusion 明确返回 `UnauthorizedScope`；只扫描 config 或 caller catalog
不能调用本 API 声称完整。owner 是调用方的授权断言，不是身份认证服务。

支持范围是可信 project root 下的**全部 regular files**，包括隐藏文件、`.git`、缓存、
ignore 文件、其忽略的 marker 和非 Python 文件；空目录也枚举及验证。没有 scanner ignore
或生成目录默认排除。已有 scanner 的 shared manifest 是 ignore-filtered，不能满足此契约，
因此复用其政策研究结果而不复用过滤后的 catalog。不得在预算失败后悄悄缩小范围重试。
大 repository 的 `.git` 或其他目录超预算就是 refusal；排除支持属于后续独立契约。

成功对象私有构造，`owner()` / `scope()` / `inventory()` / `config()` / `provenance()` /
`outcomes()` 只返回只读视图。actual bytes 是有界读取所得 Vec，之后只共享借用；
不接受任意 inventory assertion、serialized ConfigInput 或 AST tree。

## 平台和 alias 明确政策

仅 Linux，且要求内核/运行环境支持 `statx(STATX_MNT_ID)`；否则 typed `UnsupportedPlatform`。
未验证 macOS/Windows；它们直接拒绝，没有弱路径检查 fallback。
可信 project root 本身 `canonicalize`，接受其普通 symlink/alias（有真实 temp fixture）；
root 以下逐 component descriptor-relative `openat` / `NOFOLLOW`，目录通过 `fdopendir/readdir`
流式枚举。先 `fstatat(AT_SYMLINK_NOFOLLOW)` 拒绝 nonregular，再 open；`NONBLOCK` 避免
FIFO swap 阻塞，打开后的 metadata 再判 kind。任一 symlink、FIFO、socket/device 等拒绝。

native name 必须 UTF-8，且 v1 更保守地只接受 ASCII portable spelling；不把 Unicode
normalization 证明寄托于 UTF-8。反斜线、冒号/drive/alternate stream、control、Windows
reserved basename、末尾 dot/space 等拒绝。大小写仍按 Linux 实际 spelling 作语义解释，
但同 scope 内大小写折叠冲突拒绝；目录和文件都检查。精确重复 key 在建图前拒绝，
不接受已由 BTreeMap 覆盖的外部记录流。TOML duplicate key 由已有 Loader parser 拒绝。

所有 regular files 要求 `nlink == 1`，即使另一 hardlink 在 scope 外也拒绝；不把 digest 相等
当同一来源。全部 entry 的 `(dev, ino)` 唯一；不同 mount ID 的 child 也拒绝，覆盖 bind mount
以及跨设备 mount 的政策。root mount ID 也保留供前后比较。本轮没有 mount 权限，没有实际
bind-mount fixture；不把 mount 拒绝分支称为已执行 mount 集成测试。已执行 root symlink、
child/parent symlink、外部 hardlink、大小写碰撞、非 UTF-8、Unicode 和 portable alias fixtures。
依赖 Linux 本机文件系统提供准确 native metadata；没有对不可信 FUSE/远程文件系统认证。

## 配置证据和模型绑定

完整 stat inventory 的全部预算和模型 size preflight 通过后才读 bytes、hash/parse config。
配置使用原 `config_cache::Loader`，没有第二个 TOML parser、任意 cache 断言或 Python 执行。
Loader 实际 digest 必须等于 immutable inventory 内完整 `pyproject.toml` bytes 的 digest。
随后调用原 `python::build`，要求实际 v1 provenance 为 Explicit、无 limitation、单 normalized root，
并要求该 root 在实际目录 inventory 中存在。

v1 只支持根 `pyproject.toml` 的 setuptools 空-key `package-dir` 和 `packages.find.where`
collection-root 指令；可以同时给出同一个 normalized root，全部原始 directive/value 均保留。
`./src`、`src/./` 和 `src` 的 lexical 合并有明确证据，不丢 supporting directive。
defaults、无配置、非法/partial、多个 root、named mapping、nested/suffix pyproject、Poetry
selector、find include/exclude/namespaces 及其他 setuptools keys 都明确 refusal。
其他 build tools 的 runtime package discovery 不在支持语义内；这里证明静态 collection root，
不证明 build backend、importability、发布包集合或 CPython 编译合法性。

原 config 全 bytes（包括注释/空白）保留在 inventory；完整 parsed document 在 ConfigInput；
全部 provenance/evidence 保留。`ConfiguredRoot.directive` 是全部原始 supporting evidence
的 JSON 数组字符串（不是选取第一项），仍受原模型 4096-byte metadata 限制。
这只是本 opt-in bridge 的编码，没有修改原 ConfiguredRoot/model/provenance wire。

真实 `declaration_inputs` 接收同一 bytes slice，再传入
`DeclarationSnapshot<&BTreeMap<String, Vec<u8>>>::with_limits` / `resolve_with_limits`。
`Derived` / 模型 `Unavailable` 原样返回；AST whole-file refusal、模型 resource refusal 或
任一 capture 错误丢弃全部临时结果。绑定包括 owner、完整 scope regular-file inventory、
配置完整内容和 evidence、source 原 bytes、package marker；marker absence 和碰撞由该完整
inventory 判断。无关文件变化也保守改变 binding。空目录不进入模型内容 digest，但捕获过程中
仍检查它们；其不携带 Python marker/collision 内容。

## 有界性和一致性

`CaptureLimits`：entries（root/目录/file 全计数）、files、total/file bytes、depth、单条/累计
path bytes、aggregate outcome count/serialized bytes；没有 unlimited sentinel。root depth=0，
root path 字符串为空，不计累计 path bytes。额外 compatibility ceiling 为 depth 256/path 4096。
读取每个 readdir entry 时在复制 key/open 前检查 count/path/depth；无 unbounded list-all。
全列表及 sizes 通过 capture 和 DeclarationLimits preflight 后才分配 content Vec 和 hash。

aggregate output 的 JSON bytes 包括 file keys、空结果数组、分隔符、Derived 和 Unavailable；
不再分配整个 serialized output 来测大小。每个 outcome 在加入最终 Vec 前检查；临时单个 AST
input 列表和模型 outcome 分别受显式 AST/模型预算限制。没有证明 parser/TOML 峰值分配、
allocator/RSS 硬上限或 OS deadline；tree-sitter timeout 保留原 best-effort 语义。

返回前重新枚举全部目录/file，比较 identity、mode/link/size、mtime/ctime（忽略读取引起的
atime），重读比较每个完整 byte slice，然后再次枚举；检查 requested root canonical path。
任何观察到的 rename、config/source/marker/content、碰撞、symlink、root replacement 等
变化均拒绝。test-only hook 位于首次 bytes capture 后、Loader/AST/最终 verify 前，确定性
执行 8 种真实文件系统 mutation；不是 public 注入入口或任意 source assertion。
**不声称跨文件 OS 原子性**，不保证防御 hostile writers 的 mutation+restore，最后检查之后
仍可发生变化。返回值是已验证的 immutable capture，不是 filesystem 持续锁或发布凭据。

## 验证和后续边界

未找到 workspace/repo AGENTS.md、`.agents/.codex` instruction 或适用 coding skill；读取
CONTRIBUTING 及 capture/walk、资源、AST/provenance 和完整 PR138 report。用户指定 focused
checks 优先于贡献指南的 broad-suite 建议。官方 Rust `1.95.0 (59807616e 2026-04-14)`、
原 Cargo.lock、`--locked`，无新 dependency。可复现脚本、实际 argv/exit/源码 SHA256 和
日志（仅去除末尾空行）在 `artifacts/checkpoints/python-inventory-capture-20261004/`。

本轮 91 tests passed / 0 failed / 0 ignored：新 integration 9、drift library 1、原组合 6、
原 provenance library 12、模型 42、AST 21。新 integration 用自编 real filesystem fixture，
含 regular package/initializer、conditional duplicate、BOM/CRLF/trivia、ignored marker、scope
omission、raw 多指令、marker absence/collision、config rename/invalid/duplicate/unsupported、
root/child symlink、native/hardlink aliases、FIFO；capture 九个资源及 aggregate output 的 exact
和 one-over，独立 AST/model refusal。focused strict Clippy、workspace fmt 和 diff check 通过。

新 binding 没有存储/缓存失效协议，未建立 serialized input ingestion 的版本化重新推导、
publication guard、DB additive identity column 或 MCP/retrieval 可选协议。旧 UID/qname、parser
extraction/search/resolver 生产行为无改动；只有新 API/module、tests/docs/evidence。
本 slice 尚待独立审查。未执行 publicDEV/100k/holdout、第三方项目代码或 excluded
post_index/broad/private42/GCWALfaults。没有 merge/deploy、release 或质量/规模认证。
正常 origin 交付结果另记 publication receipt。
