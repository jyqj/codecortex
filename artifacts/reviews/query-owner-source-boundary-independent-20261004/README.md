# 源 token 边界修正独立 exact-head 复审

结论：**REJECT**。实际测试 delivery SHA `9164657d1d04ddd8250480d11ee128910a4971fa`，production/test 源码与作者 tested source `142549a65b7eaee78ae5a06a82ba8695a6f33bd9` 相同。四臂另包括 baseline `3c8c204216cb54c41850c6da826a68fec3586430`、原 candidate `cbe347fa513200de7fb8b12d1549b5e8ec20fad8`、上次 rejected delivery `9294ec09d5b6fbd1dee9a04680fa733f579469dd`。上次两个具体 P2 code-reference negatives 和 P3 driver 缺口已修复；本次新发现仍阻止接受。

## 新 P2：完整比较子句的 wrapper 重新触发 owner suppression

位置：`crates/cc-search/src/query_target.rs:198–219`，尤其 `pair[1].strip_suffix(closing)`。当前 detector 要求匹配 wrapper 必须在第二个词 `than` / `of` 后闭合；因此接受 `(rather than) Sink`，拒绝 `(rather than Sink)`。后一形式包含同样的独立比较词和一个匹配、完整的 prose parenthetical clause，没有 callable syntax、独立 identifier wrappers 或 sigils。接纳代码引用修正不应把这条真正比较表达重新当成 owner-only 查询。

两个主要独立反例：

- `Which Beacon API (rather than Sink) accepts state?`
- `Which Beacon API (instead of Sink) accepts state?`

两条查询在三个 fresh source variants、engine/in-process MCP 两条路径均为 **base Beacon → candidate Sink → rejected Beacon → delivery Sink**。新 delivery 撤回 Beacon 的 `+0.18`，再次把明确用于比较的 Sink 推到 Top1；source identity、其余 scores/reasons/trace 不变。这是新 head 相对于上次 rejected head 的新 regression，并非仍开放的 stronger API inversion。

在当前 delivery 内，仅将 `Which Beacon API (rather than) Sink accepts state?` 的 closing parenthesis 移到 Sink 后，就从 Beacon Top1 变为 Sink Top1。base variant 的 Beacon 分数 `0.4715148087392313 → 0.2915148087392313`，所有 non-name trace 相同。`instead of` 也如此；`wrapper-contrast.json` 保存六个 variant/phrase 对照。

作者明确写了有限 wrapper 仅跨两个 phrase words；本次不把它扩写成完整 NL 能力声明。阻断依据是同一 comparative query 的完整 prose wrapper 范围变化会重新触发先前明确禁止的 owner suppression，且存在实际 name-bonus/ranking regression。额外匹配弯/直引号、case/whitespace 和含 article 的 wrapped clauses 也逐项归档；没有给新增查询附造 semantic gold 或修改原 expected semantics。建议扩大到真实 prose clause 的源边界，同时继续拒绝独立 callable/identifier wrappers、sigils 与错配记号；不要求恢复任意 punctuation trimming。

## 上次 P2/P3 与原合同复核

- 两个原独立 code-reference negatives（分别带 backtick-call syntax 和 `$` sigils）在全部 12 个 variant/API 观测保持 candidate 行为，inversion 的 Top1 回到 Commit。原 negative expectations 不改。
- 两个 durable Git driver 与 archive members 均实际存在。85-query driver SHA256 `422111a37e97170c48f86a3edf8840fee7aebea598f163a7370c738ee5ebe0a3`，恢复的 32-query driver SHA256 `0fdc96fb2b30df45def2ba0782135575a516b8becf4ca1599bde3f1b818ea22c`；与 binding/member/Git 文件一致。旧缺失 archive 未覆写。
- 32-query driver 的 query set 与旧 32-query outputs 一致；原 22 个 query→expected/scorer tuple 在两个 driver 中均原样保留。85-query driver 完整包含原 32 个 tuples。本审查验证现有内容/完整性/重放引用，不能独立证明此前未归档文件的历史来源。
- 作者 archive SHA256 与 binding 一致，113/113 manifest entries 核验通过；其 stored-output comparator 重跑为 510 个四臂对照、7,404 common-hit checks、12 个确切 code-reference challenges PASS。这和本次 fresh runs 分开记录。
- QueryTarget production bytes 四臂相同；NameBonusContext/role helper 和旧 single-word comparison helpers 与 rejected head 相同；DSL/原 lock 四臂相同，`plan.rs` 与原 candidate/rejected 相同。exact-identity、name/kind filters、ranking constants、候选生成/过滤不变。
- 原 85 个 query 的固定 contracts/scorer 全部保持；原 genuine phrase、receiver API/method Top1 0→1 / MRR .5→1 继续通过。stronger lexical inversion 的原 API 查询仍 Top1=0 / MRR=.5，不称新成功。

