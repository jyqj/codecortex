# P8 真实 stdio 负载与长时观测

本驱动执行真实 `codecortex mcp` 子进程，测量默认包的本地索引和符号查询。
单份通过的观测不改变任务状态，也不授予发行批准；原始任务的依赖、跨平台、规模、质量和其他验收仍分别核对。

## 负载和分母

`scripts/p8_runtime.py` 的 mixed 模式分别设置 C=1/4/8/16。C 是同时处理工作请求的客户端线程上限；写操作还经过同一写锁，因此实际并行数另记。每组 C 个请求按固定时间同时投递，组间距离为 C 倍配置间隔。所有投递、排队拒绝、错误和取消均保留唯一序号、计划时间、实际投递、开始和完成时间。C>1 必须实际观察到 read/build 请求交叠才可通过。

约三分之一请求修改源码并调用增量 index，另三分之二执行真实符号查询。六种修改按实际写入次序循环：固定大小的符号替换、增加文件、改名、删除、切换真实 Git 分支、恢复 API。Git 操作只允许在本驱动新建、带精确 marker 的夹具中执行，hooks 被禁用。每条成功查询必须返回 `stable.py` 中精确名称的符号。

每秒的资源观察会额外发送一条 status RPC；它不占工作请求的 C，也不混入工作延迟分布。观测报告明确说明这个额外并发来源。默认包不执行语义 backfill；独立 `p8_backfill.py` 使用原有真实 CodeIndex worker、受控假 provider、三个固定种子和 C=1/4/8/16 验证持有期间的读、写、恢复和排空，不调用付费 provider。

## 一小时 soak

soak 使用均匀投递。工作计划和实际工作时长都必须至少一小时。正常配置为 3601 次请求、1000 ms 间隔、1000 个源文件，整个期间保持同一个产品进程。停止投递后等待原工作排空，再读取终点状态。

RSS 来自产品对自身进程的原生测量，不使用无法证明属于该子进程的 PID。所有样本必须为正数；预热后的第二个四分位区间中位数与最后四分位中位数按现有规则比较：允许增长为前者的 25% 加 32 MiB。另要求样本覆盖完整工作时段，包含开始和结束的最大观测间隔不超过 5 秒。缺失、零填充、聚集在短时间内的样本都不能认证长时内存趋势。真实分支切换和 resolver catalog 压实日志也必须出现。

## 终点一致性和退出

最后一次增量完成后，复制当前源码及 Git 状态到独立目录，仅对复制目录执行 fresh full build。增量侧不做补建或修复。`p8-oracle` 直接调用现有只读、完整 15 表 oracle，保留原投影、重复行、完整性、外键和资源上限。两侧还执行公开符号查询。

成功观测返回 0；实际门失败返回 1；驱动、收据或完整性错误返回 2。已写出的原始记录不会覆盖。排空超时会停止后续修改、取消未开始工作、记录取消终态并终止本驱动拥有的产品进程；不会通过 executor 的隐式退出再次执行整个等待队列。

## 构建和证据

`p8_runtime_build.py` 在同一次明确的 locked/offline release 构建中保存 `codecortex`、`p8-oracle` 和 `p8-runtime-statistics`，保留实际 Cargo JSON、stderr、工具链、完整 crate/Cargo 输入、7 个 Python observer 模块的 Git blob 和 SHA256，以及源 executable 到保留复制件的路径、长度和摘要。旧 `binary_*`、`oracle_*` 和对应 artifact 字段保留；新版收据为 schema 2。

执行入口必须核对每个原 Cargo event 的完整 manifest/src 路径、默认 feature/release profile、原 target 内 executable、实际复制来源与保留字节。开始和结束都检查当前 commit、源码、observer 和三份原 executable/复制件。该合同要求同一 job 的原 target 仍在；下载产物之后可以核对封存完整性，但缺失原 target 时不能冒充新的当前构建认证。

构建目录和观测目录分别生成 `seal.json`，逐个记录其余全部常规文件的 SHA256 和长度，拒绝符号链接及特殊文件。观测 seal 包含 report 本身、plan、所有原 RPC/stderr/process 日志、资源、真实 Git/源码/SQLite（含当时存在的 WAL/SHM）、完整 parity、两次统计重放及其命令/退出记录。它在所有拥有的产品进程和重放进程结束后生成。观测目录还保留原构建日志、收据、observer 源码和原构建 seal 的精确副本；三份可执行文件在配套构建目录中，观测 plan 固定该目录的 seal 摘要。

`p8-runtime.yml` 在独立 runner 上先构建再观察，执行来源/篡改负控，并在失败时同样上传原件。单独的 verify 步骤核对配套两份封存；检查通过只表示文件仍与封存一致，失败观测的原状态不会变成通过。

```sh
python3 scripts/p8_runtime_build.py --output /absolute/new-build
python3 scripts/p8_runtime.py \
  --binary /absolute/new-build/codecortex \
  --oracle /absolute/new-build/p8-oracle \
  --statistics /absolute/new-build/p8-runtime-statistics \
  --build-receipt /absolute/new-build/build-receipt.json \
  --profile soak --concurrency 4 --operations 3601 --files 1000 --interval-ms 1000 \
  --output /absolute/new-soak
python3 scripts/p8_runtime.py verify \
  --output /absolute/new-soak --build-output /absolute/new-build
```

`--statistics` 省略时，只从这份严格构建收据中选择统计二进制。结束后，驱动将未筛选的原 `plan.json` 和 `raw.jsonl` 交给 `p8-runtime-statistics` 两次；两份报告必须逐字相同。重放器验证完整、唯一的投递 ID、原读/写分类、终态、时间顺序、实际成功响应和 mutation 次序，再直接调用现有 `benchmark::statistics::distribution` 与 `quantile_interval`。每个 profile/C 分别报告 read/build 主类别和六种实际 mutation 的附加视图；主类和细分视图重叠，不能相加、跨 C 合并或凑样本下限。

原 mixed 默认每 C 的 600 次 read、300 次 build 全部保留。失败、取消和队列拒绝留在全部投递分母，同时展示成功子集、缺失阶段时间及各自分布/95% 分位区间。区间使用原有 IID 假设，无法确定的端点保持 null；样本少的 mutation 层保留原统计状态，不增加整个 mixed 失败条件。全部 raw 仍以 ns 保存，新重放按现有统计接口使用整数 μs。保留原 `latency`、`latency_by_operation` 字段，并新增独立 `statistics` 工件引用。

冷构建、全新进程重开、未命中和缓存命中的分层统计由独立 lifecycle 流程负责。这里的 mixed 分位数和 CI 是描述性观测；并发下的缓存状态未隔离、build 主类包含不同修改、时间序列可能相关，这些边界不会因 N 足够而自动消失。原 RSS、等待、完整 parity 和终态门保持原值，统计重放不引入质量评分、稳定尾延迟声明或新性能门槛。

## 原始失败门

`p8_gate_controls.py` 通过共同的生产测试收据函数执行原有六项门控测试，其中两项是库级控制，四项实际调用 CLI。显式证据目录让测试退出后仍保留质量失败、延迟失败、样本不足、raw 锁漂移、损坏策略及拒绝覆盖的原始夹具。数值仍被标明为合成控制，只证明失败路径、非零退出和工件保留，不作为产品性能数据。
