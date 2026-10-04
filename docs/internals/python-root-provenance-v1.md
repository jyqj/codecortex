# Python 源根 provenance v1

范围：在 `37dd042eaa1209a86e0cafdcd92ae77e036e76f5` 上给
`PythonProject` 增加输入证据。没有 declaration identity、DB/MCP 接线、
qname/UID/ranking 或 resolver 规则变更。与纯模型
`866cbed73f303463a80cee07462824d1816f2a44` 的文件修改无重叠；
不把其 `ConfiguredRoot` 构造器接入生产。

## 固定来源与绑定

`discover` 继续使用同一个 `config_cache::Loader` 捕获和解析 TOML；
Python adapter 同时借用 `documents` 和对应 `ConfigInput`，不读磁盘、不二次解析。
`PythonConfigBinding.path` 是实际原始捕获 key，digest 是原始完整 bytes 的 BLAKE3，
包括注释和空白，不是 normalized roots 或重序列化 TOML 的摘要。
`captured_document` 仅表示捕获有合法格式摘要、parsed 与 adapter document 相等且无
capture error；adapter 信任已有 Loader，而不声称能从一个任意构造的 ConfigInput
证明磁盘来源。publication 仍通过 `CapturedProject::verify` 重读绑定的配置。

每个 normalized root 保留全部已采纳的 `PythonRootEvidence`，包含原始字符串值与
确切 JSON pointer；array evidence 带原始 index。空 package-dir key 的 pointer 为
`/tool/setuptools/package-dir/`（末尾 slash 表示空 key）。`./src`、`src`、`src/./`
可归并成同一个 root，但证据不去重。跨 setuptools/Poetry 指令同样全部保留。
配置移动改变 path，任何 bytes 修改改变 digest，即使最终 roots 不变。

| state | 含义 |
| --- | --- |
| Unknown / provenance 缺失 | 旧记录或未知证据，绝不能反推 explicit |
| NoConfig | 此捕获没有该 scope 配置，沿用仓库 `.`/`src` 默认策略；不是文件系统 absence proof |
| InferredDefaults | 配置可读且所检查指令没有 explicit root，采用默认 roots |
| Explicit | 所支持的 root 指令可绑定，无已检测 limitation；不是完整 build config 语义验证或 identity admission |
| PartialOrUnsupported | roots 可与错误共存；缺失/不匹配 capture、named package-dir、非法 shape、outside root、预算截断等 |
| InvalidConfig | TOML/UTF-8/read 等错误，或不是 TOML document；默认 roots 仍然 inferred |

legacy roots 顺序、去重、fallback 时点与 diagnostics 不变，包括只发现 outside root 时
归一化后 roots 为空而不二次 fallback。新 limitation 不改变 resolver 的状态。
setuptools `where` 超过 32 项仍拒绝整个数组并记录旧 `invalid_python_where`；额外
limitation 明确超限。Poetry 超过 32 项仍只收前 32 项并记录 `python_root_limit`。
截断后仅保存已采纳的 evidence，不声称剩余指令已捕获。Poetry 缺失/非法 `from` 或
缺失/非法 `include`、非 table/array 形状均标记 limitation，不能把局部 explicit
根提升为完整配置事实。这里没有运行 setuptools/Poetry 或验证完整发布配置。

## 捕获与 admission 的真实边界

本改动没有任何 identity-admissible 输出或 conversion API。未来 adapter 至少必须
重新推导 v1 evidence，要求真实 path/digest/document 绑定、非 inferred、无 limitation，
并独立完成以下证明。只看一个 `Explicit` enum 或 `captured_document=true` 不够。

- 配置 capture 是有界文档 capture，不是完整源码 inventory。source catalog 来自
  shared walk 或 scoped catalog；ignored/excluded/未扫描文件、package marker absence、
  module/package collision、完整 owner inventory 不能由 config documents 推导。
- Loader 每份配置 1 MiB、总读取 16 MiB、1024 个输入；count/total 超限中止捕获。
  个别 size/read/parse 错误保留 Invalid evidence。配置 UTF-8、TOML duplicate key 检查由
  已有 parser 完成；snapshot validate/digest 与 publish verify 沿用原实现。
- Unix 的 `input_file::read` canonicalize 可信 project root，然后 descriptor-relative
  `openat`/`NOFOLLOW` 读取；拒绝 root 以下任意 symlink component 和非 regular file。
  这不验证 source-root 字符串实际对应的目录或源码文件，也不是跨文件原子 snapshot。
  非 Unix 仅 checked path walk，不能声明 race-proof；本次 Linux 验证不能替代 Windows 测试。
