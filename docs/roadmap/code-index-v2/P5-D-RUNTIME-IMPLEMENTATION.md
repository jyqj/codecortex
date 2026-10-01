# P5-D：能力状态、项目生命周期与策略接口子集验收

日期：2026-09-30。仅接受 **P5-016、P5-017、P5-018** 的本地实现、契约、回归范围；P5-019 查询消融与 P5-020/G5 整体验收仍未完成，不宣称 M2 或发行通过。

Git HEAD：`0a56a257f9a92c54d06ea5be0ce1d1763917a527`。冻结工作树摘要：`44b30ae15be8c0ab1cb1fe71c8cb3c5027d4d0085af17678565af89c9483c5a0`；623 个文件、6675050 字节。成果尚未提交或推送，未创建 PR/合并，也未操作日常索引。

## 1. P5-016：能力状态有明确边界

新增 capability_status.rs，status(capabilities).retrieval 区分 no_project、closed、empty、available、error，保留原能力布尔字段。不可用或读取错误时数量为 null，不伪造零。状态读取复用 stats、ReadGeneration、resolution_freshness，并在读取前后核对版本；不触发索引、provider 或网络调用。

local_state 只表示本地路径的可用性或解析欠账，query_coverage 保持 not_measured。未注入端口时 semantic_state=not_configured；宿主注入接口只报告 port_attached_unverified，不据 fake 成功声称真实模型就绪。当前 dense_state=disabled，provider/vector publication 和持久化 semantic epoch 仍未实现。

BuildExplain、GraphExplain、lane receipts、source_freshness、selection、packing 沿用既有唯一来源；status 指向对应返回位置，不建立重复的最后查询结果缓存。执行器统计明确为进程共享，query_pins 是拥有资源的视图租约，不是请求计数。

## 2. P5-017：在途视图、LRU 与清理

QueryHandle 携带共享 QueryPin，克隆视图共用一次租约，最后一个副本退出才释放；通过 runtime 捕获的视图还拥有原 SharedCodeIndex，但不持 RwLock 或读池连接跨 await。请求取消后仍执行的 blocking 工作继续持有租约，清理器不能抢先关闭它。

空闲清理移到 blocking 工作，采用 try_write 与 build_gate.try_lock；锁忙、构建中、视图仍在使用时跳过。已打开的活动/缓存实例走 try-lock 热路径，不因另一项目的冷初始化排队。冷初始化在同会话内串行，owned 登记锁由实际工作线程持有直到发布到缓存；取消等待方不能提前释放锁并创建重复实例。弱引用登记让已被 16 槽 LRU 淘汰、但仍有在途视图的项目复用原运行时与构建门；弱引用不强持数据库。

SessionTasks 统一拥有监听初始化和轮询任务、至多一个空闲循环；关闭阻止重新安装任务，最后所有者 Drop 也终止循环。监听器固定使用捕获的项目，不在 await 后换成另一活动项目；自动索引标记由实际工作线程 RAII 释放。原生通知初始化/析构与已经运行的同步工作不可强制抢占，Tokio 看门狗不等于原生线程终止保证。

开发期受控锁测试先复现了热路由受冷锁阻塞、取消初始化过早释放锁两个错误，再验证修复。 本次恢复轮对原空闲关闭测试真实运行30次，保留2次预期2/实际1的失败；确认首次非阻塞sweep与监听启动短读锁争用。新增受控reader测试证明第一次只关闭无锁实例、释放后全部回收且重复sweep为零；原测试以5秒有界累计所有sweep保留精确2及同实例透明重开，生产锁策略未改。修后40轮共120项idle测试全部通过。旧停止版本 final-v1 保留，不替代当前冻结证据。原生通知订阅在共享 Mac 上测得约 7.5 秒；新增可用性用例由最初 5 秒改为单独 20 秒启动看门狗，失败记录保留。该测试不是启动延迟认证，后置关闭/资源回收的检查没有删掉，既有 debug 索引 500 毫秒门限未改。

## 3. P5-018：兼容的显式策略

search 和 context 各新增一个可选 retrieval_strategy 参数：local/auto/semantic，省略或 null 使用项目配置。参数类型、sanitize、真实 MCP 分发、handler、配置/能力状态、文档示例一体接入；错误名称和非法类型不会静默回退。

