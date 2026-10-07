# P8 本地测量分层与资源账目

本文件记录 P8-008、P8-009 的本地工程进展。统一实现位于现有
`cc-eval::benchmark::{statistics,sampler,report}`，原 `Row`、suite、adapter
版本和评分公式保持兼容。这里没有完成 P7 前置、100k/release profile、
真实 provider 费用认证或 G8，不把两项原始 TODO 标成全部完成。

## 现有 runner 的实际输出

`cc-eval run` 与 `cc-eval replay` 会根据已有原始行和资源快照增加两个文件：

| 工件 | 真实输入 | 输出含义 |
|---|---|---|
| `latency-strata.json` | `normalized.jsonl` 中每一行的 status、elapsed_us，suite 计划请求数 | 全部 attempt 的分母、遗漏数量、成功/失败/timeout/partial/cancelled、分层样本数和成功样本尾部分位数 CI |
| `resource-ledger.json` | `resources.jsonl` 的阶段 RSS 快照 | 按角色分别列 available/unavailable 数量和观测峰值；没有证据的磁盘、I/O、账单为 unavailable/null |

原 `latency-summary.json` 保留 success/no-match-only 语义，避免旧比较器和
replay 发生隐式口径变更。新报告说明两份分布的区别。`costs.jsonl` 继续
保存 originating retrieval work，不能将其中的查询工作或内部 cache
counter 当成“本次 result cache 命中”或真实模型账单。

现有 runner 没有逐请求 lifecycle/cache receipt。因此它写出的全部查询
都属于 `unknown`，不会依据 profile 名字、同题重复、warmup 次数或未支持的
raw `cache_hit` 字段推测分类。prepare 时长也不会仅凭“调用了 prepare”就
标为冷建。旧 run 没有 `resources.jsonl` 时 replay 可以继续，输出 unavailable；
日志存在但 JSON 损坏则返回错误。资源日志本身没有被旧 manifest 锁定，
新账目只记录读取时的资源日志摘要，并明确不把这个摘要升级成 release 证据。

## P8-008：分层 API 与统计口径

profile 可为每次实际测量构造 `statistics::LatencySample`，携带
`LatencyEvidence`，随后调用 `latency_layers(samples, expected_samples)`。
它不会执行查询、清缓存或重开进程；这些动作及其可追溯收据由调用 profile
负责。分类条件如下：

| 分层 | 必须提供的观测 |
|---|---|
| `cold_build` | build；操作前 index、parse cache 均确认为空；没有 query/cache-hit 或 warmup 矛盾证据 |
| `process_reopen` | query；已有 index；明确重新打开既有 index 且未重建；本进程首次 query；未完成 warmup；result cache 不能报告 hit |
| `warm_uncached` | query；已有 index；不是本进程首次 query；warmup 已完成；明确 result-cache miss |
| `cache_hit` | query；已有 index；不是本进程首次 query；warmup 已完成；明确 result-cache hit |
| `unknown` | 缺失、矛盾或不满足以上条件 |

`process_reopen` 只表示进程重开，OS page cache 一律 `unknown_not_cold_disk`。
同一新进程刚完成冷建后的首查即使 query 前 index 非空，也不能充作 reopen；
必须额外提供 `reopened_existing_index=Some(true)` 的直接证据。
本轮没有调用 sudo、清 OS cache 或虚构 cold-disk 环境。

每层同时保留：

- `samples`：该层全部 attempt；error/timeout rate 使用这一分母。
- `all_attempt_elapsed`：全部有计时的 attempt，包括错误、partial、取消、timeout。
  timeout 的 elapsed 是截止时观测时长，对“完成请求所需时长”是右删失值，不能
  当成一个成功答案的 latency。
- `completed_elapsed` 和 `p95_completed_ci` / `p99_completed_ci`：只描述
  success/no-match；必须同时查看全样本失败率，不用成功子集掩盖可用性失败。
- `missing_timings`、`missing_samples`、`unexpected_samples`：保留数据缺失和
  计划分母差异，不用缺失值补零，也不丢弃失败行获取 best-of。

`quantile_interval` 用二项分布次序统计量计算 95% 分位数区间，数值计算采用
log 概率避免直接计算极小幂。区间假设同分布且独立；同题检索质量独立单位仍由
既有 quality scorer 管理，不把 query 重复次数当成独立检索问题。

尾部区间无界时端点为 null，不把观测最大值冒充置信上界。例如 N=200 时
`0.99^200 > 0.025`，p99 的双侧 95% 区间没有有限上界；因此“达到 200 条”
并不自动证明稳定 p99。空样本为 unavailable，未知分层、失败/缺失、小样本、
无界区间均为 inconclusive。即使区间有限也只给
`observations_only_requires_controlled_release_profile`，没有隐式 passed。

