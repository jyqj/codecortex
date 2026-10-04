# lexical span correction 独立 exact-head 复审

结论：**REJECT**。实际测试 delivery `1520407aabad51fe97841e51e175b4eaa3fa2ca4`；production/test 源码与 tested source `089e81f661ff6cec0bd7550162acd8c28a28b5b5` 相同。四臂另包括 baseline `3c8c204216cb54c41850c6da826a68fec3586430`、原 candidate `cbe347fa513200de7fb8b12d1549b5e8ec20fad8` 和 previous delivery `9164657d1d04ddd8250480d11ee128910a4971fa`。先前完整 grouped-clause blockers、两个 code-reference negatives 和 driver omission 均已解决。当前新增 blocker 严格属于先行 lexical contract，不扩大到无关 NL 语法。

## P2：closing group 的 `::` qualifier 没有被标为 code

位置：`crates/cc-search/src/query_target.rs:258–266`。交付 `LEXICAL-CONTRACT.md` 第4条要求 attached closing-group qualifier suffix 使该 context 成为 code。实现判断了 `.member`、call/index、identifier/sigil/escape，却没有判断 `::member`。开 group 时将 colon 当作 code boundary、word decorations product 也覆盖了 `word::`，closing-group 这一分支漏掉同类 qualifier。

主要独立反例：

- `Which Beacon API calls (rather than Sink)::Commit with Commit?`
- `Which Beacon API calls "rather than Sink"::Commit with Commit?`

同一问题出现在两种 phrase × round/square/curly/直双引号/backtick/弯双引号，共 **12** 个 code-suffix cases。它们符合已声明的 code evidence，应继续 candidate owner withholding；实际 scanner 返回 prose comparative，恢复 Beacon class `+0.18`。独立 expectations 根据事先声明的 qualifier rule 编写，没有改变原矩阵、expected/scorer 或 candidate。

inversion variant 的两条 API 路径均由 **candidate/previous Commit → delivery Beacon**。主要反例的 Beacon `0.4583333333333333 → 0.6383333333333333`；Commit 保持 `0.5561538461538461`。base/receiver variants 仍 Top1 Commit，但同样错误恢复 +0.18。所有 source、identity、non-name ranking components/reasons 保持。不是仍开放的原 stronger API inversion，也不是新 semantic gold/公共质量分数。

建议在 closing-context code evidence 中按同一 qualifier 规则识别 `::` attachment，并增加 grouping × qualifier-suffix 的直接交叉控制；保留新实现正确支持的 prose contexts 与 prior code negatives。本审查没有改实现。

## 原 blockers / 文档合同 / archive 复核

- 两个原完整 parenthetical-clause 反例在全部12个 variant/API 观测恢复 baseline，原九个 wrapper-clause expectations 保持；原 backtick-call 和 `$` negatives 全部12个观测保持 candidate，inversion Top1 Commit。
- 已读 lexical contract 与实际 source/scanner。初始6047075文档先于patch，其第4条后来增补 closing identifier/qualifier wording；交付文档与tested source089e81f相同，不能声称全文与最初checkpoint逐字相同。本次按冻结交付合同评判，contract-history.diff 保留这一差异。独立选择 matched/nested contexts、pair/clause extent、thin-space/case、attached call/index/identifier/escape/qualifier suffix、opening identifier/sigil、独立 quoted words、inter-word delimiter、malformed groups、apostrophe controls。没有追加不相关 NL 意图要求。
- complete driver 实际245 cases，SHA256 `f6f875efabd28e8b803a3e29d206ffb57cb4fb727a9ed05f016088591e57236e` 与 Git 文件、archive、binding 一致，link script 引用可找到的该 member。旧独立111 tuples 与原22 scorer tuples 完整保留。
- archive **116/116** manifest entries 的 SHA/size 通过，matrix **5,633** rows 及其 hash 与 binding 一致。作者 stored-output comparator 重跑为 **1,470** 四臂对照、**23,910** common-hit checks、原 grouped/code challenges PASS，和本次独立 fresh runs 分开记录。
- QueryTarget production bytes 四臂一致，NameBonusContext 除 scanner argument 外与 previous 相同，旧 single-word helpers 不变；DSL/原 lock 四臂相同，plan.rs 与原 candidate/previous 一致。原 explicit name/kind/identity controls、primary-query contracts 和19个 focused tests 保持；scanner 前的 name/kind bypass 未变。没有候选生成/过滤、taxonomy、ranking constants 或 exact-identity tier 改动。

