# P8-012 / P8-016｜平台构建入口与有限回滚演练

## 本轮结论

本轮为两个原始 TODO 提交可执行入口、负对照和真实产品回执，不据此完成
P8 平台认证或发行认证。P8-012 的八格目前全部 `not_run`；P8-016 在明确绑定的
既有产品上通过有限本地回滚，主动读取未知 cache 格式与真实新旧发行包配对仍
`not_run`。原任务的硬依赖、验收和发布条件不变。

本轮实现基线为 `ae906513886bef4501d0a1d2ecffd68ef76c3f5d`。冷构建输入验证
覆盖 777 个已提交的 Cargo / crate 文件，manifest SHA-256 为
`2bb5982b325afe796b19bdaa538075e27dea0997d72faf829ea1ae772013fb0c`。
脚本本体另记摘要，不将新增脚本声称为该基线构建出来的产品。

证据索引：`artifacts/checkpoints/p8-next-ten-20261008/platform/README.md`。

## P8-012：冷构建入口

`scripts/p8_cold_build.py` 总是列出 Linux / macOS × Rust 1.95 / stable ×
default / semantic 八格。默认只读环境并生成 inventory；只有明确 `--execute`
才开始构建。本轮因执行环境空间紧张，没有在本任务中发起新产品构建。

| 平台 | 工具链 | 包 | 本轮状态 | 具体原因 |
|---|---|---|---|---|
| Linux | 1.95.0 | default | not_run | 编译器可用，未执行冷构建 |
| Linux | 1.95.0 | semantic | not_run | 编译器可用，未执行冷构建 |
| Linux | stable | default | not_run | 工具链不完整，rustc 缺失 |
| Linux | stable | semantic | not_run | 工具链不完整，rustc 缺失 |
| macOS | 1.95 | default | not_run | 本机没有 macOS runner |
| macOS | 1.95 | semantic | not_run | 本机没有 macOS runner |
| macOS | stable | default | not_run | 本机没有 macOS runner |
| macOS | stable | semantic | not_run | 本机没有 macOS runner |

本机可用 Rust 为 `rustc 1.95.0 (59807616e 2026-04-14)`。stable 在本地
rustup inventory 中存在，cargo 报 1.99.0，但 `rustup which --toolchain stable
rustc` 明确失败。该状态不能用 MSRV 的成功或 cargo 的存在替代。

### 执行方式

```sh
# 只列环境；八格未完成时预期 exit 2。
python3 scripts/p8_cold_build.py --output-dir /tmp/p8-inventory-new

# 在资源充足的对应平台 runner 上执行一格。
# 依赖需预先准备在 Cargo cache；实际构建始终 --offline --locked。
python3 scripts/p8_cold_build.py \
  --execute --only-toolchain 1.95 --only-package default \
  --profile release --output-dir /tmp/p8-linux-msrv-default-new

# 单独回验这一格。其余平台 not_run 不会被单格成功掩盖。
python3 scripts/p8_cold_build.py --verify \
  /tmp/p8-linux-msrv-default-new/linux-1.95-default/receipt.json
```

矩阵入口 exit 0 只表示八格全部通过；有失败为 1；不完整或输入无效为 2；
取消为 3。单平台或单格运行的矩阵仍然不完整，因此 CI 应检查其明确选中的
cell receipt，再单独执行 `--verify`。多平台 receipt 聚合与 CI 接线留后续，
当前不伪造一个已合并的八格通过报告。

每次 output 必须不存在，不覆盖旧回执。每格拥有新 target；使用显式 Cargo、
rustc、target triple、`--locked`、`--offline`、`--no-default-features`，禁用
rustc wrappers。回执核对以下独立证据：

- 已提交源码的实际 bytes / Git blob / executable mode；即使文件被标记
  assume-unchanged，漂移也拒绝；未绑定的 untracked / ignored build 输入拒绝。
- rustc 与 cargo 的真实路径、文件摘要、版本、host，构建前后源码 / 编译器
  摘要一致，Cargo config 文件摘要不变。
- Cargo JSONL 的唯一 codecortex binary artifact、完整特性集合、profile、
  `fresh=false`、`build-finished.success=true` 与二进制摘要。
- 产物必须处于新 target 的确切 host/profile 路径；日志、manifest、compiler
  或产物丢失 / 改写后，`--verify` 非零退出。

