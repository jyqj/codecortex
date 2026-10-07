# P8-002 / P8-004：公开开发语料与边界审计

本轮交付两个可执行审计入口，推进原有 P8-002、P8-004。它们使用标准库，只读固定 Git 对象和当前源码；不构建新的 Rust target，不联网，不改题、改标签或改排名，也不访问 holdout 正文。**这两个原始任务仍待完整验收；本轮没有认证 G8 或正式发布。**

## 输入身份与实际执行

原始要求见 [04-PHASES.md](04-PHASES.md)、[06-VALIDATION.md](06-VALIDATION.md) 和 [09-BENCHMARK.md](09-BENCHMARK.md)。公开开发语料的固定入口是历史提交 `5385f5a7a2a875c6d5cbd049bdde039bf71bbf32` 中的 `protocol/global-dev-review/typescript-extension/admission.json`，SHA256 为 `b4388ca60612f6734719b348bf30e05dd0752b7b754f47cc078b590e35c1e425`。

该历史入口记录四仓开发快照的独立审阅范围，但其语料目录没有整体纳入本轮主线。审计通过其中 179 个精确 `commit:path → SHA256` pin，重新读取公开 dev、源码、许可和既有审阅回执；另验证 4 个协议控制文件，共 183 个对象、3,029,158 字节。没有将旧报告中的“通过”字段直接当作本次运行结果。每个实际输入的 SHA256、当前工作树 HEAD、审计脚本 SHA256 与本次时间均写入新回执。

代码限制了固定完整 commit、允许的 public-v19 路径、单文件和总字节数；拒绝未列入 pin 的文件、符号链接、路径逃逸、重复 JSON key、非 dev 查询和 hash 漂移。缺失 Git 对象只返回输入无效，不自动 fetch。读取不会把历史目录展开到工作树。

运行位置是仓库根目录；输出必须使用新文件名：

```bash
PYTHONDONTWRITEBYTECODE=1 python3 scripts/p8_corpus_audit.py \
  --output /tmp/p8-public-dev-audit-unique.json
PYTHONDONTWRITEBYTECODE=1 python3 scripts/p8_leakage_audit.py \
  --output /tmp/p8-public-boundary-audit-unique.json
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover \
  -s scripts/tests -p 'test_p8_*audit.py' -v
```

退出码 `0` 只表示该脚本声明的局部审计通过；`1` 表示有效执行发现漂移或需要审查的边界命中；`2` 表示输入或执行无效。输出已有时拒绝覆盖。`cc-eval` 自身使用 BLAKE3 锁，本次 Python 审计使用既有 SHA256 证据 pin，并检查实际源码、跨度、投影；它不替代 `cc-eval validate` 或任何评分器。

本轮新证据保存在 `artifacts/checkpoints/p8-next-ten-20261008/corpus/`：`corpus-audit.json` 是当前语料审计，`final/` 中的 `leakage-audit.json`、`tests.log`、`commands.json` 与 `boundary-triage.json` 是补齐 workspace 继承依赖解析后的边界审计。第一版原始回执保留，`commands.json` 分别标明实际退出码。

## P8-002：多仓公开开发语料的实际覆盖

| 固定开发仓库 | Native 题 | Compat 题 | Native 无答案题 | 源码文件 | 核验答案跨度 |
|---|---:|---:|---:|---:|---:|
| Express / JavaScript | 70 | 59 | 11 | 7 | 110 |
| Requests / Python | 91 | 83 | 8 | 20 | 141 |
| Gin / Go | 67 | 55 | 12 | 53 | 127 |
| TypeScript | 73 | 59 | 14 | 20 | 98 |
| 合计 | 301 | 256 | 45 | 100 | 476 |

256 条 compat 都可映射到对应 native 的有答案题；查询身份、类别、语言、难度、split、路径范围和主答案优先的路径顺序一致。45 条 native 无答案题不进入 compat 分母。每条答案的路径必须属于冻结源码范围；跨度检查边界和 UTF-8 对齐，有源码证据 hash 的注记另核对片段、文件或正文。

固定题库实际有 301 个 family 标签；历史全局相关组件登记将其保守聚合成 280 个组件。本次验证登记的成员完整且不重复，**未新证明 280 个统计独立样本**。规范化后查询的完全重复组为 0；这不等同于语义改写没有相关性。

| 实际类别 | Native 题数 |
|---|---:|
| api_usage | 24 |
| architecture_understanding | 46 |
| call_chain | 58 |
| component_location | 26 |
| configuration_lookup | 2 |
| error_handling | 30 |
| semantic_feature | 90 |
| symbol_location | 25 |

