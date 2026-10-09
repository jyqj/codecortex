# D0 原研究取消后的原件独审

原 study `37854240827` 为 `completed/cancelled`，固定 controller `ca72a2dde363e35203f7b3cb30ed00729d460fd7`、native source `d0cb69c601e530dcef738af0c2dffb8d3b8bcf28`、attempt 1。完整 150 个初始测量 job identity 与原登记逐一相等。152 个实际 job 为 admission success、原 rep8 failure、aggregate failure、其余 149 cancelled；其中 138 个取消格的 steps 为空，不能从 job started_at 推断已开 native 测量。

11 个先前已开测 job 的 native step 于 2026-10-09 02:13:37–39 UTC 取消，随后原 artifact 上传成功。官方日志只出现 `The operation was canceled.`；没有证据给出取消请求时点、取消者或基础设施原因。run updated_at 02:15:11 UTC 是状态更新时间，不是取消请求时点。原 rep8 更早的 shutdown/143 失败与本次 11 个取消分开保留。

新增 11 个原 shard ZIP 加一个 matrix ZIP，共 52,944,040 B，全部按官方 API size/SHA-256 核对。原 run 的 14 个 artifact 元数据完全未变，当前共 26 个。每个新 ZIP 的全部成员经过冻结 v6 zip_guard 的规范路径、重复、类型、范围、真实解压流/长度/CRC 检查，并逐成员 SHA-256；没有执行 ZIP 内代码。临时解包只发生在单次调用的私有 `/dev/shm` 目录，逐件清除，不复制 corpus，不打开数据库。唯一 artifact binary 调用是已授权原 `p8-scale --hash-file`，用于原 build BLAKE3 身份核验。

原 upstream build、完整 1087 native inputs、两份 observer 输入与固定 D0 source 全部相符；每格 source-before 与固定 snapshot 完全相等，原计划仍 100k、30 repetitions、对应原 rep、5h、512MiB、原 capacity profile。每格 source-after 和终点收据均缺失，因此不宣称执行全程源码闭环或成功。

| rep | 原 job | 原 artifact | 实际 native step 秒数 | 已完成且原验证器核过的 15 表比较 | 最后原事件 |
|---:|---:|---:|---:|---|---|
| 0 | 113580044390 | 11591206925 | 9913 | cold / no_op / body | api incremental-113 started |
| 1 | 113580044374 | 11591029769 | 10851 | cold / no_op / body / api | config full_control started |
| 3 | 113580044362 | 11591054479 | 5873 | cold / no_op | body incremental-395 started |
| 4 | 113580044326 | 11591471315 | 10777 | cold / no_op / body / api | config incremental-255 started |
| 5 | 113580044352 | 11591186114 | 2740 | cold | no_op full_control started |
| 6 | 113580044452 | 11591590457 | 8983 | cold / no_op / body | api full_control started |
| 10 | 113580044429 | 11591645982 | 4610 | cold / no_op | body incremental-265 started |
| 11 | 113580044433 | 11591485709 | 8739 | cold / no_op / body | api incremental-129 started |
| 13 | 113580044526 | 11591039584 | 3822 | cold / no_op | body incremental-124 started |
| 14 | 113580044492 | 11591505637 | 6733 | cold / no_op | body full_control finished；body 比较未记 |
| 15 | 113580044432 | 11591296599 | 9113 | cold / no_op / body | api incremental-145 started |

11 份 raw 共 178,436,892 B，保留 5751 次 build 返回、29 次完整 15 表比较；这些已写比较全部 equal。各份完整原 `validate_shard` 均拒绝缺失 `shard.json`，原 `inspect_raw` 均拒绝 `missing registered mutation stages`。raw 无截断 JSON 行，也无原生 terminal outcome、预算超限事件或已知 parity failure。取消时所有已开测格都尚未到原 18000 秒预算。上述部分进度不是完整样本，不能并入原 1500 样本统计。

matrix 原件明确 failed：缺 rep0 的最终 `shard.json`。新增有效完整格数 0，研究完整覆盖仍 0/150。原 TODO 新完成 0，剩余 29。

冻结 wave002 的范围可以保持为唯一原 rep8 / ordinal2。`load_package` 检验固定包和全部输入；`validate_original_run` 检验原 run/head/attempt/event/branch/workflow/repo，没有要求原 study 顶层仍 running。`observe` 为其他 149 取消格逐一保留官方结论、terminal_unclassified 和 unclassified_official_failure，与旧错误取并集；完整 26 件 inventory 也保留。各 row 的 artifact_ids 仍继承旧账本，故具体新增 raw 的失败必须由完整 receiver 收件，不把 controller 当 raw 验收器。`check_ledger` 只允许固定 rep8 的已审两前驱进入 ordinal2，其他 149 格没有获得补跑资格。

本次额外逐字核对，原 rep8 job 与 c33… 日志、wave001 job/日志/ZIP 元数据均未变。没有必要仅因原 run 顶层取消而扩大或更改冻结 wave002 controller；未来实际准入仍须读取其真实 current-wave identity 和全部原证据。完整 coverage 始终 false；任何其他格的继续执行需要另行审查的合同。本报告没有发布、调度、取消、重启或修改任何工作流。

主要机器收据：`summary.json`（小结），`independent-readback.json`（完整 source/build/逐成员/逐格验证及原计时），`wave002-cancellation-applicability.json`（冻结源码行号与前驱字节核对），`official-job-log-manifest.json`，`temporary-extraction-lifecycle.json`。原 ZIP 在 `archives/`，精确官方 job 日志在 `logs/`，固定 API 原字节在 `api/`。

生成小结第一次误写了候选 workflow 文件名，因 FileNotFoundError 停止；此时原件和独审结果未改。随后按冻结 controller 的实际 `.github/workflows/p8-d0-recovery.yml` 路径生成小结。这是审核汇总脚本路径错误，不是产品、原 artifact 或原 validator 的失败。
