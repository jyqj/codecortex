# 三项有界 groundwork 集成（2026-10-04）

固定 base：`37dd042eaa1209a86e0cafdcd92ae77e036e76f5`。
固定组合产品（含独立测试、不含本次文档修正与验证回执）：
`e4a8df4cbc6dfae29af8cb0eac9ee83fbba4d696`。
分支：`integration/bounded-groundwork-20261004`。
test-only fix source：`d62215f6ab075aadb4da0f99d52a29ce22c027c5`。
文档/回执提交与远端状态见交付响应和 `PUBLICATION.md`，不把文档 SHA 当作新产品源码。

结论：三项 accepted limited scope 已整合；49 scoped tests 与70 frozen controls通过。
初始 strict all-target lint 失败历史保留；用户随后明确授权单表达式 test-only lint 修正，
最终 `d62215f6ab075aadb4da0f99d52a29ce22c027c5` 上六条 affected tests、strict all-target
lint、fmt与source guard均PASS。生产源码仍逐byte等于产品e4a8df4，不是release/public quality认证。
没有生产冲突或新增runtime source修补。

## 精确来源和历史

| 范围 | 实现（原历史 merge 保留） | 独立测试/报告（仅 isolated cherry-pick -x） |
| --- | --- | --- |
| 未接线声明身份模型 | 866cbed73f303463a80cee07462824d1816f2a44 | 8d5282acbc0ff630b37429c258968bb07090ba65 |
| Python 显式 root provenance | a65c655f7822bc8d025d6c01b1aa86371bbd9de9 | bc0d0a25 + 26becaaa5287a12c63c12c0aaac3f4b71f0a7e03 |
| explicit query-target gate final | 54b2b92cfca035ef1de2b4d6478f314231fd5511 | 56fd54e2 + 537618bae7084f08ecdcf80b18dbbcbcc2735c7b |

query history 完整保留 `443ca67a` expectation freeze → `c1aa6607` source →
`f8dd834f` V2 freeze → `54b2b92c` fix；未用 reviewer ancestry 覆盖 final source。
原28条 controls、旧P2 REQUEST CHANGES/反例与修复证据原样保留。
独立 Python 报告记录最初按错误文档假设 `is_err()` 的失败 probe，保留该历史，
并复制其 accepted-contract probe 到组合 cc-model 测试。唯一事实文档修正：
prototype 接受 normalized 空根，也接受并归一化 `.`；模型实现未改。

检查 `/workspace`、repo、`.agents`/`.codex` 与 relevant AGENTS/SKILL 文件，未发现适用
本地指令或技能；遵守 CONTRIBUTING 中文文档、官方 Rust 1.95、原 lock。

## 实際验证

环境：官方 `rustc 1.95.0 (59807616e 2026-04-14)` / LLVM22.1.2、默认 features、
原 Cargo.lock，Cargo命令均 `--locked`。命令与完整日志在 `receipts/`；共享 target cache
仅用作构建缓存，Cargo实际编译的是本 integration worktree。driver链接只使用当前
Cargo JSON返回的 target-profile artifact，未按mtime或旧review binary选库。

| 检查 | 结果 |
| --- | --- |
| declaration_identity_v1 | 15 passed / 0 failed / 0 ignored |
| declaration_identity_independent_review | 11 passed / 0 failed / 0 ignored |
| provenance_compatibility | 1 passed / 0 failed / 0 ignored |
| cc-index --lib python_provenance | 12 passed / 390 filtered / 0 ignored |
| python_provenance_independent_review | 4 passed / 0 failed / 0 ignored |
| cc-search --lib query_target | 6 passed / 298 filtered / 0 ignored |
| fmt --all --check | PASS |
| build --workspace | PASS |
| clippy --workspace --all-targets -D warnings | 初始FAIL（exit101）；授权test-only修正后PASS |
| clippy --workspace --lib --bins -D warnings | PASS |
| source union/evidence/lock guard、wire absence guard | PASS |
| 中央roadmap派生 | PASS；192 = 150 done / 41 todo / 1 in_progress |

