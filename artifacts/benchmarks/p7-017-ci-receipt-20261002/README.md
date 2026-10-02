# P7-017 CI product receipt 接线

基线 `243fa6f14ba12366b82bc38693641840721973e4`（PR26）；本块只改 CI 的 stdio 构建/调用步骤、自己新增的 adapter harness、专用 build-receipt 脚本和本证据目录。PR25 生产审查及其三条冻结测试未改。

CI 原先仅构建默认产品并传 `CODECORTEX_BENCH_BINARY`，`--ignored` 会执行新增 P7 测试但没有 package receipt。修复由 `scripts/p7_stdio_build_receipt.py` 真正调用 Cargo，读取 `compiler-artifact` 的 feature/target/package 信息，保存独立二进制快照并绑定 SHA256。默认构建为 `--no-default-features`，semantic 构建显式 `--features semantic`；不是靠测试环境标签猜产品身份。失败构建先删除旧 receipt，无法沿用旧成功凭据。

测试要求显式 `P7_017_PACKAGE_KIND` 和 `P7_017_BUILD_RECEIPT`，校验真实 artifact feature、命令、目标、路径、hash。缺失不默认通过。原有全部行为断言及 ignore 属性不变；增加了 receipt 不一致负测。默认接入原 P0 stdio 步骤，semantic 在旧默认产品回归步骤之后独立构建/验证，避免改变它们的二进制身份。

## 实际验证

- 默认 adapter + eval-http 全 target：**9 passed / 0 failed / 0 ignored**，包含原断言、receipt validator 负测及纯 loopback HTTP fixture。
- 重放 checked-in workflow 的全部 **17 个涉及真实产品 stdio 的步骤**：**15 步完整通过，2 步原有架构前置检查失败**。精确原始/实际命令、退出码见 `ci-stdio-replay.json`。仅将 `$PWD/target/debug/codecortex` 路径映射至本地 `CARGO_TARGET_DIR=/tmp/p7-017-build`；保持 `RUSTFLAGS=-D warnings`，并用缓存离线执行。
- 两个失败没有被取消或修改：P3-D 检查 `MODULE_CAPABILITIES.json database_schema=21` 对生产常量 22；P4-A 检查仍要求 `handlers/context.rs` 含 `validate_envelope_generation`。它们已在 PR26 基线存在，留给主集成者修正；本块不侵占这些文件。
- 原架构检查提前失败导致 P3-D/P4-A stdio 未在对应整步执行；随后各自独立运行 stdio target，均 **1 passed / 0 ignored**。整步仍记 failed，不能据此把 CI 填绿。
- 默认和 semantic 两产品均用真实 receipt，在既有低权限、进程继承 seccomp wrapper 下跑全部非 HTTP adapter：各 **4 passed / 0 failed / 0 ignored**。IPv4/IPv6/exec 继承探针报告见 `network-*.json`；仍仅声明进程 syscall 禁网，不声明历史 namespace 拒绝已通过。
- 实际缺失 receipt、semantic 产品搭配 default receipt 两个命令都 exit 101，在产品 spawn 前拒绝，见 `real-receipt-negative.json` 和对应日志。单元负测还覆盖 artifact feature、命令、目标、hash、路径和构建失败身份。
- targeted eval-http clippy `-D warnings`、独占 Rust 文件格式、Python 语法及 diff check 通过。

## 重放

```sh
python3 scripts/p7_stdio_build_receipt.py --package-kind default --output-dir /tmp/p7-default --offline
CODECORTEX_BENCH_BINARY=/tmp/p7-default/codecortex \
P7_017_PACKAGE_KIND=default P7_017_BUILD_RECEIPT=/tmp/p7-default/build-receipt.json \
cargo test -p cc-eval --features eval-http --test benchmark_adapters --locked --offline -- --include-ignored
```

semantic 用相同脚本指定 `--package-kind semantic`、独立输出目录，传对应 binary/receipt/kind；测试无需 provider key。普通 CI 有 loopback fixture，不能当禁网证明；显式 seccomp 验证单独保存。未改 workflow permissions、secrets、网络、安全设置或取消检查。

构建 artifact/真实 hash 见 `cargo-default/`、`cargo-semantic/`；行为观察见 `p7-017-*.json`。没有提交产品 ELF。CI/GitHub Actions 整个 job 的成功未认证，上述两个旧前置检查仍阻塞绿色流程。P7-017 正式依赖 P7-016/完整 V18/V21，本块不翻 done。所有 fixture 合成，无真实/付费 provider，无外传源码。
