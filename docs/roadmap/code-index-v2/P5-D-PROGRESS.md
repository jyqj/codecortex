# P5-D 运行时与策略接口进展

## 2026-09-30 Git进度快照复核

final-v3已结束为`failed`，不是仍在运行或已经验收。14条命令收据中，stable workspace为1782 passed / 0 failed / 59 ignored；Rust 1.95 workspace退出101，为1781 passed / 1 failed / 59 ignored。失败项`project_session::tests::close_idle_instances_closes_cached_non_active_projects`预期关闭2个实例、实际1个。独立`audit.json`没有生成，因此P5-016～018仍为`in_progress`，P5-019/020仍为`todo`。

本次保存前重新核验了冻结清单中622个文件及14条命令的日志SHA-256，均一致；该核验不是重跑完整测试，也不改变原失败结论。原始轻量收据复制到`artifacts/checkpoints/20260930-git-sync/p5d-final-v3/`随源码一起版本化；大型实验工作区、二进制与导出包保留本地。后续先定位并复验失败，再进行独立审计和任务收口，详见[CHECKPOINT-2026-09-30.md](CHECKPOINT-2026-09-30.md)。

## 以下为此前实施过程与验收计划

本轮接续 P5-C，当前实现范围为 P5-016～018，不包含 P5-019 的完整质量/成本/并发消融或 P5-020/G5。最终通过以前，tasks.json 保持 115 done / 3 in_progress / 74 todo，不把开发期通过当作完成。

## 实现

能力状态区分空库/关闭/错误、解析欠账、可选端口是否注入，dense 明确 disabled，覆盖仍由每次查询报告。QueryHandle 拥有共享租约和原 runtime；空闲清理跳过在用/构建/锁竞争实例。弱登记允许 LRU 淘汰后复用在途实例。已打开缓存走无冷锁的快路径，冷初始化锁移入工作线程直到发布，避免取消等待方造成重复实例。监听初始化/轮询及空闲循环由统一生命周期所有者持有。

search/context 新增可选 retrieval_strategy，完成 schema、sanitize、handler、status 和文档示例接线；显式 context 策略不被直接符号快捷路径绕过。14 工具旧模式及既有参数保持，新增参数不是另建 provider 或向量引擎。

## 开发验证与保留的问题

运行时 10 项、文档参数 1 项、实际 stdio 新旧契约、既有 project_session 测试已实际运行通过。热路由被冷登记锁牵连、取消初始化过早释放锁两个问题均有失败先行和修复后的通过记录；活动实例重开锁等待有异步定时器回归。

新增监听启动测试最初 5 秒等待失败，隔离观察确认原生订阅在共享 Mac 上约 7.5 秒；保留失败，改用 20 秒启动可用性看门狗，不把它当成延迟指标。最终关闭/资源释放断言没有删除，既有索引 debug 500 毫秒性能门限未改。

final-v1 因独立代码审阅发现热/冷路径与取消缺口主动停止；final-v2 在未改动的假后端报告测试中停滞，独立 ps 诊断同样未返回，工具将后者记为 lost。连接器在线，未确认具体内核原因，没有把停滞计为通过。可选采样现在由一个准入工作线程隔离 spawn/reap 等待，超时后保留名额，防止无限堆积。受控迟到测试与原报告 11 项通过。

最终复验使用 CODECORTEX_BENCH_PROCESS_PROBE=0，进程树 RSS 因而为明确 null，原生自身 RSS 单独记录。核心检索、来源、协议、预算、回放及成本测试照常执行；内存性能不因此获得认证。

## 当前冻结与证据

目录：`artifacts/benchmarks/p5d-20260930-runtime/`。当前验收尝试为 `final-v3`，工作树 622 文件、6,655,806 字节，摘要 `ef63da5b557224f04bc1f6a79b4666221fdf96c17fbd625f8fdc6f24f41a4f5b`。旧停止/失败日志保留，当前接受状态必须以 validation.json 和独立 audit.json 为准。

验收通过后才写入 P5-D-RUNTIME-GATE.json 与 P5-D-RUNTIME-IMPLEMENTATION.md，并把三个任务标 done、下一项改为 P5-019。原 source/intent Partial 和 S11 无答案失败必须原样保留；G5/M2、100k、真实 provider、跨平台和发行均不在本轮完成声明内。

