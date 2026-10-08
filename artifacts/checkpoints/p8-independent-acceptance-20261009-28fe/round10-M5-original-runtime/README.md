# M5 原始运行证据保全（第 10 轮）

本目录只保存固定产品 M5 `fec0698c7fa4d76076b828cc17cf277ad8b307e0` 的原始运行证据及其已有独立审查。它不是新的一次产品执行，也不提供 runtime 或原 TODO 验收通过结论。原始任务账本仍为 192 项，其中已完成 163 项、未完成 29 项；本包新增完成数为 0。

## 原运行状态

| 原运行 | 必须保留的实际状态 |
| --- | --- |
| mixed C1 | 原 CLI 0、verify 0；已有独审确认该格 900 次终态、600 次读取、300 次构建，以及对应 raw、RPC、统计和 parity 范围。该单格成功不使四格矩阵成功。 |
| mixed C4 / C8 / C16 | 三格原 CLI 2、verify 2，四格矩阵失败。原 report 在封存之前曾写 `passed_observation` 和 exit 0，这些原字节完整保留，但不能覆盖实际失败退出。C4/C16 没有成功生成 seal；C8 原 seal 存在但随后的 inventory 验证失败。未重封、重试、删项或修改原报告。 |
| M5 小时 soak | 原 receipt 仍为 `running`。23:08:23 UTC 的单独观测记录会话返回 Unknown process id，没有观察到终态退出码或最终 verify；这是未完成证据，不是 1 小时通过，也不能据此断言进程为何停止。 |
| M5 新 backfill | 原 receipt 仍为 `running`，构建未观察到完成及随后 backfill 执行验收；没有终态退出码或最终 verify。保留同一中断观测的未知边界。 |

所有原因分析仍按原报告保留：不能将 mixed 封存异常归因于产品或环境；四格同机运行且与 soak 重叠，不能据此做隔离性能或因果结论。后来产品修复不追溯改变本包中的 M5 运行结果。

## 两个独立压缩包

- `M5-four-mixed-originals.tar.xz`：按预先冻结的 `mixed-original-size-plan.json` 与原测量 recipe 首次落盘。100 个普通文件，共 174,704,197 字节，包含原 99 文件清单及该清单本身，其中 `failed-originals/` 为 84 个文件。保存实际 raw、主/对照 RPC、统计、parity、日志、原报告、seal（如原本存在）及 14 个 SQLite/WAL/SHM 字节快照。压缩结果必须精确为 11,728,748 字节、SHA-256 `a37d29dafce06eca6444cee9f258d8a379256bebf24f4a7e6b432c07f6d840eb`。这是冻结选集，不是四格项目目录的完整展开副本；原夹具和构建身份由已有收据绑定。
- `M5-interrupted-soak-and-backfill-originals.tar.xz`：原包逐字复制，没有重新压缩。包含 2,093 个原文件，共 278,231,211 字节，另有包内 `preservation-manifest.json` 和 282 个目录记录。压缩包 4,774,280 字节、SHA-256 `96cd0f808e4e8babba7eefdbfdf2a329c114dc501ca339c71e96bf67a4e2993c`。原 collector 只保证采集窗口内观察到字节稳定，不宣称全局进程消失或原子快照。原 receipt 的 `running` 状态保持不变。

两包共 16,503,028 字节，分别保存，没有相互嵌套，也没有复制旧的大型审查包。partial 原方案明确排除可重建的源码 checkout、私有构建 target、依赖 registry，以及三个已有构建收据和摘要绑定的 native binary；这是原件保全包，不是自包含构建包。完整排除清单保留在 `partial-preservation-manifest.json`。不存在的最终结果没有被补造。

## 索引与复核范围

`archive-source-index.json` 为两个包分别列出每一个普通成员的历史 source 路径、成员路径、权限 mode、size 与 SHA-256，并列出全部目录 mode。partial 包中的额外清单成员单独注明。`copied-originals-index.json` 绑定本目录所有原样复制的报告、清单、收据、原采集/测量脚本和 interruption observation。`package-index.json` 列出本目录除其自身之外的最终交付文件；索引不自我引用。

本次复核对两个 tar.xz 全部成员进行流式读取，核对精确路径集合、普通文件/目录类型、重复成员、mode、size 与 SHA-256。所有 SQLite/WAL/SHM 只当作不透明字节读取：没有打开数据库、checkpoint 或执行归档内二进制、脚本；没有修改原现场、原 seal、共享仓库 source/index/HEAD/refs 或任务状态。`archive-verification-receipt.json` 只证明归档传输与内容保全，不是产品测试报告。

`mixed-independent-report.json`、`mixed-final-independent-handoff.json` 和 `interruption-observation.json` 是原报告的逐字副本；它们同时在对应原包的完整清单范围内保留。少量顶层副本便于读者直接查看，未再次展开大型 raw、RPC 或 SQLite 数据。原 collector 与测量脚本作为审计原件保存，不能仅因出现在本目录而执行。`prepare_archive.py` 是本轮保全脚本原件，输出目录使用独占创建以避免覆盖已冻结交付。

本包待独立复核后由主智能体另行发布到 main 的纯 artifacts PR。当前没有发布、合并、关闭 PR 或修改 TODO 的授权结果；发布本包也不构成任何原验收条款的通过。
