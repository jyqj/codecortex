# P7-017 default / disabled semantic stdio evidence

基线：`codex/cloud-p7-recovery-integration`，`855b4fc8f2435215c438db629c1bd91f7a59510d`。
只修改 `crates/cc-eval/tests/benchmark_adapters.rs` 和本目录；生产、Cargo、共享清单未改。

## 已实际执行

- 分别构建 `cc-server` 默认 feature 集和显式 `semantic` 包，`--locked --offline`。默认依赖图无 `cc-semantic`，semantic 图包含它。精确命令、工具链和二进制 SHA256 见 `receipt.json`。
- 两包分别运行 adapter target 的 `--include-ignored`：各 **3 passed / 0 failed / 0 ignored**。原有两个测试及全部断言保持原样；新增测试要求显式产品二进制，正常无此二进制的 test 运行仍按显式集成测试处理。
- 新增测试每包运行两配置：无 semantic 配置，以及 `semantic.enabled=false`（带合成无效 endpoint 和不存在的 key 引用）。产品子进程 `env_clear`，只提供 PATH 和 fixture HOME/XDG 等运行变量，零凭据。
- 每配置真实初始化并列出/调用全部 14 个旧工具；验证本地 hybrid/symbol、auto context、源码位置、图/调用链、空 traces、ADR 存储/删除；重启同项目后无需重索引即可搜索且 ADR 持久化。
- 错误契约：未知参数为 tool `is_error=true`；错误 mode 为 MCP `-32602`；显式 semantic 和缺失符号为 `-32603`。每次正常关闭前和重开关闭后审计整个 fixture 文件树：仅本地 index 数据库，无 semantic cache 目录/数据库。
- targeted clippy `-D warnings`、独占测试文件 rustfmt、`git diff --check` 通过。

## 禁网证据边界

此前父任务的 `unshare --net` 被环境拒绝，本任务未重试、未提权、未更改网络或安全设置。
使用系统已安装 libseccomp 的一次低权限进程级附加限制，见 `deny_network.py`：拒绝非 AF_UNIX socket 创建、全部 connect、io_uring 和可用 socketcall；关闭已有 socket fd，若 stdio 为 socket 则失败；允许本地 Unix socket pair。加载失败即报告 blocked，不升级权限或绕过限制。

探针及两次实际测试均记录 IPv4/IPv6 `EPERM` 和 exec 子进程继承；Rust 测试再次探测 socket 禁止，产品/重开产品 `/proc/PID/status` 显示 `NoNewPrivs=1`、`Seccomp=2`、filters=2。这是**进程及后代的 syscall 禁网**，不是 network namespace；不能替代 namespace 成功证明。运行报告在 `network-*-run.json`，实际 stdio/文件审计在 `p7-017-*.json`。

## 重放

先在相同基线构建两包并保留各自二进制（第二个 feature 构建会覆盖同名输出）。设置 `CODECORTEX_BENCH_BINARY` 为对应二进制、`P7_017_PACKAGE_KIND=default` 或 `semantic`，可设置 `CODECORTEX_BENCH_OBSERVATIONS` 为证据输出目录，再执行：

```sh
python3 artifacts/benchmarks/p7-017-cloud-20261002/deny_network.py \
  --report /tmp/p7-017-network.json -- \
  cargo test -p cc-eval --test benchmark_adapters --locked --offline -- \
  --include-ignored --nocapture
```

环境需支持 libseccomp 的低权限追加过滤器；不支持时诚实记 blocked。本次先 `cargo fetch --locked` 补缺失缓存，随后构建/测试离线；下载依赖不调用 provider。保留最初缺缓存日志及新增断言修正前日志，后二进制最终日志均成功。

## 给集成者的清单建议

登记 P7-017 的上述独立 stdio 契约证据通过，并链接本目录；正式 **P7-017 不得置 done**，`formal-tasks-baseline.json` 中 P7-016 仍 todo，且 V18 完整客户端/配置兼容和 V21 发布包、回滚、MSRV/跨平台未完成。namespace 项保留历史 blocked / 未重试，同时单独标记进程级 seccomp 限制下的真实 stdio 验证通过。

未运行全仓测试/全仓 lint、真实付费 provider 或外传源码；所有产品测试只读取合成本地 fixture，保持 D1/D2。整合只需将本提交 cherry-pick 到组合基线，按重放命令重新构建/跑两包；本块无需 P7-014 额外修复（基线已含恢复组合）。