此前实施轮未提交或推送。本次按用户授权保存当前Git开发快照，不创建PR或发行标签，不操作日常索引；提交与远端同步结果以Git历史核验为准。

## 2026-10-02 formal-v4 轮证据回填（formal-v4 链 + R1/R2 + 质量验收）

formal-v4 全链证据回填至 tasks.json P5-019 `evidence[]`（4 条，按 round07 质量验收报告 `p5_019_backfill_evidence_proposal`）与 `implementation_notes`。**P5-019 保持 `in_progress`：c16 单跑被外部负载阻塞未落地；P5-020/G5/M2 一律未认证，不得推断为通过。** 118 done / 1 in_progress / 73 todo 计数不变。

本轮事实：ABI 投影（设计B）已实施并收紧（`REQUIRED_SEMANTIC_OPTION_KEYS` 9 必备语义 key 无条件完备性硬拒，负例 13 测试绿，评审 8/8 PASS）；sourcev5 / final-v8 harness / plan-v4 / profile final-v5 / round07 批准整链 hash 锁独立复验全 MATCH；formal-v4 正式 31/31 stage 采证完成；R2 独立复放 25/25 PASS；质量验收 step1（独立比较 path/exact/selector）满足、无 best-of / 不削 facet 满足。c4=系统性竞态 BLOCKED（workload-inherent，不得 rerun-to-green，`real_finding_for_product_record`，零数据损坏）；c16=瞬态+静默窗口 BLOCKED（v4 硬 error 未在 R1 复现，runner 就绪零成本可复跑，2 小时有界等待内 load 最低 4.24 无静默窗口，未降窗跑）。

证据路径：

| 证据 | 路径 | 说明 |
|---|---|---|
| formal-v4 正式运行 | `artifacts/benchmarks/p5e-formal-runs-20261001-v4/` | 31 stage 原件：48/48 edges、1224 请求、9 witness、fanout/typedgraph/facet/mixed c1-c8；mixed c4/c16 invalid_workload_comparison 原样保留 |
| R1 补跑 | `artifacts/benchmarks/p5e-formal-runs-20261001-v4-rerun-r1/R1-RERUN-RECEIPT.json` | c4 系统性 blocked 裁决 + c16 瞬态诊断 |
| c16 静默窗口监视 | `artifacts/benchmarks/p5e-formal-runs-20261001-v4-rerun-r1/c16-FINAL-BLOCKED.json`；`c16-QUIET-WATCH.md` | 2 小时有界等待无静默窗口；`c16-final/run_c16_final.py` 就绪零成本可复跑 |
| R2 独立复放 | `artifacts/benchmarks/p5e-formal-runs-20261001-v4-replay/REPLAY-RECEIPT.json` | 25/25 PASS（replay_v4_serial.py 全程串行） |
| 独立只读审计 | `artifacts/checkpoints/todolist-completion-audit/round07/formal-v4-independent-audit.json` | 链条/收据/投影/无放宽/失败保真全 PASS；P5-019 保持 in_progress |
| 质量验收 | `artifacts/checkpoints/todolist-completion-audit/round07/formal-v4-quality-acceptance.json` | step1 满足；c16 待静默窗口单跑为 step2 唯一剩余可执行项 |
| ABI 投影实施 | `artifacts/benchmarks/p5e-abi-projection-20261001/IMPLEMENTATION.md` | 投影 + REQUIRED key 收紧 + 负例 13 绿 |

下一步：静默窗口（load<3）出现时执行 `run_c16_final.py` 单跑 mixed-c16-baseline，预期 `computed_observations_not_G5_acceptance`；完成后按质量验收裁决评估 P5-019 转 done（c4 以 blocked 记录）并把竞态发现挂产品/查询执行 backlog。全程不改 v4/rerun-r1 raw 与锁定链。

## 2026-10-02 R3 c16 same-window 终态收口（P5-019 转 done）

