# P8 第 2 轮：端到端故障与恢复原始证据

本目录推进原始 **P8-011** 一项。可执行范围是 disabled/local 产品的
kill 后重启、SQLite busy 解除恢复、删除源码后跨重启不复活。
完整说明见 `docs/roadmap/code-index-v2/P8-RECOVERY.md`。

## 可以直接核对的结果

- `recovery-receipt.json`：当前源码 `78ae91eeae6edae6bea29c27f24b251773341c00`
  的 default 产品，**3 passed / 0 failed / 4 not_run**，命令 exit 0。
- 三个真实场景共 5 次 MCP stdio 产品启动，一次预期 SIGKILL exit -9；
  所有源码、数据库与缓存均为该脚本独占自建 fixture。
- 当前产品 witness 验证完整 785 项源码、真实构建日志和二进制，明确
  `cold_build_claim=false`。本目录的恢复通过不能填充冷构建平台矩阵。
- 事务内部 crash point、活跃 provider 断网、活跃语义 cache 损坏、并发
  换库仍 `not_run`；`complete_P8_011=false`、`release_certified=false`。
- `contracts-final.log`：20 项恢复合同通过；`platform-regression.log`：
  原 30 项平台合同通过。合同控制与真实产品行为证据分别记录。

## 原始失败与归档

`raw-evidence.tar.gz` 保留 `old-candidate-01`、`old-candidate-02` 和
`current-candidate-01` 全部工件。第一轮旧候选曾有 2 failed / exit 1；
在修复子进程停止观察方法，以及源码快照误读取 SQLite `-shm` 干扰锁
注入后，同一旧候选与当前候选均实际通过。原失败未被覆盖。

`lock-observer-attribution.json` 和 `wal-lock-observer-attribution.json`
包含独立锁探针原始结果。源码 manifest 现在在读取前排除派生缓存，
新负控检查主数据库、WAL 和 SHM 都不被哈希。原 `contracts-01.log`
保留最初测试文件解析错误；后续完整测试日志分别留存。

`evidence-index.json` 保存实际命令、退出码、各次回执摘要、runner 摘要、
逐文件和 archive 摘要。archive 含每次 RPC/stderr/stages/exit、网络拒绝
探针、产品 witness 和源文件 manifest、原始 SQLite 数据库、自建源码
与配置；不包含二进制/target，也不含用户项目或 provider 凭据。

```sh
mkdir /tmp/p8-recovery-evidence
tar -xzf artifacts/checkpoints/p8-next-ten-20261008/recovery/raw-evidence.tar.gz \
  -C /tmp/p8-recovery-evidence
```

JSON 内的绝对路径保存当时执行位置，归档按相对路径展开。
最终 runner 摘要可与 `current-candidate-01/recovery.json` 复核。
旧候选第一次演练中未执行到的步骤保持失败/缺失事实；不能用后续运行
补写旧回执。所有未来复跑都必须创建新 output 目录。