## 独立执行与工件

Rust/Cargo 1.95.0、default features、原锁 SHA256 `ea6b177872de3749702e2cd967f4504efff5bcc67cd2a2459205d182243d1000`。四臂顺序执行，每次 `-j 2`，分别用本次全新 `target-delivery`/`target-rejected`/`target-candidate`/`target-base`，没有沿用上次独立审查的 targets。每臂 cc_search/cc_server/cc_eval production receipt 都验证 fresh=false；profile=debuginfo=2 的 rlib、rustc argv、binary SHA256 固定在 link receipts。

`cargo +1.95.0 test --locked -p cc-search query_target --lib -j 2` 为 base/candidate/rejected/delivery **6/10/13/15 passed**，均 0 failed / 0 ignored / 298 filtered；只额外 build cc-eval lib，不运行其 tests。

保留完整原 85-query driver 的 cases/scorer，追加 26 个无新 gold 的 adversarial queries，共 111 queries × 3 fresh variants × engine/in-process MCP = **666 个四臂 query/API 对照（2,664 arm outputs）**。全部返回 hit 重新 verify_source，原 unrelated-name scorer controls 继续拒绝。55 个 production-module direct boundary probes 包括原 17 positives、12 code-reference negatives 和26新增 cases；9 个完整 clause wrappers 未通过 reviewer boundary contract，46 个通过。原15项 tests 的成功与新增 boundary gate 的失败分别记录。

9,978 common-hit checks、base/candidate/rejected→delivery 的 408/126/186 项 bonus 变化、292 项 rank-derived metadata 差异与所有逐 hit/score/boundary 结果见 `binding.json`/`comparison.json`；九个 wrapped-clause cases 的 baseline-output mismatch 逐 variant/API 保存。除了 ±0.18 的 named-owner bonus/reason/trace 与其排序派生 `metadata.evidence_priority`，returned identity、文本/span/kind/breadcrumb、normalized source evidence、lane scores、non-name trace、非 bonus reasons 和其他 hit metadata 保持；trace sum 与 rerank_score 相符。top_k=10 本 fixture 返回身份集合一致；原 top_k=1 窗口保持 candidate/rejected。不是一般性 full retrieval candidate-set instrumentation，也不声称全部 wire payload 等值：独立索引的计时、随机 source incarnation/token estimate 不作跨索引相等要求。

`evidence.tar.gz` 保存完整 111-query driver、55-probe input、production-module wrappers、prepare/run/compare/gate/audit scripts、四臂 source/lock snapshots、12 份 raw/normalized/native outputs、fresh fixture 源码/config、focused test/build/link logs、toolchain/format/diff audit、内部 manifest；不包含 binary/dependency cache/fixture DB。Archive digest 与 manifest 数见 binding；本次打包后逐项复核。`compare.py` 检查共同不变量和原合同后生成全部观测，exit=0；`gate.py` 对当前 head 应 exit=1，以保留失败而非调整预期。

复现使用新的 owned root，分别检出四个绑定 SHA、分配全新 target/fixture/output。按机器布局调整 scripts 中的 `/workspace/independent-boundary-review` 和 `/workspace/codecortex`；仅复制 scripts/inputs 到新根，勿复制历史 artifact JSONL 后触发 run.py 的 resume 路径。先将当前作者 evidence.tar.gz 解压到新根的 author/ 子目录（不是执行根），提供原 boundary_driver.rs/control-contract.json；prepare.py 只追加本独立 probes 并生成 wrappers。原 cases/expected/scorer/fixture 不调整；先 prepare.py，再 run.py、compare.py、gate.py。

首个 archive audit 错误假设 driver 前22行顺序一致；旧32driver是在原 cases 中间插入 extras。按 query→expected tuple 比较后确认原合同不变，保留 initial-order-assumption log，未把该 reviewer 假设失败列为产品发现。本次没有 link-profile/shared-target 失效输出；四臂有效执行 receipt 全部为 exit=0。

已重新检查 repo/父目录没有可用 AGENTS.md/.agents/skills，读取 CONTRIBUTING.md 与新 correction/旧独立报告；任务 narrow scope 覆盖其全仓测试要求。未运行禁止的 semantic runtime test 或可启用它的 suite、private corpora/exports、DB GC/WAL/kill/EROFS 或 production endpoints。未编辑 candidate/registry/CI/TODO/settings/credentials、历史证据；无 PR/merge/deploy/force push。仅独立 evidence branch 普通 commit/push；parent 在新 blocker 被修复并接受前不应整合。
