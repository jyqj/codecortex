# 比较词 lexical spans / matched contexts 修正（仅审查）

实际测试源码 `089e81f661ff6cec0bd7550162acd8c28a28b5b5`；base `3c8c204216cb54c41850c6da826a68fec3586430`；原 candidate `cbe347fa513200de7fb8b12d1549b5e8ec20fad8`；上次 rejected delivery `9164657d1d04ddd8250480d11ee128910a4971fa`。本分支从独立拒绝证据 `a84130cb22825e207836f0191ae3a37bb71af509` 继续，全部之前的拒绝、source、raw、archive 和 driver 原样保留。最终 delivery 只追加本证据，源码与 tested source 相同；完整 delivery SHA 见 branch tip/回复。未获得 parent 接受。

## 先定义规则，再改实现

`6047075f79d327f1e2c1b3e1e3c66c1d1aa24d4b` 在 patch 前提交 `LEXICAL-CONTRACT.md`；`1d3c86f28413c73e9d4f2bab8566c403913712fa` 先发布 full driver 和 failing tests，production 仍是上次 rejected guard。该红 checkpoint 为 **16 passed / 3 failed / 298 filtered**，当时 matrix 为 4,791 rows，不能当成修正通过。实现后增补 qualifier/escape、closing attachment 与 word-internal apostrophe 交叉覆盖，最终 5,633 rows 和完整 245-query driver 随 tested source 冻结并推送，长时间 paired builds 前已经 durable。

新 detector 不按个别例子列 wrapper templates，而是扫描 maximal Unicode word spans 和 matched grouping contexts。两个既有 phrase（`rather than` / `instead of`）须共享 context identity stack，中间只允许 Unicode whitespace 或既有 comma+whitespace。相同 quote/backtick/group 可以在 pair 后闭合，也可在 target/article/整个 clause 后闭合；nested grouping 同样处理。分别包裹的 identifiers 不共享 context；source delimiter 不被删除。

相邻 identifier/sigil/closing expression 的 opening grouping、closing grouping 的 attached call/index/identifier/qualifier suffix、word 的 sigil/qualifier/escape/call boundary 都是 code evidence。错配/未闭合的 source structure 不提供 phrase evidence；在完成 source context 验证后才判断 pair。内部 apostrophe 不开启新 grouping。规则、允许的字符类、保留的 DSL projection 和交叉覆盖详见先行 contract。这里只做有限 lexical abstention，不解析 NL 意图，也不推断 API 是 callable。

QueryTarget production 字节不变，旧 single-word comparison helpers 字节不变，NameBonusContext 除将原 text 传给 scanner 外保持原 caller/owner grammar。DSL/原 lock 四臂一致，`plan.rs` 相对原 candidate 不变。没有改 exact-identity、name:/kind:、kind taxonomy、ranking constants、候选 generation/filtering 或原 receiver contracts。旧 tests 与两个既有 matrices 未改写。

## 聚焦 tests 与 clean paired runs

Rust `1.95.0 (59807616e 2026-04-14)` / Cargo `1.95.0 (f2d3ce0bd 2026-03-21)`；default features，原 Cargo.lock SHA256 `ea6b177872de3749702e2cd967f4504efff5bcc67cd2a2459205d182243d1000`。四臂顺序执行，每次最多 `-j 2`，使用全新独立 target-correction/previous/base/candidate。每臂 production cc_search/cc_server/cc_eval receipt 均验证 `fresh=false`，target-profile libs 与 rustc argv/binary hashes 保存在 link receipts。

- `cargo +1.95.0 test --locked -p cc-search query_target --lib -j 2`：base **6**、candidate **10**、previous **15**、correction **19** passed，均 0 failed / 0 ignored / 298 filtered。三个新增矩阵 tests 覆盖 **5,633** rows：grouping product 1,638；独立 word decorations product 3,876；delimiters 64；固定独立 probes 55。另加 SearchPlan grouped-clause trace/exact-identity 控制。
- 四次 `cargo +1.95.0 build --locked -p cc-eval --lib -j 2 --message-format=json-render-diagnostics`、scoped rustfmt 1.95、从 `a84130cb` 起的 diff check、`cargo +1.95.0 clippy --locked -p cc-search --lib -j 2 -- -D warnings` 通过。没有执行 cc-eval tests。
- 保持原 111-query reviewer driver/原 85 contracts、22 个 scorer tuples、fixture 与 native scorer，追加 systematic grouping/delimiter/code cases，共 **245 queries × 3 fresh Go variants × engine/in-process MCP = 1,470 四臂对照（5,880 arm outputs）**。没有新 semantic gold。全部 returned hits 重新 source-verify；原 unrelated-name scorer controls 继续拒绝。
- **23,910 common-hit pair checks**：身份、text/span/name/kind/breadcrumb、normalized source evidence、lane scores、所有 non-name trace/非 bonus reasons 不变；trace sum 与 rerank_score 相符。除排序派生 `metadata.evidence_priority` 的 **244** 项记录外，其他 hit metadata 相同。独立索引的计时、随机 source incarnation/token estimate 不作为 wire 等值条件；完整 raw 保存。
- base→correction **702** 个 owner bonus suppression 保持 candidate reading；candidate→correction **636** 个 genuine phrase owner hit 恢复 +0.18。positive controls 的 ordered hits/evidence/status/native scores 等于 baseline；negative 和其他原查询保持 candidate 行为。全部变化仅 Beacon class 的 ±0.18 与 symbol-exact trace/reason。
- previous→correction **390** 个变化：**378** genuine grouping restores，另 **12** removals 仅为 `Which Beacon API rather than Sink) with Commit?` / `... instead of Sink) ...`。这两个 unmatched-closing forms 在 pre-patch matrix/contract 已明确为 negative；不是改 expected 来取绿。其他新增 query 若无 bonus 变化则完整 hit ranking/trace 与 previous 相同。