十类目标中的 `cross_language` 和 `file_exact_match` 尚无题；Rust、serde、vite 与 mixed monorepo 覆盖仍缺。语料实际题数、字符串标签和审阅层级分别报告，不能用重复执行次数、自己的 synthetic fixture 或已有 56 道 own-source/fixture 题来补足外部约 600 个审阅问题的目标。

作者原始注记中 301 条 `review_status` 仍为 `pending`，历史独立回执另记录 301 条已接受的开发内容。本次保持这两个事实，不修改 gold 来回填审阅状态。工具验证回执及输入字节的绑定，**新增独立语义复核数为 0、正式验收的 600-family 数为 0**。因此 formal shortfall 仍报告 600；不能简单以 600 减去 301 宣称剩余 299 个正式独立问题。

源码是部分公开快照。Requests manifest 还含未索引的 LICENSE/NOTICE 元数据；Gin manifest 含被明确排除的测试、文档和未纳入许可范围的文件。工具仅将 suite 明确列出且有冻结 source pin 的 100 个文件计入本次源码域，不把 manifest 所有记录都当作被测输入，不声称重新验证了完整上游 checkout、依赖闭包或许可法律结论。

## P8-004：holdout、改写关联与生产边界

`p8_leakage_audit.py` 的关联检查只接收 `id_sha256/family/split/query_sha256` 四字段的无正文元数据。它拒绝混入 query/gold 字段，检查同 family、相同规范化查询指纹及登记组件跨 split 的情况；重复 ID、重复组件成员、登记缺口和未知 family 都不能被字典覆盖隐去。本次输入全部是已公开 dev，301 条记录与 280 个组件的这些机械检查未发现错误。

生产检查读取除 `cc-eval` 外各 crate 的 `src/`、`Cargo.toml` 与 `build.rs`，并读取根 `Cargo.toml` 解析 workspace 继承依赖，包含内联测试和注释。它查找具体公开 dev 的完整查询字面量、足够明确的 gold 路径字面量、public-v19 标识、benchmark include，以及普通/构建/平台依赖中通过别名或 workspace 继承引入 `cc-eval` 的情形；命中报告仅保存位置、规则和摘要，不复制题目正文。实际扫描文件和字节总数由 `final/leakage-audit.json` 的 `production_scan` 提供。

实际原始结果为 **`boundary_review_required` / exit 1**：`crates/cc-db/src/index_db_arch.rs` 第 727、728 行各有一处 `lib/utils.js`。人工定位显示，两处都在第 610 行 `#[cfg(test)]` 约束的 `mod tests` 中，一个是注释，一个是 `test_extract_package_from_path` 对通用目录处理的断言，未见这里使用 gold 路径改变生产排序。`boundary-triage.json` 记录精确源文件 hash 和这个判断。工具仍保存两条原始命中，不增加绕过词表，也不把人工判断改写为全产品无泄漏证明。

静态签名检查不能证明不存在语义过拟合、任意宏展开后的字符串或未检查的生成文件，亦不证明发布二进制的包内容。没有 frozen-config heldout 运行。

既有 custody 决策的 SHA256 为 `a8c88f2479843566d74a8a47f05da81da0588029dffa681c313e64939f344f9c`。它记载 117 个 would-be holdout ID、至少 72 个曾暴露 ID，clean holdout 认证数为 0。其余 45 个只是暴露未确认，不能因此判定干净。本轮只重读该无正文决定的字节，保持 `custody_blocked` 和已有 quarantine 语义，没有检查或变更权限、凭据和可见性。

## 正负对照与仍待验收

31 个标准库测试覆盖真实小型 Git 仓库固定对象读取、同长度内容损坏、错误 hash、符号链接、禁止路径在 Git 调用前被拒绝、字节上限、重复 ID、投影顺序和分母、源码越界与 UTF-8 边界。泄漏测试使用纯合成无正文元数据，验证翻译 family 和传递相关组件跨 split、相同指纹泄漏、组件缺项，以及注入查询词典、raw/escaped 字符串、benchmark include、重命名/平台构建/workspace 继承依赖的正反例。

P8-002 下一步仍需满足 P8-001 依赖、完整源准入与新增独立 gold 复核、六仓/目标语言及类别扩充、正式 V02/V19 和质量证据。P8-004 下一步仍需 P8-003 依赖、独立且可验证的 holdout custody、冻结配置后的一次 heldout 执行，以及人工语义改写/标签变更审查。当前主线的 corpus/source 接受、已有 P7/V19 质量与其他开放 PR 状态，均不能由本次局部审计替代。
