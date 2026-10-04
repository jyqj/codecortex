# Requests native 零分字段诊断，2026-10-04

首要结论：**candidate 已返回正确、源码绑定的声明，但产品 qname 与准入 benchmark qname 使用不同命名空间，严格 scorer 因而全拒绝。**这不是本轮所有 qname 缺失，也不是 v2 kind 再次错标。不能据此裁定 gold 错误或产品已经通过质量门禁。

固定证据提交 `5f4e7d1643802078d3c1253eec7b5670bd487d6a`；baseline `88f2cf099c8b81f3acef485fd5ac9b01c63ce790`；candidate `37dd042eaa1209a86e0cafdcd92ae77e036e76f5` / product `90858afae647a513537bf118932a7ba5020ee98b`。诊断新增仅本目录及 `crates/cc-eval/tests/requests_native_zero_analogue.rs`。无产品、gold、scorer、normalizer、排名、budget、原 raw 或默认 selector 修改；旧 quality FAIL/Partial 保留，无 merge/deploy。

## 全量因果分解

每侧 91 query × 3 = 273 行；其中 83 个有答案 query × 3 = 249 行，8 个无答案 query × 3 = 24 行。每侧 141 个 alternatives，按执行行共有 423 个 answer-group 实例。下面每一级要求**同一实际 hit 与同一 alternative**同时满足此前所有条件，不跨 hit 拼接字段。这里为了看清位置与身份，把 qname 放在最后；真实 scorer 的早退顺序仍是 path/proof/name/qname/kind/span，另列在 aggregate 中。

| 累计匹配条件 | baseline pair / hit / 行 / group | candidate pair / hit / 行 / group |
| --- | ---: | ---: |
| 相同 path | 573 / 432 / 213 / 300 | 573 / 432 / 213 / 300 |
| + source proof 非 false | 573 / 432 / 213 / 300 | 573 / 432 / 213 / 300 |
| + symbol name | 189 / 189 / 159 / 171 | 189 / 189 / 159 / 171 |
| + v2 kind | 189 / 189 / 159 / 171 | 189 / 189 / 159 / 171 |
| + [start,end) byte span 相交 | 171 / 171 / 159 / 168 | 171 / 171 / 159 / 168 |
| + qname 完全相等 | 0 / 0 / 0 / 0 | 0 / 0 / 0 / 0 |

全部 native hits：baseline 603，candidate 609；实际原 normalizer+verify_source 重算后，全部 proof valid、全部 span 存在，身份字段与原 raw 完全一致。baseline qname nonnull=0；candidate=597/609。141 个 alternatives 全部要求 qname；两侧 strict matches=0。所有 171 个只失败于 qname 的 candidate hit，均精确满足 `gold_qname = module_path + '.' + actual_lexical_qname`；其它字段同时通过。**这是分类判据，未重写前缀、hit、gold 或评分。**171 个 pair 对应 168 个 group 实例和 159 个执行行，即 53 个独特 query；不能当成 159 个独立样本。

真实 scorer 早退 pair：baseline path=324/name=384/qname=189，candidate path=342/name=384/qname=189。其中各侧 18 个 pair 还同时缺少目标 span 交集；所以不能把全部 189 都归于单一 qname 原因。正确 name 的 189 个 pair 都符合 v2 kind，这个切面没有额外 kind 阻塞。

249 个有答案行中，36 行无目标 path，另 54 行虽有目标 path 却无目标 name；剩余 159 行至少已有一个除 qname 外完全匹配的 alternative。90 行尚未召回完整声明位置，255/423 个 group 实例亦尚未通过非 qname 条件，不能用命名空间解释或修复。位置覆盖可以非零，因为 span coverage 不要求严格身份全部相等；原 span precision=0.37956728965012027 / recall=0.48510768966827983 仍成立。

24 个无答案行全部 Partial 且全部非空。`metrics::score` 在 no_answer+Partial 时直接判 false；即便空 Partial 也不能证明不存在。packing 的全量 Partial 与身份比较是独立问题，本诊断不改 Complete、不调整预算、不推导无答案绿分。

