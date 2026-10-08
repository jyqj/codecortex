# P8 原始任务独立验收矩阵

审查基线：`5610335b31f0f866cfa17712c8a55b9b22dac4cf`。权威输入是该版本的 `docs/roadmap/code-index-v2/tasks.json`、`06-VALIDATION.md`、`09-BENCHMARK.md` 和当前驱动源码；旧 checkpoint 的限制不自动等于当前候选仍有相同限制。

基线状态：192 项中 163 done、16 in_progress、12 todo、1 blocked，剩余 29。目标 P8-005～P8-013 / P8-016 十项均 in_progress；P7-020 与 P8-001～004 当前已 done。本审查未操作任务状态、GitHub、真实产品数据、主矩阵或旧 raw。#158/#159 活跃会话负责主矩阵；这里未获取其最终 raw，因此下表不是它们失败或通过的判定。

## 一、正式关闭证据矩阵

共同要求：精确候选 commit、完整输入 manifest、实际 Cargo event/构建日志、binary 摘要、已执行命令/退出码、保留全部失败与未执行状态、原测试回归、独立评审及可执行回滚。不能仅凭 workflow 绿色或 summary 的 passed 字段关闭任务；也不能把一次限定范围 smoke、deferred、驱动已实现、不同源版本的结果拼成完整验收。

