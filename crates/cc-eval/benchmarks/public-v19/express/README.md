# Express V19 作者候选分片：协议迁移与 custody 阻塞

作者起草 **100 个候选家族，独立审查/accepted 均为 0**。固定基线 PR60 `bc8e22bd6b4f85774e7b84a3e6a30da8dc4a3b29`；源码为 `expressjs/express@7ef98448f8b38099ab1ded55e458538ad47a51e7`，MIT license SHA256 `95a5762890e5c1c9808921cef095661fc482c5e1f0bba31446ac85595df6237c` 已原字节核验。只修改本目录，不改公共 registry、生产、scorer 或 ledger。

已迁移共同协议 PR65 `03abe24950f1fa89de3cce0a4e1d44d01e9aa0d6`。`provenance/protocol-v1` 是此次读到的必要协议原字节副本及哈希收据，未修改 protocol 原目录。公开当前树仅含 **68 native dev** / **57 compat dev** 家族正文；11 个 dev no-answer 不进入 compat。阈值规则产生 **32 would-be holdout** 家族，因公开暴露/无受限 custody，确认性 holdout 全部隔离，accepted holdout 为 **0**。

## 暴露与迁移记录

首块提交 `336b962ffe7cb5d144273cfc2f45b0526ecba8bc` / draft PR62 公开了初始 20 候选正文；后来 `2cf6495f5115f4694d219ced5a09abda3f0c8be1` 已推送全部 100 候选正文及作者输入。共同协议到达时这些正文已存在公开 Git 历史和共享工作区。**删除当前文件不恢复保密**，不能宣称这些候选仍是 untouched holdout，也不能把哈希收据当作真实 custody。没有指定独立受限保管者，状态 `holdout_custody_blocked`。历史访问与污染审计待独立 custodian 完成；现有所有 would-be holdout 均隔离于确认性评测之外。

`provenance/id-split-migration.json` 按已提交的原始起草行序一次性分配 `v19.express.f0001` 至 `f0100`，保留逐条原 ID、原 split、旧/新行哈希和暴露 commit。未挑 key、换盐或重平衡。`relations.json` 提出两个本仓正负 counterpart 的组件关联，canonical 是最小成员；尚待指定 global reviewer 审定，不代签跨仓关系冻结。100 draft family IDs 对应 **98 候选组件**，不是 100 已认证独立统计样本。

按协议 SHA256(`codecortex-public-v19-split-v1\n` + canonical global-family)，前 64 位 big-endian <2^62 为 holdout，否则 dev。现有 68/32 是固定概率划分结果，不是强行 75/25。原作者 72/28、最初 14/6 等仅为旧候选分类，哈希和迁移理由仍保留；没有排名观察。后续发现组件交叉 split 时必须版本化隔离/裁决。

## 当前交付

`intake/first-020` 是原首块经协议迁移后的公开 dev 投影（12 native / 9 compat，8 would-be holdout 仅哈希/计数）；`intake/full-100` 包含首块，二者累计 100 draft，不能加成 120。`blocks/*/inventory.json` 和旧 validation 收据仅为历史候选承诺，不是当前可运行套件。当前运行入口是 intake 中的分离 native/compat suite。

源读起草覆盖 20 API、20 behavior、15 architecture/facets、20 跨文件链、10 config/error、15 hard-negative/no-answer（12 bounded no-answer，3 有纠正答案的 hard negative）。此分布是作者候选义务统计；新协议使用实际 evaluator 类别，并保留 `annotations.v19.original_category`。不同措辞不另计家族；独立 reviewer 可合并事实重叠候选，不能为维持 100 拒绝合并。

`source-manifest.json`、`provenance/source-lock.json` 记录 7 个手写核心 JS 允许文件的 Git blob/SHA256/大小、真实源 SHA、许可证与完整排除清单。索引源只含 `index.js` 和 `lib/*.js` 原字节，不含依赖实现、测试资产、示例、gold 或二进制。source 是明确标注的快照，因此 evaluator `commit:null`；原上游 clean checkout 与源字节比对通过不等于快照 Git cleanliness。

`queries.native.dev.jsonl` 的所有额外字段归入 `annotations.v19`；主要义务 first group grade3，其他必需义务 grade2，各 facet 指向具体组。原字节 span 为 0-based UTF8 [start,end)，行范围 1-based inclusive。链边绑定 from/to 源证据索引；bounded no-answer 明确列出全部 7 个文件范围与邻近边界证据及字面检查，不能以 rank 判空。当前 evaluator 尚无必需 facet/graph 正确性指标，native 通过仅证明输入完整性。compat 稳定投影 literal paths，排除 no-answer，不冒充 span/facet/链评分。

## 复现与审查

构建 PR60 合同的 cc-eval，公开 crates.io 依赖缺失记录在 review；未运行任何检索/provider/调参。克隆公开 Express 并 checkout 固定 SHA，再运行：

```sh
python3 crates/cc-eval/benchmarks/public-v19/express/scripts/verify.py --block full-100 --evaluator /tmp/express-eval-build/debug/cc-eval --upstream /tmp/express-v19
python3 crates/cc-eval/benchmarks/public-v19/express/provenance/protocol-v1/check.py --shard express:native=crates/cc-eval/benchmarks/public-v19/express/intake/full-100/queries.native.dev.jsonl --shard express:compat=crates/cc-eval/benchmarks/public-v19/express/intake/full-100/queries.compat.dev.jsonl --relations crates/cc-eval/benchmarks/public-v19/express/relations.json --evaluator /tmp/express-eval-build/debug/cc-eval --suite crates/cc-eval/benchmarks/public-v19/express/intake/full-100/suite.native.dev.json --suite crates/cc-eval/benchmarks/public-v19/express/intake/full-100/suite.compat.dev.json --output crates/cc-eval/benchmarks/public-v19/express/review/protocol-check-full-100.json
```

正常 verify/check 不刷新 BLAKE3 lock；命令仅输出数量、哈希、状态。`migrate_protocol.py` 复现历史固定 commit 的迁移，would-be holdout 只在进程内计算 exact-byte commitment，不落盘、不打印正文；这不是独立 custody，也不挽救历史污染。重建 dev suite 后须显式 author freeze 再 validate。协议 checker 通过，四个实际 native/compat dev suite validate 通过；源/span检查收据在 review，质量结论仍无。

`corpus-receipt.json`、`review-receipt.json` 只报告计数、哈希、作者/空审查者、暴露和 custody 状态。后续需作者 C / 指定 global reviewer 独立核验源/gold/家族/组件/负例/替代答案，再由独立 custodian 审计访问边界。作者不能签自己的独立 review。所有 formal complete block / accepted 计数保持 0，直到独立审查与 custody 条件真实满足。本成果不是已丢失历史 306 raw，也不是 live 质量结论。
