# P5-020「P5本地增强版验收」执行进度

- 执行日期：2026-10-02。执行依据：`artifacts/checkpoints/p5-020-playbook-20261002/PLAYBOOK.md`（预写手册）。
- 裁决（手册 §2.0）：**采纳「c4 修复纳入 source-v6」推荐路径**。理由：修复迟早要做而 F0 只能有一个；修复落冻结前 = F0 一次成型（手册利弊表关键不对称）。c4 修复已于本轮冻结前实施完毕（`artifacts/checkpoints/c4-race-analysis-20261002/IMPLEMENTATION-20261002.md`，方案 d P0+P1+P2，TDD 红绿）。
- 性质：本轮为证据生产轮。**不定稿**：GATE 以 `docs/roadmap/code-index-v2/P5-GATE-draft.json` 落盘（draft），定稿与 tasks.json 状态收口由独立审计轮统一做（任务红线）。

## 六项证据逐项状态

| # | 证据 | 状态 | 收据 |
|---|---|---|---|
| ① | c16 单跑/same-window 终态收据 | 已核验并引用（未重跑） | `E1-c16-receipt-verification.json`；原件 `artifacts/benchmarks/p5e-formal-runs-20261001-v4-rerun-r1/c16-final/same-window/C16-FINAL-RECEIPT.json` |
| ② | c4 修复红绿测试收据 | 完成（红绿 + 定向复验绿） | `E2-c4-redgreen-RECEIPT.json`；实施记录 `artifacts/checkpoints/c4-race-analysis-20261002/IMPLEMENTATION-20261002.md` |
| ③ | mixed-c4 判据复测 | **判据 PASS**（candidate 0 硬 error / 0 Partial） | `artifacts/benchmarks/p5e-formal-runs-20261002-fix1/MIXED-C4-FIX1-RECEIPT.json` |
| ④ | source-v6 冻结闭包 + F-1 断言 + F0 | 完成（629 文件，0 stale，F0 登记） | `artifacts/benchmarks/p5e-g5-freeze-20261002/F0-FREEZE-RECEIPT.json` + `p5e-candidate-release-20261002-v6/source-manifest.json` |
| ⑤ | 冻结源双工具链全回归 | 完成（唯一失败 = benchmark_fixture 环境 flake，HEAD 对照实验证明与本轮 delta 无因果） | `artifacts/benchmarks/p5e-g5-freeze-20261002/regression/validation.json` + `E5-regression-disposition.json` |
| ⑥ | V18 契约 + semantic-disabled 探针 + additive-contract | 完成（14 工具零漂移，semantic 仅 disabled） | `v18-probe-frozen-v6.json`、`v18-probe-baseline.json`、`additive-contract.json` |

## 关键数字（快照）

- source-v6：629 文件 / 6,815,216 字节 / `source_digest_sha256=d1f5a7af5ff9b0d025b6dd8dc475c110f178735d757d22e3546ba8470b45a167`；head=`0de7c890fcb2a152b4b21eafc4d8ad1a2c3885a6`；F-1 断言 623 条对 head MATCH、6 条声明 worktree delta、0 stale。
- 冻结 binary：`5f935adc5264b707a61e5ce6d02f1cc0efb9c4a24297db319e8db91953a7ecc9`（release，由冻结拷贝 `--manifest-path` 构建）。
- mixed-c4 复测：candidate_fixed_v6 `{Success:300, build:30}` 330/330 jobs、failures=0；baseline_prefix 同窗对照 `{Partial:100, Success:200}`（known-red 形状，0 error）；环境 loadavg ~7.8–8.9 如实记录（判据为 error/Partial 计数，非延迟认证）。
- V12 fanout（绑冻结 v6 binary）：candidate `passed_independent_typed_graph_fixture`、failed_checks=0、40 queries；paired 诊断因 baseline 臂 1/401 sampler 瞬态（`native_snapshot_unavailable`，与 c16 收据记载的 R1 instrumentation transient 同类）记 `invalid_measurement`，原样保留；r1 尝试（漏 probe env）raw 保留，r2 修正重跑。
- 全回归（冻结 rev2 源，7 命令）：clippy 双链 0 warning；workspace 1839/1/60（stable 495s、1.95.0 558s）、http 35/1/5、release-cost 4/0/0（127.5s）。唯一失败 = `cc-eval lib::tests::benchmark_fixture`（index warm p95 549–665ms vs 500ms 阈值，环境 load 7.6–12）；**因果对照实验：detached-HEAD 0de7c890（无本轮 delta）同测试先失败后通过 → 环境敏感 flake，非本轮回归**；阈值未动，失败日志原样保留。
- V18：tools/list 14 工具名与 baseline release binary 完全一致、schema 零 drift（`changes={}`）；semantic `semantic_state=not_configured`、`dense_state=disabled`（reason=`provider_and_vector_publication_not_implemented`），无 ready 假象；协议冒烟握手 OK / 未知工具 -32602 / 有效 search `is_error=false`。