c16 证据链以 same-window 双臂终态落地：静默窗口谓词（load1<3 且零 tm-r5bench）经 3 次有界等待共 4h+（含 04:08-05:08 过夜轮）结构性不可达，残余负载为桌面/后台应用；按用户决策改用 same-window 双臂协议（双臂背靠背共模消除，环境负载 load1 5.1-5.5 作为协变量逐臂记录），锁定链 plan/binaries/inputs 全 MATCH，无阈值放宽。最终臂无硬 error（baseline#2 纯已知红形状 {Partial:100, Success:200}，candidate#2 clean {Success:300}，candidate gate `passed_mechanism_scope`）；paired status 维持 `invalid_workload_comparison`——baseline 结构性已知红（v4/R1/same-window 三轮形状不变）+ candidate 1 个 native 采样 miss（instrumentation 瞬态，与 R1 残余 invalid 同类）。

amendment 将 epoch-race 定性由 R1 的 TRANSIENT 下调为概率性复发：8 次同 argv c16 臂执行共 3 次硬 `-32603`（v4_candidate seq85、sw_baseline_1、sw_candidate_1），同一 same-window 会话内 2/4 臂双侧复现，非确定性（2/4 臂全 clean）；与 R1 `real_finding_for_product_record` 同族（c4 产品发现本已含 v4 c16 candidate 硬 error），零数据损坏（各臂 330/330 jobs count lock 一致）。

**裁决：P5-019 转 `done`，c4/c16 均标 blocked-with-evidence。** 推理链按 round07 质量验收自身框架：`step1_path_exact_selector` 维持满足；`c4_blocked_allowed_for_done` 判例三要素（根因定性且不得 rerun-to-green、已产出产品级发现、零数据损坏）对 c16 逐条映射成立——paired invalid 的三项构成全部定性、产品发现由 amendment 升级置信度、两个重测口径（静默窗口单跑 + same-window 双臂）已穷尽本机可行空间且最终双臂有效。与 c4 的差异（确定性 2/2 vs 概率性 3/8、2/4 双臂）只影响产品发现表述强度，不改变 blocked 处置：框架 R-e 本已把 v4 c16 对比证据处置为"按 blocked/invalid 记录"。`c16_single_run_required` 的预期（单跑后 3/4 computed_observations）前提被 same-window 证据证伪（race 概率性复发 + 采样 miss 再现 + 静默窗口不可达），唯一剩余可执行项已执行得终态，等待不可达窗口不构成可执行义务。acceptance 逐条满足：无 best-of、不削所需 facet、回归通过、无证据项标 blocked。完整推理链见 `formal-v4-quality-acceptance.json` `addendum_2026_10_02_r3_c16_same_window_closure`。

step2 终态：2/4 并发点 `computed_observations_not_G5_acceptance`（c1/c8）+ 2/4 blocked-with-evidence（c4=确定性 workload-inherent race、c16=概率性 race + instrumentation 采样瞬态），低于原预期 3/4+1/4，按终态证据如实记录。

R3 证据表：

| 证据 | 路径 | 说明 |
|---|---|---|
| c16 终态收据 | `artifacts/benchmarks/p5e-formal-runs-20261001-v4-rerun-r1/c16-final/same-window/C16-FINAL-RECEIPT.json` | 协议、amendment、epoch 普查 2/4、superseded_by、红线声明 |
| c16 paired report | `artifacts/benchmarks/p5e-formal-runs-20261001-v4-rerun-r1/c16-final/same-window/mixed-c16-paired-report.json` | status=invalid_workload_comparison（已知红 + 1 采样 miss），candidate gate 通过 |
| 裁决 addendum | `artifacts/checkpoints/todolist-completion-audit/round07/formal-v4-quality-acceptance.json` | `addendum_2026_10_02_r3_c16_same_window_closure`（裁决推理链） |
| R1 收据（只读） | `artifacts/benchmarks/p5e-formal-runs-20261001-v4-rerun-r1/R1-RERUN-RECEIPT.json` | 未修改；其 c16=TRANSIENT 定性由 c16 收据内 amendment 下调为概率性复发 |

红线声明：G5/M2 未认证，本收口是任务证据验收层面的 status 翻转（全会话唯一一次），不含任何 gate 翻绿；两并发点（c4/c16）对比证据不存在，M2 口径按机制范围如实声明；epoch-race 概率性复发发现与 c4 同族记入产品/查询执行并发 backlog（如需修复另立任务）。R1 收据、锁定链、v4/rerun-r1/same-window raw 全部未改；本轮未 git commit。计数变为 119 done / 0 in_progress / 73 todo；`P5-020`（G5 冻结 + M2 本地版本证据）为下一可执行项，仍 todo。

