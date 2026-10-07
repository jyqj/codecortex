# P8-007 / P8-010：有界本地混合负载与连续修改

本轮增加 `cc-eval-p8-load` 开发工具，复用现有 `CodeIndexBackend` 的真实
in-process MCP dispatch、`Mutation` 和 `oracle::canonical`。它提供
P8-007、P8-010 的可运行本地工程能力，两项原任务仍需后续证据推进。
这里没有声明长期稳态、100k、真实模型 backfill 或完整 V20 通过。

## 使用方式

```sh
# 输出默认的严格 JSON 配置；需要更长或更高并发的本地观测时修改此配置。
cargo run --offline --locked -p cc-eval --bin cc-eval-p8-load -- config

# 默认 C4、24 个操作、12 个文件的短 smoke；输出目录必须尚不存在。
cargo run --offline --locked -p cc-eval --bin cc-eval-p8-load -- run --output /tmp/p8-load-new

# 使用自定义但仍受上限约束的配置。
cargo run --offline --locked -p cc-eval --bin cc-eval-p8-load -- run --config /tmp/p8-load-config.json --output /tmp/p8-load-custom
```

配置支持 C1/4/8/16；队列容量 1–256；操作数 2–10,000；fixture 文件数
2–256；单轮总 deadline 最大 1 小时；raw JSON 工件预算最大 64 MiB。
默认单请求 deadline 为 10 秒、进程总 deadline 为 30 秒、取消 drain
期限为 5 秒。配置不包含真实仓库、任意工具、网络地址、provider 或凭据。
只生成自有 fixture，项目配置明确关闭 auto index 和 semantic provider。
supervisor 用绝对路径启动本工具的 worker，并将子进程 PATH 指向本次输出中
专门创建的空目录；worker 验证此环境。因此 Indexer 中按名称启动的可选
本地 git 命令不可用，也不会探测输出目录祖先仓库的 git history。
这不是操作系统网络隔离声明；本配置不调用外部 provider。
子进程也清除继承的 `CODECORTEX_*` 环境变量，worker 再次校验：尤其不能
让 `CODECORTEX_CACHE_DIR` 将 IndexPaths 指向自有 fixture 之外的缓存目录。
实际 live/full 数据库位于各自 fixture 的 `.codecortex/index.sqlite3`。

普通 CLI 与产品工具接口保持不变。新工具的 supervisor 启动的是这个
评测 binary 的 worker 子进程，worker 内部通过既有 in-process duplex
MCP 调用产品逻辑。因此它不是当前产品 binary 的外部 stdio L3 认证。

## P8-007：并发、offer 与排队

固定大小 worker 池共享一个长期 MCP backend/session。每三个计划操作中
一个是修改后增量构建，其余是实际符号查询。写操作额外串行化
“修改源码 + index”，读取可以与构建重叠；C1 是串行混合基线，C4/8/16
允许真实并发。报告提供实际 backend 调用峰值及是否观测到 read/build
时间区间重叠，不仅记录配置的线程数。

offer 按预定单调时间生成，不等待上一请求完成。队列满时明确记录
`queue_rejected`，不会通过降低完成驱动的供给量隐藏过载。记录包括：

- scheduled、offered、admitted、started、finished 单调时间。
- dispatch lag、queue、service、端到端时长。
- worker 获得写锁前的额外等待；build、query 的实际调用开始与结束。
- success、error、timeout、cancelled、queue_rejected 全部分母。
- 计划数、实际 offer 数、未 offer 数、缺失终态行、队列容量及观测峰值。

build 的 freshness 不完整或存在 parse error 会被记录成失败。
不同请求的 JSON 原始响应不因过期而丢弃；晚完成响应保留，并仍计 timeout。
所有时长均为观测，短 smoke 或 debug 的 p95 不能声明为稳定性能结果。

## deadline 与取消边界

既有 `CodeIndexBackend::call_tool` 是同步调用。新工具没有宣称能中途
安全终止这个调用：排队中已过 deadline 的任务不会启动；运行中的调用
完成后按实际时长判定是否超时，保留 late result。单请求 deadline 是
观测与准入边界，不是已证明的产品请求抢占取消能力。

整个 worker 由独立 supervisor 的进程 deadline 兜底。Ctrl-C 或
`cancel_after_ms` 注入会要求停止 offer，取消尚未执行的工作，等待已开始
的操作 drain。到总 deadline 或取消 drain 上限仍未退出时，supervisor
终止并回收 worker 进程，使其中的 Rust/backend 线程一并结束；不会把
阻塞线程遗留在评测进程中继续修改已删除的临时目录。

此监督边界只针对本工具自身的 worker：已用到的产品路径关闭自动 watcher、
semantic provider 及可选本地命令。`supervise` 不是任意 executable 的通用
进程树沙箱。worker 退出后回收 stderr reader，前提是这个可信 worker 没有
启动持有 stderr pipe 的后代进程。

输出目录不会复用或截断。每条 raw 事件写入后 flush；异常退出、工件预算
耗尽、超时、取消保留已有前缀及非零 supervisor 收据。强制终止可能留下
不完整的最后一条 JSONL，不能把此时的前缀误当完整全样本结果。

