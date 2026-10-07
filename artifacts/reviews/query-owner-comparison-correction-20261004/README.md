# Contextual-owner 比较表达 P2 修正（仅审查）

实际测试源码 commit：`9a4322d326a6630f7729a97b4658a3efbea5d9fa`。base：`3c8c204216cb54c41850c6da826a68fec3586430`；原 candidate：`cbe347fa513200de7fb8b12d1549b5e8ec20fad8`。修正基于独立证据 commit `416f671391307400663af2ce08728a211215cbb3`，完整保留作者和独立审查工件。交付 commit 只追加本证据目录；最终 SHA 在 Git branch tip/交付回复中给出，以免自引用。

## 修正范围

`NameBonusContext` 在原有单词 comparison/coordination guard 外，增加相邻独立词 `rather than`、`instead of` 的 abstention。匹配忽略 ASCII case，使用 DSL 规范化后的 whitespace tokens，复用原有边缘标点清理规则；不拆开内部标点、下划线或 CamelCase。因此 `RatherThan`、`rather_than`、`rather.than`、子串以及被其他词隔开的 pair 不触发新解释。

`QueryTarget` production 部分与 base/candidate 一致，`plan.rs` 与 candidate 逐字节一致。没有改变 exact-identity 优先、name:/kind: filters、ranking 常数、候选生成/过滤、taxonomy 或其他 fallback。两个源文件的完整修正见 `correction.patch.gz`（原始 unified diff 压缩文件；归档内同时保留 `correction.patch`）。原测试和两个既有 matrix 均未改写。

新增三项聚焦测试：真实 DSL/tokenizer 下的比较 phrase fallback；case/whitespace/标点/identifier/nonadjacent negative controls 与全部六个原独立 probe；SearchPlan 的 +0.18 trace、同名 method、Sink 和 exact-identity 保留。原候选加新测试的红阶段为 **10 passed / 3 failed / 298 filtered**，三项均在确切 `rather than` regression 处失败；日志保留。红阶段不是一个有 SHA 的已提交源码状态。

## 执行与结果

Rust `1.95.0 (59807616e 2026-04-14)`；Cargo `1.95.0 (f2d3ce0bd 2026-03-21)`；default features、原 Cargo.lock；每次最多两条 build jobs，三个 arm 顺序执行，分别使用全新 `target-base`、`target-candidate`、`target-correction`。每臂 production `cc_search`/`cc_server`/`cc_eval` artifact 均检查 `fresh=false`。link receipts pin 对应 target-profile rlib 和可执行文件 SHA256。

- `cargo +1.95.0 test --locked -p cc-search query_target --lib -j 2`：base **6 passed**，candidate **10 passed**，correction **13 passed**；均 0 failed / 0 ignored / 298 filtered。
- scoped rustfmt 1.95、从独立证据 commit `416f671` 起的修正范围 `git diff --check`、`cargo +1.95.0 clippy --locked -p cc-search --lib -j 2 -- -D warnings` 均通过。
- 各臂 `cargo +1.95.0 build --locked -p cc-eval --lib -j 2 --message-format=json-render-diagnostics` 成功；只编译 lib，没有运行 cc-eval tests。
- 32 个自写查询 × 3 个全新 Go 源变体 × engine/in-process MCP = **192 个三臂 query/API 对照（576 个 arm 输出）**。独立 driver 的原 22 个 query/expected case 不变，新增 10 个 phrase/negative 查询不附造新 gold。所有返回 hit 经实际 source verifier 验证；未改 native scorer 的 unrelated-name 控制均拒绝。
- 两组 paired comparison 合计 **1,768 common-hit 检查**：身份、文本、kind、source evidence、lane 分数、所有 non-name trace 与非 bonus reasons 一致；trace sum 与 rerank_score 相符。
- candidate→correction 恰有 **36** 个 Beacon class hit 恢复 +0.18/symbol-exact，对应六种真正 comparative phrase 的六个 variant/API 组合。其他查询的有序 hits、规范化 source、status 与 native score 保持 candidate 行为。
- base→correction 仍有 **54** 个 owner hit 抑制 bonus，这是保留 candidate 的非比较 owner-reading 和 identifier negative controls。四个 rank-derived `metadata.evidence_priority` 差异仅出现在 inversion 的 `Commit/Accept` 查询、base→correction；candidate→correction 此项无差异。其他 hit metadata 逐值一致。运行计时、随机 source-generation incarnation 与由序列化大小产生的 token estimate 不作为跨独立索引相等条件；raw 输出完整保留，不声称完整 wire payload 相同。
- top_k=10 本小 fixture 的返回 hit 集合均相同；这不是一般性 full retrieval candidate-set instrumentation。top_k=1 与 candidate 完全一致，receiver 的 base Beacon→correction Commit 是预期排序/截断结果，不是新增候选过滤。

