# P8-005 / P8-006 本地规模驱动证据

此目录保留本次规模驱动的原始失败、共享机器 timeout、最终小规模成功与构建环境问题。它支持 **local profile 准备**，不关闭 P8-005/P8-006，不改变 P7 前置门槛、原 oracle、gold 或任何发布性能阈值。1k/5k/10k/50k/100k 完整矩阵、真实向量 provider、C=1/4/8/16 与完整 100k 认证均未执行。

## 固定源码与实际验证范围

| 对象 | 固定标识 / 结果 |
|---|---|
| 原 main | `6d02d77f018a5965a6f289b0b43558ed4b9f8322` |
| 根代理独立 route reload 修复 | `462f74800670e5a9e29fdcd0eb52954c2aea2c62` |
| 本工作树等价修复 | `8a285a63`；与原修复 patch bytes 相同 |
| 完整驱动与监管修复 | `4917a43c04f72022240537761eeb785a745f3153` |
| 此 SHA 的真实合同测试 | **6 passed / 0 failed**；总计 166.19 秒 |
| 同次真实 60-file smoke | **9 / 9 样本通过**；worker wall 76.823 秒 |
| 后续缓存隔离小补丁 | `f94e32055830411a55f297f6795e5c06efa5e05e`，仅清除 `CODECORTEX_CACHE_DIR` 继承覆盖与对应说明；该补丁经过静态审阅，未声称在此 SHA 重跑完整合同 |
| Scoped clippy | **未完成**；共享磁盘用尽后按根代理要求中断，退出 130，不记为通过 |

最终通过运行的 `run_started.engine.engine_head_observed` 确实为 `4917a43c04f72022240537761eeb785a745f3153`。它从全新、独占的 `target-scale` 构建，使用 Rust 1.95.0、Linux x86_64、`--offline --locked -j2`、dev/test debug=0、incremental=0。输入 SHA-256 清单在 `build-debug/`；驱动本身也记录实际 binary、源码、Cargo.lock 摘要。它们是观察与重放收据，不是独立签署的 release source/binary 证明。

通过运行的 worker 二进制 BLAKE3 前后均为 `ee6931d62a5922b13369036d2a6c590e7761d73f47b8f55f2c452b6412a74f37`；raw BLAKE3 为 `91170f24ba7a1fb7ec1c714737f1c6a19431e626a6c08924ec5132b85640b44f`，625462 字节。测试结束后，独占构建缓存为释放共享磁盘而删除；源码、日志、JSON 与这些摘要保留。

## 原始运行都保留

| 目录 | 原始结果 | 如何解释 |
|---|---|---|
| `baseline-red/` | 4 tests passed、1 failed；驱动退出 1 | 原 main 上，cold/no-op 与 fanout 通过；body/API/config/batch 的完整 oracle 仅 `resolution_manifests` 不同。该失败没有被容忍或删除。 |
| `after-fix-timeout/` | 4 tests passed、1 failed；驱动按 120 秒 deadline 退出 3 | 独立 target 的已完成 cold/no-op/body/API/config 全表相等；在 batch_1/full_control 后触发监督时限。没有 worker-summary，不能算完整规模通过。 |
| `after-fix-passed/` | 6 tests passed；驱动退出 0；9 个样本全部通过 | 显式采用 300 秒的 correctness 集成测试预算，实际 76.823 秒结束；CLI 默认仍是 120 秒，原 release/performance 阈值未动。 |
| `build-debug/` | 共享缓存链接错误、源输入清单与中断的 clippy log | 编译失败发生在 tests 执行之前，不混为 correctness 失败或通过。 |
| `route-reload/` | 两文件最小反例、修复后结果、根代理的测试日志及独立静态审阅收据 | 根代理提供原始执行；本代理按同一 file_path 独立核对 payload 集合差与限定 SQL 修复。 |

每个原始 run 的 plan/raw/report/stderr/summary（如果存在）逐字节复制。`manifest.json` 提供所有交付文件的 SHA-256；目录缺少某个 worker-summary 是原运行被 deadline 终止的事实，不以补造 summary 覆盖。

共享 target 曾出现新 `.rmeta` 与旧 `.rlib` 混合；即使 touch 全 571 个 workspace Rust 文件并仅清 7 个 workspace 包，仍有链接错误。因此最后验收改用完全独立的 target，从第三方依赖开始真实编译。旧错误日志一并保留。

## 60-file 实际内容与正确性

固定 seed 为 12648430。生成器实际写入 60 个文件；驱动再加入一个 tsconfig，并在三个既有 TS 文件加入配置目标见证，所以可见输入清单为 61，实际 DB `files` 为 60。cold 实测为 356 symbols、396 chunks、314 call edges、372 个语法语义关系表记录；test_edges 为实测 0。向量工作明确 `disabled / count=null`，未把缺少 provider 测量伪装成完整 0-vector 基准。

| 主矩阵阶段 | 实际增量 build 数 | 结果 |
|---|---:|---|
| no_op | 1 | added/updated/removed 全为 0；完整 oracle 相等 |
| body | 29 | 真实函数体修改与 bounded resume；完整 oracle 相等 |
| API | 13 | 真实 hub 公开签名变化；完整 oracle 相等 |
| config | 21 | 仅改 tsconfig alias；独立 call-edge truth 目标从首个文件切到第二个，完整 oracle 相等 |
| batch_1 | 1 | 恰好一个文件发生写入，完整 oracle 相等 |
| batch_10 | 1 | 恰好十个文件发生写入，完整 oracle 相等 |

每个阶段的 incremental 与独立 full control 均报告 complete；15 个原 oracle 表先完整比较，再导出摘要，没有采样或弱化 normalization。上述 build 数和 elapsed time 只是实际工作量观察，不是性能优越性或阶段预算通过声明。

独立 fanout=2 fixture 为 3 个文件，1 次增量 build；fanout=8 fixture 为 9 个文件，首轮确实 incomplete，4 次增量后收敛。两者均保留原 mutation_case 完整 canonical 事实与逐调用者手写目标断言。fanout 的全 replay 时间与 IndexReport.elapsed_ms 跨 resume 总和分栏，未虚构缺失的 full rebuild timing。

## 发现的产品缺陷

原重解析 reader 把 routes 表中的解析边 `route:*` 和派生读模型 `route_node:*` 一并当作解析输入。fresh full manifest 只包含原解析边，dirty reload 却再封入派生节点 lookup。最小反例的同文件 `routes.ts` 因此有 13 对 12 条 payload.records：增量恰好多一个 `route_node:72d910926eb59335`、query=`handle`，其余 payload 字段及其余 14 张表相等。

独立生产修复只在 `load_file_edges_for_reresolve` 的输入 SQL 排除 `route_node:*`，未改公开 routes 读写或 oracle。修复后原/派生两条 routes 仍在，真实 consumer 仍解析到 api.ts，完整 canonical 比较通过。此修复防止后续 dirty reload 再污染；它未新增对既有污染 manifest 的无变化 no-op 自动迁移。

## 六项合同覆盖

测试覆盖真实 parser/SQLite/MCP smoke、显式矩阵与 release 准入约束、故意错误 fanout truth 的拒绝、证据预算停止后保留完整 JSONL、deadline 与旧 output 不可覆盖，以及一个真实后代进程在 worker 退出后继续持有 stderr pipe 的回归。最后一项使用 20 秒 sleep，但监督按 250ms 计划完成自身进程组清理；不会以无界 thread join 等待该 pipe。

完整重放命令与配置见 `docs/roadmap/code-index-v2/P8-SCALE.md`。本目录没有生成可信 p95/p99、CI、提速结论或完整发布认证。
