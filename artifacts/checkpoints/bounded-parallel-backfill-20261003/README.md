# 有限并行 backfill — 单一候选，等待父审

固定源码 [`5cce6eb3af90b79c786d30f349ca6a674dfcd4a1`](https://github.com/jyqj/codecortex/commit/5cce6eb3af90b79c786d30f349ca6a674dfcd4a1) 已推送并以 `git ls-remote` 核验；分支 `perf/bounded-parallel-backfill-20261003`。精确实验 base PR114 `52a50730b58e2b59351449fc80f65771b24c26a8`，不是生产接受；canonical PR113 未改。原 checkout ff458bc 未改，候选隔离在 `/workspace/bounded-parallel`。

## 实现契约

单一 project coordinator、cursor、running gate、64轮/30秒有限 job、outer pin/permit/factory 生命周期不变。每个 drain 恰有1或2个 scoped worker，显式现有 `max_concurrent_per_project>=2` 才是2，其余包括0是1。总 claim budget 仍16，不是每 worker16。短 admission mutex 只线性化 claim：FIFO claim 顺序不变，完成顺序明确允许并行；每个 task 独立 lease/token，经原 renew、hand-back/retry 和 publish CAS。DB连接、事务和 admission mutex 均在 provider调用/join 前释放。所有worker物理join后才返回，outerpin/Running/permit/factory保留到真实退出。

取消、普通空间变更、错误停止后续claim；未started的pre-work取消/错误按原 hand-back不耗attempt，started按原 fence/retry。并行 handler错误全部join后返回（多路错误保留在聚合文本），provider失败按原 NeedsRetry落账。degrade计数原子聚合并在错误后也转写共享ledger；workerstatus仍由原协调器处理。旧 FnMut 串行 queue API和公开 drain_worker_batch 保留。

没有更改 cache/layout/fsync/GC/WAL/DDL/FIFO SQL/版本/DEV/中央 TODO/direct_writer；三个DB production文件与PR114及冻结807f471逐字节一致。原 Publisher/CAS/admission/service_factory/Cargo锁定输入均字节不变。

## 限定验证（真实范围）

- 8个新queue synthetic +9个原serial consumer tests passed；新fixture明确自有 `/tmp` 可写路径。
- 3个新runtime tests passed：默认未配置不查cache root、0calls；physical close期间pin=1/Running=true，join后pin=0；factory原blocking线程销毁；邻project可调用。factory生命周期测试使用真实 AdmittedProvider + **局部注入真实 gate**；纯 InjectedProvider/Fake没有自动gate，不能作为共享gate证明。
- 实际宽度0/1→peak1，2/4→peak2；共享claim16、独立16token、无重复、原CAS；ordinary close、preclosed/unstarted、provider retry、两路error、space switch、doc edit 均通过。
- 真正 AdmittedProvider decorator配局部ProviderGate能实现global4/perproject2，并阻止被拒调用进入Fake；这**不等于**进程共享first-wins gate兑现配置。
- 真实rustfmt四个修改Rust文件检查通过；cc-semantic/cc-server all-target semantic-http Clippy `-D warnings`通过。不是全workspace test/GC/WAL/heldout通过声明。

### 共享 gate 真实阻断（未修复，不吞断言）

正常先assemble一个 max_concurrent=0项目，recall.health lazy getter初始化permissive共享gate；随后正常assemble 4+2项目，OnceLock first-wins仍是 `usize::MAX/None`。独立进程 + 真正process共享gate + AdmittedProvider已复现global5/perproject3，其中B为2个doc attempts+1个query，C为2个doc attempts。日志 `shared-gate-doc-query-counterexample.log` / `shared-gate-isolated-counterexample-final.log`。这是**约束失败反例**，测试返回0只表示成功观察反例，不表示上限合约通过。该测试默认ignored，仅精确单独 `--ignored --test-threads=1`执行，防止其它tests提前初始化singleton污染顺序；没有删除原4/2上限断言。

最小后续边界见 MINIMUM-GATE-BOUNDARY.md。当前保留既有first-wins政策，未替换singleton/permit、未添加semaphore。这个问题使本候选尚不能声明完成“真实共享gate上限”承诺，需要父亲审定后续guard政策，不能生产接受。

### 空claim/partial续调

queue `let Some(task)...else{break}`仅退出本worker，不写stopped，不增加count；`*count+=1`只在成功claim之后。另一worker在flight时可正常继续claim。新普通enqueue fixture初始1条、provider有限等待时enqueue1条，最终claimed=completed=2（小于16）、ready stranded=0、attempt均1。日志 `normal-enqueue-validation.log`。本测试与代码路径证明未新增“None停止同伴/空claim消耗预算”的问题，不宣称解决既有任意未来retry调度。

原runtime续调谓词 `backfilling || batch.claimed==16`未改；round后仍检查requested，真实job退出后仍swap requested再schedule，因此normal build/request在partial轮尾到达仍由既有coalescer保全；任意裸DB enqueue在idle时仍需要composition root调度，未造常驻timer或广改retry调度。

## 预注册有限 AB（两对全部ready）

顺序严格为base1k/5k→candidate1k/5k→candidate-repeat1k/5k→base-repeat1k/5k。全部实际release real MCP stdio→原pipeline→正常首次响应loopback fake HTTP→128dims→durable put/readback→原CAS，fresh独立repo/cache。原config4+2、claim16、parse4、poll.2、ready或90s终止规则未改；没有provider hold，没有测量中并行build。两binary共用相同临时timing probes及实际AdmittedProvider调用peak计数（不是HTTP返回日志区间猜测）。probes只在scratch构建，未进入production。

| 运行 | n | ready s | claim s（调用累计） | provider peak | 物理写 MB | root RSS HWM MB |
| --- | ---: | ---: | ---: | ---: | ---: | ---: |
| baseline | 1000 | 3.424 | 0.071 | 1 | 135.55 | 54.08 |
| baseline | 5000 | 15.353 | 0.396 | 1 | 700.53 | 156.24 |
| candidate | 1000 | 2.620 | 0.096 | 2 | 136.11 | 52.80 |
| candidate | 5000 | 11.723 | 0.449 | 2 | 700.89 | 153.82 |
| candidate-repeat | 1000 | 2.215 | 0.084 | 2 | 135.97 | 52.45 |
| candidate-repeat | 5000 | 11.935 | 0.481 | 2 | 702.27 | 156.54 |
| baseline-repeat | 1000 | 3.222 | 0.058 | 1 | 135.56 | 53.58 |
| baseline-repeat | 5000 | 14.552 | 0.394 | 1 | 700.63 | 158.36 |

1k ready观察变化-23.49%/-31.26%；5k -23.64%/-17.98%。**仅两对synthetic观察，不作统计显著性、因果独占、正式100k或生产接受结论。** claim累计时间反而略增；并行durable-put/provider跨度互相重叠，不能把累计跨度当wall-time分解。5k物理写+0.051%/+0.234%，不能宣称消除cache/fsync成本。

全部8运行EOF exit0、v2 ready、n done/attempts/doc+semantic manifests、无pending/failed、integrity/FK通过。输入字节、排序后的真实HTTPbody hash、normalize ephemeral port后的config完全相同；HTTP调用n且batch1。/proc IO包括启动/index/drain/status/probes，read_bytes=0可能是pagecache，不是无读；RSS只报owned root，完整tree unknown。完整压缩RPC/HTTP/resources/input/phase logs、原运行顺序/噪声/首次失败保留。二进制/源码SHA与probe验证见identity.json/instrumentation-verification.json。

父追加first-wins/normal-enqueue验证仅改cfg(test)与integration fixture；AB后production代码字节未改，identity.json保留原AB文件hash及后续test-only身份，并核runtime production prefix等同实测source。当前测量是fresh进程先显式4/2的单项目路径，不证明混合first-wins顺序或全部provider限流。

## 原失败与环境记录

PR116 `9a4f97877f1b18af77013f7bed7cd047994e26f3`正式100k单pair300s仍均未ready：25081/48809；cleanup尾段另列、原压缩失败日志留在PR116。旧PR114的5k index-alone ready回归/IO+4.4%保留，未借本次并行结果洗掉。只读receipt在pr116-prior-failure-receipt.json。

初始rustup代理写 `/home/agent/.rustup` EROFS及第二次cargo-fmt选代理的失败全部保留；没有重试初始化、改HOME/RUSTUP_HOME、chmod/remount或安装toolchain。父随后明确授权已存在真实官方binary路径的独立执行。实际Cargo依赖缺reqwest后按授权从官方registry获取，Cargo.lock未改。真实rustfmt/Clippy路径直接执行且成功。原编译错误（不存在tracing/space字段）、新fixture列名/FromSql错误、Clippy错误原日志保留，未伪造初次通过。

交付仍是draft review候选；没有Forbidden/自动审批拒绝、真实provider/heldout、GC/WAL kill/crash/fault、正式100k、merge/forcepush/deploy。本轮在证据和draft交付后停止，共享gate修复/生产决定归父。
