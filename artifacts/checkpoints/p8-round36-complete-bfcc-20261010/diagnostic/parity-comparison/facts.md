# 同源两条原 parity 记录的有限比较

固定 source `9cc6bf49f6dd81e4069a8004eed49addba0ab79b`、run `38012409915`、attempt 1。这里只读 checkpoint 02 的 `cold_parity` 和 checkpoint 04 的 `no_op` stage 记录及相邻已保全 build 快照；没有重跑 reader、CRC、validator、ELF 或测量。

**逻辑结果和已报告排序配置没有变化，现有字段不能解释耗时差。** 两条 parity 对象的递归差异恰好只有两个 DB+WAL/SHM 文件字节计数。未证明必要修复，不据两个观测点判断性能回归，不提出新实验。

| 原字段 | Cold parity | No-op parity |
|---|---:|---:|
| `parity_wall_us` 换算秒 | 921.742980 | 1623.147962 |
| 两次原 wall 差 | — | +701.404982 秒，原因未知 |
| 比较表数 | 15 | 15 |
| 每侧合计行数 | 6,653,993 | 6,653,993 |
| `canonical_bytes` | 10,302,684,584 | 10,302,684,584 |
| `scratch_peak_bytes` | 5,984,387,072 | 5,984,387,072 |
| Full DB+WAL/SHM 字节 | 6,883,108,064 | 6,883,202,536 |
| Incremental DB+WAL/SHM 字节 | 6,883,140,928 | 6,769,709,056 |

15 个完整 per-table 对象逐字段相等：`document_manifest`、`resolution_frontier`、`semantic_edges`、`dispatch_sites`、`resolution_manifests`、`resolution_dependencies`、`public_surfaces`、`files`、`symbols`、`imports`、`symbol_refs`、`call_edges`、`chunks`、`test_edges`、`routes`。每表 cold/no-op 的 incremental/full 四个 digest 均相等，双方 row count 相等，difference count 均为 0，表的记录次序也相同。完整行数和四组原 digest 在 `comparison.json` 保留。

双方原报告均为 `disk_backed_exact_v1`、`without_rowid_value_ordinal_v1`、`scale_capacity_v1`、`sorted_row`，sorting cache 配置均 2048 KiB；canonical/scratch 上限分别 16 GiB/8 GiB，每行 1 MiB、每表 5,000,000 行。没有 `equal_input_order_witness` 或其他 witness 字段；这不能证明实际输入游标次序相同。已排序的 digest 和配置字段也不说明物理页布局、实际 cache 命中或运行时 I/O 状态相同。

| 可关联原 build 快照 | `end_us` | PID RSS 字节 |
|---|---:|---:|
| Cold parity 之前的 cold full-control 完成 | 1,186,365,870 | 7,236,632,576 |
| Cold parity 之后的 no-op incremental-0 完成 | 2,110,745,857 | 6,407,557,120 |
| No-op parity 之前的 no-op full-control 完成 | 2,726,435,232 | 11,084,951,552 |
| No-op parity 之后的 body incremental-0 完成 | 4,401,245,349 | 9,933,819,904 |

这些是 Linux `/proc/PID/stat` 的单 PID RSS 页数估计，不是进程树或 parity 峰值。后侧快照包含下一次 build 的工作；两条 parity 事件本身都没有独立 start/end 时间戳或 RSS/CPU snapshot。不能把上述 RSS 差或邻接 CPU 累计值当成 parity 独占消耗或原因。

原记录没有 per-table wall、序列化/spool/sort/cursor 分项、SQLite VM 计数、磁盘 I/O 或 cache hit-rate。因而可以排除“报告中的逻辑数据量、digest 或配置字段发生变化”这一解释，但不能从这些记录识别增加耗时的原因，也不能把原因移植到旧 G8/GW4。

精确原字节位置、chunk SHA、行号、行 SHA、chunk 与完整 raw 两种 offset 范围在 `comparison.json` 的 `cold_record_pointer`、`no_op_record_pointer` 和 `associated_build_boundary_snapshots` 中。原件分别来自 artifact `11656371239`、`11656209993`；相邻 build 快照另复用已接收 `11655694522`、`11656517936`。此次比较脚本实际 exit 0；所有原测量、样本与 TODO 计数不变。
