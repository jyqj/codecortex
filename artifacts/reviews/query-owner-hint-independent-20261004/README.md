# contextual-owner hint 独立审查

审查对象 `jyqj/codecortex`，candidate `cbe347fa513200de7fb8b12d1549b5e8ec20fad8`（`review/query-owner-hint-20261004-0741`），唯一 baseline `3c8c204216cb54c41850c6da826a68fec3586430`。parent 已将 P2 发现接受为 blocker，candidate 未获批准。本 evidence-only branch 直接基于 candidate；按 parent 后续授权提交/push 独立证据，没有修改产品、source registry、CI、TODO、main，也没有 PR、merge 或 deploy。

## 独立发现

**P2：`rather than` 对比语法未 abstain，能把明确排除的另一名称推到 Top1。** `crates/cc-search/src/query_target.rs:124–145` 只检查有限单词列表，任意非空 tail 即成立。全新 synthetic 查询 `Which Beacon API rather than Sink accepts state?` 在 base/receiver/inversion 三个源变体、engine 与 in-process MCP 两条路径中均为 `Beacon → Sink`。`Beacon` 的 fallback +0.18 被取消，而 `Sink` 保留其 bonus。这不是 public gold 的质量分数，而是一个可复现的独立 comparative false-positive control。建议整合前至少给 `rather than` 明确的 abstention/control；如果决定接受这个边界，应把该实际排名变化明确列为已知限制。仅保持旧有限比较列表，不能宣称普通比较表达均保留 fallback。

**成员集合声明需限定。** top_k=10 的 132 个 paired query/API 对照中没有成员集合变化；但是 receiver 的同一 API 查询在 top_k=1 时由 `chunk:micro.go:2 / Beacon` 变为 `chunk:micro.go:4 / Commit`。这里的 top_k=1 变化是排序后的返回窗口变化，**不是 full retrieval candidate-set membership 变化**。静态 diff 确認 candidate generation/filtering 没变；top_k=10 本小 fixture 全部已返回 hit 的集合相同不等于一般性全候选集合 instrumentation。代码仍在 rerank 后 sort/truncate (`plan.rs:609–610`)，因此“不新增过滤或改变检索候选”成立，“任意 top_k 返回成员集合不变”不成立。该变化是 receiver 排名修复的预期结果，不是单独产品缺陷。

**API-as-owner 仍存在歧义。** `Which Beacon API is deprecated?` 也移除 `Beacon` container 的 +0.18；本 synthetic fixture 没有 deprecated 事实，这条仅测试 bonus 触发范围，不能解释为语义正确率。这个例子没有 Top1 变化。`Which Beacon API calls Commit/Accept?` 也触发抑制，而对应 `... Commit and Accept?` 保持 fallback。`... differs from Sink?` 的既有 comparison word 能正确 abstain。有限 grammar 的宽容 tail 和单词检测边界应保留为风险，不推导 general NL 接受。

## 确认的行为和证据

- `QueryTarget` production implementation byte-for-byte 不变（归一化末尾空白后的 SHA256 在 identities.json）。独立 `NameBonusContext` 只 gate additive exact-name boost，不写回 target/kind，也不改 taxonomy、lane budgets、hard scopes 或 retrieval。
- `name:` 生成 Named target；已有 named syntax、ContainerMethods target 和任意有效 `kind:` 都阻止 context hint。原 exact-identity 分支仍优先，compare_hits 仍首先按 exact-target tier，再按 rerank_score 和 chunk_id 排序。
- 指定 owner 同名 class/interface/type_alias/enum/module/namespace 才 withholding；同名 callable、未知 kind、其他名称和 primary/conversation 区分由真实候选聚焦测试覆盖。直接 class/interface/type_alias、dot/filename、原 comparison/coordination 与显式 filters 均通过。
- candidate scoped tests：10 passed / 0 failed / 0 ignored / 298 filtered；base：6 passed / 0 failed / 0 ignored / 298 filtered。命令为 `cargo test --locked -p cc-search query_target --lib -j 2`，没有运行其他测试 target。
- scoped rustfmt、`git diff --check` 通过。
- 原 Cargo.lock SHA256 `ea6b177872de3749702e2cd967f4504efff5bcc67cd2a2459205d182243d1000`，两臂相同；candidate 四个 source/control SHA256 与作者 binding 全部一致；归档 SHA256 `083ce321bc9ce2e3fef7bda0b2870fe11bf45ad13042b12edf69c7726f5deadc`；归档 manifest 73 项逐一核对无差异。
- fresh fixture 使用 Beacon/Commit/Sink/SeedCode，沿用作者小 fixture 结构但改名并新增 6 条独立 challenge。22 queries × 3 variants × 2 API paths = 132 paired outputs（264 arm outputs）。driver 对返回 hit 全部重新 verify_source；unrelated-name scorer 控制均拒绝。
- 524 common hits 的 chunk identity、path/span/name/kind/text/breadcrumb、normalized source evidence、fused/lexical/grep/graph、所有 non-name trace 项完全相同，trace sum 等于 rerank_score。30 个同名 owner hit 恰好少 +0.18 与 symbol-exact reason，没有其他 score/reason 差异；top_k=10 membership differences=0。新增 challenge 解释了为何变动数超过作者的 12。
- 两条 API 路径一致：receiver/API 和 receiver/method Top1 0→1、MRR .5→1；inversion/API 仍 Top1 0→0、MRR .5→.5；inversion/method 0→1。不能将其称为 general natural-language 或 public quality 接受。