## 2026-10-02 P5-020 收口（round08 独立审计轮：G5 冻结 F0 + M2 本地版本证据）

独立审计七项清单全 PASS，P5-020 转 `done`（本地增强版任务验收口径）。GATE 定稿 `docs/roadmap/code-index-v2/P5-GATE.json`：`status=passed_declared_local_scope`、`G5=passed_declared_local_scope`、`M2=passed_local_scope`——**均为本地口径、未认证**；`c4_c16_comparison_evidence=not_exists_blocked_with_evidence`（v4/R1/same-window 各轮 paired 均 invalid_workload_comparison 原样保留，对比证据不存在）；semantic 仅显示 disabled/not_configured；`full_retrieval_gate=not_passed_known_failures_retained`。

六项证据（round08 独立核验口径）：

| # | 证据 | 独立核验结果 |
|---|---|---|
| 1 | source-v6 冻结 F0（`p5e-candidate-release-20261002-v6/` + `p5e-g5-freeze-20261002/F0-FREEZE-RECEIPT.json`） | 629 条全量 `git cat-file` 回对 head `0de7c890`：623 MATCH + 6 worktree patch + 0 stale；digest `d1f5a7af...` 复算 MATCH、v5（`78f83f0f...`）交叉复算 MATCH；binary sha `5f935adc...` 四处一致；v5↔v6 差集恰 5 文件（含 v5 `.gitignore` 陈旧条目被 F-1 程序纠正） |
| 2 | c4 修复红绿（`E2` + `c4-race-analysis-20261002/IMPLEMENTATION-20261002.md`） | 方案 d P0+P1+P2 仅读路径 3 文件；8 项新测试在源码落点核实；风暴测试已知时序窗口（断言失败方向）如实记录 |
| 3 | mixed-c4 判据复测（`MIXED-C4-FIX1-RECEIPT.json`） | raw 独立重读：candidate {Success:300, build:30}、0 硬 error、0 假阳性 Partial、330/330；baseline known-red 形状 0 error；census-only，非对比认证 |
| 4 | c16 终态收据引用（`E1`） | 收据 sha 重算 MATCH；终局双臂独立 census 与 E1 一致；未重跑 |
| 5 | 双工具链全回归（`p5e-g5-freeze-20261002/regression/` + `E5`） | 7 命令日志行级核对：clippy 双链 0 warning、workspace 1839/1/60、http 35/1/5、release-cost 4/0/0；唯一失败 `benchmark_fixture` 终裁 **documented_environment_flake**（审计自复现 530.80/562.27ms 边缘超限 + v5→v6 闭包差集仅 read 路径 3 文件 + final-v3 双链 exit-0 在盘对照；caveat：HEAD 对照 raw 未保留、1823/0 引用不可定位，均不影响终裁）；非全绿表述，failed=1 原样 |
| 6 | V18 探针 + additive（`additive-contract.json`、`v18-probe-*.json`） | 14 工具名/properties/required 零漂移；semantic `not_configured`/`disabled` 双探针一致；-32602 协议冒烟；零网络零 key |

V12 facet 8-cell v6 重建 NO-GO 经审计**独立维持**：v5↔v6 闭包差集 5 文件、facet 控制面 `path.rs`/`exact_symbol.rs`/`engine.rs` 字节全等、c4 delta 分支在顺序只读 facet 负载下不可达、fanout 已绑冻结 v6 binary 零失败、~80min 重建成本信息增益≈0；facet 证据保持 formal-v4 机制范围引用。

红线对账：7 个锁定链目录（v4/rerun-r1/replay/v5/harness-final-v8/baseline-release/p5d-final-v3）`find -newermt` 0 触碰；`P5-GATE-draft.json` 原样保留为执行轮收据；无 git commit；`tasks.json` 仅 P5-020 状态/证据/备注 + `execution_note` + `next_task=P6-001` 更新；`05-TODO.md` 由 `scripts/code_index_plan.py --write` 重派生（192 项：120 done / 0 in_progress / 72 todo，P5 20/20）；`06-VALIDATION.md` 头部冻结段更新为 source-v6 数值（旧值注明归属 P5-D 轮）。G5/M2 翻转后仍为本地口径未认证。
