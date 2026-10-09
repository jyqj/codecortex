# G275 lifecycle 原件独立复核

固定来源 `275e8799d4947d297329073eaa3ca675d3fd0777`；GitHub run [37890757049](https://github.com/jyqj/codecortex/actions/runs/37890757049)，artifact `11597589468`。本目录仅收录小型复核记录，原 ZIP、SQLite、产品和 evaluator 二进制留在执行工作区。

## 结论

原 `p8_lifecycle.py verify` 已实际通过 2,381 个密封文件；保留的原 `p8-measurements` 已纯离线重放，exit 0，报告与原件逐字节相同。source/build manifest 的 1,089 个产品输入、release/default Cargo artifact 和二进制 SHA 全部对应 G275。

所有 30 cold、400 process reopen、400 warm uncached、400 cache hit 样本均从 raw event/RPC/response/cache counter 复核；另有 1 个显式不计样本的 warmup。总计 431 个正常 EOF 会话、4,555 对 RPC、1,261 份资源快照、30 个闭库 SQLite。所有数据库 integrity 为 ok、FK 错误为 0，均有 32 files / 402 symbols / 402 chunks。

## 范围

OS page cache 未清。Cold N30 的 p95/p99 CI 上界仍为 null；query N400 只支持原实测分布及其 IID 假设。Cold 记录的是初始化后的 full-index RPC，reopen 记录新进程重开索引后的首次查询，均不等于完整进程启动时长。

资源只统计 stage snapshots，各角色非同时、互有重叠，不相加为总峰值。128,901,120 字节是 30 份保留 fixture 的共享数据库/WAL/SHM 总量，FTS 逻辑页已包含其中。未观测角色和 provider 账单保持 unavailable/null；不记为 0，不认证真实 billing。

本复核只接受 G275 原观察子范围，不认证 b4fef722 或 PR #181。P8-008 的 P8-007 依赖和 P8-009 的 P8-008 依赖仍未完成，两项继续 in_progress。原账本仍为 192 总项、163 done、29 未完成；本轮关闭原 TODO 为 0，至少完成 10 项的目标尚未完成。
