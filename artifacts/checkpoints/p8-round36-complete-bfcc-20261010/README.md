# 第 36 轮：50k 原分片验收与独立 100k 诊断前缀

本轮新增完成 **0** 个原 TODO；本会话累计 **1/10**。项目 **192 total / 164 done / 28 remaining**。主线仍为 `b9b089bb4eae072affe9326681d4980eae15fd84`；任务账本未修改。

## 完整规模研究

原 run `38013753078/a1`、source `9cc6bf49f6dd81e4069a8004eed49addba0ab79b` 的 50k/0 原件通过一次原 `validate_shard`。新增 9 个观测，冷建及全部八个更新阶段的完整十五表对账相等、更新闭包完成。使用本 run 已验收的 own build，不重验之前三个分片或 build。

累计 **4/150 个分片、41/1500 个观测**，仍缺 146 个分片、1459 个观测。02:55:11 UTC 官方记录中 100k 首片仍运行，后续 145 片继续遵守原工作流前置条件。全量聚合尚未接受，不混入独立诊断或旧研究。

- [50k 原验收](full-cohort/50000-0/independent-fourth-shard-review.json)
- [四片累计进度](full-cohort/50000-0/cohort-progress-accepted-four.json)
- [原 ZIP 完整字节分块与来源](original-zip-custody/full-cohort-50000-0.json)

## 独立 100k 诊断

本轮接受 checkpoints 03–05；00–05 原 raw 前缀连续至 **13,992,488 B**。05 新增 38 个完整事件，记录 body incremental-5 至 23 完成，之后 body/full_control 开始；尚未看到该 full control 完成。原生最终报告、supervisor terminal 和 observer-after 仍缺失，最终结果保持 unknown，不声明 EOF。

- [checkpoint 03](diagnostic/checkpoint03/checkpoint03-prefix-review.json)
- [checkpoint 04](diagnostic/checkpoint04/checkpoint04-prefix-review.json)
- [checkpoint 05](diagnostic/checkpoint05/checkpoint05-prefix-review.json)
- [02:55 三路官方原记录](official/dual-study-0255-originals.json)

Cold 与 no-op 的两个原 parity 记录在十五表逻辑内容、规模及已报告排序配置上相同。两次耗时相差 701.404982 秒，但现有原字段无法证明具体原因；不据此引出修复、跨研究因果或新增验收门。

- [有限逐字段比较](diagnostic/parity-comparison/facts.md)
- [完整只读比较支持](diagnostic/parity-comparison/complete-readonly-comparison-support.json)

## PR 与任务边界

PR200 已于上一轮合并，本轮更新了其实际合并与有日期的研究进度说明。研究仍绑定原 9cc6 提交，未改绑到合并提交。

任务原文与当前 150 分片协议的适用范围正在独立复核；硬依赖来自原账本。本归档不修改任务定义、依赖、状态或当前注册总体。

[完整本轮收尾与全部真实 blob 路径映射](formal-closeout.json)。所有叶子均为新增；原始 ZIP 按原字节保存，没有重新打包。旧失败、未运行项和历史日期记录继续保留。
