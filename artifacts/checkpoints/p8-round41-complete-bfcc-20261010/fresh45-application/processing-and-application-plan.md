# New45 原结果到 P8-006 的处理预案

本文件仅准备报告和证据映射，`apply_now=false`。没有运行原聚合器、reader、validator、Rust、工作负载或任务生成器。当前不产生任务完成信用。

## 复用已有处理，不再增加验收器

固定 G2 `f97c5068d056969705e8387ba1f37d83847f5bee` 的 `scripts/p8_task_profile_matrix.py` 已提供需要的结果：`validate_build`（157 行）、`validate_shard`（270 行）、`combine`（330 行）、`aggregate`（407 行）。原 workflow 的 aggregate job（633–678 行）调用这些函数，并保存 `task-matrix.json`；不能用新的摘要替代它。

1. 先接收实际本研究 build、`task-registry.json`、45 个完整 cell 原件和原 aggregate；保留原失败、退出与 custody。复用 execution_recon 的 `round41-task-profile-original-intake-preparation/pending-original-intake-contract.json`。本预案不授权或增加一次重复 aggregate。
2. 原 aggregate 必须按其原谓词得到完整 45/85。若它失败、缺槽、缺完整原件或身份不同，直接报告原 `input_results`、`input_errors`、`missing_slots`，不筛掉慢点或改为部分通过。
3. 按相邻 JSON 模板的字段选择直接生成描述性报告。五个 cold 点和各 profile 点已经在原结果内，无需重新扫描 raw、统计重采样或制造另一套接受规则。
4. 将完整原件/正式原聚合、来源兼容性及实际相关 CI、原 V07 适用性与原依赖状态交给独立 P8-006 任务评审。`passed=true` 的 matrix 不是任务状态决定。
5. 只有实际任务决定成立后，才按原 `b4af966dd0c82e48f10d26ca314b4c5a65bc7868` 预案填真实 evidence/date/implementation_notes；保留旧模板所有 null 和历史，不就地回写。Root 掌最终应用及发布。

## 五档规模如何对应原任务

`tasks.json` 的 P8-006 标题为“增量规模与fanout曲线”；steps 是测 no-op/body/API/config/batch、超预算闭包及输出各 phase 计数；acceptance 要求时间可归因、完成闭包后 full parity、未完成显式 status，以及相关旧回归。任务 steps 本身没有枚举五个数字；其原 evidence 明确保留“完整1k到100k”缺口，所引用 `09-BENCHMARK.md` D4（154 行）列 1k/5k/10k/50k/100k，§8（163–167 行）要求 pristine A/B 与完整对照，§9（171 行）分开冷、增量、config 和四种 batch。

新协议在 **每个** 1,000/5,000/10,000/50,000/100,000 档运行八种 profile：no_op、body、api、config、batch_1、batch_10、batch_100、batch_1000。每个 cell 独立 fresh A/B，保留 setup 和所选 mutation，共 40 cells/80 records。五个独立 fanout 1/4/16/64/128 再给 5 records，合计 45 cells/85 records。fanout 输入是 N+1 个文件；其 plan 的 `files:[1000]` 只是容量字段，不能写成五个 1k 测量。

五档 cold 曲线预先选择每档 **no_op cell 的 setup**。它们已经包含在 40 setup 内，不能再次加成 90 records，也不能从每档八个 setup 选最快者。其他 35 setup 完整保留。每档源文件数量另有一个辅助配置文件；报告沿用原 requested/written/auxiliary 口径。

该设计对应原 006 的描述性规模、操作、归因及 fanout scope；不是 09 的全部操作/故障矩阵、真实仓库、查询分布或发布认证的自动全覆盖。其他原范围沿已接受的 V07/旧回归证据及其真实执行来源引用。它不声称连续历史等价，不重标旧 N30/150/1350 研究。

## 报告表及单位

