# 比较 phrase 源 token 边界修正（仅审查）

实际测试源码 SHA：`142549a65b7eaee78ae5a06a82ba8695a6f33bd9`。base `3c8c204216cb54c41850c6da826a68fec3586430`；原 candidate `cbe347fa513200de7fb8b12d1549b5e8ec20fad8`；被拒绝 delivery `9294ec09d5b6fbd1dee9a04680fa733f579469dd`。新分支从独立拒绝证据 commit `ca65b7073f78687a2e6286d7bf8a549d1a3e6443` 继续，所有历史实现、拒绝报告、失败观测与工件原样保留。最终交付 commit 仅追加本证据目录，源码与测试 commit 一致；交付 SHA 见 branch tip/交付回复。

## P2 修正与有限语法

之前把每个 token 两侧任意符号都清理后再匹配 pair，错误地把 `` `rather()` `than()` `` 和 `$rather $than` 当成比较短语。本次 `comparison_phrase` 直接检查原 token 拼写：只识别 plain `rather than` / `instead of`，或一个跨越整个 pair 的匹配括号/引号/backtick；保留既有第一词末尾单个 comma。有限 wrapper 为 `()`、ASCII 单/双引号、backtick、弯单/双引号。ASCII case、DSL 后的 whitespace（含 Unicode whitespace）行为保持。

分别包裹的 identifiers、调用 `()`、sigils、各自的 colon/其他标点、错配 wrapper、内部标点/下划线/CamelCase 与非相邻 words 不再被清理成 phrase。新增匹配范围是被拒绝实现的子集，没有新增 broad NL 推断。whole-phrase quoted positives 与单独 quoted/code-identifier negatives 明确分开。DSL 仍先提取 filters，因此原 `rather lang:go than` free-text 观测保持；没有改变 DSL contracts。

仅改 `query_target.rs` 的 phrase recognizer 和新增测试。QueryTarget、NameBonusContext 调用方与旧 `comparison_word`/single-word comparison helper 相对于拒绝 delivery 字节不变；`plan.rs` 相对于原 candidate 字节不变。原始 Cargo.lock 与 DSL 四臂一致。exact-identity 优先、显式 name:/kind:、receiver gain、normal unsupported fallback、taxonomy、ranking 常数与候选生成/过滤均保持原合同。两个既有控制矩阵和全部既有测试没有改写。

新增两项测试覆盖两个确切独立 negatives、分开的 quotes/parentheses、callables/sigils/colon/标点和不匹配 wrapper，同时保留全部 15 个独立 genuine phrase positives；SearchPlan 另验证 owner withholding、exact-identity、同名 callable、Commit 与 trace。红阶段为 **13 passed / 2 failed / 298 filtered**，均在确切 `` `rather()` `than()` `` negative 失败；这是加测试后的未提交状态，不能绑定为已提交 SHA。修正后 **15 passed / 0 failed**。

## 实际执行结果

官方 Rust `1.95.0 (59807616e 2026-04-14)`、Cargo `1.95.0 (f2d3ce0bd 2026-03-21)`；default features、original lock SHA256 `ea6b177872de3749702e2cd967f4504efff5bcc67cd2a2459205d182243d1000`。四臂顺序执行，每次最多 `-j 2`，分别使用全新 `target-correction`/`target-rejected`/`target-base`/`target-candidate`，没有跨 checkout 共用 target。每臂实际 production `cc_search`/`cc_server`/`cc_eval` artifact 的 `fresh=false` 均断言通过；link receipt 固定 target-profile rlib、rustc argv 与 binary SHA256。

- `cargo +1.95.0 test --locked -p cc-search query_target --lib -j 2`：base **6**、candidate **10**、rejected **13**、correction **15** passed；均 0 failed / 0 ignored / 298 filtered。
- 四次 `cargo +1.95.0 build --locked -p cc-eval --lib -j 2 --message-format=json-render-diagnostics` 成功；只 build lib，没有运行 cc-eval tests。
- scoped rustfmt 1.95、从 `ca65b707` 起的 source diff check 与 `cargo +1.95.0 clippy --locked -p cc-search --lib -j 2 -- -D warnings` 通过。历史日志 EOF whitespace 仍按原报告保留，不宣称从原 candidate 起的全部历史证据 diff check 没有 warning。
- **85 查询 × 3 fresh Go variants × engine/in-process MCP = 510 个四臂 query/API 对照（2,040 arm 输出）**。合并原独立 70-query、原作者 32-query 及新增 boundary negatives，去重后为 85；原 22 个 expected/scorer cases、fixture 和 native scorer 不改；新增 controls 没有附造 semantic gold。所有返回 hit 重新 verify_source；unrelated-name scorer controls 均拒绝。
- **7,404 common-hit pair checks**：身份、文本、span/kind/breadcrumb、normalized source evidence、lane scores、全部 non-name trace、非 bonus reasons 不变；trace sum 等于 rerank_score。其余 metadata 相同，排序派生的 `evidence_priority` 有 **200** 项记录（base→correction 152、candidate→correction 0、rejected→correction 48）。随机 source incarnation、运行计时和由序列化产生的 token estimate 不作为跨独立索引相等条件；完整 raw 保留。
- rejected→correction 恰有 **72** 个 Beacon class hit 撤回误恢复的 +0.18，全部限定在 12 个明确 code/reference/punctuation negatives。其他查询的 ordered hits、normalized evidence、status/native scores 与 rejected 一致。
- candidate→correction 恰有 **102** 个真正 phrase owner hit 恢复 +0.18；17 genuine phrase controls 的 ordered hits/evidence/status/scores 全部等于 baseline。其他查询保持原 candidate 行为。base→correction 仍有 **276** 个 candidate 所规定的 owner suppression，没有把所有 prose 一律恢复。