`worker.log` 使用独立 reader，最多保存 **64 KiB 前缀**；后续内容继续读取
并丢弃，supervisor 发现超限即停止 worker，写入 invalid measurement / 2。
收据列出 limit、observed、retained、dropped、是否读到 EOF 及 IO 错误。
此日志上限独立于 raw JSON 预算；配置/manifest/supervisor 控制收据和自有
fixture/index 目录也不计入 raw JSON 预算，后者受文件数/修改大小上限约束。

二进制绑定包括 supervisor 启动前 hash、worker 自行 hash `current_exe()`
并与 manifest 比较、supervisor 收尾再次 hash。同模块源码但依赖不同的
binary 不能靠模块 digest 混过 worker 校验；binary 收尾变化或缺失时即使
worker 原本成功也输出 invalid measurement / 2，原始工件仍保留。

## P8-010：连续修改与停写对账

执行过的写操作循环包含：正文改写、新增文件、rename、删除文件、API
改写、恢复 API。使用现有 `Mutation` 校验并作用于独立 fixture。
修改覆盖现有文件内容，文件数量和单文件大小有界，不依靠无限追加内容
制造“持续运行”。backend/session 在整个负载期间复用。

停止 offer 后关闭并排空队列，等待所有 worker 退出，再复制最终源码给
独立 fresh-full backend。**比较前不对增量侧补一次完整重建或修复**，以免
掩盖遗漏修改。随后执行既有 canonical oracle 的全部表、integrity/FK
检查，并对三个公开查询核对结果位置。

任何差异都保留完整 incremental/full canonical JSON、差异表名与原始查询
响应，并输出 failed 和非零退出码。真实产品差异与 harness 控制测试结果
分开：测试可以证明“失败被正确保留”，不能因为 harness 测试通过就把
实际 oracle 差异改成成功。

真实 git 分支切换、catalog 压实、semantic backfill、长期 RSS 趋势、
100k 与 release performance certificate 在本工具中明确列为 `not_run`。
队列的工程上限也不能代替长期内存无增长的观测。

## 工件与退出码

| 工件 | 内容 |
|---|---|
| `manifest.json` / `config.json` | 作用域、原始配置、编译模块摘要、binary 摘要、debug/release 标签 |
| `worker-binary.json` | worker 自行读取的执行文件摘要及其与 manifest 的比较 |
| `events.jsonl` | offer、执行、mutation、原始响应、终态、停写 drain 的追加事件 |
| `initial-build.json` / `full-build.json` | 真实构建原始报告 |
| `incremental-final.json` / `full-final.json` | 既有完整 canonical 表输出 |
| `reconciliation.json` | 差异表、完整原始文件引用、公开查询原始响应 |
| `worker-summary.json` | 分母、各类终态、并发/队列观测、完整对账与 not_run 范围 |
| `supervisor.json` / `worker.log` | 子进程退出、超时/取消/强制停止、最终 binary 绑定和有界失败日志 |
| `live-worktree` / `full-worktree` | 本次自有且有界的合成源码及索引，用于检查；不碰用户工作区 |

0 表示本地观测完整且本轮确定性对账相等，仍不是性能或发行认证；1 表示
有效运行中发现请求失败、丢失/拒绝或产品 oracle 差异；2 表示配置、工件、
初始化、进程总 deadline 或其他基础设施使测量无效；3 表示取消。

## 验证与环境限制

新增测试分别验证配置边界、真实 C1/C4/C8/C16 混合操作与对账保真、queue burst 的
拒绝和 deadline、取消后停写对账、进程总 deadline，以及工件预算与拒绝
覆盖已有输出。额外负例核验 worker 的 binary manifest 不匹配、worker 自检
之后 executable 被删除，以及 stderr 前缀上限/写入失败。
另一个独立 CLI 负例通过子进程环境注入外部缓存位置，验证该目录的 sentinel
和条目保持不变、两个真实索引都留在自有 fixture。命令：

```sh
cargo test --offline --locked -p cc-eval --test p8_load -j 2 -- --test-threads=1
cargo test --offline --locked -p cc-eval --lib benchmark::p8_load::stderr_tests -j 2 -- --test-threads=1
cargo clippy --offline --locked -p cc-eval --lib --bin cc-eval-p8-load --test p8_load -j 2 -- -D warnings
```

上一轮 native sampler 的环境诊断使用独立 Python 进程复现了 PID 与可见
`/proc` 不一致：父、子都自行报告的 PID，对应 `/proc/<pid>/stat` 却显示
不同程序名和 RSS。因此本环境不能凭这类 PID 读取声明进程归属认证。
这与共享 Cargo target 的跨 worktree 旧库复用是两个问题；前者不依赖
Rust 或 Cargo 即可重现。新工具的 RSS 为 null，明确等待受控资源 profile。

本轮发现共享 target 的跨 worktree `.rmeta` / `.rlib` 不一致，刷新源码 mtime
和按包清理仍未恢复，因此改用本工作分支全新的独立 `target-load` 重建。
这避免把其它 checkout 的构建产物当成本轮证据；并行编译期间的短计时
依然不能当作真实性能数据。