首个 paired checker 错误假定所有 previous→correction delta 都是 +0.18，在上述 malformed closing case 失败；该 log 保留为 `first-comparison-assumption-failure.log`。修正 checker 为依据已冻结的 positive/negative contract 判定方向后 exit=0。没有改 source、matrix、original85 contracts/scorer，也没有重新替换 raw 运行。实际 product tests 与 checker assumption failure 分开记录。

## 具体 regressions 和剩余界限

两条 `Which Beacon API (rather than Sink) accepts state?` / `(instead of Sink)` 在三变体、两条 API 路径的全部 **12** observations 均为 **Beacon → Sink → Sink → Beacon**（base/candidate/previous/correction）；九个独立 wrapped-clause contracts 全部保持 baseline 行为。原普通 `rather than` 仍 Beacon→Sink→Beacon→Beacon。

此前 `` `rather()` `than()` `` 和 `$rather $than` 的全部 **12** observations 保持 candidate behavior，inversion Top1 **Commit**。receiver API/method 的两条路径继续 Top1 **0→1**、MRR **.5→1**；stronger lexical API inversion 仍 **Top1=0 / MRR=.5**。deprecated、Commit/Accept、未标记的 code/NL 歧义与旧 single-word helper 的有限行为不因此宣称解决。

top_k=10 本 tiny fixture 返回身份集合一致；原 top_k=1 保持 candidate/previous 的返回窗口，receiver 的 Beacon→Commit 为正常 rerank/truncate。不是一般性 full candidate-set instrumentation，也不是 general NL/public DEV 质量接受。

## 可复核交付

`LEXICAL-CONTRACT.md` 为先行 rule，`prepare_controls.py` 为独立期待的 matrix/driver generator；`lexical-matrix.json` 与 `control-contract.json` 固定 source/API contract，`lexical_driver.rs` 为完整 **245-query** source（SHA256 `f6f875efabd28e8b803a3e29d206ffb57cb4fb727a9ed05f016088591e57236e`）。driver 在 tested source 和最终 archive 中逐字节一致，link script 引用它，不存在之前的 omission。

`binding.json` 固定 tested/checkpoint/previous SHAs、source/lock/driver/matrix hashes、counts/archive digest；`comparison.json` 保存逐 hit、delta、metadata、original/grouped/code challenges 与 native scores；`correction.patch.gz` 是两 Rust 源文件的完整统一 diff。`evidence.tar.gz` 包含 **116** manifest entries：完整 driver/rule/matrix/contracts、prepare/run/link/compare/package scripts、四臂 source/lock snapshots、12 份完整 raw/normalized/native outputs、fresh fixture source/config、所有实际 test/build/link/lint/format logs、checker failure log 和 receipts。逐 member SHA/size 核对通过；没有 binary、dependency cache 或 fixture DB。

使用新的 owned root 和四个绑定 commit、分别分配全新 target/fixture/output，按本机布局只调整 scripts 路径，不调整 queries/expected/scorer/fixtures。`run_validation.py` 顺序运行，`compare_results.py` 应 exit=0；`execution.json`/`extra-execution.json`、toolchain/source pins 与 link receipts 给出实际命令、exit 和限制。MCP 是真实 in-process rmcp dispatcher，不是 subprocess stdio。

构建前空间仅 2.6GB，已删除两次作者旧 validation 的七个 Cargo target caches（不是交付物）；所有历史 commits/archives/raw/receipts/source/driver binaries 保留。旧 receipt 的 rlib 路径如今需要重建，不能称作当前可读文件。latest rejection archive 的 **124** entries 全部按原 hash 核验，所有历史 review 文件对其原 commit 的 diff 为零。

repo/父目录无可用 AGENTS.md/.agents/skills；遵循已读 CONTRIBUTING.md 与任务明确 scope。不运行禁止的 semantic runtime test 或可启用它的 suites、private corpora/exports、DB GC/WAL/kill/EROFS 或 production endpoint；不改 registry/CI/TODO/settings/credentials，无 PR/main merge/deploy/force push。仍须 parent 对本 exact delivery 独立复审。
