# CodeCortex 第36轮交付

截至 2026-10-09 15:40 UTC，main 仍为 `d22d36dddf8b38f1a864c933deefe0b6d3b5baf3`，任务 blob `959a25ba851ff88f286bab0fa14167878129b1dd`。账本保持 **192 total / 163 done / 16 in_progress / 12 todo / 1 blocked，剩29**。

本轮新增完全完成原 TODO：**0（IDs：无）**；相对163基线累计 **0/至少10**。

## 新终态

固定 G3 run 37910924354 已 completed/failure。100k rep0 在原 18,000,000ms deadline 后报告 deadline_exceeded，driver exit3，worker exit未知，stderr不完整；measure skipped，aggregate exit1并报告 shard execution failed or is missing。100k失败ZIP和matrix ZIP经官方接口下载，API digest与本地SHA一致，CRC全过。失败raw 20,826,459B/2018行，最后事件为batch_10 full_control build_finished，但没有该stage_finished、batch100/batch1000或run summary。成功仍为4/150，失败1，未物化145。

## PR管理

PR180由owner推进到 fffd950d：第35轮formatter修复已逐字吸收，原失败门已在新CI通过；ready PID竞态修复尚未吸收。快照25 jobs为7 success/0 failure/5 running/13 queued，继续Draft/Open。

PR188确实修改G3使用的reverse dependency查询，并在密集排除fixture降低rows/VM，但没有把G3失败因果定位到该路径；稀疏场景VM callbacks增加22.57%，相关扩展候选还有wall-time回归。它没有100k/N30/150-shard证据，且与PR180分叉，故保持Draft/Open，不整支合入，也不能称作G3修复。

## 结论

P8-005完整规模子门仍open，严格配对因果性能未建立，whole-tree memory peak仍unknown。P8-006依赖005，P8-007虽已有observer窄子门但依赖006；其余优先目标继续受硬依赖链约束。本轮不修改tasks.json，不重跑失败研究，不改预算，不跨源拼样本。

下一步只能基于G3固定失败原件定位具体产品瓶颈；只有存在可证实的物质修正，才能在fresh current组合形成新P/R/G，并从头完成该新来源的150-shard/1500-sample认证。