## 本次独立执行与工件

Rust/Cargo1.95.0、default features、original Cargo.lock SHA256 `ea6b177872de3749702e2cd967f4504efff5bcc67cd2a2459205d182243d1000`，四臂顺序执行，各用全新 target-delivery/previous/candidate/base，每次 `-j 2`。全部 cc_search/cc_server/cc_eval production artifacts 验证 fresh=false，profile=debuginfo=2 的 libraries 与 rustc argv/binary hashes 固定在 receipts。只 build cc-eval lib，不运行其 tests。

`cargo +1.95.0 test --locked -p cc-search query_target --lib -j 2` 为 base/candidate/previous/delivery **6/10/15/19 passed**，均 0 failed / 0 ignored / 298 filtered。19项同时执行冻结5,633 rows；它们成功不替代新独立 qualifier gate。

独立 direct production probes **182** cases，覆盖两个 phrase 的代表性 lexical cross-product；**170**通过、**12** closing-qualifier cases 失败。real API driver 原245 queries/expected/scorer 不改，仅追加47个 representative 无新 gold 的查询，共292 × 3 fresh Go variants × engine/in-process MCP = **1,752** 四臂 query/API comparisons（7,008 arm outputs）。全部返回 hit 重新 verify_source，原 unrelated-name scorer controls 保持拒绝。原245 contracts 与 prior blockers 全通过，新12 qualifier cases 的72个 variant/API baseline/candidate合同差异逐项保存。

28,860 common-hit checks、base/candidate/previous→delivery 的822/798/540项bonus变化、532项rank-derived metadata差异见binding/comparison；24项inversion Top1由Commit变为Beacon。returned identity、source text/span/kind/breadcrumb、normalized source evidence、lane scores、non-name trace/reasons 与其他 metadata 保持；变化仅 owner symbol-exact ±0.18 与相应 reason/trace，以及排序派生 evidence_priority；trace sum 等于 rerank_score。top_k=10 是本 tiny fixture 的返回集合核对，不是 full candidate-set instrumentation；原 top_k=1 窗口保持。跨独立索引不要求运行计时、随机 source incarnation/token estimate 相等，不声称 wire payload 全等。

receiver 原 API/method Top1 0→1 / MRR .5→1 保留；stronger lexical inversion 的原 API 仍 Top1=0 / MRR=.5。不能据本 synthetic/lexical review 声称通用 NL 或 public quality 接受。

binding 固定 SHAs/counts/archive digest；comparison 保存逐 hit、bonus、metadata、scores、prior/grouped/qualifier challenges；identities 保存 source/lock/static/archive checks。evidence archive 保存292-query完整driver、182-probe inputs/production-module wrappers、prepare/run/compare/gate/audit scripts、原作者driver/contract参考、四臂source/lock、12份完整 raw/normalized/native结果、fresh fixture源码/config、tests/build/link/toolchain/format/diff receipts 和内部manifest；不保存 binary/dependency cache/fixture DB。封包后125/125 manifest entries与archive binding核验通过。

复现取新的 owned root，四个绑定SHA分别检出，分配全新 target/fixture/output。将当前作者evidence.tar.gz解压到该根author/子目录，提供原 lexical_driver.rs/control-contract.json；仅复制独立scripts/inputs到执行根，勿复制旧 artifact JSONL 触发run.py恢复路径。按本机布局调整路径，不改queries/expected/scorer/fixture，执行prepare.py、run.py、compare.py；compare成功保存包括失败的全量观测，gate.py对当前head应exit=1。

构建前空间2.0GB，只删除本 reviewer 两轮旧工作根下七个可重建 Cargo target caches；全部已提交历史review/archive/raw/source/receipts保留。旧rlib路径需重建，未冒称仍可读取。repo/父目录无可用AGENTS.md/.agents/skills，已读CONTRIBUTING与新/旧报告，任务narrow scope覆盖其全仓测试要求。没有运行禁止的semantic runtime test或可启用它的suites、private corpora/exports、DB GC/WAL/kill/EROFS或production endpoints。没有candidate/registry/CI/TODO/settings/credential或历史证据修改，无PR/merge/deploy/force push。仅独立evidence branch普通commit/push。
