# comparison phrase correction 独立 exact-head 复审

结论：**REJECT**。实际测试 delivery SHA `9294ec09d5b6fbd1dee9a04680fa733f579469dd`；其 production/test 源码与作者 tested source `9a4322d326a6630f7729a97b4658a3efbea5d9fa` 完全相同。baseline `3c8c204216cb54c41850c6da826a68fec3586430`，rejected candidate `cbe347fa513200de7fb8b12d1549b5e8ec20fad8`。当前 correction 不满足代码标识符边界，不应据本审查整合。

## P2：代码引用被清理成 comparison phrase（blocker）

位置：`crates/cc-search/src/query_target.rs:198–208`。`comparison_word` 无差别去掉 token 两侧所有非字母数字、非下划线字符；新 pair detector 因而将分别引用的 callable identifiers `` `rather()` `than()` `` 清理为 `rather` / `than`，也把 `$rather $than` 清理成同一 pair。这两种代码引用本身不是独立的比较短语。QueryTarget 仍为 Ambiguous，但 NameBonusContext 从 candidate 的 owner=beacon 变成 None，导致本来应继续 withholding 的 container bonus 被恢复。

独立反例：

- ``Which Beacon API calls `rather()` `than()` with Commit?``
- `Which Beacon API calls $rather $than with Commit?`

每条查询在原来的三个 fresh Go variants、engine/in-process MCP 两条路径中均恢复 Beacon class 的 `+0.18`。在 inversion variant，两条路径均由 **candidate Commit → delivery Beacon**；baseline 也是 Beacon。Beacon 分数从 `0.4243335161896402` 到 `0.6043335161896403`，Commit 及全部 non-name ranking components 保持不变。`challenges.json` 保留 12 个实际三臂观测及两项失败的边界断言，`challenges.py` 预期 exit=1。这里没有为这些新查询附造 semantic gold；发现是代码标识符误触发新 comparison guard 及其实际排名影响，不能转述为通用语义准确率。

建议下一 correction 区分 prose 边缘标点与 callable/identifier 的代码记号，增加上述 negative controls，同时保留已有 case/whitespace/parenthesized/comma phrase positives。没有修改本候选实现或其测试、控制矩阵。

## P3：作者 correction 归档缺少宣称的 driver

correction README 声称 `evidence.tar.gz` 包含 32-query driver，实际 83-entry manifest 和 archive 中没有 `comparison_driver.rs`。归档的 `link_driver.py` 正好引用这个不存在的文件，单纯调整路径不能重放。所有 83 个现存 manifest entries 的 SHA256 均通过，archive SHA256 与 binding 一致；这不是文件损坏。仓库旧独立 driver 只有原 22 个 cases。本次独立 driver 单独完整归档，没有冒充缺失的作者 32-query 源码。应补齐该作者源工件；此 documentation/replay 缺口不单独作为 code blocker。

## 已确认的通过项与范围