local 不调用可选端口；auto 未配置端口时等价 local；semantic 未配置明确报不可用。显式 context 策略不能被旧的直接符号快捷路径绕过。search.mode 仍只有 hybrid/symbol；symbol 只接受省略/null/local，保持原数组响应。十四工具、旧属性、必填项和旧有效请求均保留；不是宣称所有 input schema 字节完全不变。

真实 stdio 执行 MCP_TOOLS.md 中受标记的示例；双工具链新 schema 一致，扣除上述两个可选字段后逐工具与 P5-C 基线契约完全一致。原未知字段拒绝与非法 mode 回归继续通过。

## 4. 同一冻结源码验证

| 工具链 | Workspace/doctest | HTTP | 专项 | 取消协议 | 真实 MCP | watcher |
|---|---|---|---|---|---|---|
| stable | 1786 passed / 60 ignored | 270 passed / 53 ignored | 82 passed / 3 ignored | 1 passed / 0 ignored | 25 passed / 0 ignored | 17 passed / 0 ignored |
| 1.95.0 | 1786 passed / 60 ignored | 270 passed / 53 ignored | 82 passed / 3 ignored | 1 passed / 0 ignored | 25 passed / 0 ignored | 17 passed / 0 ignored |

严格 Clippy、格式、架构守卫、输入锁和 git diff --check 通过。38 条最终命令均核对日志，源码归档和不可变二进制一致；不复用旧全仓测试。忽略项不计通过，重叠测试组不相加为唯一测试总数。计时相关测试组仍串行执行，显式并发用例内部保持并发。

评测过程中，未改动的假后端报告用例停滞，独立 ps 诊断也未在工具期限内返回；连接器保持在线，未将该状态计作测试通过。可选进程采样现用单个准入工作线程隔离 spawn/reap 等待，超时后仍占有名额，避免无限创建线程；新增受控迟到测试验证恢复。本次完整复验显式设置 CODECORTEX_BENCH_PROCESS_PROBE=0，资源记录因此缺少进程树 RSS（null，不是零），原生自身 RSS 单独保留。检索、来源、协议、预算和性能断言均未跳过；该环境条件不是内存性能认证。

## 5. 固定题库与局部成本

沿用同一 51 题、每题三次、两个版本，共 306 次请求；逐题 Top-1/nDCG 零负差分，invalid hit 为零，回放一致。旧 source/intent Partial 与 S11 无答案失败保留，状态计数及 lane 限制原因与 P5-C 相同，没有新增完整性失败。完整检索 gate 仍为 `not_passed`，不能把本子集通过解释成 G5 通过。gold 和评分公式未改。

新增 12 组 release 清理观测：16 个缓存项目、0/4/8/16 个在用视图、每档三次。每次先只清理无租约实例，再释放租约清理剩余实例，验证弱数据库引用全部释放。全部样本保留，不取最好一次。

| 在用视图 | 样本 | 首轮清理中位微秒 | 释放后清理中位微秒 |
|---:|---:|---:|---:|
| 0 | 3 | 16440 | 12 |
| 4 | 3 | 12313 | 4113 |
| 8 | 3 | 8413 | 8235 |
| 16 | 3 | 9 | 17363 |

另复跑 60 次 lane 成本、30 次 P5-C 证据装配和 192 次有界准入请求。这些不是完整独立通道/selector 消融、100k 规模、共享负载吞吐、峰值内存或 p95/p99 认证；它们不能替代 P5-019。

## 6. 证据与下一步

证据：`artifacts/benchmarks/p5d-20260930-resume/final-v3`。归档 SHA-256：`b497cba0c054795da5da5a3b110a03780d5f83b6671c2bb7542408a83f251d71`。包含 validation.json、audit.json、source-review.json、additive-contract.json、paired/summary.json、lifecycle-cost-summary.json、源码清单与归档。

权威任务：**118 done / 74 todo，P5 为 18/20，下一 P5-019**。P5-D 整批仍未完成。后续须完成真正的质量/成本/并发消融与本地增强版整体验收，明确处置已知 Partial、S11 与性能边界，不能修改 gold、压掉 Partial 或以 fake 冒充真实语义就绪。