## qname 约定的出处与责任边界

1. 作者 `e49f9ada5826b4206d2128f3bfb8d31603ff42fa`，`crates/cc-eval/benchmarks/public-v19/requests/scripts/author.py:43` 明确生成 `requests.<module>.<symbol>`。这项约定不是本轮新造。
2. 原 owner 审查 `3aae8bf2a690213426af91427dc837fedbe50c83`，`crates/cc-eval/benchmarks/public-v19/reviews/requests/review_remaining.py:94`、`review_delta.py:93`、`review_dev20.py:87` 逐 source/span 检查同样的模块前缀身份。
3. v2 独审 `47706907868007f71246f74674164263019bba79` 的 `artifacts/checkpoints/public-dev-pygo-v2-independent-20261003/audit.py:141` 再次要求模块前缀加 AST lexical scope。独审中的 “lexical owner/qname” **实际检查包含模块前缀**，不能仅凭文字称其与产品词法 qname 相同。
4. 登记 `974ccf5f90a1062512c9a645078e4948ebad5a8e` 原 loader 保留 qname，仅应用 Requests 85/Gin 81，合计 166 个 kind token delta。原 v1 broad function 有依据；本报告不混用历史 v1 分数、不重新出 gold。
5. 固定 product 的 `crates/cc-parsers/src/python/mod.rs:109` / `:178` 根据声明容器拼 qname，顶层只取名字；`:819` 明确为 dotted lexical qnames。它不从文件路径导出 package/module。`crates/cc-db/src/symbol_identity_store.rs` 精确绑定 parser identity，`crates/cc-search/src/plan.rs:516` 把它传入 metadata，实际 machine_pack/raw 与 normalizer 保留一致。
6. 公开 `docs/MCP_TOOLS.md:182` 暴露 qname，且声明 hybrid 原文以 machine_pack.hits 为准；文档未定义 Python qname 为可导入模块身份，也未定义 benchmark namespace 适配。native `SymbolGold` schema 只有未标注命名空间的 qname 字符串；`metrics.rs:89` 起的比较为完全相等，既不推断模块也不解析别名。

作者、owner、独审与准入的 benchmark 约定彼此一致；产品实现为另一种约定，公开 schema/文档缺少二者的语义桥梁。**已证 contract mismatch / native 字段语义缺口，尚未证哪一方违反既定规范。**模块前缀声明地址也不能自动声称一定可 import：package roots、别名导出、局部声明均需独立模型支持。

