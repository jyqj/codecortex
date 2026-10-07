# A+B 组合源码窄验证

在独立 `work/combined-validation-20261007` 工作树中，从 A + roadmap + v4 guard
基线 `afba69e3665c87d174aa0cbabfc37ccd1670fb27` 无冲突 cherry-pick B 实施
`4a1f4f6fbb2cd1b17e62ed9bf819bb1b9a52941e` 及独立接受证据
`707b6b64048b9fc119ab1c396eb082be8de2d8a5`。

实际组合 HEAD：`bd875341bf1065b902f21209d78056b3d15e3ca6`。
实际组合 tree：`96e14a8f6b80f8996026e37b9f8a20f88a76b796`。
验证前后工作树均干净；没有更改 main、任务状态、CI 或 source guard。

## 实测

官方 Rust 1.95.0，原 Cargo.lock，`--offline --locked`，`-j2`，dev/test debug=0、
incremental=0。独占复用 `target-validation-cost` 缓存，实际重编译组合后的相关 crates。

| Target / 检查 | 结果 |
| --- | --- |
| `cc-eval --test p7_validation_work` | 5 passed / 0 failed / 0 ignored |
| `cc-index --test python_inventory_revalidation` | 11 passed / 0 failed / 0 ignored |
| `cc-index --test python_inventory_capture` | 11 passed / 0 failed / 0 ignored |
| `cargo fmt --all -- --check` | exit 0 |

合计 **27 passed / 0 failed / 0 ignored**。实际 argv、退出码、逐命令摘要、耗时和日志
SHA-256 在 `validation.json`；日志只规范化末尾空行，没有改动测试内容或测试结果。

## 源码绑定

`source-sha256.json` 保存完整 **767 个 crates/Cargo.toml/Cargo.lock 输入**的逐文件
SHA-256。manifest 原 bytes 的 SHA-256：

```text
fabb3261d72072a54ca425d94da307442bbb6ccc17fb9b2e72c764e486b07d1d
```

运行前后重新枚举和计算同一集合，**0 个输入新增、删除或改变**。后续最终组合若只
改变 docs、CI 或 source registry，可以按此逐文件 map 核对 Rust 实现/测试输入是否
仍完全相同；这里的完整 Git tree 则只绑定上述实际测试的中间组合。

本中间树仍带 v4 guard，因此没有运行旧 guard 或尚未整合的 v5 guard；父线程分别
处理新 guard 与最终证据接入。本轮没有重跑 broad Clippy/full workspace tests，
也没有 public quality、100k、remote CI、live provider 或非 Linux 验证声明。

## 复现

使用一个新的输出目录保存新运行，避免覆盖本次固定证据：

```sh
python3 validate.py --repo /path/to/combined-checkout \
  --target-dir /path/to/isolated-target --output-dir /path/to/new-evidence
```

脚本只执行上述三个测试 targets、格式检查及源码摘要读取；所有输出写到显式的
`--output-dir`，不会更新任务状态、CI、guard 或源文件。