新 target 不等于完全密闭构建：依赖缓存可复用，Cargo 配置有明确摘要，未声称
对编译器内部、外部依赖或 build script 的整个执行环境完成 hermetic 认证。
本入口也不代替 workspace 测试、MSRV 测试与 V21 发行门。

## P8-016：真实隔离回滚

`scripts/p8_rollback.py` 只接受产品及其原始 build receipt，自己创建临时 fixture、
数据库、配置和 cache 根。没有“指定现有用户项目”的入口。

本轮两个既有包均绑定 source commit
`a213a4cfab0e0e87198bc279acacda51e4ff62cf`，不是本轮新构建产物：

| 包 | SHA-256 |
|---|---|
| default | `08a651297e39ee066ca9c016e80781deaed5f866e3aedc7846795d378cd3a695` |
| semantic | `84122fdb41ca247e8eb05046915c04b2a794327b562e8f4975d24a6c11f065ed` |

最终演练四次独立启动真实 MCP stdio 产品，每次核对 14 个工具、索引完成、
本地符号命中、`semantic_state=not_configured`、`dense_state=disabled`，并正常
关闭子进程。产品在 inherited process seccomp 下运行，socket / connect 等
网络调用被拒绝；IPv4 / IPv6 与 exec 子进程探针回执保留。没有真实 key、
远端模型或付费调用。

| 场景 | 结果 | 证据界限 |
|---|---|---|
| 旧产品打开“未来 schema” | passed | 对自建 schema 25 注入 user_version 1025 与 future-only 表；旧包受控重建回 25，哨兵表消失，2 个文件重新索引，SQLite/FK 完整性通过 |
| semantic 包回退 default 包 | passed | 恢复备份的 disabled 配置后，本地符号检索保持可用 |
| 旧数据库备份恢复 | passed | SQLite backup API 保留 committed WAL；通过 staging 还原后真实产品重新打开并查询成功，旧备份摘要保持不变 |
| cache 版本根隔离 | passed | future-format-999 与 rollback-format-1 使用不同物理目录；future 原始 bytes 保留，disabled 包未创建 rollback cache |
| 源码 / 配置完整性 | passed | 所有自建源码文件的增删改均纳入前后 manifest，配置原始 bytes 和配置备份相同 |
| 主动 cache reader 拒绝未知格式 | not_run | disabled 路径没有读向量，不能证明 reader 校验；需运行现有 artifact_cache 集成门或接入受控 fake-provider 产品 |
| 真正的新发行包 → 旧发行包 | not_run | 本轮 schema 增号是注入故障，不是真实新版发行格式；未验证两个实际发行版本配对 |

SQLite 备份带有限 deadline，失败保留不完整备份。演练全程保留旧库、注入后的
未来库、重建库、原配置及候选配置。数据库恢复仅替换脚本自身 fixture 的缓存
文件，不删除源码。`rollback.json` 的成功名为 `passed_limited_local_drill`，
不表示 P8-016、G8 或 M4 完成。

```sh
python3 scripts/p8_rollback.py \
  --default-binary /path/to/previous-default/codecortex \
  --default-receipt /path/to/previous-default/build-receipt.json \
  --semantic-binary /path/to/previous-semantic/codecortex \
  --semantic-receipt /path/to/previous-semantic/build-receipt.json \
  --output-dir /tmp/p8-rollback-new \
  --deny-network-wrapper /path/to/reviewed/deny_network_exec.py
```

network wrapper 是显式传入的本地隔离执行器，完整副本及摘要随回执归档；未提供
时只能记录 disabled 配置行为，网络强制隔离标 `not_measured`，不得称零网络实证。

## 合同测试与未完成门

`python3 -m unittest discover -s scripts/tests -p test_p8_platform.py -v`：
**30 passed**。该测试使用标为 `CONTRACT FAKE ONLY` 的 synthetic compiler 验证
脚本协议，不计为真实冷构建。正负对照覆盖暖缓存冒充、错特性 / 平台 / MSRV /
profile、越界产物、source / compiler / log 漂移、旧回执覆盖、缺工具链、无效
输入的八格保留，以及 SQLite 备份和源文件完整性。

下一步仍需：对应平台的真实新 target 构建与测试；多 runner 证据聚合；当前
候选产品的回滚复跑；主动 cache 格式隔离；真实发行包配对。P8-011、P7-020、
P8-013 等原硬依赖不因本轮准备工作而被跳过。
