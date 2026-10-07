# P8-020：最终任务状态的独立 G8 总账运行

本目录记录一次新的实际 `scripts/p8_release_review.py` 运行，固定任务状态
为 `2d060834de39cf19db67428ec4a2a84050f72b6d`，run ID 为
`p8-g8-final-state-2d060834-20261008`。
**进程 exit 1，整体 `not_accepted`；local / semantic 均为 `blocked`。**
`release_certified=false`、`todo_completed=false`，不改任何原任务状态。

## 当前任务计数

| 状态 | 数量 |
| --- | ---: |
| done | 152 |
| in_progress | 22 |
| todo | 17 |
| blocked | 1 |
| deferred | 0 |
| 原始任务总数 | 192 |
| 剩余未 done | **40** |

工具按原 192 个任务定义及原 G8 依赖、条件依赖评估两个范围，保留未完成
前置、缺少的发布证据与外部验收项。任务状态、仓库记录及局部通过都不能
代替 G8 发布接受。此次执行没有模型调用、网络调用、输入修改或 Rust 构建。

## 13 条固定证据保持不变

输入来自既有 `release-review/current-evidence-manifest.json`，只更新
`run_id` 和 `state_commit`。13 条记录的顺序、commit/path/SHA256、profile、
scope、coverage 及 status/source/input 指针全部保持原样；读取到的原始
记录字节和观察结果也与旧 `current-evidence-01` 完全一致。

候选继续固定为：

- source commit：`78ae91eeae6edae6bea29c27f24b251773341c00`。
- 完整输入摘要：`dbb2a2233a51da0423984778da840df8f4cbd079a66d900c5a28e7ab0e1b9aac`。
- 摘要口径：按路径排序的紧凑 path→SHA256 JSON，无尾换行。

后续 `65dd32934b3f8cb3ff4f5f5deb154a431a8c09e3` 只修改 worker TEST。
本次总账保留之前产品、恢复和回滚记录各自的原身份，不把它们重标为
这个后续 TEST 提交上的执行。没有追加冷构建、worker 或其他新记录。

旧 `f99b2dc2…` 状态运行保持原件。本目录仅追加最终状态的总账，不覆盖
旧 manifest、receipt、stdout/stderr、原定义快照或已有任务派生页面。

## 原始输出与复核

- `input-manifest.json`：此次实际输入。
- `stdout.json` / `stderr.log`：实际子进程输出，stderr 为 0 bytes。
- `run-01/receipt.json`：工具实际生成的完整总账。
- `run-01/metadata/manifest.json`：原字节输入副本。
- `run-01/metadata/record-0000.json` 至 `record-0012.json`：13 条固定 Git 记录原字节。
- `execution-metadata.json`：实际命令、退出码、脚本摘要、旧运行身份和逐项核对结果。
- `file-manifest.json`：本目录工件的路径、字节数与 SHA256。

已检查 stdout 与 receipt 解析后相同；原始 manifest 副本相同；13 个记录
的 SHA256 和固定 Git blob 逐字节匹配；候选及记录观察结果与旧运行一致。
未重复任何合同测试、指标运行或产品 raw 重放。

```sh
PYTHONDONTWRITEBYTECODE=1 python3 scripts/p8_release_review.py \
  --manifest artifacts/checkpoints/p8-next-ten-20261008/release-review/final-state-2d060834/input-manifest.json \
  --output /tmp/p8-g8-final-state-new-run
```

复跑应使用尚不存在的输出目录，预期仍为 exit 1。完整外部验收、当前
metrics/raw 重放、候选源码 integrity 执行及 live semantic 认证仍由各自
证据和评审决定，本工具不为这些项目签发通过。