| 表 | 原结果位置 | 行数与含义 |
|---|---|---|
| 五档 cold | `cold_curve_from_no_op` | 5；先按 `slot[1]` 数值排序；N1/点 |
| 所有 setup | `setup_pairs` | 40；五 cold 的超集，资源是 snapshots |
| mutation | `mutation_samples` | 40；八 profile × 五档，每格 N1 |
| fanout | `fanout_samples` | 5；N+1 实际文件、first_build_incomplete、builds |
| 环境 | `environments[environment_id]` | 原 host/CPU/config/临时路径逐项保留；不折叠 ID |
| 原件追踪 | `cells`、`evidence`、`input_results` | 45 槽与原 receipt/hash/custody 对应 |

完整记录身份使用 `(source, run, attempt, slot, record_role)`；不能仅以 `sample` 字符串去重，因为各 fresh profile 的 `scale-X/repetition-0` 会相同。record_role 为 setup、mutation 或 fanout；cold 图只是 setup 的展示视图。

保留原整数 `engine_ms`、`full_control_ms`、六个 `phases.*_ms`、`index_wall_us`、`outer_incremental_wall_us`、`parity_wall_us`、`whole_fixture_wall_us` 和 `build_timing`/`full_control_build_timing`。展示秒只分别除以 1,000 或 1,000,000，不覆盖原值。没有的字段写 unavailable/null，不补 0。fanout 不一定具有单独 parity_wall_us，不能制造该计时。

`phases` 是时间，不是假造的工作计数。需要 files/dirty/project-model/document-change 等 phase 工作量时，引用各 cell 原 `raw.jsonl` 中相应 `build_finished.report`；原 validator 已检查它们，aggregate 的 compact measurement 没有保留所有字段。只在报告确实需要时从这些已接收原行提取，保留 label、实际源和原字节定位，不重跑原函数。

不把嵌套 elapsed/build/phase/full-control/parity 相加成互斥总耗时；也不计算 pooled p50/p95/p99、CI、speedup 或跨 host 因果曲线。`full_staging_us=null` 与 vectors disabled/count null 原样保留。资源 snapshots 不能称 peak/process-tree 峰值。完整 typed parity 不要求两侧 digest 字符串必然相同，保留原 signed-zero 语义。

## 85 身份、失配与预算的原责任边界

原 `combine` 负责 exact slots、单一 source/binary/driver/build/registry/run/attempt、45/85 和实际 over-budget 闭包覆盖。原 `_validate_shard`/`_inspect_raw` 负责完整 EOF、真实 native/worker exit、原 plan、512 MiB、五小时 native 执行状态、15 表/重复行/完整闭包/原 independent facts、dirty200/resume1024 与 fanout8/128。报告只引用实际原谓词及其完整输出，不从 Actions 绿色、文件存在或 capture prefix 推定通过。

若发现原 validator 未覆盖的新事实，应把事实交 root 判断；本模板不扩大原 gate。传输 CRC/身份与原测量通过是两件事，各自保留真实收据；不再验证同一原件来制造新信用。

## 未来应用与当前缺项

当前已知实际 P/R/G/树及固定 driver；run、attempt、event、实际 registry、build/ELF、45 原片、85 records、正式 aggregate、该源实际 Rust/CI、完整 custody 与正式 006 决定均未在本次准备中取得。它们在模板中仍为 null，accepted 为 0，仅表示本准备没有接收新结果。未来必须以实际完整来源填入，不能挪用旧失败研究或诊断。

九项顺序沿原预案：006、007、008、009、010、011、012、013、016。后八项的原 own-scope/source/deps 不重写、不全部迁往新源重跑。应用只允许原 progress 字段 status/evidence/implementation_notes 和生成器导航；definitions、deps、acceptance、subgates 与旧 evidence 不变。

正式候选才执行原 `python3 scripts/code_index_plan.py --write`，然后 `python3 scripts/code_index_plan.py`；本次两者均未运行。只涉及 tasks.json 和原四个派生视图：05-TODO.md、根 README.md、roadmap README.md、08-HANDOFF.md。新鲜主线复核与其他183任务/全部定义不变按原预案执行。

全部九项真实合入后才可能从164→173 done、28→19 remaining、session1→10。017–019仍沿自己的原依赖和已归档 own-scope 另行决定；此报告不自动关闭它们，也不把 P8-020/full G8 发布门写成 passed。
