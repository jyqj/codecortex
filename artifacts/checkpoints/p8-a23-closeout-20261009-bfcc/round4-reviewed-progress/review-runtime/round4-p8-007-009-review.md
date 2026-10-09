# 第 4 轮：P8 运行时与账本独立审查

原任务基线仍为 **163 done /29 剩余**。本次审查没有提前关闭 TODO，也没有修改原 workflow、source、工作量或验收器。

固定 source：`a23bb72d3c954f385b99fe81ce9189885c208557`；1087 个 crate/Cargo 输入及相应 observer 原件均已逐文件绑定。四档 mixed 与 fake backfill 的原观测、以及 lifecycle 原观测已完成独立审查；原 1 小时 soak 仍在运行。

## P8-007：原四档 mixed 与 backfill

[原 runtime run](https://github.com/jyqj/codecortex/actions/runs/37871838957) 的每档固定为 1000 files、900 offered operations（300 build、600 read），按原并发大小的固定 burst 日程投递。

| 配置 | 实际调用峰值 | 原操作成功 | read/build 重叠 | 原 15 表 | 审计 |
|---:|---:|---:|---|---|---|
| C1 | 1 | 900/900 | 否；C1 原规范允许 | 15/15 相等 | [11592751905](runtime-11592751905-review-v2.json) |
| C4 | 4 | 900/900 | 是 | 15/15 相等 | [11591493782](runtime-11591493782-review-v2.json) |
| C8 | 7 | 900/900 | 是 | 15/15 相等 | [11593165899](runtime-11593165899-review-v2.json) |
| C16 | 12 | 900/900 | 是 | 15/15 相等 | [11592154271](runtime-11592154271-review-v2.json) |

每档原始 transport 都保留了 301 次 index（初始 full 一次及 300 次 incremental）、600 次 workload symbol read 及一次终点 symbol read。独立 full control 仅执行一次，并在所有 workload 回包后启动；没有额外 incremental 修复。每档都有 50 次真实 branch switch、6 次 catalog compaction，原 raw 与统计双 replay 一致。调用峰值来自原记录和独立时间区间重算，不能用配置值替换。

[Fake backfill 独立审计](backfill-11591821446-review.json) 覆盖原 3 seeds × quiet/held × C1/4/8/16 ×32＝768 次请求。原 2 秒 query、5 秒 progress 和 100 ms writer watchdog 未改变；每 seed provider 最大活跃 4、旧空间发布 0、最终 pending/claimed/uncovered 为 0。该原测试使用同进程 fake provider，没有 live-provider 或新 tail SLA 认证。

这些分布按各自 profile、并发和 mutation stratum 分开；不把 3600 mixed 操作池化为一组尾延迟样本。原 build/mutation 的不可界定 CI 上界继续保留 null。P8-007 的原 P8-006 依赖及更广回归/验收仍须完成。

## P8-008：原 lifecycle 分层

[原 lifecycle run](https://github.com/jyqj/codecortex/actions/runs/37871838975) 与 [独立审计](lifecycle-11591607348-review-v2.json) 证实 30 cold、400 reopen、400 warm uncached、400 cache hit，1230/1230 success；另有一次不计样本的真实 warmup。431 个原生 product session 均正常关闭，1200 个 raw query witness 经原 Rust collector 源码核验；原 native evaluator 双 replay 字节完全一致。

全部尝试和 CI 均保留。30 cold 的 p95/p99 CI 上界为 null；400 query 各层的原 CI 有界。OS page cache 未清理、仍 unknown；不能称 cold disk、稳定 p99 或性能提升。P8-008 仍保留原 P8-007 依赖。

## P8-009：单位、归属和 unavailable

1261 个原资源快照中，native client/server/process-tree 都有真实观测，采样 RSS 最大值依次为 149929984、33886208、33656832 bytes；server 与 tree 在不同采样时点，且前者包含在后者内，不能相加。client 的 ps 替代项及 external/LSP/OCE 为 unavailable，保持 null；它们不构成新增认证门。总 RSS、连续采样间隔也保持 null。stage snapshots 不能证明连续峰值或未采到的临时子进程。

30 个闭合 cold fixtures 的 90 个唯一物理对象全部归 shared SQLite，合计 128901120 bytes；30 个零长度文件是原实际 stat=0 的对象。每 fixture 的逻辑 FTS pages 分别为 chunks 118784、file_paths 20480、files 36864、literal 20480、symbols 90112 bytes；它们已在 DB 物理字节中，不再加总。该物理账本包含 30 份 fixture，不是一份 active index footprint。

原 artifact 逻辑长度快照为 343527625 bytes＝source 669540＋fixture storage 128901120＋raw/reports 213956965；记录点早于最后 receipt 写入，不能称完整 zip 或 allocated-disk 总量。费用 reported/estimated/input/output tokens/billed requests 全为 null，disabled provider 标为 not applicable；network filter 为 not_measured，不推断零账单。

审计 v2 按“可用则核单位/归属，不可用则保留 null”复核；没有把原规范允许的 unknown 变成额外性能门。原 server CPU 使用纳秒，runner SELF 秒值保留秒单位；query CPU/IO delta 含前后 status 观察开销。

## 待完成的原验收

Soak job [113631481157](https://github.com/jyqj/codecortex/actions/runs/37871838957/job/113631481157) 于 2026-10-09 02:48:58 UTC 进入原 1 小时 workload。须等待真实终态，再核 3601 原操作、2400 次四 RPC 复合读取、四季度真实 cache hit/miss/invalidation、原 RSS 趋势与覆盖门、真实 compaction/切分支和无修复 15 表终点对账。现阶段没有 P8-010 完成结论。

P8-011 还需独立原 failure-gates 和其 P7-020/P8-007/P8-010 依赖，不能由这组观测推断完成。

Mac 端执行的是原件独立审查，包括原 Python source/observer/seal/cache/RSS/latency 等验证器，以及所有原 payload/计数/CI 的复核；原 Linux ELF collector 与原 GitHub Cargo target 没有在 Mac 重新执行。原 native workload、双 replay 与 oracle 由原 GitHub 日志、原 bytes 和 receipt 绑定。

完整机器可读映射：[round4-p8-007-009-evidence-map.json](round4-p8-007-009-evidence-map.json)。
