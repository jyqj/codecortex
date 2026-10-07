# A+B+packing 修复的固定源码准入（v6）

当前显式版本 `p7-capture-revalidation-20261007-v6`，固定 PRODUCT `87a8c8a787604a03d3db6567a6fd429f057e7251`。
独立接受的完整 A 差分为 7 个 crate 输入，B 差分仍为原 3 个输入，二者不重叠。完整 768 输入与最终 48 项组合实测源码逐字一致。

[receipt.json](receipt.json) 保存主线程实际运行的源码守卫、14 项拒绝控制、当前计划检查、时间和日志摘要。
[guard-review.json](guard-review.json) 保存另一 subagent 独立读取 Git/disk/manifest 的原始复核记录；它与行为测试的执行归属分开。
Guard 相对 v5 只更新四个显式 pin 常量，逻辑与 14 个 controls 未变；CI 只改变固定版本选择器。

[packing-storage-check.json](packing-storage-check.json) 保存整合后的只读文件交付核验：106 个原文件全部匹配，78 个归档成员、28 个磁盘文件，无重复存储。
旧 A/B review 与 v4/v5 原始证据保留历史范围。11 项路线图 controls 及 40 项历史源码 controls 不重复改名为本版本新执行；原日志与版本见 receipt 的单独引用。

最终行为验证为 48 passed / 0 failed / 1 个既有显式 stdio ignored，详见 [组合收据](../../combined-validation-fix-20261007/validation.json)，不与旧重叠组累加。
本目录只接受固定源码完整性和命名检查，不认证完整最终 head CI、全 P7、正式质量或 100k。原 CI run37623178087 的失败及其修复过程另行保留。
