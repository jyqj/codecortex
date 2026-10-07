# P8-011：有限子进程故障与恢复证据

本轮为原始任务 **P8-011「端到端故障与恢复认证」** 增加可执行子矩阵、
真实 MCP stdio 故障工件和独立负控。原任务依赖 `P7-020`、`P8-007`、
`P8-010`，并要求 kill、断网、缓存损坏、数据库忙和换库的完整认证；本轮
仅推进其中可以真实观测的本地部分。回执固定保留 `complete_P8_011=false`
与 `release_certified=false`，不能据此关闭原任务或认证 G8。

实现为 `scripts/p8_recovery.py`，复用 `p8_rollback.Product` 和已有
`resource_harness.runtime.StdioRPC`。没有第二套 MCP 协议实现，也没有修改
production crate、历史测试、任务定义、CI 或 source registry。

## 当前候选的实测结果

`artifacts/checkpoints/p8-next-ten-20261008/recovery/recovery-receipt.json`
记录 **3 passed / 0 failed / 4 not_run**，命令 exit 0。五次真实产品启动
全部在自建、独占项目中进行；四次正常退出 0，一次按明确预期被 SIGKILL
终止并退出 -9。外部网络拒绝 wrapper 的原始文件、SHA256 与每次进程的
IPv4/IPv6/继承子进程探针均留档。

| 子项 | 实际注入和恢复 | 成功所需证据 |
| --- | --- | --- |
| `kill_restart` | 已提交索引后 SIGSTOP；通过所拥有子进程的 `waitpid(WUNTRACED)` 观察停止；修改源码并发出 pending 索引请求，再 SIGKILL；新进程完整重建 | exit -9；原 pending 请求收到真实进程或管道终止错误；新符号返回真实源码行；SQLite/FK 完整性通过 |
| `database_busy` | 对实际 `.codecortex/index.sqlite3` 持有 `BEGIN IMMEDIATE`；第二连接确认 `SQLITE_BUSY`；修改源码并请求增量索引；解除锁后有界重试 | 持锁窗口无新符号提交，请求保持 pending 或明确 busy；解锁后公开检索和持久化符号均存在 |
| `deleted_source` | 同进程两次成功预热查询后删除源码；增量索引；关闭并重启，关闭 auto-index 且不显式调用 index | 删除符号及其路径不出现在公开结果，持久化表计数为 0；重启后仍不出现，源码文件未被重建，保留符号仍可检索 |

当前候选的数据库忙窗口实测持锁 **5.321 秒**（预算 6 秒），第二连接
返回错误码 5；产品原始响应为 JSON-RPC `-32603` / `database is locked`，
持锁时新符号表计数为 0。解锁后成功增量索引并通过源码行、公开能力、
SQLite integrity 和 foreign-key 检查。WAL 读者仍可观察旧快照；脚本独立
检查新版本是否真正提交，不能用旧快照可读冒充更新成功。

每次成功查询都验证结果数组中的符号名、项目内相对路径、真实文件、合法
起止行及该行的函数定义，不能用响应中回显的查询字符串代替命中。
能力检查要求本地索引可用、identity 在 observation boundary 已检查、
transport 未终止；同时要求 semantic=`not_configured`、dense=`disabled`、
全局 query coverage=`not_measured`。禁用配置不会被解释为语义服务 ready。

## 产品与源码身份

当前产品为 default 包，源码提交
`78ae91eeae6edae6bea29c27f24b251773341c00`，共 **785 个 Cargo/crate 输入**。

- 二进制 SHA256：`4cb5a38d47acb9999afb5cb9d7d42cde33ee1362a825676ecf0d58dfbc67e9a2`。
- 源码 manifest SHA256：`229acb8d2e7bed3f92c715f02ac5a22fed00b9baac04cfde6d06e1e442559db3`。
- 实际构建命令：`cargo build -p cc-server --bin codecortex --locked --offline --message-format=json-render-diagnostics`。
- 实际 compiler artifact `fresh=false`、features `[]`。构建复用了此前测试依赖，
  命令没有 `--no-default-features`，因此本次身份材料是独立的
  `post_build_source_and_binary_witness`，明确 **不认证冷构建**。

`--engineering-witness` 路径验证 witness 类型、真实退出码、包/features、
二进制摘要、唯一成功 compiler artifact、两份构建日志、完整 manifest
及每项源码相对固定 Git blob 的字节一致性。它不将构建后的证据宣称为
编译器证明。原 `--build-receipt` 严格验证路径继续保留；负控确保这种
engineering witness 无法通过原 cold receipt 路径。

