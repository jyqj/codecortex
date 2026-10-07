# P7-011 最终组装 SQL 成本

最终组装现在单独报告本次 manifest/identity 和候选投影 SQL；缓存命中仍重新计量。
旧 retrieval.cost 继续描述原始查询工作。两个真实 Python 声明分别产生 9 statements/10 rows
和 1 statement/2 rows。空输入、失败前工作、重复 cached statement 和真实暖缓存均有回归。

固定发布源码 `bfeafe85373504ffdd041b8da2d9a779b4faf18c` 与实测本地 `d0d54dbd`
完整 Git tree 相同。定向 21 passed / 0 failed / 1 existing ignored，相关 strict Clippy、fmt 通过。
精确命令、日志和源码哈希见 receipt.json，独立于实现者的父级代码审查见 review.json。

计量仅覆盖当前 final-assembly attempt 的命名 SQL，不累计被 generation fence 丢弃的
前次尝试，不包含事务控制、generation、dense fence、graph 或磁盘工作。
该实现子项完成；P7-011 整体验收状态不据此升级。