初始all-target lint唯一诊断是原作者新增 `crates/cc-search/src/engine_lane_tests.rs:1235`
`let mut hits = vec![member, exact]` 的 `clippy::useless_vec`。按只允许机械 module
registration conflict resolution 的授权边界，保留精确原测试而未替换为array、降低lint
或修改生产源。随后用户明确授权仅把该expression替换为`[member, exact]`，
未改assertions、未加lint allowance；affected六条tests及strict all-target lint重新通过。
完整原测试bytes保存在`test-only-lint-fix/engine_lane_tests.rs.original`，digest与精确
单表达式transformation保存在`transformation.json`；更新source guard仅承认这一test-only
exception。初始失败日志/原guard报告/独立review证据不覆盖，最终回执在`test-only-lint-fix/`。

70条独立delta-v2冻结控制均使用本次实际 CodeIndex + **in-process MCP wire** + 原
normalizer/source verifier，非子进程stdio。覆盖direct type/callable、显式name/kind、
纯`::`、严格narrowpointer、bare dot/mixed separators、filename/未知extension fallback、
malformed/Unicode/context augmentation。41 fallback、29 supported-hint检查通过，
683 returned hits/source traces通过，678共同hit non-name分量保持、102 exact身份保持、
224 hint eligibility检查、19 name-only trace变化，最大trace误差1.11e-16。

本次683条，历史独立fixed报告684条；保留实际packing/returned-count差异，未把历史
计数当成本次计数。checker验证共同retained prefix并单独记录packing差异，不声称未返回
候选完整性或全部行字节一致。两条原filename P2反例与原base逐hit完全恢复。
`results.json`、`comparison.json`、`binding.json`保存raw和normalized observations、
argv、driver/binary/lock/matrix/fixture digest。checker仅改输入输出位置与产品SHA，原证据未改。

## 生产路径与禁止变化核对

相对fixed base仅以下8个生产路径变更；每个byte等于其已接受来源：

- `crates/cc-model/src/declaration_identity.rs`
- `crates/cc-model/src/lib.rs`
- `crates/cc-model/src/module_inputs.rs`
- `crates/cc-index/src/project_model/mod.rs`
- `crates/cc-index/src/project_model/python.rs`
- `crates/cc-search/src/lib.rs`
- `crates/cc-search/src/plan.rs`
- `crates/cc-search/src/query_target.rs`

`source_guard.py`按精确source union核验，review新增路径按独立commit逐byte核验；
原author controls及lock也逐byte核验。gold/scorer/weights/budgets/parser/normalizer、
qname/UID/旧生产identity实现均无改动。声明identity仅新增module注册，没有生产调用；
实际engine/MCP返回没有新的声明identity/binding/address字段。Python派生JSON新增
provenance字段不被误称为整份JSON字节兼容；旧缺失字段默认Unknown，不自动授权转换。

## 本切片 TODO 与仍开放的父项

- [x] 三组精确source history集成，独立reviews隔离导入并保留旧失败证据。
- [x] normalized空根/`.`文档事实修正与组合兼容probe。
- [x] combined scoped tests、实际engine/MCP controls及source/wire guards。
- [x] strict all-target lint：单独授权的原作者test-only useless_vec机械修正，六条affected tests与strict gate通过。
- [ ] AST adapter：同一source bytes/AST产生完整typed ancestry/kind/range，不用token assertion替代。
- [ ] source capture：完整inventory、native alias/duplicate、marker absence/collision、owner与symlink/race政策。
- [ ] resource/cache：总bytes/file-count/ancestry/identifier限制，验证capture复用与完整dependency/invalidation成本。
- [ ] versioned ingestion重新推导，之后独立设计optional DB/MCP identity及publication guard。
- [ ] 原四仓public quality **FAIL**、Gin broad-prose regression **OPEN**；不关闭parentquality/P7/V19。
- [ ] 后续授权后固定combined source重新做formal/public eval与规模验证；本轮未跑。

用户报告既有 `90858afae647a513537bf118932a7ba5020ee98b` 的100k已通过，保留其精确
旧source适用边界；它不自动认证本组合。未跑新100k、whole-public formal eval、excluded
old post_index runtime或包含它的suite、private42/GCWAL faults。build/lint只编译targets。
未merge到main、未deploy。正常origin fetch成功；初始push成功且唯一draft attempt已Forbidden。后续用户明确授权
正常origin push文档/拒绝回执及test-only修正，并用git ls-remote核验SHA；不重试PR/API，详见PUBLICATION。
