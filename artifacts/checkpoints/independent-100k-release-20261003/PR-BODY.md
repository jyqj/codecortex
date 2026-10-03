固定 PR101 `574f7598662334c63e020da136c87f4f7281554d` 的 synthetic 100k release 独立证据，仅新增 `artifacts/checkpoints/independent-100k-release-20261003/`。

冷索引 21.638s，100,000 文件全部解析、0 跳过；固定 300 秒本地 fake-provider backfill 未达到 ready，失败后 semantic_manifest=29,696，outbox pending=70,304。完整性/FK 为 ok/0。并发与 normal-exit/reopen 验证明确未跑，因此不宣称 100k 全通过、完整 V20 或其他源码通过。

保留两轮前置失败（父 Git 历史污染、脚本表名 KeyError）及最终自然超时、完整命令、输入 hash、实际 Cargo profile/features/sourceSHA、隔离 target 与 binary SHA256、资源采样及进程树不可枚举的限制。附有限复跑/验证脚本，证据身份验证通过，产品结果仍为 failed。50k 作者证据未复跑，生产源码/tasks/TODO/版本只读；无 heldout/真实 provider/merge/deploy。

应以 PR101 固定 head 对应分支为 base，仅审查指定证据目录；保留 draft 状态，核心结论由父任务判断。