## 偏离手册之处（如实记录）

1. **手册 §2.1 步 3 预期 mixed-c4 状态 `computed_observations_not_G5_acceptance` 类**；同窗 c16 paired report 实际 status=`invalid_workload_comparison`（compare_mixed 对跨代源对比的判定），c16 同理。两处均原样记录、未干预；判据按任务口径取 error/Partial 计数。
2. **混合负载在非静默窗口执行**（loadavg 5–12，含回归期 7.6–12）：沿用 c16 same-window 协议先例（静默谓词在本机结构性不可达，3 次有界等待记录在案）；mixed-c4 双臂同窗 + 协变量落盘；判据与延迟解耦。
3. **冻结发生 rev1→rev2**：rev1（digest `b4dbead7...`）在全量回归首次 clippy 暴露 `engine_cache.rs:529 unused_mut` 后被替代；修复为 cfg(test) 内删一个 `mut`（语义零变化，release 字节经 hash 验证不变）；rev2 重新拷贝/manifest/断言，**任何下游证据未消费过 rev1 digest**。首次回归批（stable-clippy rc=101 + 中断的 stable-workspace）日志已删除并以冻结 rev2 源重跑，避免日志与冻结源不一致。
4. **风暴测试 flake 一次**：lint 修复后默认并行模式下一次 `fence_backoff_rides_out_a_back_to_back_commit_storm` 断言失败（已知时序敏感窗口，失败方向=断言失败非假绿）；`--test-threads=1`（回归口径）3/3 绿、实施轮 6/6。如实记入 E2。
5. **benchmark_fixture 环境 flake（回归唯一失败）**：双链 workspace/http 各计 1 失败均为该 perf 阈值测试；HEAD 对照实验（无 delta 源同测试失败+通过交替）证明与本轮改动无因果；处置收据 `E5-regression-disposition.json`，终裁留给独立审计轮。
6. **fanout r1 尝试被替代**：runner 漏设 `CODECORTEX_BENCH_PROCESS_PROBE=1` 致 baseline 臂资源证明缺失；r1 raw 保留（不覆盖），r2 修正 env 重跑；r2 paired 诊断仍因 baseline 臂 1/401 sampler 瞬态记 `invalid_measurement`（原样保留；V12 判据绑 candidate 零失败，不受影响）。
7. **登记位置 2/3 部分顺延**：`06-VALIDATION.md` 头部冻结段更新与 tasks.json P5-020 evidence 回填按红线留待收口轮；F0 已在 manifest 与本轮收据登记。

## 红线对账

- v4/rerun-r1 raw、R1/R2 收据、v5 manifest 与 `78f83f0f...` 锁定链：零触碰（本轮仅引用）。
- 无 git commit；tasks.json 未改（status 维持 todo，evidence 回填留收口轮）。
- 未改阈值/gold/known-red；原 Partial/S11 失败未删改。
- semantic 仅 disabled 显示，未冒充可用。
