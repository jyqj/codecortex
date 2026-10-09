# P8-007～010 原任务 evidence 追加草案

本文件仅供审查；对应 JSON 未写入 tasks.json，四项状态都保持 in_progress，基线 163 done / 29 剩余。已有 evidence 应完整保留，不能由本草案覆盖。

source: a23bb72d3c954f385b99fe81ce9189885c208557

| 任务 | 可追加的原观测范围 | 仍须保留的原硬依赖 | 终态/范围 |
|---|---|---|---|
| P8-007 | C1/C4/C8/C16 四档 mixed 各 900 次，加独立原 fake-backfill 768 次 | P8-006 | 原 component 已审，不能单凭组件通过关闭原任务 |
| P8-008 | 30 cold + 400 reopen + 400 warm uncached + 400 cache-hit，原双 replay | P8-007 | 原分层已审；OS page cache unknown、cold CI 的 null 上界保留 |
| P8-009 | 1261 原 client/server/tree 阶段快照，物理 SQLite/逻辑 FTS/费用账本 | P8-008 | 单位/归属已审；合法 unavailable 不新增性能门 |
| P8-010 | 原 3601 次/1000 files/1 小时 soak 的真实执行身份与待验收项 | P8-009 | 仍在原 workload，没有终态、完整 artifact 或 raw 通过结论 |

JSON 中每项提供现有 evidence 常用字段 target_sha、status、artifact_paths、scope、review、rollback_status、summary，并附原 source/observer/build/raw/statistics/receipt 的具体路径和 SHA256。P8-007 单独保存各 C 的实际峰值 1/4/7/12，不用配置值冒充实测并发；P8-008/009 共用同一原件但保留不同验收含义。

P8-010 当前 pending 记录必须保留为时间点事实。原 soak 终态后应追加独立终态评估，附完整原 artifact、原 RSS/缓存/四季度/15 表门结果；不得把本 pending 条目无说明改成已通过。原 V11/V20、V07/V17/V20 以及相关旧功能回归与任务依赖仍由完整集成审查闭环。

候选条目：[p8-007-010-evidence-append-draft.json](p8-007-010-evidence-append-draft.json)；没有改 tasks.json。
