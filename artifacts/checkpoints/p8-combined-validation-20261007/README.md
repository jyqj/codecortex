# P8 组合源：33 个集成测试与 scoped clippy

本证据实际执行的固定源是
`853385b7ccb2780818f9f8e8a83791f1c197efcf`。执行者为
`/root/round1_measurement`，同一工作树中的 crate 源码与该提交一致，测试后
也再次检查了 tracked code diff 和 untracked code 均为空。
其他远端提交与该源是否逐字节相等，需要独立的源码等价证据；本目录没有
把后来出现的 ref 写成实际执行来源。

## 实际结果

| 目标 | 通过 | 失败 |
|---|---:|---:|
| `benchmark_cli` | 6 | 0 |
| `p8_measurements` | 11 | 0 |
| `p8_load` | 9 | 0 |
| `p8_route_reload` | 1 | 0 |
| `p8_scale` | 6 | 0 |
| 合计 | 33 | 0 |

上述五组集成测试是同一个 Cargo 调用、`--test-threads=1` 顺序执行，
实际 session 退出 0。另一个实际 session 完成 `cc-eval` 的
`--all-targets --no-deps` clippy、`-D warnings`，退出 0。
完整原始日志分别为 `cargo/final-combined-tests.log` 和
`cargo/final-combined-clippy.log`；没有移除 Cargo 产生的空行或改变字节。

```sh
cargo clippy --offline --locked -p cc-eval --all-targets --no-deps -j 1 -- -D warnings
cargo test --offline --locked -p cc-eval --test benchmark_cli --test p8_measurements --test p8_load --test p8_route_reload --test p8_scale -j 1 -- --test-threads=1
```

这组测试验证组合后的 gate、计量、负载、route reload 和规模工具能够
共同构建并满足这些具体合同。它不是整个 workspace 测试集，也不是
长期稳定性、100k、V20、P7 前置条件或完整 P8 性能认证。原有 native
sampler 的 Linux PID 归属测试不在这次目标列表中，不能据此声明该环境
问题已经消失。短 smoke 与失败控制用例也不能代替性能样本量和 CI。

## 构建与二进制来源

此前共享跨 worktree target 存在库元数据不匹配，因此 load 原始验证已使用
全新私有 `target-load`。本次组合源复用同一工作树和这个私有 target：
先切到固定提交，再只执行 `cargo clean --offline --locked -p cc-eval`。
清理记录在 `cargo/final-combined-clean-cc-eval.log`，实际移除了该包的
39 个文件、428.5 MiB；没有清理其他会话或项目的缓存。

组合源相对原 load 源 `25399fc51bb004885c18e8ba13fd1aae9614be17`，
非 `cc-eval` 的 crates 没有源码差异，继续复用这些依赖的构建缓存。
环境固定为 `CARGO_PROFILE_DEV_DEBUG=0`、`CARGO_PROFILE_TEST_DEBUG=0`、
`CARGO_INCREMENTAL=0`、`-j 1`、`--offline --locked`。实际工具链在
`cargo/toolchain.txt`。

`source-binary-binding.json` 保存固定 source/tree、Cargo.lock SHA256、
代码未修改检查，以及测试成功后 `cc-eval`、`cc-eval-p8-load`、
`p8-scale` 三个实际 binary 的字节数和 SHA256。这是固定本地构建
和文件来源的记录，不是可复现构建证明；二进制本体和可再生 target
缓存没有提交。

负载工具另有自己的 before / worker-self / after binary 校验合同；
原固定 load 源的四组完整 CLI 观测在独立
`artifacts/checkpoints/p8-load-20261007/`。那些早先观测保留原来的源标签，
不被重新标记成本次组合源执行。

## 文件完整性

`manifest.json` 列出本目录每个原始日志和说明文件的字节数与 SHA256。
`SHA256SUMS` 包含 manifest 本身的摘要，并按通常约定不包含自身。
在本目录运行 `sha256sum --check SHA256SUMS` 可校验保存的字节。
