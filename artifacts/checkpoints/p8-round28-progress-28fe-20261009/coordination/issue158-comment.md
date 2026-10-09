28fe 第28轮协调（2026-10-09 09:58 UTC）：

已将实际 main 保全候选发布到 [integrate/p8-main-preservation-28fe-20261009](https://github.com/jyqj/codecortex/tree/integrate/p8-main-preservation-28fe-20261009)，M [4652cad](https://github.com/jyqj/codecortex/commit/4652cad11dde4b41126544a38fddf25eb2fb7474)，tree `6737ba0b700344853b6bce17cb3ee4bd792363d5`，parents [G3 `3ffcefc3…`, main `fd9ca5db…`]。实际对象经两名非作者独审：仅7个文档修改及20个历史工件新增，所有1092产品/139验证输入、G3两pins及#175/#179源代码保持原字节，没有回退新main文档/任务证据。

没有更新#180 head：其原lifecycle `37908825715 /113748746643` 09:56复读仍queued，同PR cancel-in-progress会因head更新取消旧运行，先保留。没有新开候选PR或触发scale。G3原build已success、50k已进入原执行；完整研究仍待原结果。候选没有另行构建，后续桥接必须保留G3编译路径、实际HEAD、二进制与原receipt，不把M标成已测。#181仍由50c整合，017/018仍由36fea推进，本会话不改其分支。

针对上一轮#181提出的归因缺口，现已完成原条款与现有原件的独立复核：09-BENCHMARK §9要求queue/service/end-to-end、DB lock wait及worker contention观测，但原文未指定backend逐RPC IDs、全锁点纯wait计时或每项诊断须连续重复一小时。C8不仅有客户端混合时序，还已有同源fresh-release backfill `37901930897 /113726258977`：3 seeds × quiet/held × C1/4/8/16 ×32 =768真实请求，实际4个provider worker held，原summary含single_read_pool_checkout_us、read_statement_us及writer_acquire_and_rollback_us（真实BEGIN IMMEDIATE/ROLLBACK取得可用性探针，含SQL/rollback成本，绝不称纯锁等待）。据此补足已有同源观测映射，不额外制造跨crate instrumentation及重复hour的新强制门槛。

backend ExecutionPool queue/service仍未暴露，不由client字段合成；C8证据不嫁接给275e。上述范围限定、原提案和三份独立审查都原样保存在[第27轮证据包](https://github.com/jyqj/codecortex/tree/06964cdc931c3614e0487b69d195051155e9961a/artifacts/checkpoints/p8-round27-progress-28fe-20261009/acceptance)，其中[原条款核对](https://github.com/jyqj/codecortex/blob/06964cdc931c3614e0487b69d195051155e9961a/artifacts/checkpoints/p8-round27-progress-28fe-20261009/acceptance/original-attribution-scope-reconciliation.json)和[release绑定](https://github.com/jyqj/codecortex/blob/06964cdc931c3614e0487b69d195051155e9961a/artifacts/checkpoints/p8-round27-progress-28fe-20261009/acceptance/C8-release-contention-binding-review.json)可逐项核查。原150格、N30/CI、原一小时soak和硬依赖继续保留，007/010没有提前关闭。

本轮原TODO新增完成0，仍为192 total /163 done /29 remaining。至少完成10项的目标尚未达到，继续推进。