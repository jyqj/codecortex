# Contextual-owner hint：独立审查冻结

基线：`3c8c204216cb54c41850c6da826a68fec3586430`。本次只交付 review branch；没有 PR、合并、部署或质量接受。该变更是有限 English owner-role grammar，不是任意自然语言意图模型。

## 最小生产行为

保留 `QueryTarget` 的显式 target 语法及 V2 classification，另加 `NameBonusContext`：在 `Which/What OWNER member-role ...` 或 `Which/What member-role on/of/in OWNER ...` 语法成立时，仅取消该 owner 同名 container-kind 的既有 exact-name bonus。`API` 不会被转换成 Method/Function。

- 原 exact-target 身份优先层、name:/kind: filters、不匹配语法的 legacy fallback 保留。
- 直接 class/interface/type_alias、裸 dot/文件名、已有比较/协调反例保持行为。
- 同名 callable、未知 kind、其他名称的 type 不受全局降权。
- 不改 parser taxonomy、ranking 常数、候选 budgets、gold、scorer 或过滤候选。

四个代码/控制路径：

1. `crates/cc-search/src/query_target.rs`
2. `crates/cc-search/src/plan.rs`
3. `crates/cc-search/src/engine_lane_tests.rs`
4. `artifacts/controls/query-context-owner-20261004/neutral-matrix.json`

源文件全部保留；没有修改 source registry、CI 或中央任务文件。

## 实际执行和证据

官方 Rust / Cargo 1.95.0 与 rustfmt 组件逐一核对 vendor SHA256，安装限于隔离目录。原 `Cargo.lock` 不变，默认 features，两条构建任务，baseline/candidate 使用独立 target 目录。

- 最初隔离编译：实际 QueryTarget/context/DSL source slices，SymbolKind 仅去除 serde 派生，token 为预置 unit 输入。两臂各 91 tests / 342 bonus assertions 通过。这个阶段不是完整仓库验证。
- 随后真实 cc-search 聚焦编译/测试：baseline 6 passed；candidate 10 passed；两者均 0 failed、0 ignored、298 filtered。
- 真实 matrix 测试执行 91 条查询、342 个 bonus probe。45 条旧 V2 target classification 不变。
- 新 SearchPlan 测试验证 exact-target 优先、trace 总和、同名 kinds、filters、dot/比较 fallback，以及 primary query 与 conversation context 分离。
- scoped rustfmt 和 `git diff --check` 通过。
- 三个自写 Go fixture 变体，每个 16 条查询，分别走实际 engine 与 in-process MCP；两臂共 192 个 query/API 输出。使用实际 normalizer、fresh source verifier 和未改 native scorer。
- 380 个共同 hit 的身份、文本、source evidence、lane 分数和所有 non-name trace 项逐值相同；返回成员集合无差异。
- 恰有 12 个 owner hit 移除了既有 0.18 bonus 及对应 symbol-exact reason。其余 84 个 query/API 对照的排序与 native score 不变。

MCP 是进程内 wire 路径，不是独立 stdio 子进程。synthetic 合同由这些自写 fixture 定义，没有使用 public gold/holdout。第一次手工链接误选 host-profile serde_json，编译失败而没有产生有效 driver；修正 target-profile artifact 选择后成功。最初失败日志保留。

## 修复与仍失败的控制

| 自写变体 / 查询形式 | engine Top1 | in-process MCP Top1 |
| --- | --- | --- |
| base / Which Harbor API ... Seal? | 1→1 | 1→1 |
| receiver / Which Harbor API ... Seal? | 0→1 | 0→1 |
| receiver / Which method on Harbor ... Seal? | 0→1 | 0→1 |
| stronger lexical inversion / Which Harbor API ... Seal? | **0→0** | **0→0** |
| stronger lexical inversion / Which method on Harbor ... Seal? | 0→1 | 0→1 |

receiver/API 的 MRR 在两条 API 路径均为 .5→1。stronger lexical inversion/API 仍失败，不能将本次结果称为通用 broad-query 或正式 public DEV 质量修复。

## 审查工件与复现

- `binding.json`：四个候选文件、lock、归档摘要和边界。
- `evidence.tar.gz`：六份完整 raw/normalized/scorer 输出、comparison、driver/source、test/build/link receipts 及哈希。
- 归档 SHA256：`083ce321bc9ce2e3fef7bda0b2870fe11bf45ad13042b12edf69c7726f5deadc`。
- 归档内 `archive-manifest.json` 可逐文件复核；可执行文件/依赖缓存未归档，binary pins 仅为 build-receipt 证据。

复现应使用全新 owned 工作目录，分别检出上述 baseline 与本 review commit，保持默认 features/original lock。先运行 `cargo test --locked -p cc-search query_target --lib -j 2`。若复核 engine/MCP，分别 `cargo build --locked -p cc-eval --lib -j 2 --message-format=json-render-diagnostics`，用对应 target-profile rlib 链接归档的 `receiver_micro_driver.rs`，传入全新 fixture/output 路径及 `base|receiver|inversion`。`link_micro.py` 和 linking receipts 记录实际方法；不要误选 host-profile serde_json。`check_micro_results.py` 记录逐 hit 比較，所有旧输出保留不覆盖。

## CI、范围与建议任务

基线树仅有 `.github/workflows/ci.yml`，push 只匹配 main，另一个触发为 pull_request。本次非 main review branch 不匹配该 push trigger，且不创建 PR。没有借助 CI 执行额外测试。

未运行 formal public DEV、holdout、100k、private export、原 denied post_index/其他 excluded runtime 或全仓测试套件。准确 taxonomy、旧 controls 和全部既有证据未被改写。

建议后续任务（仅提案，没有修改中央任务状态）：

1. 独立审查 API-as-owner 的误判、同名成员边界、未覆盖的比较/协调表达。
2. 把仍失败的 stronger lexical API 控制作为单独问题保留，不能用改权重/gold 的办法将其伪装成通过。
3. 获得变更接受后，按既有 source-registry 集成流程审查新的源文件绑定；当前 registry 仍指向已批准基线。不得盲目重写 registry 或削弱 validator 来获得绿色 CI。
