# 贡献指南

## 最低 Rust 版本

1.95（2021 edition）。由 rusqlite 0.40（libsqlite3-sys 0.38 使用
`cfg_select!`，1.95 才稳定）决定，CI 强制。

## 构建

```bash
cargo build
cargo build --release    # 带 thin LTO 的优化二进制
```

## 测试

```bash
cargo test                 # 全部 crate
cargo test -p cc-model     # 单个 crate
cargo test -p cc-eval      # 评测套件（fixture + 语料）
```

依赖严格单向，每个 crate 都能独立编译测试——`cargo test -p cc-db`、
`cargo test -p cc-index` 不需要构建整个工作区。

测试布局与 eval 语料见 [docs/TEST_PLAN.md](docs/TEST_PLAN.md)。

## Lint 与格式化

```bash
cargo fmt --all -- --check
cargo clippy --workspace --all-targets -- -D warnings
```

## 提交前检查

未配置 pre-commit 钩子。提交前运行：

```bash
cargo fmt --all -- --check \
  && cargo clippy --workspace --all-targets -- -D warnings \
  && cargo test --workspace \
  && cargo test -p cc-eval -- integration_fixtures_and_corpus
```

真实工作区性能回归检查：

```bash
CODECORTEX_WRITE_REAL_BENCHMARK=1 \
  cargo test -p cc-eval benchmark_real_workspace -- --ignored --nocapture
```

基准细节见 [docs/BENCHMARK.md](docs/BENCHMARK.md)。

### Code Index V2 / P0 评测底座

新的开发用 `cc-eval` binary 提供 `validate/run/replay/compare/import-oce/mutate/schema`；生产 `codecortex` CLI 不变。使用方法见 [P0 USAGE](crates/cc-eval/benchmarks/USAGE.md)，失败口径见 [KNOWN-FAILURES](crates/cc-eval/benchmarks/KNOWN-FAILURES.md)。真实协议测试需要显式指定当前构建的产品二进制，不能把进程内 duplex 测试称为子进程 stdio。

本机遇到 SDK/TAPI 不兼容时，只为构建子进程指定兼容 SDK，不切换系统全局设置：

```sh
python3 scripts/p0-validation.py \
  --sdkroot /path/to/compatible/MacOSX.sdk \
  --target-dir target/p0-validation \
  --evidence-dir artifacts/benchmarks/a-new-validation-run \
  --product-binary /path/to/exact/codecortex
```

该脚本保留命令、退出码、源码摘要、ignored 数量和日志。`rustc 1.97` 的成功不替代 `1.95` MSRV 验证；未安装的工具链要记录 blocked/not_run。现有全仓 `-D warnings` 规则不因 benchmark 变更而降低；旧代码产生的 warning 需要作为独立基线问题报告。

记录平台结论时同时注明源码、实际 OS/架构、Rust 工具链与所选 SDK；没有采集的 SDK
版本写明 unknown，不从构建成功推断。新 target 的产品冷构建、完整 workspace 测试、
单个测试和独立诊断各自保留命令、退出码与前置条件；使用预先执行过的 helper 时明确
记录准备步骤，不能据此重标首次执行 helper 的旧失败。已知范围见
[平台证据边界](docs/BENCHMARK.md#p8-平台证据边界2026-10-09)。

## 文档约定

- 文档语言为简体中文；代码标识符、命令、日志、错误信息保留原文。
  `docs/benchmarks/` 下的报告由测试生成（英文），不手工编辑。
- 文档里的可核对数字（测试数、语料数、schema 版本等）改动后运行
  `scripts/update-doc-baselines.sh` 核对 `docs/TEST_PLAN.md` 的基线。
- 跨 crate 的结构性决策写 ADR，约定见
  [docs/adr/README.md](docs/adr/README.md)。

## CLI 命令

```
codecortex mcp [--project-path PATH]   启动 MCP stdio 服务器
codecortex install [--force]           为检测到的 AI agent 安装 MCP 配置
codecortex uninstall                   从所有 AI agent 移除 MCP 配置
```
