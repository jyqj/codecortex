# Closing qualifier 修正独立 exact-head 复审

结论：**ACCEPT（bounded lexical correction）**。实际测试 delivery **`026b566f18525568ae16e505eca13ea5e258dff5`**，源码/测试与作者 tested source `d32924f0b1b08f55956659df2cb253d0aba8146f` 相同。对照 rejected delivery `1520407aabad51fe97841e51e175b4eaa3fa2ca4`。本范围无新 blocker；允许 parent 按该 exact source 的有限规则推进 publication decision，不构成 general NL/public DEV 质量接受、PR/merge/deploy 授权或 source registry 集成完成。

## 原 P2 / 边界规则核实

closing group 与 keyword word boundary 共用 qualifier_attachment。连续 `::` / `->`、`.word_char`、有 word/call/index 后继的 `?.` 作为 code attachment；单冒号、终止句点/终止`?.`、分开的 operator characters、空白保持已声明的有限 prose reading。inner/ancestor code contexts 屏蔽其中 pair，只有比较对象 qualified 时保持外层 prose。原 balanced context identity、maximal Unicode words、ASCII keyword case、comma/whitespace gap 不变。

全部12个原 `::Commit` failures 在三个 source variants、engine/in-process MCP 共72项 observations 继续 withholding Beacon class bonus；inversion 的24项 **previous Beacon → delivery Commit** 全通过。两条主要反例中 Beacon `0.6383333333333333 → 0.4583333333333333`，Commit `0.5561538461538461` 不变。先前 grouped-clause、backtick-call 与sigil negatives 的契约保持，不再是 blocker。

独立新增 **166** direct production-module probes全部通过，覆盖 Unicode member names、thin whitespace、连续/分离 qualifiers、nested祖先code与target-onlyqualification、keyword附件、explicit name/kind bypass。16个direct filter cases的QueryTarget模型、permits及NameBonusContext与previous完全相同；实际API另有 **96** 个filter observations逐值保持。没有把无关NL语义加入接受条件。

## 执行与实际结果

Rust/Cargo1.95.0、default features、原始Cargo.lock SHA256 `ea6b177872de3749702e2cd967f4504efff5bcc67cd2a2459205d182243d1000`。两个全新、互不共享的 target-delivery / target-previous，Cargo顺序执行、每次`-j 2`。两臂production cc_search/cc_server/cc_eval均断言fresh=false；rlib/binary SHA、profile=debuginfo=2、rustc argv/exit保存在link receipts。cc-eval只build lib，未执行其tests。

`cargo +1.95.0 test --locked -p cc-search query_target --lib -j 2`：previous **19**、delivery **21 passed**，均0 failed / 0 ignored / 298 filtered。21项包含作者2032新rows及之前保留的lexical matrices；没有重跑全仓或禁止的tests/suites。

完整保留原558-query driver/原292与245queries、22个scorer tuples、fixture、native scorer，仅追加82个独立representative查询，无新semantic gold。共 **640 queries × 3 fresh Go variants × engine/in-process MCP = 3,840 paired query/API checks（7,680 arm outputs）**，全部evidence重新verify_source，unrelated-name scorer controls保持拒绝。

**21,500 common-hit checks**：identity、source text/span/name/kind/breadcrumb、normalized source evidence、lane scores、non-name trace/reasons、其他metadata相同；trace sum匹配rerank_score。**1,152**个owner变化均为已声明negative qualifiers的Beacon class **-0.18**及symbol-exact trace/reason移除，**768**项rank-derived evidence_priority变化逐项保存。无bonus变更时ordered hits/status/native scores完全相同。没有新增boost、filter或taxonomy变更。

166个direct expectations与396个新增实际API boundary observations通过；96个explicit-filter observations无漂移。原245queries全都与previous相同，并另对本reviewer此前byte-pinned baseline/candidate输出完成 **1,470** 个archived-reference checks：positive contracts匹配baseline、其他原contracts匹配candidate。历史references明确不是本轮fresh baseline/candidate builds，原locks/source/输入SHA固定在历史独立证据，六份reference raw结果在本archive中单独保留。

