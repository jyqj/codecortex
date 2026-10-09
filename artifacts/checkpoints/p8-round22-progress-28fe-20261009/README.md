# 第22轮进行中：归档合并与原Mac回归实测

原任务总账：192项，163 done，29未完成；本轮截至本记录新增完全完成0项。

## 已完成

PR #172已于2026-10-09 05:59:04 UTC正常合并为a6b51ffc7d02a798b0230b9f7ec5ccda1b5757ed。实际双父为7d5402a17cfdd06274e8204ec544bd4db104f872与c6a19b32bd2f676f024df9d5070ab673a85515c1，实际树48cb1d9798fc23a636ac759b5ecfbf44a8a029f1与原CI checkout的完整树相同。完整原check日志、独审和实际合并收据均在本目录。

原CI37888773299的三个job均实际成功；check原日志中46项资源控制、409项P8 Python、181项源码完整性均具名通过，v15/原v14、历史、facts和plan通过。归档仅新增453个artifact文件，原源码、工作流、任务和Mac失败材料保留。

固定A23的Mac原测试run37891034688/job113691785319实际成功，唯一原named test为1 passed、0 failed、0 ignored、11 filtered out。测试框架报告0.12s；该值不是内部worker的精确wall time。实际Rust/Cargo1.95、macOS26.6.2 arm64、Xcode26.6选择SDK26.5，原250ms/小于2秒断言未变。前后源码HEAD/tree/clean及两原文件完整身份一致；完整原Cargo输出和有限独审在../p8-original-mac-supervisor-28fe-20261009/。

## 仍在推进

旧用户Mac/SDK15.4的原失败仍为failed、原因未知。这次原test成功只适用于实际受验环境。正在按P8-012原有“发布平台范围明确”条款，逐项绑定既有八格冷构建环境和已知失败，不把单case推广成整个Mac workspace通过。

06:05 UTC官方状态：A23原规模37872522779仍完成4/150分片，100k job113638100879继续原测量；G2工程100k job113654983347也仍运行。两研究不混用、未取消、未重启；N30/150分片/1500复合样本身份及原预算不变。
