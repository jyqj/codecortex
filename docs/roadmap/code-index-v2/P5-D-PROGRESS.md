# P5-D 运行时与策略接口进展

## 2026-09-30 Git进度快照复核

final-v3已结束为`failed`，不是仍在运行或已经验收。14条命令收据中，stable workspace为1782 passed / 0 failed / 59 ignored；Rust 1.95 workspace退出101，为1781 passed / 1 failed / 59 ignored。失败项`project_session::tests::close_idle_instances_closes_cached_non_active_projects`预期关闭2个实例、实际1个。独立`audit.json`没有生成，因此P5-016～018仍为`in_progress`，P5-019/020仍为`todo`。

本次保存前重新核验了冻结清单中622个文件及14条命令的日志SHA-256，均一致；该核验不是重跑完整测试，也不改变原失败结论。原始轻量收据复制到`artifacts/checkpoints/20260930-git-sync/p5d-final-v3/`随源码一起版本化；大型实验工作区、二进制与导出包保留本地。后续先定位并复验失败，再进行独立审计和任务收口，详见[CHECKPOINT-2026-09-30.md](CHECKPOINT-2026-09-30.md)。

## 以下为此前实施过程与验收计划

本轮接续 P5-C，当前实现范围为 P5-016～018，不包含 P5-019 的完整质量/成本/并发消融或 P5-020/G5。最终通过以前，tasks.json 保持 115 done / 3 in_progress / 74 todo，不把开发期通过当作完成。

## 实现

能力状态区分空库/关闭/错误、解析欠账、可选端口是否注入，dense 明确 disabled，覆盖仍由每次查询报告。QueryHandle 拥有共享租约和原 runtime；空闲清理跳过在用/构建/锁竞争实例。弱登记允许 LRU 淘汰后复用在途实例。已打开缓存走无冷锁的快路径，冷初始化锁移入工作线程直到发布，避免取消等待方造成重复实例。监听初始化/轮询及空闲循环由统一生命周期所有者持有。

search/context 新增可选 retrieval_strategy，完成 schema、sanitize、handler、status 和文档示例接线；显式 context 策略不被直接符号快捷路径绕过。14 工具旧模式及既有参数保持，新增参数不是另建 provider 或向量引擎。

## 开发验证与保留的问题

运行时 10 项、文档参数 1 项、实际 stdio 新旧契约、既有 project_session 测试已实际运行通过。热路由被冷登记锁牵连、取消初始化过早释放锁两个问题均有失败先行和修复后的通过记录；活动实例重开锁等待有异步定时器回归。

新增监听启动测试最初 5 秒等待失败，隔离观察确认原生订阅在共享 Mac 上约 7.5 秒；保留失败，改用 20 秒启动可用性看门狗，不把它当成延迟指标。最终关闭/资源释放断言没有删除，既有索引 debug 500 毫秒性能门限未改。

final-v1 因独立代码审阅发现热/冷路径与取消缺口主动停止；final-v2 在未改动的假后端报告测试中停滞，独立 ps 诊断同样未返回，工具将后者记为 lost。连接器在线，未确认具体内核原因，没有把停滞计为通过。可选采样现在由一个准入工作线程隔离 spawn/reap 等待，超时后保留名额，防止无限堆积。受控迟到测试与原报告 11 项通过。

最终复验使用 CODECORTEX_BENCH_PROCESS_PROBE=0，进程树 RSS 因而为明确 null，原生自身 RSS 单独记录。核心检索、来源、协议、预算、回放及成本测试照常执行；内存性能不因此获得认证。

## 当前冻结与证据

目录：`artifacts/benchmarks/p5d-20260930-runtime/`。当前验收尝试为 `final-v3`，工作树 622 文件、6,655,806 字节，摘要 `ef63da5b557224f04bc1f6a79b4666221fdf96c17fbd625f8fdc6f24f41a4f5b`。旧停止/失败日志保留，当前接受状态必须以 validation.json 和独立 audit.json 为准。

验收通过后才写入 P5-D-RUNTIME-GATE.json 与 P5-D-RUNTIME-IMPLEMENTATION.md，并把三个任务标 done、下一项改为 P5-019。原 source/intent Partial 和 S11 无答案失败必须原样保留；G5/M2、100k、真实 provider、跨平台和发行均不在本轮完成声明内。

此前实施轮未提交或推送。本次按用户授权保存当前Git开发快照，不创建PR或发行标签，不操作日常索引；提交与远端同步结果以Git历史核验为准。
