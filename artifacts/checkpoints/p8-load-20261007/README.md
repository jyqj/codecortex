# P8-007 / P8-010：本地 mixed-load smoke 的原始证据

这份证据绑定源提交 `25399fc51bb004885c18e8ba13fd1aae9614be17`，由独立
`target-load` 中的 debug binary 实际执行。其父提交含 route reload 修复，
是原提交 `462f74800670e5a9e29fdcd0eb52954c2aea2c62` 的 cherry-pick。
本目录不代表后续组合源、P7 前置条件、P8 整项或发行性能验收已经通过。

## 实际运行

每组生成 12 个 Python 文件，offer 36 个操作：12 次修改及增量构建、
24 次符号查询。12 次修改完整执行两轮“正文改写、新增、rename、删除、
API 改写、恢复 API”。初始化与停写后的独立 full 构建及探针不计入这
36 个工作负载操作的分母。供给间隔 5 ms，队列容量 64，请求 deadline
10 秒，总 deadline 30 秒；完整配置在各组 `config.json`。

| 配置并发 | offer / 终态 / success | 实际修改 | 队列峰值 | 实际 backend 调用峰值 | read/build 重叠 | 最终对账 |
|---|---:|---:|---:|---:|---|---|
| C1 | 36 / 36 / 36 | 12 | 29 | 1 | 未观测到 | equal |
| C4 | 36 / 36 / 36 | 12 | 23 | 3 | 已观测到 | equal |
| C8 | 36 / 36 / 36 | 12 | 11 | 6 | 已观测到 | equal |
| C16 | 36 / 36 / 36 | 12 | 3 | 8 | 已观测到 | equal |

四组 CLI 都返回 0，合计 144 个已 offer 操作、144 个成功终态和 48 次
实际修改。全部既有 canonical 表及三个公开符号探针对账相等；比较前
没有重建或修复增量库。此 Python 合成语料中未出现的语言和图结构，不因
比较包含全部表名而获得正确性认证。四组 worker 的 stderr 都为空，
supervisor 启动前、worker 自检、supervisor 收尾的 binary BLAKE3 一致。
`observations.json` 另外保留观测结束时 executable 的 SHA256。

这些是有界 debug smoke。观测到的队列峰值、时长和调用重叠不能证明
吞吐达标、p95/p99 稳态、无饥饿、长期无泄漏或任意平台上的行为。
本 profile 的 RSS 为 null。long steady state、100k、真实 git branch
switch、catalog compaction、backfill、外部 stdio 产品认证与 release
performance certificate 都是 `not_run`。

## 控制与失败测试

`cargo/load-final-integration-tests.log` 对固定源执行了 9 个 load 集成测试
和 1 个真实 route 回归，全部通过。覆盖实际 C1/C4/C8/C16 混合调用、
offer/终态分母、queue rejection、request timeout、取消后 drain/对账、
进程总 deadline、raw 预算、拒绝覆盖已有输出、worker binary 不匹配、
运行中 binary 被删除，以及外部 cache override 不得写出 fixture。

`cargo/load-stderr-tests.log` 的两个单元测试通过：64 KiB 前缀与丢弃后缀
计账、写入失败可见且继续 drain。这两个单元测试不冒充实际 CLI 的
日志洪水或任意进程树试验。

`cargo/load-integration-tests.log` 是增加环境清理补丁**之前**的首次独立
构建记录，8 个 load 测试和 1 个 route 回归通过；它仅保留为先前记录，
不能替代固定源的 final 日志。`cargo/load-clippy.log` 是已经启动但在父
agent 要求转向最终组合源统一 lint 后主动中断的检查，退出 130；它不是
clippy 通过证据。组合源的检查应有自己的源提交和日志。

使用同一分支独立目标目录，`CARGO_PROFILE_DEV_DEBUG=0`、
`CARGO_PROFILE_TEST_DEBUG=0`、`CARGO_INCREMENTAL=0`，全部 Cargo 操作为
`--offline --locked`、`-j 1`。实际工具链见 `cargo/toolchain.txt`。
固定源命令如下：

```sh
cargo test --offline --locked -p cc-eval --test p8_load --test p8_route_reload -j 1 -- --test-threads=1
cargo test --offline --locked -p cc-eval --lib benchmark::p8_load::stderr_tests -j 1 -- --test-threads=1
```

## 文件与重放

每组保留 events、配置、manifest、worker binary 自检、初始/full build、
完整 incremental/full canonical、reconciliation、worker summary、
supervisor、CLI stdout/stderr 和 worker log。文件内容是原样复制，
没有过滤失败行、改写时间或重新归一化 canonical。可重建的 fixture
工作树、SQLite 和 executable 未提交。

在固定源重新构建，然后用对应配置与一个尚不存在的输出目录重放：

```sh
cargo run --offline --locked -p cc-eval --bin cc-eval-p8-load -- run --config artifacts/checkpoints/p8-load-20261007/c4/config.json --output /tmp/p8-load-c4-new
```

本工具启动自己受信任的 worker，清除继承的 `CODECORTEX_*` 环境变量，
并把 PATH 指向本次输出中校验为空的目录；不是任意 executable 的通用
进程树沙箱。生成语料的 semantic/auto-index 关闭，可选 git 程序不可用。

`manifest.json` 列出文件字节数、SHA256、固定源码与运行来源；
`SHA256SUMS` 也包含该 manifest 的 hash，并按通常约定不包含自身。
可在本目录运行 `sha256sum --check SHA256SUMS` 校验保存的完整字节。
