# 第五轮：原 a23 runtime / lifecycle 终态独审

原 source 为 a23bb72d3c954f385b99fe81ce9189885c208557。七个原组件（C1/C4/C8/C16 mixed、fake-backfill、lifecycle、one-hour soak）均已有完整原件与独审通过证据。本文件只是追加终态评估；第四轮 pending 草案保持原字节，没有修改 tasks.json。原账本仍为 163 done / 29 剩余，本次审查未自行关闭任务。

## 原 soak 终态

GitHub 原 run 37871838957 / job 113631481157 于 2026-10-09 03:49:15 UTC success。工作流 workload step 为 02:48:58–03:49:03 UTC。原实际工作时长为 3,600,053,467,347 ns；driver-relative 开始/结束为 764,326,883 / 3,600,817,795,323 ns，两次不同 clock 调用形成的 1,093 ns 差异保留。step UTC 包含准备和收尾，不是精确工作边界的 UTC 映射。

全部 3,601 次 offered 操作成功：1,201 次 build / 2,400 次 compound read。四个 read 角色各 2,400 次，总 9,600 role RPC 仍只形成 2,400 个 read 延迟样本。实际调用峰值为 1，配置 C4，read/build overlap=false；这是原 soak 共用 admission lock 的真实结果。四档 mixed 的实际峰值仍为 1/4/7/12，绝不把 soak 结果并入它们的分布或升格为重叠认证。

200 次真实 Git 分支切换、25 次 catalog 压实、停止投递后完整排空与唯一 fresh-full 对照均已从原件复核。原 15 表全部一致。两个 product 均停止，sampler 停止，unfinished_work=0，两个 construction_pending=false；比较前无增量修复调用。

## 原 RSS、缓存与分母

| 原门 | 原实测值 | 原结论 |
|---|---|---|
| 实际工作时长 | 3,600.053467347 秒 | 通过原至少一小时门 |
| RSS 样本数 | 3,594 | 完整原分母 |
| RSS warmed / tail median | 115,081,216 / 144,728,064 bytes | tail 小于原 allowed 177,405,952 |
| RSS sampled peak | 149,659,648 bytes | 只是采样峰值 |
| 最大采样间隔 | 1,028,141,448 ns | 小于原 5,000,000,000 ns |
| cache hit / miss / invalidation | 1,400 / 1,000 / 999 | 原 cache identity 与四季度门通过 |
| 每实际时间季度 read | 各 600 | 每季度 hit 350 / miss 250；invalidation 249/250/250/250 |
| 原 status RPC | 8,395 | 4,800 compound probes + 3,594 resource + 1 endpoint |

RSS 的首尾样本序列季度与 cache 的实际完成时间季度保持不同定义。server SELF current RSS 与 runner lifetime high-water 保持不同归属，runtime 未采完整 process tree；lifecycle 的阶段树快照不能补成连续 runtime tree。原 unknown/unavailable 保留，不新增资源或付费 provider 门。

完整 stdio 补充绑定核验了 2,400 个 compound read 的四个角色原载荷、4,800 status probes、3,594 native resource projections，并逐一核清所有 8,395 status responses，不重用、不丢弃。原 native statistics 双 replay 字节一致；各 operation/mutation 的全部分母、IID 限制与原 null CI 上界都保留，不声称稳定 p99 或性能加速。

## 原任务 evidence 映射

| 原任务 | 可追加的已复核证据 | 集成时仍需连接的原依赖 |
|---|---|---|
| P8-007 | 四档 mixed 各 900 次及独立 fake-backfill 768 次；真实排队/尾部/全部终态 | P8-006 与完整 V11/V20、相关回归 |
| P8-008 | 30 cold、400 reopen、400 warm uncached、400 cache-hit；完整分层/CI/原 replay | P8-007 与完整 V20、相关回归 |
| P8-009 | lifecycle 1,261 阶段账本及独立 soak 3,594 server/runner 观测；物理/逻辑/费用归属 | P8-008 与完整 V20、相关回归 |
| P8-010 | 原一小时 3,601 操作、RSS/coverage/cache/实际修改/15 表完整终态 | P8-009 与完整 V07/V17/V20、相关回归 |

冻结源码里的依赖状态只是历史记录；不能据此另加“重复已经成功的原测量”要求。父级可以在完整固定 source 证据确实闭环后共同推进依赖和任务。独审没有自行授予未审验证族、发行批准或新 P 的执行信用；原 native report 的 task_complete=false / release_approval=false 也没有被改写。

原官方 artifact 11594089439 的 ZIP 为 51,069,087 bytes，SHA256 d6debfa5e4a51bb59902cdb69a35ae7626a1e20b09bcaa5ab6cfda9ee3237086。完整原件在 central raw 保留。新 JSON 为 round5-p8-007-010-terminal-evidence.json，所有字段含原 raw/stdio/build/observer/statistics/receipt 的具体路径与 SHA256。