| 原任务 | 原标准要求的证据 | 当前可执行入口与范围 | 正式关闭前的独立核对 |
| --- | --- | --- | --- |
| P8-005 完整规模 1k–100k（V20；前置 P8-004） | 同 seed、release 构建的 1k/5k/10k/50k/100k 实际样本；真实 files/symbols/chunks/各类 edges/vector 工作量；不能只报文件数、拿不同工作量直接算倍数。 | `p8_scale_matrix.py`、`p8-scale.yml`；原计划 30 repetitions，五种规模与批次、fanout 分片；完整矩阵共 50 组、1500 个计数测量。规模数据完整 raw 回放；vector 明确 disabled/null，仅是默认本地 profile。 | 必須有全部分片与原始大表 parity、源/二进制一致、各规模实际计数；`complete_measurement_coverage` 只认证覆盖。`scale_capacity_v1` 按 host/CPU/kernel 分层，不能把各主机 N=1 池化成同环境 N=30 的性能/尾延迟证据。所选语义发布范围若需要真实向量规模，disabled/null 无法证明该范围。真实仓库结果与合成规模分别归档。 |
| P8-006 增量规模与 fanout（V07/V20；前置 P7-020/P8-001/P8-005） | no-op/body/API/config/batch、超预算闭包；phase 计数与时间可归因；闭包完成后 full parity；未完成显式 status。 | 同一 scale 驱动；batch 1/10/100/1000；fanout 1/4/16/64/128；完整 15 表投影、重复行与独立 config/call-edge 事实；至少一个真正 first-build incomplete 后恢复。 | 全部阶段/全局 repetition 唯一且无遗漏；no-op 零文件增删改；中间 incomplete、最终 complete、独立 full control 全部正确；检查原完整计时而非只看较短的旧 elapsed。与 P8-005 相同候选，完整前置关闭后才能关闭本项。 |
| P8-007 并发混合负载（V11/V20；前置 P8-006） | C1/4/8/16 的 read/build/backfill；offered load、排队、所有 timeout/error/cancel/rejection 分母；无死锁、饥饿；吞吐不能隐藏尾部。 | `p8_runtime.py` + `p8-runtime.yml` 的真实 stdio read/build；`p8_backfill.py` 的原生产 worker + 三个固定 seed 的假 provider read/write/backfill。两种执行范围应分别标明。 | 四个 C 都有实际重叠、排空与完整终态；offered ID、开始/结束、超时和缺失不能只剩成功子集；回放原 read/build 和 backfill 类别统计，不能跨 C 或重叠视图凑 N。确认两套执行在同一候选上覆盖原要求；假 provider 是工程并发证据，不是真实模型质量或账单。 |
| P8-008 冷建/重开/热查（V20；前置 P8-007） | OS cache、process cold、result-cache hit、uncached warm 分开；无 best-of；全部样本、N、分布、CI。 | `p8_lifecycle.py` + `p8-lifecycle.yml`；30 个独立空 index 冷建、400 reopen、400 warm miss、400 cache hit；沿用 Rust `p8-measurements` 统计/normalizer/source verifier。 | 原进程启动/已有 DB/no-index RPC/持久 generation 与前后 cache counter 能直接证明分层；成功响应匹配 authored source；所有尝试不删。OS cache 未清理必须保持 unknown_not_cold_disk。原分位区间、无界端点/IID 限制完整，不能以达到最低 N 自动声明稳定 p99。 |
| P8-009 内存/磁盘/费用总账（V20；前置 P8-008） | 分 client/server/process-tree；artifact/FTS 占用；reported/estimated 分列；单位和归属正确；unavailable 不填 0、runner 不冒充 server。 | lifecycle 的实际 native SELF/status、`p8_resources.OwnedProcessProbe`、独立 inode/SQLite dbstat 分区账目与共享 Rust ledger。默认语义禁用时费用 amount/tokens 保持 null。 | PID/namespace/owner/source 可追踪；瞬时 RSS、高水位与采样最大值分列；不可访问树明确 unavailable，不能从 server SELF 推导全树峰值；FTS 是同一物理 DB 的逻辑子项，不重复累加。冻结的最终存储清单需包含实际 WAL/SHM；各币种及 reported/estimated 分栏，缺账单不推造费用。 |
| P8-010 长时 soak 与连续修改（V07/V17/V20；前置 P8-009） | 持续编辑、删除、切分支、catalog 压实、cache/worker 复用；终点 full parity；内存/队列不无界增长。 | `p8_runtime.py --profile soak`；现有注册计划至少一小时与实际时长一小时、3601 次请求、1000 ms 间隔、1000 文件；同一个产品进程；原 RSS 趋势阈值、5 秒最大观察间隔。 | 必须有全时段样本和真实 Git switch、catalog compaction 日志；不能只改计划时间或采短时密集样本。停止投递后排空，增量侧不补建/修复，复制源码的一侧 fresh full，再做完整 15 表和公开查询对账。保留 original RSS/队列阈值，不以缩短时间/提高容限换通过。 |
| P8-011 端到端故障恢复（V14/V17/V18；前置 P7-020/P8-007/P8-010） | kill/网络断开/cache 损坏/DB busy/换库；raw 故障工件；无假 ready、删除复活；恢复与费用影响明确。 | `p8_recovery.py --full-matrix`、platform workflow recovery job；3 个本地 stdio 故障、223/227/229 三 seed 的真实 loopback HTTP 语义产品故障；原 SIGKILL、交换窗口、cache tamper/space/namespace 测试。 | 看全部故障实际触发和恢复后的事实，非只看最终 ready；统计 backfill/retry/coverage/费用不确定性；当前候选测试 executable 与 logs 有完整身份。旧 limited local recovery 的 3 案例不能替代主动断网、active cache 与换库。mock loopback 不冒充 paid provider，付费数据范围由独立真实语义门处理。 |
| P8-012 MSRV/平台冷构建（V01/V21；前置 P8-011） | Linux/macOS × 声明 MSRV/stable × 默认/semantic 共八格；全新 target；SDK blocker 有解决证据或发布范围明确；旧测试/stdio 与忽略项明列。 | `p8_cold_build.py`、`p8-platform.yml`；源字节、toolchain/配置、profile/feature、实际 Cargo artifact、两份日志、新 target 与 stdio 五请求；collector 合成八格。 | 八格缺一不可计 passed；实际 MSRV release、compiler host、package features、完整日志、source/binary、clean EOF/查询事实需要重新核验。**本轮修复了导出 collector 比 local validator 弱的问题及失败无回执问题；42 项合成契约测试只证明修复，不能替代八格真实构建和 V01 回归。** |
| P8-013 指标/门槛与失败退出（V03/V04/V20；前置 P8-012） | 故意 quality/perf/lock 失败；红线非零；raw 保留；inconclusive 不自动 passed；原 scorer/report 功能回归。 | `p8_gate_controls.py` + `p8-gates.yml`；六个原控制（两库级、四实际 CLI）；0计划、不可测 latency、quality/perf、inconclusive、lock、坏 policy/拒绝覆盖；生产测试收据验证恰好一个未忽略测试。 | 所有故障必须有当前 candidate 编译产物、执行结果和 retained fixtures；失败 JSON 与 raw 可重放，原 bytes 不覆盖；V03 scorer goldens 和原 report/transport 回归另行核验。合成数字证明非零失败路径，不是产品质量/性能改进。 |
| P8-016 数据库/配置/包回滚（V13/V17/V21；前置 P7-020/P8-012/P8-013） | 真实旧 binary 开新 schema 的受控重建、cache 版本隔离、disable 语义回退；源码不丢、新向量不误读、恢复步骤实测。 | `p8_rollback.py --version-pair` 的实际 schema 24→25 源版本对和 backup restore；default/semantic 两个实际产品的有限 drill；主动 cache reader 拒绝未知格式由 recovery suite 补足。 | 区分真实旧新 binary 对与人工 PRAGMA 故障注入；后一种本身不够。看源文件/配置与 SQLite WAL 一致备份、恢复前后 schema/完整性/公开源查询、active cache reader；禁用语义下目录未改不是主动 reader 认证。源码版本对不能被写成已发布 tag/package 对。**本轮另实证 cold manifest=list 与 rollback manifest=dict 的契约不兼容，root 正单独修复。** |

