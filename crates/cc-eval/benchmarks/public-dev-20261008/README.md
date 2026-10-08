# 600 题公开 DEV 原生语料入口

本入口把历史已接受的 **301 道公开 DEV 问题**与本次完整交叉复核的
**Serde 150 道、Vite 149 道**连接起来。总计 **600 道 native 问题**，
覆盖 6 个真实仓库；其 **554 道 compatible 记录**是同一批可回答问题的路径投影，
不另计问题数量。46 道有明确范围的无答案问题只进入 native 分母。

[index.json](index.json) 是机器可读的固定登记表。
[p8_native_registry.py](../../../../scripts/p8_native_registry.py) 会核对所有输入，
导出可供现有 `cc-eval` 使用的 suite，并真实调用原生 `validate`。
它不修改旧题、历史审批、评分器、锁定值或原有 source guard。

## 实际覆盖与审批范围

| 仓库 | Native | Compatible | 无答案 | Family 标签 | 去重后的已准入源文件 | 来源模式 |
| --- | ---: | ---: | ---: | ---: | ---: | --- |
| Express | 70 | 59 | 11 | 70 | 7 | 历史固定 Git blob 快照 |
| Requests | 91 | 83 | 8 | 91 | 20 | 历史固定 Git blob 快照 |
| Gin | 67 | 55 | 12 | 67 | 53 | 历史固定 Git blob 快照 |
| TypeScript | 73 | 59 | 14 | 73 | 20 | 历史固定 Git blob 快照，5 个题块 |
| [Serde](serde/README.md) | 150 | 149 | 1 | 118 | 60 | 干净的固定上游 Git checkout |
| [Vite](vite/README.md) | 149 | 149 | 0 | 138 | 148 | 干净的固定上游 Git checkout |
| **合计** | **600** | **554** | **46** | **557** | **308** | 两种来源模式分别保留 |

原有 301 道题的审批来自固定历史 admission、review、来源与许可对象。
本次重新核对其精确字节、完整允许清单、投影、source span 和原生锁，
保留原审批的 snapshot 范围；不把它描述为本次重新阅读的 301 道新题。

新增 299 道题由作者实际阅读固定源代码编制，再由另一 agent **逐题**阅读题义、
主/支持答案、必要 facets、反例和相关上下文。Serde 作者为
`/root/p8_corpus_closeout`、独立审阅者为 `/root/build_validation`；
Vite 作者与审阅者相反。两份完整独审收据绑定最终 query 和 source manifest
的 SHA-256，并覆盖所有问题 ID；作者原有 `pending` 字段保持原字节。
注册器根据外置独审收据接受数据，不把作者自己填写的状态当作独立审批。

原有 301 个 family 标签映射到 **280 个历史保守相关分量**，注册器重新核对其
完整成员。新增题分别保留 118 和 138 个 family 标签，同一意图的变体不重复
计算 family。全局 557 个标签只属于 DEV，没有跨仓库标签冲突。
这不证明 557 个统计独立样本，也没有把 280 个旧分量加上新标签数当作新的
全局独立样本数量。原目标中的约 600 道 reviewed questions 不被改写成
600 个独立 family。

实际按题的 `language` 标签统计如下；配置和 HTML 问题按答案文件类型如实标注：

| 标签 | 问题数 | 标签 | 问题数 | 标签 | 问题数 |
| --- | ---: | --- | ---: | --- | ---: |
| Rust | 145 | JavaScript | 70 | TypeScript | 214 |
| Python | 91 | Go | 67 | mixed | 5 |
| TOML | 5 | JSON | 2 | HTML | 1 |

10 个类别全部有实际问题：`api_usage` 54、`architecture_understanding` 81、
`call_chain` 83、`component_location` 36、`configuration_lookup` 34、
`cross_language` 5、`error_handling` 102、`file_exact_match` 4、
`semantic_feature` 163、`symbol_location` 38。5 道跨语言题来自 Vite 的实际
JS/TS、HTML、CSS、WASM 连接；Serde 不冒充跨语言覆盖。
这些类别分布并不均匀，不能由“都有覆盖”推断每类已形成充分的质量样本。

## 固定输入与导出方式

登记表显式记录了：

- 历史协议锚点 `5385f5a7a2a875c6d5cbd049bdde039bf71bbf32`，
  以及 **183 个**允许读取的 `完整 commit:path → SHA-256` 对象。
  其中包括 admission、来源锁、历史相关分量、审批、DEV suites/queries、
  source manifest、源文件和许可资料。
- 原有 `p8_corpus_audit.py` 与 `p8_release_evidence.py` 的精确哈希。
  注册器复用原来的 `PinnedBlobs`、`query_rows`、`source_check` 和
  `check_projection`，没有替换或放宽这些谓词。
- 两个新语料目录内 **全部 40 个文件**的 SHA-256，包括最终题体、两个评分
  入口、来源清单、许可资料、完整审阅与原始冻结/验证回执。
