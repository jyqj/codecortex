# 查询视图、项目复用与会话后台任务

P5-D 接续原 QueryHandle，不另建索引引擎。`CodeIndex::query_handle` 获取一份 QueryPin；视图克隆共享同一租约，最后一个克隆退出才减少计数。通过运行时捕获的视图额外拥有对应 `SharedCodeIndex`，不持 RwLock 或数据库连接跨 await；同步直接构造的视图仍独立拥有 DB/engine。

## 空闲清理与 LRU

已打开的缓存命中和活动实例检查采用 try-lock 快路径，不等待冷初始化登记，也不为读取一个现存实例创建 blocking 任务。活动实例遇到生命周期锁竞争或需要重开时同样移至 blocking 工作，异步调度器仍能处理取消和定时器。冷路径的 owned 登记锁由实际初始化工作线程持有，直到实例进入缓存才释放；取消等待者不会提前解锁并允许另一个初始化器创建第二份运行时。该正确性以受控锁竞争回归验证，不用重试碰巧通过。

清理在阻塞工作线程中遍历最多一个活动实例加 16 个 LRU 槽。只尝试取得 CodeIndex 写锁及构建门，锁忙、查询视图存在或正在构建则跳过，不在异步调度线程等待写锁。显式 `CodeIndex::close` 的旧语义保留；它不是清理器的无条件替代。

同一路径在 LRU 与活动路由之间复用一份运行时。弱引用登记不强持资源，并发冷缺失在同一锁内重新检查后仅初始化一次；初始化 I/O 在 blocking 工作中执行；缓存实例重开沿用短生命周期写锁，不在 async 调度线程等待它，也不持锁跨网络调用。LRU 被淘汰但在途视图仍拥有运行时时，新调用通过弱引用恢复同一实例和 build gate。失效弱项在后续访问时清理，最后所有者释放即允许回收。直接外部创建第二份 CodeIndex 的行为不受 ProjectSession 登记约束。

## 后台任务

每会话一份 SessionTasks，追踪监听器的初始化及整个轮询任务，以及至多一个空闲循环。替换监听器先中止旧任务，不再存在未跟踪初始化任务在 shutdown 后安装新轮询的窗口。每个监听器固定使用订阅时的项目，不在 await 后改读另一活动项目。

停止会话会禁止新后台任务，abort 两个循环并在两秒看门狗内等候它们退出；最后 SessionTasks 所有者 Drop 也会 abort。已启动的 blocking 构建由事务机制自然退出，不把 future 取消当成线程终止。监听构建的自动索引标记由工作线程中的 RAII permit 释放，不能因等待方被取消而泄漏或提前释放。

## 能力与诊断

`status` 复用数据库现有 stats、ReadGeneration 和 resolution_freshness，在读取前后核对代际。损坏元数据是 error，索引不可读时统计为 null，不使用零伪造结果。无可选端口为 not_configured；注入 fake/自定义接口只表示 port_attached_unverified，不能推断真实 provider 或 dense 已就绪。覆盖在每次查询中报告，不能从 has_index 或 ready 推断。

旧 BuildExplain、GraphExplain、lane receipts、source_freshness、selection、packing 各自保持唯一来源；状态返回它们的观测位置，不再维护另一份“最后成功查询”缓存。

原生通知初始化和既有 FileWatcher 的 native/thread 清理不受查询 deadline 强制抢占；Tokio 的关闭看门狗是协作等待，不代表可以终止阻塞线程或原生析构。共享主机曾测得通知初始化约 7.5 秒，启动可用性测试因此单独使用 20 秒看门狗，不拿该测试作延迟认证。既有 HTTP/debug 索引 500 毫秒门限保持不变。

## 验证范围

`p5d_runtime` 使用实际 ProjectSession/parser/SQLite，验证 8 个并发冷路由同实例、18 个项目压力下的慢 fake 查询存活、busy 锁跳过、最后资源回收、会话循环停止及能力状态。fake 只验证编排；这些机制测试不等于 100k 文件、吞吐或尾延迟认证。P5-019 的质量/成本/并发消融以及 P5-020/G5 的整体验收仍需独立完成。
