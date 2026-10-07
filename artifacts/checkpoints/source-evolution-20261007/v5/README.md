# A+B 固定源码并集（v5 历史检查点）

v5 将独立审查的 Python capture revalidation 增量与 validation work 组合，完整清单为 767 个 crate/Cargo 输入。
实际守卫通过；14 项拒绝控制、11 项路线图测试通过；完整哈希关系由独立审查者复核。
[receipt.json](receipt.json) 保存结果和摘要，[guard-review.json](guard-review.json) 保留独立原记录。
40 项历史源码控制的日志来自 v4 准入，单独标明版本，不重绑定到 v5。

此时 PR #143 head781f0b79 的 CI run37623178087 在严格 packing 夹具观察到 omitted_hits3、原期待2。
该失败保留并由后续窄修处理。本目录仅证明固定 v5 的源码完整性和命名控制通过，不表示全 CI、质量、规模或后续源码已通过。
后续修复必须登记新的固定源码/审查与显式版本；本目录保留原样。
