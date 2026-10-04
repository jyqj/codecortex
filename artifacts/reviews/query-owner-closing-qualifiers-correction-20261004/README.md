# Closing qualifier bounded P2 correction（仅审查）

实际测试 source `d32924f0b1b08f55956659df2cb253d0aba8146f`；上次 rejected delivery `1520407aabad51fe97841e51e175b4eaa3fa2ca4`；base `3c8c204216cb54c41850c6da826a68fec3586430`；candidate `cbe347fa513200de7fb8b12d1549b5e8ec20fad8`。从独立拒绝证据 `a2d7f7bb7dcf40d16b1ef31dc6059b0e5819f4c7` 继续；所有前次 source、报告、raw archive 原样保留。delivery 只追加证据，product 与 tested source 相同。未获得 parent 接受。

## 窄规则与先行红测试

`9546dde6924bbff9497a8ee66e58e39593fe5da1` 先冻结完整 558-query driver、2032-row matrix 和两个 regression tests，production 保持 rejected 实现。前置红结果 19 pass / 2 fail / 298 filtered，两个失败各对应新 matrix 与真实 SearchPlan guard；原 tests 未削弱。源码修正后 21 pass / 0 fail / 298 filtered。

修正仅在 comparison_phrase scanner 内统一附件判定：contiguous `::`、`->`、`.member` 与具备 member/call/index 后继的 `?.` 使闭合组成为代码上下文；同一判定用于词边界。祖先代码上下文屏蔽比较短语；仅比较目标被限定不屏蔽外层比较。单冒号、终止句点、终止 `?.`、分开的 operator characters 和空白仍按旧行为。详情见 QUALIFIER-CONTRACT.md。关键词的 maximal Unicode source word、ASCII case-insensitive 匹配、完整平衡组、同一 context identity、comma/whitespace gap 规则不变。

QueryTarget production、旧 single-word helper、DSL、plan.rs、原 Cargo.lock 均验证不变；明确 name:/kind:、exact identity 的优先级有真实 plan negative controls。matrix 保留 reviewer 全部 182 probes，覆盖全部 12 精确失败、8 种 wrapper、pair/clause closure、nested ancestor/inner closure、9 种 code 与 10 种 prose suffix、独立词边界与 qualified-target prose。不是 template 特判 12 个输入。

## 配对验证

Rust/Cargo 1.95.0，default features，原 lock SHA256 `ea6b177872de3749702e2cd967f4504efff5bcc67cd2a2459205d182243d1000`。两个全新独立 targets、三变体 fixture/index/results；顺序执行 Cargo，每次最多 `-j 2`。previous 19 tests，correction 21 tests 全通过；cc-eval 只 build lib，不跑其 tests。两臂 cc_search/cc_server/cc_eval production receipt 均 fresh=false，链接使用本臂 target 的实际 rlibs；完整 argv、hash、exit 保存。

完整 558-query driver 逐字保留 reviewer 原 292（含既有 245）queries/scorer gold/fixture，新增 266 bounded qualifier controls。三个 synthetic Go variants 为 base / receiver / inversion，所有返回 evidence 均重新 source-verify；MCP 为真实 in-process rmcp dispatch。不是 private corpus 或 production endpoint。

**3348 paired query/API checks**，**19088 common-hit checks** 全通过。**972** 个变化仅为已冻结 negative qualifier query 中 Beacon class 的 -0.18 / symbol-exact 移除；全部 non-name trace、其他 reasons、hit identity/span/text/kind、normalized source evidence 相同，trace sum 与 rerank_score 一致。**648** 个 rank-derived metadata.evidence_priority 变化逐项记录，其他 metadata 相同。若无 bonus 变化，ordered hits 完全相同。

全部 12 精确 reviewer failures 在三变体两路径共 72 个 observations 均无错误 Beacon bonus；其中 inversion 的 24 个 Top1 全为 previous Beacon → correction Commit（Beacon .6383333333333333 → .4583333333333333，Commit .5561538461538461 不变）。原 top_k=1 结果保持 previous 行为；tiny synthetic top_k=10 身份集合一致。这不是一般性 full candidate-set instrumenting。

Scoped rustfmt、diff check、clippy cc-search lib -D warnings 全通过。早期 base-only partial checker 的固定 footer 错误打印未执行的 inversion 结论；保留原 log 并改为按实际 executed variants 计数。完整 final checker 与其 assertions 均通过；source/expected/raw runs 未因此改变。

原 `Which Beacon API rather than Sink accepts state?` 三变体两路径均保持 Beacon；先前 245 契约没有 bonus/ranking/trace 漂移。receiver API/method 的 Top1=1 / MRR=1 保留；stronger lexical API inversion 仍 Top1=0 / MRR=.5。未宣称 full NL、general NL 或 public DEV qualification。

## 完整可重放证据

binding.json 绑定 exact source/checkpoint/previous/base/candidate SHAs、lock/source/driver/matrix hashes 与 archive hash；comparison.json 保留各 delta、rank-derived metadata 变化和 12 challenge 的真实 Top1。correction.patch.gz 为相对 rejected 1520407 的两文件统一 diff；candidate-to-source.patch.gz 为相对原 candidate 的累计 search source diff。

evidence.tar.gz 包含完整 .rs driver、matrix/contracts/rule、prepare/run/link/compare/package scripts、reviewer 原 probes/driver/controls、两臂 source/lock snapshots、六份 raw/normalized/native outputs、synthetic fixture source/config、红绿测试/build/link/lint/format logs、execution/link receipts；archive manifest 每个 member SHA/size 核对。没有 executable、rlib、target cache、DB 或 private export。重放时用新的 owned root、两个绑定 commit、全新独立 target/fixture/output，只调整本机绝对路径；保留 queries/expected/gold。

latest independent archive 125 members hash 核验通过；旧 review artifact 对 a2d7f7b 的 diff 为零。repo 与父目录无 applicable AGENTS.md/.agents/skills，遵循 CONTRIBUTING.md 和明确 scope。没有运行禁止的 semantic-runtime test/suites、DB GC/WAL/kill/EROFS、私有数据或生产 endpoints；没有 registry/CI/TODO/security/credential edits、PR、main merge/deploy/force push。本次 correction branch 留待 parent 独立复审。