receiver原API/method的Top1=1 / MRR=1保留；stronger lexical API inversion仍 **Top1=0 / MRR=.5**。`rather than Sink`和原完整grouped-clause controls保持baseline ordering。tiny top_k=10返回身份集合相同，原top_k=1窗口不变；不是一般性full candidate-set instrumentation。独立索引计时、随机source incarnation/token estimate不作wire等值保证；完整raw保留。

## Source / artifact / preservation audit

QueryTarget与NameBonusContext production prefix、旧single-word helpers不变；DSL/plan.rs/原锁逐字相同。源码diff限于scanner内word.after及shared附件函数、对应tests；明确name:/kind:、exact identity优先级、原caller/owner grammar和候选生成/过滤保持。

作者archive **83/83** manifest members的SHA/size核对成功；558-query driver、2032-row matrix及hash与durable Git文件/binding一致，实际link-script依赖存在。red checkpoint `9546dde6924bbff9497a8ee66e58e39593fe5da1` 的driver/matrix/control expectations与交付逐字相同；QUALIFIER-CONTRACT.md与tested source相同，该说明文档是在source commit加入，并非red checkpoint中的文件。

作者stored-output comparator独立重跑为 **3,348**paired checks、**19,088**common hits、**972**bonus removals、**648**rank metadata变化PASS，原24 qualifier inversions及receiver/known-open scores核实。与本次fresh3,840checks分开记录。旧独立review目录相对a2d7f7b的Git diff为零，其archive binding SHA相符；没有改历史失败结果。

audit最初错误假设QUALIFIER文档已存在于red checkpoint，读取该不存在路径失败；随后按实际历史验证red driver/matrix/control tuples及source-commit文档，保留initial-contract-location-assumption log。没有把reviewer位置假设列为产品失败，也没有改expected/source/raw。全部有效build/link/API执行receipt为exit=0。

binding固定tested/source/previous SHAs、counts、driver/archive digest和verdict；comparison保存逐hit/bonus/metadata、原qualifier/current filter/API/archived-reference检查；identities保存static/source/hash/preservation核实。evidence archive包含完整640-query driver、166-case inputs及production wrappers、prepare/run/compare/audit scripts、作者driver/matrix/contract参考、两臂source/lock snapshots、6份fresh raw/normalized/native结果、6份历史baseline/candidate raw references及其原comparison输入SHA、fixture source/config、tests/build/link/toolchain/format/diff/audit logs与内部manifest。不保存binary、rlib、dependency cache、fixture DB或private export。打包后83/83 manifest entries及archive binding逐项复核通过。

复现用新的owned根、两个绑定SHA分别检出，指定全新target/fixture/output。把作者evidence archive解压到根的author/子目录；仅复制独立scripts/inputs到执行根，勿携带旧artifact JSONL触发run.py恢复路径。historical-references/保持字节不改，compare会先验证其原inputSHA。只按本机布局调整路径，保留cases/expected/scorer/fixture；prepare.py、run.py、compare.py应成功并得到ACCEPT。

空间准备只删除本reviewer上轮四个可重建Cargo target caches，全部历史commits/archives/raw/source/receipts保留；旧rlib路径需重建，不冒称仍可读。已读CONTRIBUTING与新/旧报告，repo/父目录无applicable AGENTS.md/.agents/skills；任务scope覆盖其全仓测试要求。未运行禁止的semantic-runtime test/可启用suites、private corpora/exports、DB GC/WAL/kill/EROFS或production endpoint。未改candidate/CI/registry/TODO/settings/credentials/历史证据，无PR/main merge/deploy/force。仅独立evidence branch普通commit/push；后续publication/integration由parent按授权范围执行。
