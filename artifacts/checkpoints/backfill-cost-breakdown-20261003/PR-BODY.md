固定 PR101 源码 `574f7598662334c63e020da136c87f4f7281554d` 的100k证据仍为300s backfill失败。本PR只新增 `artifacts/checkpoints/backfill-cost-breakdown-20261003/` 分析与独立有限SQL诊断，不修改生产/版本/tasks/TODO。

293次status调用累计243.5s，292次generation变化错误；轮询暴露占80.6%，poll桶覆盖96.18%的逻辑读取字节。HTTP实际单input、峰值1，ungated服务段只占2.48%；明确区分28846条测量窗口返回与850条失败cleanup尾段返回。1k/5k原DDL候选SQL诊断显示临时排序与线性VM steps，配置差异/非产品绝对时延边界单列。

下一生产优化建议先降低昂贵status统计及重试扫描，再验证claim访问路径；没有因果A/B，不宣称关闭poll的加速比例，也不把磁盘sync/逐轮64项查询误报为已精确归因。

验证：17个immutable git blob身份、HTTP/RPC/window计数与资源时间加和、固定binary receipt身份、有限SQL fixture VM步数通过；无产品重跑、真实provider、heldout、故障或GC/WAL测试。完整报告、JSON、可复现脚本及误差在指定目录。基于原证据分支的增量draft，禁止merge/forcepush/deploy。