Python `__qualname__` 不含模块名，局部函数包含 `<locals>`，并不保证可从模块属性沿名字遍历；参见 [PEP 3155](https://peps.python.org/pep-3155/)。产品 dotted lexical 命名在本自编 nested 例中为 `outer_probe.inner_probe`，Python runtime 为 `outer_probe.<locals>.inner_probe`，因此也不能把产品字段一概命名为严格 runtime `__qualname__`。`runtime-name-semantics.json` 保存实际执行本小源码的证明。

## 独立微型复现及最小源切面

自编 `src/beacon/signals.py`，不引用 public DEV query/gold：自由函数 `amber_probe`、类方法 `Lantern.violet_probe`、函数内函数 `outer_probe.inner_probe`。实际新 build 的 codecortex 子进程，经原 `McpStdio` index/search → 原 normalizer → 原 verify_source → 原 metrics；不伪造 raw，也不向 MCP 传 gold。

| 微型声明 | 实际 symbol-mode qname | hybrid 直接声明 hit | 原 scorer（词法 expectation / 模块 expectation） |
| --- | --- | --- | --- |
| amber_probe | amber_probe | 有，proof valid | direct Top1 1 / 0，module nDCG 0 |
| Lantern.violet_probe | Lantern.violet_probe | 有，proof valid | direct Top1 1 / 0，module nDCG 0 |
| outer_probe.inner_probe | outer_probe.inner_probe | 无；返回 outer_probe 的 chunk | 两种 full-hit expectation 均 0 |

前两行仅 expectation 的自编 qname namespace 不同，其余 name/kind/path/span 完全相同，实际 hit 不改；full-hit lexical 与 module 结果也保留。第三行原样报告独立的召回/投影限制：symbol-mode 能找到 nested 声明，而 hybrid chunk 的直接 owner 为父函数；未用父 chunk 改名填充答案。最初 fixture 对 nested hybrid 直接 hit 的断言失败，随后把该真实 miss 改为显式观察和保留 symbol-mode 检查；没有选最好一次、调 query/top_k 或删 miss。

若 root 决定补产品/native contract，最小切面是 `cc-parsers/python` 的声明身份生成、`cc-model::symbol`/native `SymbolGold` 的字段语义声明，以及 MCP 公共文档。词法容器身份与 module/package identity 应各自明确、可验证、版本化；不能仅为此 benchmark 把 qname 加路径前缀，因为 qname 还参与 stable UID、resolver 与 DB source-bound identity。模块身份要从通用 project/package model 得到，处理 src roots、namespace packages、重导出和局部声明；在该模型未确立前，不能把简单 path→dotted 字符串称为可导入身份。这里仅提供现有路径与独立 fixture 作为 root 决策证据，未实施产品修正、schema 更新、scorer 放宽或 benchmark 补丁。

## 二次核验与交付

`verification.json`：archive SHA256 `45ebffc53dc519a8132e886dafec396e532d4ee1515fb99995480dd2696a5d00`，1118 个归档文件 bytes/SHA256 全通过；base/candidate binding 的 384/387 个源码和 Cargo blobs 分别与固定 Git commit 全通过。candidate/product src/Cargo/lock diff=0。原 selector 再验 73 个 source 文件及固定 candidate/独审/登记字节；admitted native SHA256 `7847fbe8326977121e964ee3729d47ce4855e7ce51b32c3918d8bd5ef77c3730`。注意 evaluator 的 queries.jsonl 是重新序列化快照：核验其 JSON 与 admitted loader 输出全量相等，不能误将其文件 SHA256 当作原 admitted 字节 SHA。

scoped Rust test 核验四 run 的全部 source file bytes/BLAKE3；逐 1044 个 raw 重跑未改 normalizer/verify_source，完整 hit JSON/status 与留存 normalized 相等；再逐行比较未改 metrics 的所有 Scores 与留存 scores.jsonl，全部通过。native 均 0，compat 原 Top1/nDCG 保持相等。`unchanged-replay.json` 的 nDCG 简单顺序求均值与原报告最后几位浮点 reduction 可有约 5e-16 差异，逐行 Scores 精确相等的断言才是核验依据，原 report/metrics 不改。

本地重新构建固定 candidate 同源码的官方 1.95.0 / 原 Cargo.lock 产品，仅用于独立微型 fixture；`build-receipt.json` 记录本次 binary pins，**不冒充原 worker binary**。历史二进制不在原安全归档，不能在此重新 hash 原运行中的 binary；原 build logs/绑定 receipt 已核验归档字节，没有证据支持混编猜测。

实际检查：locked `cc-server`/`cc-eval` build 成功；两个 scoped tests 通过；格式仅校验新增 Rust test；`git diff --check`。未重复 Requests 正式检索、全仓正式评测、100k 或历史排除项。仓库及 `/workspace/.agents`、`.codex` 未发现适用 AGENTS.md/SKILL.md；遵循 CONTRIBUTING 中文文档及 scoped test 布局，用户禁止重复全仓正式评测优先于通用贡献指南的全仓检查建议。

产物：`aggregate.json`/`verification.json`/`diagnose.py`、独立 `analogue.json` 与三个实际 raw、runtime 名字证据、原 scorer 重放聚合、build/test logs/receipt、本报告、文件封存 manifest。`bash artifacts/diagnostics/requests-native-zero-20261004/reproduce.sh` 可在已 fetch 固定对象的此环境重现，只跑已有 raw 重放与自编微型 fixture。只发布安全聚合和自编源码；无 query 列表、原 gold/body 或 source tree 复制到本目录。root 尚需决定字段 namespace 契约与单独产品工作范围；本诊断不关闭质量 FAIL、clean holdout、CI 或全 V19/100k certification。