- 此次固定 release `cc-eval` 的身份和编译来源：
  `fb772551cff6b4620a6fcdb94c57b78350cebb33`，798 个产品输入，
  输入清单 SHA-256
  `ef3fcf15da0523391b9c25abbc589b3c14498896eff581df416228619ef22aa6`；
  二进制 SHA-256
  `554d1baefeadd367614a0755a94b2db5a6194b284a83c03b7d8fe172368dd33d`。

旧数据导出到 `OUTPUT/legacy/COMMIT/原仓库相对路径`，保持 suite 和 query
原字节、原目录关系、`source.commit = null` 及原相对 `source.root`。
这些源是允许的历史快照，不伪造为当前干净上游 Git checkout。
原生和 compatible suite 的文件域可能不同，注册器保留每一份原文件清单。

新数据导出到 `OUTPUT/current/原仓库相对路径`。只在 **导出的 suite 副本**里
将 `source.root` 改为显式指定的上游目录；query 文件仍是被审阅的精确字节。
`source.commit`、source/query digest、文件域、评分 profile、top-k、重复次数
及其他配置全部保持。原生 `validate` 在新位置再次核对 BLAKE3 锁与 source。
注册器不调用 `freeze`，不向 source 根写文件。

## 重建与运行

使用包含本登记表的 CodeCortex checkout，以及记录的固定 release 二进制。
两个上游源可按以下命令建立；不需要安装或执行上游仓库的依赖：

```sh
git init /absolute/path/upstream-serde
git -C /absolute/path/upstream-serde remote add origin https://github.com/serde-rs/serde.git
git -C /absolute/path/upstream-serde fetch --depth 1 origin 6693a89cca77e0151437da1c7f890090b9ebf04c
git -C /absolute/path/upstream-serde checkout --detach 6693a89cca77e0151437da1c7f890090b9ebf04c

git init /absolute/path/upstream-vite
git -C /absolute/path/upstream-vite remote add origin https://github.com/vitejs/vite.git
git -C /absolute/path/upstream-vite fetch --depth 1 origin 10033218d239c927cdc375970b5741cce408e81b
git -C /absolute/path/upstream-vite checkout --detach 10033218d239c927cdc375970b5741cce408e81b
```

CodeCortex 的本地 Git 对象库还必须包含 index 中列出的历史完整 commits。
如完整克隆仍缺对象，可从原 PR 或保存的 Git 归档恢复；也可对登记表列出的
固定 SHA 显式执行 `git fetch origin 完整SHA`。对象不可用时注册器会失败，
不会改读当前工作树中的同名文件，也不会自动 fetch 或遍历未登记题库。

从 CodeCortex 根目录运行，输出目录必须不存在并位于代码和上游目录之外：

```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/p8_native_registry.py \
  --cc-eval /absolute/path/frozen-binaries/cc-eval \
  --serde-source /absolute/path/upstream-serde \
  --vite-source /absolute/path/upstream-vite \
  --output-dir /absolute/path/new-public-dev-validation
```

该版本要求二进制哈希与 index 中的固定 release 身份相同。不同机器重新
编译的字节可能不同，不能用另一二进制冒充这次回执；新的二进制实验需要另行
登记来源与身份。构建命令本身为 Rust 1.95 下的
`cargo build --locked --release -p cc-eval --bin cc-eval`。

成功输出包含 `coverage.json`、`suites.json`、`registry-validation.json`、
20 份逐调用原始日志和完整允许输入的导出目录。`suites.json` 给出每个现有
suite 的绝对路径，可直接用于原有 `cc-eval` 命令。每次 `validate` 的命令、
退出码、耗时、日志哈希和行数均保留；运行后的源、原数据、导出数据和二进制
重新核对。失败不覆盖旧回执，不重 freeze，不把 `not_run` 当作通过。

## 本次已执行的验证

固定 release 验证器已真实执行 **20 次 validate，20 次退出 0**：
旧语料 16 套、两新仓库 4 套；合计 10 个 native suite、10 个 compatible suite。
每份原始日志均报告其预期 query 数、文件数和 `locks valid`。
600/554 总数、46 道无答案、完整 ID、同 family 的 DEV 范围、554 条精确投影、
所有 source/manifest/query 哈希均通过。规范化题面完全重复组数为 0；这项
文本检查不替代语义上的相关性判断。

新增 6 个测试包含外置审批与作者自签、审批 hash/ID/逐题状态漂移、全局重复
ID/非 DEV/兼容答案漂移、同长度源篡改、父目录 symlink、只迁移 source.root，
以及原生验证失败必须保留且不得转换成 freeze 等负控。

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest scripts/tests/test_p8_native_registry.py -v
```

此入口完成公开 DEV 数据的登记、复用与原生准入验证。它不运行 retrieval，
不读取任何受保护题体，也不单凭题数证明检索质量、独立样本量、规模性能或
release gate。Facets、graph constraints 与 hard negatives 保留现有原生评分器
实际支持的约束范围，额外文字注释不会被声称为新增的自动评分门禁。
