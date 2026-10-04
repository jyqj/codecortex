# Python inventory capture v1 独立审查

审查结论：**request changes**，一项 P2 配置准入缺陷。该结论仅覆盖下面的固定源码、契约和有界自编 fixtures，不是 release、规模或安全认证。

审查 product/source：`8d2b312c066f0514fb5cee555767955bf34708d0`。
审查 delivery：`892fd103f543424323e1d4e6691eca5774ede1a6`。
正常 origin fetch 后 `refs/pull/139/head` 与该 delivery 精确相同。
已完整读取 `docs/internals/python-inventory-capture-v1.md`、capture 主文件、native walk/verify、Loader、Python provenance、borrowed declaration model 和 AST adapter 的相关实现，以及原交付 README。
workspace/repo 内没有 AGENTS.md；`.agents`/`.codex` 无指令文件。读取 CONTRIBUTING；用户指定的 focused 检查优先于 broad-suite 建议。读取 Agent Architecture skill，依其要求区分源码证据、真实 fixtures 和未验证声明。没有派出子 agent。

## P2：有显式 package-dir 时，partial/default find 指令绕过拒绝

位置：`crates/cc-index/src/project_model/python_inventory.rs:312-319` 的 `supported_document` 仅验证 find object 的 key whitelist，没有要求存在非空 `where`；`python.rs:111-137` 只在 where 存在时投影 array，空 array 不产生 error。另一 package-dir 提供一个 root 时，legacy provenance 仍为 Explicit、limitations 为空，bridge 因此成功准入。

两个独立真实文件系统复现：

```toml
[tool.setuptools.package-dir]
"" = 'lib'
[tool.setuptools.packages.find]
# where absent
```

以及相同配置追加 `where=[]`。完整 fixture 实际有 `lib/reef` package、其他文件和隐藏目录。两个调用均返回成功、单根 `lib`，provenance 只有 package-dir evidence；find 没有完整 collection-root evidence。

契约明确只支持空-key package-dir 和 **find.where collection-root directives**，要求 defaults/partial 拒绝。允许另一个显式 directive 掩盖 absent/empty find，使这个 conservative admission 承诺失效。这里不推断 build backend 实际包集合，也没有执行 setuptools/Python。P2 是文档所声明配置白名单/partial 拒绝契约的缺口，而不是旧 provenance 的生产行为变更。

最小修复建议（本次没有修改产品）：在 opt-in `supported_document` 中，如果出现 packages/find，要求非空 where array 且每项都是受支持的明确 root；继续由原 provenance 校验 normalized root 一致性和全部 evidence。分别测试 absent where、empty where，以及只有 find.where/两个一致指令的成功路径。不要修改旧 extraction/UID/qname 或 legacy provenance 默认政策来修复新 bridge。

`review_contract_partial_find_must_refuse` 是刻意保留的红色契约测试；两个 admitted case 都被收集后断言失败，不把该失败写成通过。

## 新的独立验证

环境官方 rustup 安装路径使用 `static.rust-lang.org`（setup script 可核验），实际 `rustc 1.95.0 (59807616e 2026-04-14)`；原 Cargo.lock、`--locked`，没有增加依赖。`run.py` 在 tests 目录临时生成测试 target，逐 byte 复制 subject 原源码作为前缀，仅追加独立 test module，以使用已有 private after-capture hook；native/原 tests 文件也逐 byte 复制。Loader/provenance 等支持模块直接引用原 source。只运行 `review_` filter，结束时清理临时 target。fixture 正常成功结果另与公开产品 API 的实际调用逐项比较。没有注入产品 public API、任意 inventory assertion 或新解析器。

复现：

```sh
python3 artifacts/checkpoints/python-inventory-independent-20261004/run.py
```

最终 **13 tests：12 passed / 1 failed / 0 ignored / 3 filtered**，exit 101；唯一 failure 为上述 P2。完整日志 `tests.log`，argv/exit/源码与 Cargo.lock SHA256 在 `validation.json`。

独立覆盖：

