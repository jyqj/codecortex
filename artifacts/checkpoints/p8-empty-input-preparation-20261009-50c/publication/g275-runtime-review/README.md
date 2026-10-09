# G275 runtime / soak 原件独立验收

本包审查固定源 [275e8799d4947d297329073eaa3ca675d3fd0777](https://github.com/jyqj/codecortex/tree/275e8799d4947d297329073eaa3ca675d3fd0777) 的既有 [runtime workflow 37890757030](https://github.com/jyqj/codecortex/actions/runs/37890757030) 六份原始 artifacts。审查日期为 2026-10-09 UTC。

**结论：P8-007 的混合负载与 fake-backfill、P8-010 的一小时 soak，其本次独立原件子验收通过。两个原 TODO 仍是 `in_progress`。** P8-007 依赖 P8-006，P8-010 依赖 P8-009；它们继续受 P8-005 规模验收及后续依赖链约束。本包没有关闭原任务、改验收协议或批准 release。原任务总数 192，已完成 163，**剩余 29**。

本包只认证上述 G275 已有观测，不给 PR #181 或任何其他源码候选提供交叉认证。审查过程中没有启动新的 study、产品、索引、构建或 workload。

## 原始结果与实际观测

| 原 cell | 配置 C | 原 offered | build / read | 原记录最大 active client calls | 真 Git 切换 | catalog compaction |
|---|---:|---:|---:|---:|---:|---:|
| mixed C1 | 1 | 900 | 300 / 600 | 1 | 50 | 6 |
| mixed C4 | 4 | 900 | 300 / 600 | 4 | 50 | 6 |
| mixed C8 | 8 | 900 | 300 / 600 | 7 | 50 | 6 |
| mixed C16 | 16 | 900 | 300 / 600 | 12 | 50 | 6 |
| soak | 4 | 3,601 | 1,201 / 2,400 | 1 | 200 | 25 |

所有原 offered IDs 完整、唯一，所有终态均为 success。混合 C4/C8/C16 保留了真实 read/build overlap。soak 使用原协议的 read/build 共用写入准入锁，观察最大 active client calls 为 1。配置并发数没有被冒充为同时运行数。

### 固定源、构建与原件完整性

六份 ZIP 的实际 size/SHA-256 分别与 GitHub artifact API 和原 job 上传日志一致。首次及末次共 12 个原 seal verifier 全部 exit 0；末次复核六份 ZIP 未改变。原始大 ZIP、SQLite 和 JSONL 保留在审查工作目录，本包仅保存紧凑结果、身份与执行收据。

每份构建收据的全部 **1,089 个 Cargo/crates 输入**逐文件匹配固定 G275 Git blob 内容；**9 个 observer 输入**的字节、SHA-256、Git blob、mode 与固定源一致。before/after/final 的适用记录及原工具链身份已核对。保留的 Cargo 消息确实包含成功的 release 构建结果、选中的非 fresh 可执行产物，实际可执行文件 size/SHA 与 copy_source 及构建收据一致。当前审查机器没有被描述为原编译机器。

### 原统计与完整终点 oracle 回放

对五份 runtime 原 plan/raw，复制的原 `p8-runtime-statistics` 二进制离线回放均 exit 0，输出与各自的原 `statistics.json`、原第二次 replay **逐字节一致**。原 Python all-outcome 描述性统计也从所有原操作行复算一致，没有只挑成功子集或把 mutation 视图加成额外样本。

对五对原始 persisted DB 的副本，复制的原只读 `p8-oracle` 二进制回放均 exit 0，**15 张完整表比较均相等**；结果除绝对左右路径与本次 elapsed_ns 外与原报告一致，数据库副本字节未变化。原 RPC 记录证明 incremental 一侧仅有初始 full index 和计划内 incremental 操作，之后没有补做 catch-up index。fresh-full 一侧只做原始独立 full control。两侧终点 fixture 源文件字节和公开 symbol 返回一致。

### RPC 分母、原 wire 与进程生命周期

所有实际 request IDs 都具备唯一的 request、send、原 stdout wire 和 delivered response，顺序与字节 SHA 对齐，没有漏回包、额外错误/超时事件或 EOF 后响应；两个原产品进程均留下正常 EOF、exit 0 和完成清理的记录。

原操作行没有携带 JSON-RPC IDs。本审查逐项核对请求参数及结构响应的完整 multiset，并将资源和端点的原投影对应到实际 status 响应；相同返回值不能据此建立唯一的单请求 ID 归属，也不证明 backend queue/service 分离时间。

soak 原产品共 **14,400 次 JSON-RPC**，包括初始化、独立资源观察、工作与端点 RPC。其中 **2,400 个 offered compound read**各含 before-status、symbol、local hybrid、after-status 四次顺序 RPC；统计分母仍为 3,601 个 offered operations。独立 status 观察不计入 workload C 或 offered latency。

### 一小时、cache、资源与队列

soak 原 observed work 为 **3,600.053700026 秒**；直接由原最后操作完成时间减首个计划 offer 计算也达到 **3,600.045218965 秒**。

2,400 次 compound read 的全部四个请求、exact stable.py entity/span/source bytes、native PID、generation、前置已完成 mutation、cache counters 和 shared query pool 完成计数均按原纯验证函数重新验证。原 cache 汇总完全复算一致：**1,400 hits、1,000 misses、999 invalidations**。四个真实完成时间区段各有 600 次 read、350 hits、250 misses，invalidations 分别为 249 / 250 / 250 / 250；每段均覆盖六类 mutation。

原资源采样 **3,594 次**，最大时间空隙 **1.025965648 秒**，包括工作起止边界的覆盖条件通过。原 RSS 判据为 warmed median + 25% + 32 MiB，实际 warmed median **113,172,480 bytes**，tail median **143,433,728 bytes**，允许值 **175,020,032 bytes**，sampled peak **148,467,712 bytes**，完整复算与原报告一致。

另核对五个 cell 共 **10,193 份原保留 status** 的 query-pool 计数，其中 soak 8,395 份。原限额稳定为 CPU 4、async 8、queue 32，soak 观察到 CPU admitted/in-flight 峰值均为 1，async 为 0，rejected 为 0，端点 admitted/in-flight 均为 0。客户端原 queue capacity 为 128，所有 offered 终态成功。这里只声明保留观测和原终态的范围；没有把离散观察当作连续 occupancy 或瞬态峰值测量。

### 真实 worker 与 fake-backfill

原单个测试执行成功，三个固定 seed 7/19/43 × quiet/held 两阶段 × C1/4/8/16 四 cell × 每 cell 32 次，共 **768 条原请求**，分母和 nearest-rank 汇总均重新核对。每条 source hit 都匹配固定 Rust fixture 的实际 entity/span/text bytes，原耗时组成与整数微秒舍入一致，全部满足原 2 秒 local-query watchdog。

每个 seed 原 held-before 都记录实际 provider active/waiting 4、4 个 held inputs，以及 outbox claimed 4、pending 20、published 2、uncovered 24。保留 DB control 显示读连接可用、原 100 ms busy timeout 下 writer BEGIN/ROLLBACK 成功且 generation 不变。退休后旧 held inputs publication 为 0，新 provider 调用数为 25，最终 pending/claimed/uncovered 均为 0。

held 查询后 provider/queue 不变化及 stale symbol 删除是固定源中由原成功二进制执行的断言，由源码与原 stdout/构建收据共同支持；没有捏造独立的 held-after queue 快照或最终数据库。资源归属为 **test runner + CodeIndex + in-process fake provider** 同一进程，独立 server/tree/external RSS 的原 null 被保留。原测试二进制没有重跑。

## 使用本包

- `raw-review.json`：原件审查结果、范围、各 cell 指标与 P8-007/010 子验收结论。
- `artifact-manifest.json`：六个官方 artifact/run/job 普通 URL、ID、原 bytes/SHA、上传日志身份。
- `source-manifest.json`：固定源、1,089 输入 manifest 摘要、observer 原记录、协议/验收器源码身份、原工具链及可执行文件身份。
- `replay-receipts.json`：22 个原只读 verifier/seal/oracle/statistics 命令的实际 argv、exit、stdout/stderr 与摘要，以及 raw 审查器的实际执行收据。
- `queue-review.json`、`queue-review-command-receipt.json`：额外原保留队列观测的核对结果与实际 exit 0 收据。
- `review_integrity_and_replays.py`、`review_raw_observations.py`、`review_queue_observations.py`：实际执行过的审查脚本。`finalize_review.py` 是原件末次校验和紧凑包整理脚本。
- `SHA256SUMS`：以上交付文件的冻结摘要。

离线复核需要按 `c1/c4/c8/c16/soak/backfill` 下载各自 **同一原 ZIP**，核对 manifest SHA 后解压到各自 `extracted/`。runtime ZIP 根包含 `p8-build/` 与 `p8-runtime/`；backfill ZIP 直接包含其 receipt/raw/observer-source。

```sh
python3 -B review_raw_observations.py --evidence-root /path/to/originals --output /path/to/new-raw-review.json
python3 -B review_queue_observations.py --evidence-root /path/to/originals --output /path/to/new-queue-review.json
```

统计/终点回放的确切原二进制命令及输入路径见 `replay-receipts.json`。`review_integrity_and_replays.py` 封存的是本次实际脚本，其 `REPO` 保留原审查 checkout 的绝对路径；另一工作区需把该只读定位值指向含 G275 Git 对象的 checkout，并保持所有验证逻辑和原件不变。脚本默认新建结果，防止覆盖原证据。所有 Git 命令仅用于读对象，oracle 使用原数据库副本。

原 `verify` 子命令主要验证 seals；它本身不会把一个已封存的 failed observation 变成通过。本包额外核对了原 report 终态、完整 raw、实际 wire、原统计及完整 oracle，才给出上述限定范围结论。原统计仍是描述性/IID 区间范围，不是已认证稳定尾延迟；没有新增 SLA、费用或 live-provider 认证要求。

本包不包含临时 signed URL、file URL、token 或签名 query，也不复制庞大的原 ZIP/DB/JSONL。