## 执行身份和复现

Rust `1.95.0 (59807616e 2026-04-14)`，Cargo `1.95.0 (f2d3ce0bd 2026-03-21)`，default features，original lock，所有 Cargo 调用最多 `-j 2`。正常许可网络只下载官方 locked crates。仓库和父目录没有可用 AGENTS.md 或 .agents/skills；读取并遵循 CONTRIBUTING.md、指定 review README/binding 与相关源控制。

候选构建在 `/workspace/codecortex/target`；**有效 base production build** 在全新 `/workspace/review-owner-evidence/target-base`。candidate/base 各自 linking receipt 记录实际 target-profile rlib 和 binary hashes。`base-artifacts.jsonl` 中 cc-search/cc-server/cc-eval 均为 fresh=false（实际重编）。所有有效 source fixtures/output dirs 创建前必须不存在。

本审查初次错误共用了 production target，Cargo 将 candidate libraries 当作 base fresh cache；该初次 pair 完全无效，未用于上述发现。相关初次 logs/output 留在本地 `initial-shared-target-invalid` / `invalid-shared-target-*` 路径，没有纳入有效证据包。随后通过独立 base target 重建、重新 link、全新 fixture 重跑并完成有效比较。base focused tests 本身实际重编，且结果 6 passed，不受该 production cache 问题影响。

`commands.txt` 给出 exact commands/environment；`identities.json` 给出 commit/tree/blob/source hashes；`*-link.json` 给出二进制和 libraries hashes；`independent_driver.rs`、`link_independent.py`、`compare_independent.py` 以及六份 `results/*/results.json` 可以核对上述统计。MCP 是真实 rmcp in-process duplex dispatcher，不是独立 stdio subprocess。

没有运行 formal public DEV、holdout、private exports、100k、semantic_runtime::tests::post_index_worker_crosses_pages_and_reopen_reuses_artifacts、workspace/cc-eval suites、GC/WAL/kill/EROFS 实验或生产 endpoint。未触碰 settings/credentials。没有执行 source registry 集成；该工作仍由 parent 负责。

## 持久化边界

branch `review/query-owner-hint-independent-evidence-20261004`；目录 `artifacts/reviews/query-owner-hint-independent-20261004/`。`independent-evidence.tar.gz` 保留原有效证据包逐字节不变；其 SHA256 为 `cc9ade4c5d7f7659a1bb6836be8bd57ace5f46e679bf9b1504612b558026c58b`。直接文件保留未修改的 test/probe sources、有效两次 production builds 的 artifact JSONL、link receipts、source snapshots 和 original locks；额外 complete archive 包含本完整目录（manifest/archive 自身除外）。

`invalidated-shared-target/` 保留初次错误 build/artifact rows、错误 comparison 和三个错误 base raw outputs；`fixture-sources/` 保留其 fresh source/config。该初次错误 base link receipt 与 binary 后来被正确 base link 覆盖，原始版本不再可恢复；不伪造它们。初次 artifacts JSONL 中的 reused libraries 路径和三份实际错误输出仍保留，可审计为何排除。原 candidate link receipt/binary hash 与纠正后的独立 base link receipt/binary hash 保持原值。

初次和最终比较脚本/driver 的 expected cases 没有为 regression 调整：独立 comparison 明确保留该 `rather than` 查询的 actual ranking divergence，用户未授权修正产品。无新运行/分数替换；manifest 可逐文件审计持久化内容。