- 捕获 key 必须满足原有 lexical canonical repo-path policy；normalized roots 仅 lexical
  join，不是 filesystem realpath。project root 自身 alias/symlink 可 canonicalize 接受；
  hardlink、case folding、Unicode normalization、bind mount 或平台路径别名未检测唯一性。
  不保留 inode/native raw bytes 映射，不能据此声明 native spelling 唯一。
- BTreeMap 入参的相同 key 已在建图前覆盖，无法追溯 duplicate records；Loader 重复
  同 key 复用捕获不是 duplicate inventory 检查。不同 hardlink config path 可有相同
  digest；保留不同 path，不把相同 digest 当作相同来源。legacy suffix 匹配可遇到
  非标准 pyproject 名或同 scope 多 document，provenance 显式标记 limitation。
- pure prototype v1 不支持 multiple roots。normalized root 合并不等于可丢掉 supporting
  directives；未来转换必须保留完整 binding。根 `.` 的生产 lexical 值是空字符串，
  pure prototype 构造器当前拒绝空根，转换策略须另行明确。当前没有自动转换。
- declaration AST 完整 ancestry/kind/range、源码摘要、owner、source inventory、namespace
  和 collision 规则均不在此改动验证范围内。也没有改变现存 declaration/qname 路径。

## 版本与 fingerprint

`PYTHON_ROOT_PROVENANCE_VERSION = 1` 版本化派生 evidence；旧 PythonProject JSON
缺失 `provenance` 时默认为空，旧证据不升级为 explicit。新增 evidence 结构的缺失字段
默认 Unknown/version 0/captured_document false；serialized evidence 仍须在 ingestion
重新推导，不能作为授权。

`PROJECT_MODEL_VERSION = 3`、`PROJECT_INPUT_KEY` 和 `ProjectInputs` wire 未改；
持久化 snapshot 存的是已有 ConfigInput，不存 PythonProject，所以无需迁移或改变
现有 input digest 算法。ProjectModel 的 JSON 增加非空 provenance，若消费者对整个
派生 JSON 求 hash，其 fingerprint 会改变，不能声称字节兼容。原配置 path/bytes
变化已经改变 ProjectInputs digest；future identity fingerprint 必须绑定所有 evidence
和 inventory，不能只 hash normalized roots。原 SymbolRecord wire/UID 不受新字段影响。

## 聚焦验证

使用官方 `rustc 1.95.0 (59807616e 2026-04-14)`，Cargo.lock 不变。
自编 fixtures 覆盖 `.`、`./src`、无配置/默认/invalid、nested、Poetry、setuptools
重复及多指令、多根、outside、rename/comment change、32 项预算、missing/stale/error
capture、UTF-8/size/duplicate TOML、symlink/parent symlink/hardlink/path policy，真实
`discover` parse-cache reuse 与 publication verify，以及 resolver 不读取 provenance。
具体命令和结果随提交说明提供。不运行 excluded post_index/broad/private42/
GCWALfaults、gold/scorer/eval100k 或 merge/deploy；未测规模性能。

本次验证结果（全部 `--locked`，无 ignored）：

```sh
cargo +1.95.0 test --locked -p cc-index --lib python_provenance
# 12 passed; 390 unrelated tests filtered out
cargo +1.95.0 test --locked -p cc-index --test p3c_modules python_uncaptured_environment_differs_from_missing_local_submodule
# 1 passed; 13 unrelated tests filtered out
cargo +1.95.0 clippy --locked -p cc-index --lib -- -D warnings
# exit 0
cargo +1.95.0 fmt --all -- --check
# exit 0
git diff --check
# exit 0
```

临时 detached checkout 基于相同 base，应用纯 prototype 和 independent review 的
原始补丁，并复制本改动的 `module_inputs.rs`；原分支没有合入二者：

```sh
cargo +1.95.0 test --locked -p cc-model --test declaration_identity_v1 --test declaration_identity_independent_review
# 15 + 11 passed
```

这证明 additive model 可以共存且纯模型测试通过，不是生产 identity 接线验证。
源码变化只在 PythonProject 模型、Python adapter/shared capture 调用、聚焦测试与本文件。
AGENTS.md / *.agents 文件搜索未发现项目指令文件，遵守 CONTRIBUTING.md 的 1.95
要求；用户限定的 focused checks 优先于其 broad-suite 建议。
