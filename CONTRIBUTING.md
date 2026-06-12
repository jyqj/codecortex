# 贡献指南

## 最低 Rust 版本

1.88（2021 edition）。与当前依赖下限一致，CI 强制。

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