确切独立 negative（无新 semantic gold）：

| 查询 / inversion variant | base | candidate | rejected | correction |
| --- | --- | --- | --- | --- |
| ``Which Beacon API calls `rather()` `than()` with Commit?`` | Beacon | Commit | Beacon | Commit |
| `Which Beacon API calls $rather $than with Commit?` | Beacon | Commit | Beacon | Commit |

两条 API 路径均如此，Beacon 从 rejected `0.6043335161896403` 回到 candidate/correction `0.4243335161896402`；全部 non-name ranking components 保持。两个确切 negatives 共 **12 个 variant/API 观测**全部通过。

原 `Which Beacon API rather than Sink accepts state?` 仍在全部六个组合为 **Beacon→Sink→Beacon→Beacon**。receiver 的 API/method 在两条 API 路径均保留 Top1 **0→1**、MRR **.5→1**；stronger lexical inversion 的 API 仍 **Top1=0 / MRR=.5**，不能声称修复。`deprecated`、`Commit/Accept` 等未支持的语义仍保持 candidate 的有限 owner grammar 行为。top_k=10 本小 fixture 的返回身份集合一致；top_k=1 保持 candidate/rejected 的窗口，receiver 的 Beacon→Commit 是预期 rerank/truncate，不是 full candidate-set instrumentation。

## P3 driver 缺口与可复核交付

上一作者归档确实遗漏了宣称的 32-query driver，不能靠调整路径重放。历史 archive 不覆写。本目录的 `prior-author-32-query-driver.rs` 是当时 workspace 留存的原始文件逐字节副本，SHA256 `0fdc96fb2b30df45def2ba0782135575a516b8becf4ca1599bde3f1b818ea22c`；不是从缺失 archive 假称提取，也不是重新生成。它同时完整归档为 `prior-author/comparison_driver.rs`，可拷入旧 archive 的 unpack root 补齐原 `link_driver.py` 引用。原 32-query 构建/结果不重标为本次新 source 的结果。

本次完整 `boundary_driver.rs` 有 85 个 cases，SHA256 `422111a37e97170c48f86a3edf8840fee7aebea598f163a7370c738ee5ebe0a3`。两个 driver 都作为独立 Git 文件和 archive member 保存。封包验证显式断言两者 manifest/member 存在、SHA 相符、case 数为 85/32，且本次 link script 引用的 driver 可在 archive 找到；所有 **113** manifest entries 逐项核对。

`binding.json` 固定 SHAs/counts/driver/archive digest；`comparison.json` 保存逐 hit、bonus、metadata、score 与 code challenge；`correction.patch.gz` 是源码统一 diff。`evidence.tar.gz` 包含两个完整 driver、顺序 build/link/run 与 compare/package scripts、原始独立 probes（仅历史观测参考，未改为新接受合同）、新的明确 positive/negative contract、四臂 source/lock snapshots、12 份完整 raw/normalized/native 结果、fresh fixture 源码/config、test/build/link/lint/format receipts 与内部 manifest。未归档 binary、dependency cache 或 fixture DB。

复现采用新的 owned validation root，分别检出四个绑定 commit、分配全新独立 target/fixture/output 路径。取归档 scripts，仅按机器布局调整 `/workspace/owner-boundary-validation`、`/workspace/codecortex`、各旧 source worktree 路径；不调整 cases、expected、scorer 或 fixture。`run_validation.py` 顺序运行四臂，`compare_results.py` 应 exit=0；`execution.json` 保存实际 argv/cwd/target/exit，`toolchain.txt`/`identities.json`/link receipts 保存 pins。MCP 是 rmcp in-process dispatcher，不是独立 subprocess stdio。

仓库/父目录没有可用 AGENTS.md/.agents/skills；已按 CONTRIBUTING.md、两次独立报告与任务显式 narrow scope 工作。没有运行禁止的 semantic runtime test 或可启用它的 suite、private corpora/exports、DB GC/WAL/kill/EROFS 实验或生产 endpoint；没有 registry/CI/TODO/settings/credential 更改、PR、main merge、deploy 或 force push。两次拒绝证据和此前失效 shared-target attempt 保持原样披露。本次仍是 review-only，须 parent 独立重审后才能接受。