- Rust/Cargo 1.95.0、default features、原始 Cargo.lock；三臂 lock SHA256 均为 `ea6b177872de3749702e2cd967f4504efff5bcc67cd2a2459205d182243d1000`。
- `cargo +1.95.0 test --locked -p cc-search query_target --lib -j 2`：base 6、candidate 10、delivery 13 passed，均 0 failed / 0 ignored / 298 filtered。没有运行其他 tests。
- 三臂顺序执行，分别采用全新 `target-delivery`、`target-candidate`、`target-base`；各臂 `cc_search`/`cc_server`/`cc_eval` production build receipt 均为 fresh=false。只 build cc-eval lib，未运行 cc-eval suite。rlib/binary SHA256 与实际 rustc argv 在 link receipts 中。
- 57 个独立 production-module probes，覆盖 ASCII case、tab/newline/Unicode whitespace、prose 标点、quoted phrase、underscores/CamelCase/internal dot/hyphen/slash/qualifier、非相邻词、substring/Unicode identifier、显式 name/kind 与 named syntax。支持范围的观察断言通过；代码引用两项另用 reviewer negative assertions 明确失败。初始 probes 中对这些形式记录的是实际有限 grammar 行为，不能把该 PASS 当成 reviewer 接受。
- 原 22 个 cases/scorer/fixture 不改，新加 48 个无新 gold 的独立查询：70 queries × 3 fresh variants × engine/in-process MCP = **420 三臂 query/API 对照（1,260 arm outputs）**。全部返回 hit 重新 verify_source；原 native scorer 的 unrelated-name 控制保持拒绝。
- **3,964 common-hit pair checks**：chunk identity、source text、span/name/kind/breadcrumb、normalized source evidence、lane scores、non-name trace、非 bonus reasons 不变；trace sum 与 rerank_score 相符。candidate→delivery 114 个 owner bonus 恢复、base→delivery 174 个 owner bonus withholding，变化均恰为 ±0.18。116 项 `metadata.evidence_priority` 差异由排序引起并逐项保存；其他 hit metadata 一致。
- top_k=10 本 fixture 的返回身份集合一致；top_k=1 与 candidate 一致（原 receiver 查询）。不是一般性全检索 candidate-set instrumentation。比较不要求独立索引的运行计时、随机 source incarnation 或 token estimate 相等。
- QueryTarget production bytes 三臂一致；DSL、原 lock 一致；`plan.rs` 与 candidate 一致。exact-identity priority、显式 filters、ranking 常数及候选生成/过滤不变。原聚焦 tests 和真实 API controls 覆盖 exact-name/filter 优先级。
- 原 regression `Which Beacon API rather than Sink accepts state?` 在全部六个 variant/API 组合仍为 **Beacon → Sink → Beacon**。原 receiver 的 API/method Top1 0→1、MRR .5→1 保留；更强 inversion 的 API 仍 Top1=0、MRR=.5，未因此声称修复。
- 对作者保存输出重跑其 comparator，独立核实原 192 对照、1,768 common-hit checks、36 bonus restores 和54 suppressions；这是 archived-output audit，和本次 fresh 420 对照分开记录。
- scoped rustfmt 1.95、delivery 从 `416f671` 起的 diff check、新 evidence diff check 通过。未修改历史证据。

## 可复核工件与执行说明

`binding.json` 固定 verdict/SHAs/counts/archive digest；`identities.json` 保存 production module/plan/DSL/lock hashes 与作者 archive audit；`comparison.json` 保存逐 query/hit 检查、实际 scores/regression/rank metadata；`challenges.json` 保存 blocker。`evidence.tar.gz` 包含 96 项 manifest 绑定的 scripts、完整独立 driver、production-module wrappers、原锁/source snapshots、九份 raw+normalized+native 结果、fresh fixture 源码/config、test/build/link logs 和命令。没有 binary、dependency cache 或 fixture DB。

复现使用新的 owned validation root，按绑定 SHA 检出三臂，分别指定新的 target/fixture/output 路径。取归档 scripts，按本机布局调整 `/workspace/independent-comparison-review` 与 `/workspace/codecortex`；从仅含 scripts 的新目录开始（勿携带旧 artifact JSONL，否则 `run.py` 的恢复逻辑会复用 receipt）。`prepare.py` 在各 checkout 生成 wrappers，并从仓库原 22-query driver 加入本次明确的独立 probes；生产文件不编辑。先运行 delivery focused test（其命令见上），再 `run.py` 顺序 build/link/run，最后 `compare.py` 应 exit=0，`challenges.py` 对当前 correction 应 exit=1。保留 query/expectation/scorer，不调整为通过。

本次最初 external driver link 选到了 build/proc-macro profile 的 serde_json，产生 crate identity mismatch，在任何 fixture 执行前失败。随后依据 production profile `debuginfo=2` 选库并成功 link；没有共用跨臂 target。`execution.json` 保存失败与成功 argv/exit；初始失败 log 被 retry 覆盖，已按保存的原 argv 重现错误并明确标为 reproduced，未伪称原始 log。全部有效 API 输出来自成功的 profile-pinned receipts。

仓库/父目录无可用 AGENTS.md 或 .agents/skills，已阅读 CONTRIBUTING.md、作者 correction report/binding、旧独立 review、相关 source/matrices；本任务明确的 narrow scope 覆盖 CONTRIBUTING 的全仓测试要求。没有运行禁止的 `semantic_runtime::tests::post_index_worker_crosses_pages_and_reopen_reuses_artifacts` 或可启用它的 suites，没有 private corpora/exports、DB GC/WAL/kill/EROFS 实验或 production endpoint。没有修改 candidate/registry/CI/TODO/settings/credentials，没有 PR、merge、deploy 或 force push。仅向独立 evidence branch 普通 commit/push；parent 仍须在 blocker correction 接受后才整合。