## 二、已实证的可修复缺陷

原始复现脚本：`reproduce_acceptance_gaps.py`；冻结修复前结果：`negative-control-results.json`。全部使用仓库已有的明确合成测试夹具，在外部临时目录创建合成 Cargo/RPC 回执；没有真实产品性能或平台认证含义。

1. `p8_cold_build.collect_cells` 在原版本漏查 local validator 已检查的 profile、Cargo path、compiler environment、完整 Cargo logs 绑定。分别改坏一个字段并同步回执/外层 SHA，local validator 全拒绝，collector 仍给八格 passed。修复将记录契约抽为 `validate_build_contract`，两路径共用；集成额外检验 target ownership、target host、command feature、configuration drift、artifact profile/manifest/executable ownership。
2. 原 collector 忽略导出的原始 `matrix.json` 选择。修复共用八格 `selected_inventory`，要求未选七格明确 not_run，所选 source/receipt hash 与导出的实际回执一致。
3. 原 `--collect-cells` 空集合退出 2，但已创建的输出目录为空。CI 后续 upload 会找不到失败回执。修复始终写 `matrix.json`，标 invalid/cancelled 与非零退出、输入目录、expected commit、错误；不冒算已经通过的格数，不覆盖现存输出或改动 raw。
4. `p8_rollback.product_schema_version` 原先对 source manifest 直接 `.get(relative)`；标准 cold receipt 的 manifest 是对象列表，虽经 `binary_identity` 正常接受，随后报 `AttributeError: 'list' object has no attribute 'get'`。root 拥有该修复；本分支只按其新契约补真实 hash/count 到原测试夹具。

## 三、本轮代码范围与验证

只修改 `scripts/p8_cold_build.py`、`scripts/tests/test_p8_platform.py`，不提交、不触碰 task 状态、guard pins、workflow、生产 Rust/测试、旧证据。原第一阶段仅独审，第二阶段依据 root 明确委派实施上述修复。

命令：`PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s scripts/tests -p test_p8_platform.py -v`。42 项通过，包含 11 个重签后的契约负控、3 个原选择负控、删除原 CI target/编译器后的便携正控、空/坏 JSON 汇总失败收据及原 raw/旧输出保留。`git diff --check` 通过。

固定 diff：`platform-collector-fix.patch`，SHA256 `5d84868d7411c8f44f5ea35026b6394b421392170edb40d4a5a2f2fabaad6088`。

冻结文件摘要：

- `scripts/p8_cold_build.py`：`867c024d6b1bd85750d054c8802a687d5cac38c7d6752b2f11e19be19ff7435b`
- `scripts/tests/test_p8_platform.py`：`9ecf71482d1c7f0ff3c2957047ea9e1fb1a82edc941864d6ecfdcdfda1819ee6`

边界：portable collector 核验的是保留的源字节清单、构建元数据、日志、实际打包 binary 与 stdio；原 runner 上 compiler/SDK 外部文件的 live byte 验证仍在 local/export 发生，不能因为文件在另一台 CI 机器已不存在就要求 collection 重新访问它，也不声称该构建是 hermetic。