- EntireProject 包括真实 `.git/HEAD`、`.cache/data`、`.hidden`、ignore 文件及其忽略的 marker，含非 UTF-8 内容的 regular file；空目录也参与 entries/path 预算。无过滤后的 catalog；subtree/exclusion 拒绝。
- BOM/CRLF/trivia 原字节的 digest、AST name spans、同一 immutable inventory 的 borrowed model；三个原始 directive/value 完整保存（`./lib`、`lib/./`、`lib`）。返回对象只读，旧 capture 在文件变更后仍保持原有内容/identity。
- marker absence → NamespaceAncestry；module/package sibling collision → ModulePackageCollision；`.git`、cache、config、marker 内容变更均使全 scope fingerprint 失效。未遗漏 filtered marker。
- symlink file/parent、FIFO、scope 外 hardlink、大小写 collision、non-UTF8 filename、Unicode、Windows reserved basename、反斜线/冒号 native aliases 被拒绝。root symlink alias 被明确接受。配置 root 的 native 反斜线候选实际也拒绝，未发现这个候选缺陷。
- 默认配置、named mapping、重复 TOML、多个 root、include/namespaces、Poetry、package selector、hidden/cache nested pyproject 拒绝。AST whole-file syntax/resource failure 与 model failure 丢弃整体结果，没有可返回的部分 capture。
- entries/files/total bytes/file bytes/depth/path bytes/total path bytes/output count/output JSON bytes 九项 exact 通过、actual 比 limit 多 1 拒绝。真实 Derived + Unavailable + 空数组的 JSON byte length 与 meter 一致；额外验证空 `{}` exact=2，以及 root initializer Unavailable/空文件数组。
- 六项 AST 参数及七项模型参数的 fixture 最小成功阈值与 one-over rejection。AST 阈值：source_bytes=96、visited_nodes=36、tree_depth=7、declarations=4、output_segments=6、output_text_bytes=351；模型：files=8、total_bytes=310、file_bytes=110、roots=1、evidence=1、ancestry_depth=2、identifier_bytes=7。二分得到 fixture 阈值，不推广为其他输入或 parser deadline 保证。
- 十二种 deterministic after-capture 实际 mutation：source/config 内容、source/config rename、marker removal/collision、root rename+replacement、source symlink replacement、mode change、外部 hardlink、空目录 addition、`.git/HEAD` change。观察结果为 Drift（10）、SymlinkOrNonregular（1）、Alias（1）；明确覆盖 bytes/identity/directory/mode/link 元数据混合变化。不是 hostile writer 的 atomicity 测试。

## 静态审查与边界

native readdir 流式遍历；entry/path/depth 在复制 key/open 前检查，regular sizes 完整通过 capture 与 DeclarationLimits inventory preflight 后，才调用 content read/hash/Loader。真实 invalid-config/broken-AST fixture 加 late inventory/model 预算限制，仍先得到预算拒绝；顺序也由源码核验。没有分配整个 outcome serialized JSON 来量大小；key、空数组、对象/数组分隔符、Unavailable 都逐 writer 计数，每个 outcome 加入最终 vector 前检查。逻辑限制不等同 allocator capacity、RSS 或 TOML/tree-sitter 峰值上限。

walk 每个 child descriptor 执行 statx mount-ID 比较，并检查 root ID；nofollow descriptor walk、打开后 metadata kind、regular nlink=1、dev/ino uniqueness、ASCII portable policy、scope 内 casefold collision 均存在。**没有运行真实 bind mount/cross-device mount 分支，也没有模拟该分支的 integration success**；普通 Linux fixture 仅证明本机 statx 正常路径。没有系统安全/挂载变更。macOS/Windows、seccomp/statx 不可用或恶意 FUSE/远程 metadata 未验证。

最终 verify 重枚举、比较 stamps，逐文件重读完整 bytes，再枚举。root canonical spelling/identity 和 mode/link/size/mtime/ctime 被观察；atime 不用于 drift。返回是验证过的 immutable capture，**最终检查不是持续锁**。没有跨文件原子、hostile mutation+restore 防御、RSS 硬上限或硬 deadline 结论；parse timeout exact/one-over 不可由这些 fixtures 确定。

## EntireProject 的实际可用性与未来 scope

把 `.git`/cache 纳入 scope 是当前明确的 conservative 正确性选择。它会带来大仓预算拒绝、ASCII/hardlink 政策拒绝和正常 Git churn 的 binding 失效；有 `.git` 内 nested pyproject 或 `.py` 时也会触发配置/AST 政策。这里没有把这些真实仓可用性问题误称为枚举缺陷，也没有通过排除 `.git` 假装得到相同 EntireProject identity。测试实际证明 `.git/HEAD` 改变 fingerprint；它不是稳定 repository identity。

最小未来设计可单独声明一个 versioned `DeclaredPythonCollection`：明确 owner、一个 normalized collection directory、所有支持 root 的配置 bytes/evidence，以及该目录完整 children closure。closure 必须覆盖所有 source/marker/相邻 `.py` 和 initializer collision 候选及 absence 所需枚举；不能使用 ignore-filtered source list。限定 source root 为 `lib` 时，外部 `.git` 是另一种授权/绑定 scope，而不是 EntireProject 的例外。若 collection root 就是项目根，排除 `.git` 等还需单独声明固定 scope policy、证明相关候选闭包并编码在 binding 内；不能静默排除或预算失败后缩小。相应边界 drift、scope/config/version、缓存/serialized ingestion/publication 的重新推导协议也须独立定义；本 slice 不含这些实现。正常 Git churn 在现有 EntireProject 下依旧失效。

## 不变量与交付

source 与 delivery 的 `crates`/Cargo.lock 完全相同。相对 reviewed base `50a4933e48ef20b16401ac8f75c386a660aa8e5c`，crate 变化仅新增 python_inventory module/tests 和 `project_model/mod.rs` 的一行 export。既有 model、AST adapter、extraction/search/UID/qname 和 Cargo.lock 未变。本独审仅新增自己的 tests/脚本/report/evidence，不编辑 product 或权威 roadmap。

没有运行 public gold/DEV/100k、第三方项目代码、excluded old runtime/post_index、broad suites、private42 或 GC WAL faults。没有 merge/deploy。此次正常 origin commit/push 与唯一 draft attempt 的结果单独记录 `publication.json`；实际拒绝即停止该操作，不换 credential/connector/remote 绕行。
