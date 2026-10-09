# 第 12 轮：M6 本地原件与 F5 小时原件

本目录保存已经独立审阅的原执行和来源证据，不改变原任务状态、工作负载、预算、样本数或任何生产源文件。原始任务仍为 **192 总数 / 163 done / 29 未完成；本次请求正式完成 0/10**。

## 已取得的实际结果

| 执行 | 原始状态与可用范围 |
| --- | --- |
| 本地 M6 `e95fd750d2f5052d0308c970cd5032c48b765cde` 新构建 | 三个真实 release Cargo 目标均 fresh=false，构建原件通过独立审查；不是操作系统冷缓存声明。 |
| 本地 M6 C1/C4/C8/C16 mixed | 四格原 run/verify 均为 0，每格 900 终态、600 read、300 build，完整 15 表每侧 8508 行一致。四格资源采样连续性均为 false；原 mixed 门不强制该 soak 门，不据此授予 P8-009。四格共用一台主机和一次构建。 |
| 本地 M6 fake semantic backfill | 三 seed × quiet/held × C1/C4/C8/C16 × 32 = 768 个真实请求通过原控制；无真实或付费 provider 调用。原测试执行 1.923 秒与 858.345 秒构建/包装总时长分开。 |
| 本地 M6 原 parentless 启动 | 原 wrapper 最终 failed；五组原 run/verify 共十个 exit 2 保留。随后仅准备缺失父目录并以相同 argv/source/build 开始四 mixed 和 hour。没有覆盖原失败日志。 |
| 本地 M6 hour | 会话不可用、无实际终态、原 receipt 仍 running，没有 report/seal/parity/statistics。已观察到超过 5 秒的资源采样间隙。字节保全不证明进程已退出、SQLite 一致或原子快照。 |
| GitHub F5 hour `c2ad27b2b189cbc98775f20a550d718dd4913038` | run 37858536446 / job 113588467257 / artifact 11588677565。原 runtime、verify、upload 均实际成功，77 项原件检查和 18 项来源检查通过。实际 3600.045057230 秒、3601 成功终态、2400 复合 read、1400 hit/1000 miss/999 失效、四个实际时间窗、200 分支切换、25 压实、最大资源间隙 1.124027140 秒，完整 15 表一致。配置上限 C4，实际 executor/call 峰值均 1。 |

F5→M6 的独立成功路径比较确认：1087 个原生输入全部相同，9 个观察器只有最终 seal 异常处理变化，原 hour workflow 相同。F5 原执行可支撑 M6 未变的缓存与长时成功行为子门；实际身份仍是 F5，M6 新增异常处理仍依赖另行保存的回归。它不抹去任何本地失败或失联尝试，也不关闭规模矩阵和硬依赖。

## 原件与复核

`M6-local/archives/` 的两个 tar.xz 保存 18,546 个普通文件、401,589,339 原字节。第一个包保存完整 build/backfill/四 mixed 和原 wrapper 输出；第二个包保存未知 hour 的有界稳定字节观察，包括原 DB/WAL/SHM。根与非作者均逐项核对完整路径、类型、mode、size、SHA；未打开未知数据库或执行包内代码。两个实际包装脚本在 `M6-local/wrappers/`，其 SHA 与原执行收据相符。

`F5-hour/11588677565-F5-soak.zip` 是未经重新压缩的官方原 ZIP：51,012,345 B，SHA-256 `2044c562dbd820041a9ca4219052346e7566a0def4dbe220cc18de1117e9d5d3`。全部 2129 成员已流式核验；API 原件、原 job log、审查程序、统计和缓存重算一并保存。远端原 Cargo/compiler 路径属于原 receipt 与 job 证明，不能冒称它们现仍存在于本地。

`D0-wave001-publication/` 保留实际发布 commit `ad840759ba337608d3e33f2c1558042a4d5283c8`、tree `c90848b22145cdab03695ee81237d0c3f31d4fd1`、唯一 run `37865643378` 的原回读和非作者 34 项控制/33 项作者控制复放。原 wave001 的 12 文件已经在该 commit 保存，本目录不再发布或触发它。它只准入原 100k repetition 8 的 ordinal 1 补充；原 150/1500 分母与失败历史保持，当前完整收件与 coverage 未完成。`merge-multiple` 误报及基于最终字节的纠正原样保留。

`copied-originals-index.json` 映射历史绝对路径到本目录对应文件，所有复制均逐字相同。原报告中的绝对路径保留为执行身份，不改写成新的执行。`package-index.json` 覆盖除该索引自身外的每个交付文件。代码文件仅作为审计原件保存，目录存在不构成执行授权。本次没有对原运行做重试、重封存、改状态或 TODO 完成操作。
