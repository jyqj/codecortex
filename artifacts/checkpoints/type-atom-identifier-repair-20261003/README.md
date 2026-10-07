# 合法类型原子回归修复

固定起点 PR103：`da5b05ee08d84fd336d5a98f25da7cae176daebf`。
原版：`ace2bc7983be2955831c9384e44d1bdd0749c909`。委派输入的 `92ace2bc...` 多了 `92`，实际祖先和远端 seed 分支均确认上述 SHA。
冻结独立负证据：`7f650a5f8338e0d1ad2d8e8592ae31c89b34047c`，只读；其 BOUNDED_REJECT、旧断言及哈希均未修改。

唯一生产改动是 `crates/cc-index/src/resolver/helpers.rs::type_atoms`。`resolution_name_keys` 保持 PR103 的非空 leaf 过滤，无须再次改动。仅排除完全由 ASCII 语法标点组成的原子，ASCII 标识符字符 `_`、`$` 和非 ASCII 字符保守保留。没有按四个完整名字硬编码，没有把该 helper 扩为跨语言语法验证器。现有 primitives 和单 ASCII 大写类型参数的语义保持。

语法依据是已安装的 Python grammar `[_\p{XID_Start}][_\p{XID_Continue}]*` 和 JS/TS identifier grammar 的 `_`/`$` 支持。任何原先支持的合法标识符至少含 ASCII 字母、`_`、`$` 或非 ASCII 字符，因此不会被新的语法标点排除条件丢弃。测试穷举 regex 的全部 Unicode XID_Start scalar（保留既有单 ASCII 大写参数排除），PR103 丢弃 `℘` 与 `℮`，修复不丢任何 scalar。重复的 `_`、`$`、`_$`、`$_` 组合与结合字符另有覆盖。PR103 把 `_` 列入“标点负例”的错误断言改为明确的合法名称正例；其余 ellipsis/标点/空 key 负断言保留，原版本失败日志保留。

真实 ParserRegistry 合成 Python/TypeScript 源码 → catalog → 类型边 → resolution manifest，验证 target UID 和 NameBucket，并调用严格 validator。Python `_`、`__`、`℘`、`℮` 在 PR103 同时丢边和 name bucket；TypeScript `$` 等丢边，但 parser 的其他途径仍保留其 name bucket。源码仅解析，不执行业务。Python `ast.parse` 与 `isidentifier` 还在工作区独立确认了合成名字合法性。

| 同一测试（isolated target） | 原版 | PR103 | 修复 |
| --- | --- | --- | --- |
| 合法 parser 标识符的类型边和 name bucket | 通过 | 失败 | 通过 |
| qualified / generic / union 原子 | 通过 | 失败 | 通过 |
| Unicode XID_Start 全域保留 | 通过 | 失败 | 通过 |
| ellipsis / ASCII 语法标点排除 | 失败 | 通过 | 通过 |
| 实际 parser 的 variadic tuple 无伪原子 | 失败 | 通过 | 通过 |
| qualified name keys 无空 leaf | 失败 | 通过 | 通过 |
| 显式空 key 构造/normalize/序列化仍严格失败 | 通过 | 通过 | 通过 |

最终三方日志为 `original.log`、`pr103.log`、`fixed.log`：原版 4 pass/3 expected fail，PR103 4 pass/3 expected fail，修复 7 pass。所有命令、退出码、隔离 target、源码与 test harness SHA256 见 `receipt.json`；baseline 仅附加测试注册，不改函数。先期日志 `diagnostic-*` 保留用于审计，**不计为最终三方结果**。其中 `diagnostic-invalid-shared-cache-original.log` 复用了 PR103 binary，明确无效；隔离 target 重建后原版才展示正确的合法域通过/旧负例失败。

限定检查：resolver 97 pass/1 ignored；model resolution 3 pass；`cargo clippy -p cc-index -p cc-model --all-targets -- -D warnings`、`cargo fmt --all -- --check` 通过。只跑 synthetic/local unit tests，未执行 cc-eval、rank/provider、heldout、业务源码；未改变 Cargo、parser、版本、shared ledger、gold 或 publicholdout。全仓测试与旧 cache 迁移由主集成 owner 负责，本结果不宣称全验。

20 文件/2416 deps 原语料身份仍未确认；独立 review 的公开补充 Requests 为 19 Python/2420 deps，两者不合记。本 session 未重新读取或索引任何 Requests corpus，不给上述 corpus 或迁移追加通过记录。

重建（已能读取两个固定 Git objects 的 checkout）：

```bash
git worktree add --detach /tmp/type-atom-original ace2bc7983be2955831c9384e44d1bdd0749c909
git worktree add --detach /tmp/type-atom-pr103 da5b05ee08d84fd336d5a98f25da7cae176daebf
python3 artifacts/checkpoints/type-atom-identifier-repair-20261003/replay.py \
  --original /tmp/type-atom-original --pr103 /tmp/type-atom-pr103 \
  --fixed "$PWD" --target-prefix /tmp/type-atom-target \
  --output /tmp/type-atom-replay
```

`replay.py` 使用同一 test 文件覆盖两个 isolated baseline 的专属测试，并只追加私有测试模块注册；offline/locked，不读取评测语料。返回 0 仅表示退出码符合上述预期，正式接受仍需新 reviewer 对最终 fixed SHA 独立复验。

交付：修复提交 `778b20deed43db6c11a310a58910ba4b0ed2f3ad` 已推送到 `fix/type-atom-identifier-regression`，首次远端 fetch 核对 SHA/tree 完全一致；随后只追加交付状态文档。创建本修复 draft PR 的一次请求返回 `Post https://api.github.com/graphql: Forbidden`，已停止，无更改权限/credentials 或替代通道。`PR_BODY.md` 是准备好的 PR 文案，`draft-pr.log` 保留原始拒绝，`delivery.json` 保存首次 SHA/tree 核对。新 reviewer 应使用最终分支 head 复验；该 head 的生产函数和测试源码哈希仍与 `receipt.json` 一致。