## P8-009：RSS、磁盘和费用

### 真实 RSS 路径修复

生产测量函数 `sampler::sample` 现在使用同一 `ps_resources` 解析路径：

- KiB 到 byte 使用 checked multiplication，进程树使用 checked sum；任一
  子进程 RSS 不可读或总和溢出，树总量为 null。
- 解析保留不可读 RSS 子进程的 PID/PPID，不会静默丢弃它而给出低估的树 RSS。
- 拓扑行损坏或重复 PID 会使树观测不可完整认证，保留独立、无歧义 PID 的观测。
- 没找到 server PID、probe 禁用/超时/繁忙均不可填 0；真实测到的 0 仍保留 0。
- 现有可选 probe 的 250ms 调用预算和单 worker 上限保持不变。

`memory_ledger` 将 client ps、client native alternative、server 单 PID、
server tree、已有的 external-service 字段分别列出。LSP/模型服务与 OCE
外部容器没有独立观测时单列 unavailable。它没有把两个 runner 读数相加，
没有把 server 单 PID 再加进 tree，也没有把各阶段不同时间的角色峰值相加。
旧快照没有证明所有角色同时且互不重叠，因此全系统 RSS 合计为 null；
sampling interval 也为 null，阶段快照不声称连续真实峰值。

### 纯磁盘账目 API

`disk_ledger(&[DiskPartition], complete_layout)` 接受调用方实际收集的独立
物理存储对象，按 index、FTS、parse cache、vector、artifact cache、shared、
other 分类。`storage_id` 必须表示互不重叠的物理对象身份，例如 inode。
同一 SQLite 文件同时含 index 和 FTS 时使用 shared，不能为两种逻辑表各
记一次完整文件大小；有目录重叠的扫描结果不能直接充作这个 API 的分区输入。

同一身份的完全一致重复观测仅计一次；身份冲突返回错误。已测 subtotal 与
完整 total 分开，只有调用方声明完整布局、没有缺失值、且 checked sum 不溢出
时才输出完整 total。未扫描、部分覆盖、真实 0 和溢出分别处理。API 自身不在
benchmark 中额外遍历活跃索引，也不从旧 fixture 猜磁盘分区。

### 纯费用账目 API

`cost_ledger(&[CostReceipt])` 接受 reported/estimated 明确区分的账目，
按币种与 basis 分桶，不合成一个混合“实际总账单”。金额采用该币种百万分之一
的整数；requests/input_tokens/output_tokens/cache_hits 也保留 Option。
没有 usage 证据不会填 0，也不会由 request 数反推 tokens。

同一 receipt identity 的相同记录去重，冲突报错；任一值缺失或累加溢出时
对应合计为 null。`duplicate_charge_uncertain` 保留可能重复收费的不确定性，
不把 retry 简化成已确认免费或已确认重复付费。这个 API 不查询外部模型、
不发送源码、不取得实时价格，不依据 retrieval-work receipt 创建账单。

## 本轮验证范围

`crates/cc-eval/tests/p8_measurements.rs` 覆盖分层正负例、全部状态分母、
小样本/无界 CI、独立手算二项分布区间、进程树缺失与溢出、0 与 unavailable、
共享磁盘去重/冲突、跨币种 reported/estimated、重复收费不确定性，以及
真实既有 runner → 报告 → replay 的联通。测试 backend 明确标为 fixture，
不声称这个测试运行了当前产品二进制的受控 release 性能。

验证命令：

```sh
cargo test -p cc-eval --offline --locked --test p8_measurements --test benchmark_reports -j 2
cargo test -p cc-eval --offline --locked --lib process_probe_tests -j 2
cargo clippy -p cc-eval --offline --locked --lib --test p8_measurements --test benchmark_reports -j 2 -- -D warnings
```

本轮 Linux 工作区执行结果：新增 `p8_measurements` 11 项、既有
`benchmark_reports` 11 项全部通过；上述限定目标的 clippy `-D warnings`
通过。既有 `process_probe_tests` 为 7 通过、1 失败：
`linux::live_child_snapshot_is_attributed_monotonic_and_disappears` 的原生 PID
观测持续返回 unavailable。将该测试单独以 `--exact --test-threads=1` 复测，
仍复现同一失败。本轮没有修改这条 native reader/child 测试路径，也没有将
它 skip 或把缺失观测改为 0；这一失败保留为待诊断项，不能声明 sampler
整组、完整工作区或跨平台认证全绿。

构建使用本地固定 Rust 环境、隔离工作目录和本任务 target；未运行真实性能
认证、未拿并发编译期间的计时作为性能基线。完整 V20 还需要由后续受控 profile
采集直接 lifecycle/cache 证据、真实磁盘布局、资源时间轴及所选发行范围的成本证据。
