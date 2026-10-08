# 固定 P 的最终本地验证

本轮获准验证已完成；本地 Rust 门禁仍为失败。所有原始失败、忽略项和运行前后输入收据均保留，未修改产品源码或测试断言。

## 固定身份

- PRODUCT: `615662bd0e651dc40a9d1d0d6757bc28646b900d`
- Git tree: `d36023d30673cd3eeb6b7fc146254b57daaf3679`
- 输入：842 个 Cargo/crates 源文件 + 111 个验证/策略文件；七次命令前后均相同，工作树干净。
- 完整输入清单 SHA-256: `bcc92f331231b6d4e3cc0cd6250332a1a0899bc7a4e3e56404eaf538596347a1`
- Rust 1.95.0 / Cargo 1.95.0 / Python 3.12.14，2 Cargo jobs，增量关闭，dev/test debug=0，`RUSTFLAGS=-D warnings`。

## 实际结果

|命令范围|目标/测试结果|退出码|耗时（秒）|
|---|---|---:|---:|
|rust-default-all-targets|83 targets; 398 passed / 2 failed / 57 ignored|101|279.561752|
|rust-http-scoped|3 targets; 69 passed / 1 failed / 7 ignored|101|84.160140|
|rust-semantic-ci-targets|4 targets; 58 passed / 1 failed / 6 ignored|101|48.656867|
|python-all-scripts|268 passed / 0 skipped (14 modules)|0|25.596097|
|rust-fmt|passed|0|5.626755|
|code-index-plan|passed|0|0.076831|
|p8-facts|passed|0|0.098370|

Python 是全部 `scripts/tests` 的原始 discover 入口。HTTP 的 5 项专属适配器控制和新增 7 项 coverage diagnostics 均通过；semantic worker 1 项、lifecycle 2 项、strategy 7 项及 lib 的 3 项 mechanism 控制均通过。原有忽略项没有执行，也没有按通过计数。每个配置中的重复测试应分别解释，不能相加当作不同测试或 TODO。

## 两处原始失败

1. `benchmark::sampler::process_probe_tests::linux::live_child_snapshot_is_attributed_monotonic_and_disappears`：三配置均失败，`sampler.rs:892` 报 live child snapshot unavailable。只读环境观测中本地 PID 5 对应 `/proc/self` 的 824327，存活子进程的本地 PID 6 对应的 `/proc/6` 属于无关进程；原采样实现直接读取局部数字 PID 路径且未绑定命名空间。
2. `tests::benchmark_fixture`：默认首次执行 index warm p95=547.91ms，超过原 500ms。HTTP、semantic 配置下该测试通过，但不改变默认首次失败；未通过受控实验归因于磁盘或并行负载。

两处相关源码在 BASE 9f6684ac、F fb772551 和 P 615662bd 完全同字节。根代理报告已建立独立跟踪 issue #155；该 issue 不计入原 TODO 完成数。故障分析的原始证据及逐文件哈希见 `rust-default-failure-analysis.json`。

默认编译完成时出现磁盘耗尽，但日志没有明确 ENOSPC 堆栈，不能将两处失败直接归因于磁盘。完成各命令后仅清理 target-scale 中可再生成的测试/CLI 可执行缓存；共移除约 3.95 GB 的累积生成内容，每次删除路径、二进制哈希与释放量均有收据。F/D 固定二进制、target-pr148、源码及原始评测证据未动。

## 可复现命令

在固定工作树执行，并设置收据中记录的环境；不要直接使用指向 target-pr148 的旧 env 文件。

```sh
export CARGO_HOME=/workspace/scratch/031390cf22eb/cargo-cache
export RUSTUP_HOME=/workspace/scratch/031390cf22eb/toolchains/rustup
export RUSTUP_TOOLCHAIN=1.95.0
export CARGO_TARGET_DIR=/workspace/scratch/031390cf22eb/target-scale
export PATH="$RUSTUP_HOME/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"
export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0
export CARGO_TERM_COLOR=never PYTHONDONTWRITEBYTECODE=1
export RUSTFLAGS='-D warnings'
cd /workspace/scratch/031390cf22eb/codecortex-round4-validation
cargo test -p cc-eval --all-targets --locked --no-fail-fast
cargo test -p cc-eval --features eval-http --locked --no-fail-fast --lib --test benchmark_coverage_diagnostics --test benchmark_adapters
cargo test -p cc-eval --features semantic --locked --no-fail-fast --lib --test p7_worker_contention --test semantic_lifecycle --test p7_strategy_ablation
python3 -B -m unittest discover -s scripts/tests -v
cargo fmt --all -- --check
python3 -B scripts/code_index_plan.py
python3 -B scripts/p8_facts.py --check
```

以上是已执行命令，列出不表示需要重跑已失败的默认全套。初次 `--no-fail-fast` 完整执行了 83 个默认目标；后两配置按根代理在容量事件后明确收窄的范围执行。

## 范围与交接

完整标准主机 workspace/Clippy/MSRV、其余 HTTP 目标、doctests、原忽略的真实 stdio/4-arm/长时 benchmark 不在本地通过范围。最终 S 的 v14 CLI 与原 174 source-integrity 由另一代理在独立固定树执行；即使该门禁通过，也不覆盖这里的 Rust 失败。

固定 P 原任务账本：192 总项、160 done、32 未完成（1 blocked、19 in_progress、12 todo）；后续验收状态由根代理统一维护。p8_facts 只检查声明的文档事实，runtime_certified=false。

`final-validation-summary.json` 提供所有精确 argv、工具链、输入身份、日志及摘要哈希；`validation-artifact-manifest.json` 对本代理全部收据逐文件 SHA-256 封存。新回执可单独归档，旧 holdout 使用证据包未重打包。
