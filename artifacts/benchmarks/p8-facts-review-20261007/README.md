# P8-018 声明事实与独立 source-base 审查证据

这是 P8-018 的本地工程准备证据，不能提升 P7/G8 或将原始 P8-018 自动标为 done。
文档/脚本/新反例测试的源提交为
`89b5573fd209b16db9265ca3bc18cdb5c6c8c0c7`。该提交只修改文档、事实脚本和独立测试，
不修改生产 crates、任务状态或 CI。
随后 `ed2fa642091434346be2b4eda9367106d7d48533` 补齐宽枚举/窄解析的拒绝路径，
并修复 STORAGE 顶部版本矛盾。以下事实 audit 按后一个精确源提交重新执行。

## 文档事实检查

`facts-audit.py` 在临时目录复制实际声明和对应文档，调用生产 `scripts/p8_facts.py`，
原始输出见 `facts-audit.json`。共 **13 个 CLI 用例**，实际退出码全部符合预期：

| 用例 | 预期退出码 |
|---|---:|
| 当前声明与受管表/入口文档一致 | 0 |
| 受管工具数被改为 999 | 1 |
| `--write` 从实际声明恢复受管表 | 0 |
| 重新检查恢复后的受管表 | 0 |
| 配置 worker lease 默认值被改错 | 1 |
| `--write` 不能隐藏未修复的配置文档漂移 | 1 |
| 模块能力 JSON 的 schema 与代码常量不符 | 2 |
| 实际 DDL 增加表后要求文档复核 | 1 |
| 带前导空白的 SQL 新表声明不能被忽略 | 2 |
| 小写 SQL 新表声明不能被忽略 | 2 |
| 属性带空白的新增 tool 注册不能被忽略 | 2 |
| 无参数的新增 `#[tool]` 注册不能被忽略 | 2 |
| 不支持的动态 Rust 默认值表达式 | 2 |

同一 audit 在独立的内存 SQLite 中执行实际 `index_v1.sql`，得到 **30 张普通表、5 张
FTS5 虚表**，逐名称核对事实脚本结果；SQLite 内部表和 FTS shadow 表不计入产品表数。
输入文件在执行前后 SHA-256 一致。此项验证不构建或运行产品，不是实际数据库迁移、
完整运行时 schema、provider、安装或发布验收。

可重跑（Python 3.11+，标准库）：

```sh
python3 scripts/p8_facts.py --check
PYTHONDONTWRITEBYTECODE=1 python3 artifacts/benchmarks/p8-facts-review-20261007/facts-audit.py --root "$PWD"
```

`storage-doc-check.json` 另记录 STORAGE.md 顶部的窄修复：当前 schema 25 与实际常量和
能力声明一致；v22 三张语义表和 v21→v22 加法迁移继续作为历史描述保留。
本轮没有宣称全部 internals 文档已审完。

## 独立显式 base 准入审查

**结论：accepted_scoped。** 静态复核与 5 个独立小模型反例没有发现新增准入绕过。
被审查的 `scripts/verify_reviewed_source.py` 字节保留在
`reviewed-source-snapshot.py.txt`，SHA-256 为：

```text
5b57dbd4554454db4bc62f024e032ad4178cad2c152896051ca95fa3bf53bee6
```

审查时根工作树 HEAD 为 `2d47550d9b1a483c7a36c22c01ca094d09ac5ce2`，扩展尚未提交，
所以 `source-base-review.json` 明确记录 `reviewed_worktree_source: true`。
随后根提交 `98990b30fb8f742cf34d04a62d5a66a1f5ffef47` 中该脚本与被审查字节完全一致。

`tests/source_integrity/test_reviewed_base.py` 替换 Git/历史输入 oracle，执行未修改的生产
`approved_union`、pin 验证、before/after 摘要检查、磁盘 review 字节检查和最终 PRODUCT
清单/内容检查。5 个用例分别覆盖：

1. 显式不可变 base 与被接受输入一致时准入成功。
2. 新 base 在触及路径上的 before bytes 不等于已接受输入时拒绝。
3. 独立 base/source 的无关历史字节不会被导入。
4. 最终 PRODUCT 不能夹带该无关历史字节。
5. source diff 不能包含未声明的新路径。

原始日志和收据为 `source-base-review.log` / `source-base-review.json`，5 项全部通过。
本分支测试执行时预加载根工作树的上述精确脚本；本分支原 guard 尚未包含该扩展。
集成根提交 98990b30 后可直接重跑：

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/source_integrity -p test_reviewed_base.py -v
```

本次审查只覆盖新增显式 base 分支。旧的固定历史 pins、全树最终输入核验、helper 字节
约束和 CI 要求保留，完整历史 source verifier 重放仍由独立的原套件负责，不能用这 5 项替代。
