# P8 第 1 轮：平台与回滚原始证据

本目录推进原始 P8-012 与 P8-016 两项；不是两个任务或 G8 已完成的声明。
实现说明见 `docs/roadmap/code-index-v2/P8-PLATFORM-ROLLBACK.md`。

## 可以直接核对的结果

- `cold-matrix.json`：8 格，0 passed / 0 failed / 8 not_run，命令 exit 2。
  Linux 1.95 编译器可用但本任务未发起构建；stable 缺 rustc；macOS 无 runner。
- `rollback-receipt.json`：真实旧候选两包、4 次 MCP stdio 启动，有限本地回滚
  exit 0。schema 25→注入 1025→25、semantic→default、本地检索、旧库备份
  恢复、cache 根隔离、源码和配置完整性通过。
- 主动 cache reader 对未知格式的拒绝仍 `not_run`；真实新版 / 旧版发行包
  配对与当前源码产品重建不在本轮实证范围。
- `contracts-final.log`：30 个控制通过，synthetic compiler 仅验证脚本协议。
  正式产品证据仅指上述真实 stdio 演练。

## 回执与归档

`evidence-index.json` 保存实际命令、退出码、日志摘要、脚本摘要、archive 摘要。
`toolchain-msrv.json` / `toolchain-stable.json` 是实际工具链探针；stderr 中
stable 的 rustc 缺失保持原样。早期一次 stable 探针超时保留于归档的
`cold-inventory-01`，没有用后续探针覆盖失败事实。

`raw-evidence.tar.gz` 保留三轮实际记录，包括最终 `cold-final` 与
`rollback-final`：产品原始 build witnesses、源文件 manifest、每次 stdout
JSON-RPC、stderr、退出码、seccomp 探针、配置、cache sentinel 以及原始 SQLite
旧库 / 未来库 / 重建库备份。大块可再生产品 target 未复制。每个新运行都使用
新的 output 目录，旧回执不被覆盖。

```sh
mkdir /tmp/p8-platform-evidence
tar -xzf artifacts/checkpoints/p8-next-ten-20261008/platform/raw-evidence.tar.gz \
  -C /tmp/p8-platform-evidence
```

JSON 内的绝对路径是当时执行位置的 provenance；归档按相对路径保存。同名源码
和备份的逐文件摘要可从 `rollback-final/rollback.json` 复算。原始证据包含自建
测试源码，不包含用户项目或真实 provider 凭据。

本轮不触碰 production crate、历史 fixture、任务定义、CI 或 source registry。