归档也保留源码 `a213a4cfab0e0e87198bc279acacda51e4ff62cf` 的旧 default
候选演练。旧候选二进制 SHA256 为
`08a651297e39ee066ca9c016e80781deaed5f866e3aedc7846795d378cd3a695`。
旧、新产品的各次回执分别绑定自己的源码与二进制，没有把旧包归属到当前
runner checkout，也没有把这两种身份材料混成冷构建平台矩阵。

## 原始失败与观测修复

`old-candidate-01` 原始 exit 1 / 1 passed / 2 failed 被完整保留。
两项失败均在保持原恢复 oracle 的前提下定位并修复：

1. 该执行器的 Popen 子进程 PID 视图与挂载的 `/proc` 视图不同，按 PID
   读取 `/proc` 或 `ps` 没有观察到所拥有子进程的停止。脚本改为消费真正
   所拥有子进程的 `waitpid` 停止事件，并检查停止信号就是 SIGSTOP。
   最终 kill receipt 保存该观察方法及 pending 请求和退出结果。
2. 原 source manifest 先读取整个项目再过滤派生缓存；在 Python 已持有
   SQLite WAL 锁时，读取并关闭 `index.sqlite3-shm` 干扰了锁注入。
   独立 Python 子进程锁探针记录：读主数据库前后均 busy，读 `-shm`
   后另一个写者不再 busy。脚本改为在读取前排除整个 `.codecortex`；
   新负控确认主数据库、`-wal`、`-shm` 都不会被 source manifest 哈希。

`lock-observer-attribution.json` 与 `wal-lock-observer-attribution.json`
保留探针原始结果，包括锁消失后的 WAL 文件缺失。修复后同一旧包的
`old-candidate-02` 与当前候选 `current-candidate-01` 均为 3 passed。
第一轮失败并未改写成通过，也没有归因为未经证明的产品 bug。

## 尚未执行的范围

| 子项 | `not_run` 原因 |
| --- | --- |
| `transaction_internal_crash_points` | SIGKILL 注入点是进程已停止、RPC pending，尚无事务内部阶段 hook |
| `active_provider_network_disconnect` | 本轮为 disabled/offline 配置，未启用真实 provider，无法证明活跃请求断网后的恢复与费用 |
| `active_semantic_cache_corruption` | disabled 配置不读取语义向量，没有活跃 cache reader 的损坏注入证据 |
| `concurrent_database_replacement` | 本有限子矩阵未注入并发数据库 incarnation 替换 |

上述四项仍需相应真实产品、注入时点与原始工件。禁网探针证明演练进程的
网络限制，不代表启用 provider 后的故障恢复、费用或语义认证已完成。

## 复跑与审计

每次使用从未存在的 output 目录；脚本拒绝复用旧目录。协议调用及观察
均设有有限 timeout，busy 重试最多 3 次。失败场景留下独立 receipt、
阶段事件、stdout RPC、stderr、退出及网络回执，其他场景继续执行；取消
会把尚未开始的场景标为 `not_run`。退出 0 只表示上述三个有限场景通过。

```sh
# 使用具备完整严格 build receipt 的历史产品。
python3 scripts/p8_recovery.py \
  --binary /absolute/product/codecortex \
  --build-receipt /absolute/product/build-receipt.json \
  --package-kind default \
  --output-dir /absolute/new-recovery-run \
  --deny-network-wrapper /absolute/deny_network_exec.py

# 当前产品可使用独立、明确非 cold 的 engineering witness。
python3 scripts/p8_recovery.py \
  --binary /absolute/current/codecortex \
  --engineering-witness /absolute/evidence/codecortex-build-witness.json \
  --package-kind default \
  --output-dir /absolute/new-current-recovery-run \
  --deny-network-wrapper /absolute/deny_network_exec.py

python3 -m unittest discover -s scripts/tests -p test_p8_recovery.py -v
python3 -m unittest discover -s scripts/tests -p test_p8_platform.py -v
```

本轮 20 个恢复合同测试通过，覆盖错误命中/路径/行号、假 ready、错误 kill
终态、busy 重试界限、源码缓存排除、witness 漂移与失败/取消 rollup 等
正负对照；此前 30 个平台合同测试回归通过。这两个测试组验证脚本合同，
实际产品行为由上面的真实子进程记录单独证明。

归档入口为 `recovery/README.md` 与 `recovery/evidence-index.json`。
后者记录实际命令退出、runner 摘要、逐工件摘要与压缩档案摘要；档案包含
全部三次演练及原始失败，不复制可再生产品 target。