确切独立 regression `Which Beacon API rather than Sink accepts state?`：base/receiver/inversion 三变体与两条 API 路径均为 **Beacon → Sink → Beacon**（base→candidate→correction）。所有新增真 phrase 对照恢复 base 的有序 hits、规范化 source、status 与 score。

receiver 的 API/method 查询两条路径均保留 Top1 **0→1**、MRR **.5→1**。stronger lexical inversion 的 API 查询仍 **Top1 0→0、MRR .5→.5**；method 查询保留 **0→1**。不能据此称为通用 NL 或 public DEV 质量修复。`Which Beacon API is deprecated?` 和 `... calls Commit/Accept?` 仍按原 candidate 触发 owner suppression，是有限 grammar 的已知歧义，未在本次扩展修正。

## 工件与复核边界

`binding.json` 绑定三个源身份、lock 与数字；`comparison.json` 保留逐 hit/score/Top1 比较；`evidence.tar.gz` 包含源码 snapshots、32-query driver、顺序执行脚本、比较脚本、九份完整 raw/normalized/native 结果、fresh fixtures 的源文件/config、编译/test/link/lint/format 日志及内部 manifest。没有归档 binary、dependency cache 或 fixture DB。

复现应使用新的 owned validation 根路径，分别检出上面的三个 commit，并为每臂指定不同的全新 target 和 fixture/output 路径。归档 `run_validation.py`、`link_driver.py`、`compare_results.py` 给出实际命令/方法；仅按本机布局调整路径，不调整 probe/expected、scorer 或源 fixture。`execution.json` 记录执行命令及 exit，`toolchain.txt`/`identities.json`/`*-link.json` 给出 pins。MCP 是真实 in-process rmcp dispatcher，不是独立 stdio subprocess。

仓库及父目录无可用 AGENTS.md/.agents/skills；已读取 CONTRIBUTING.md 与作者/独立审查 README、binding、controls。按本任务显式限制，没有运行 CONTRIBUTING 中全仓/public/private benchmark 套件。未运行被禁止的 semantic runtime test 或可启用它的 suite，未运行 private exports、DB GC/WAL/kill/EROFS 或生产 endpoint；未修改 registry、CI、TODO、settings 或 credentials；没有 PR、main merge、deploy 或 force push。

独立证据 manifest SHA256 `34bb334af90b1ebaee0229d04810e577326872a7702e99ea71e097fff811fa50`，complete archive SHA256 `d13f3d299939c15ec5e419e0640cf74e6ca30f2449b5210124eaddf20a2bbe98`，全部 60 项核验保持不变。之前失效的 shared-target attempt 仍按独立报告披露并保留；本次三臂重新构建没有复用其 receipt 或 executable。接受前仍由 parent 独立复审。

从原 candidate `cbe347fa` 检查完整范围时，独立证据 `base-tests.log`/`candidate-tests.log` 各有一个既存 EOF blank-line warning；`diff-check.log` 原样保留这一非零检查结果。按用户要求不改这些历史日志。新增修正源码/证据从 `416f671` 起的 scoped checks 均为 exit=0，见 `source-diff-check.log`/`correction-diff-check.log`。单独 unified patch 改为 gzip，避免将 diff context 空行误算为新文件 trailing whitespace；解压内容与实际源码 diff 相同。
