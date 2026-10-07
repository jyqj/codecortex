# Python capture 显式重新准入：实施与验证

基线：`886f90a542a6174a037c79eebbb4f74848fb1f53`。
本切片完成一个 opt-in 子项：生成版本化比较收据，重新执行完整 capture →
Loader/provenance → AST → model → native/bytes verify，然后比较收据并返回新对象。
完整契约在 `docs/internals/python-inventory-revalidation-v1.md`。

生产改动只有原 `python_inventory.rs` 的六行子模块导出和新增
`python_inventory/revalidation.rs`。原 capture/native/AST/model 实现和旧测试不变；
不更改 Cargo/Cargo.lock、中央 TODO/registry/CI、历史文档或证据。

## 实测结果

官方 Rust 1.95.0，原 Cargo.lock，全部 `--locked --offline`；构建使用 `-j2`，
dev/test debug=0、incremental=0、独立 target。实际 argv、退出码、耗时、toolchain
和日志路径在 `validation.json`；完整 766 个 crate/Cargo/lock 输入的 SHA-256 在
`source-sha256.json`，manifest SHA-256 为
`88e117de86de061c39a1286872bef37c91086d758d72a6324daf5e8eba6f7669`。
日志仅移除末尾空行；初次 staged diff 发现的两份日志 EOF 空行已据此规范化，
最终 staged diff 再检通过，生产源码与测试内容不受影响。

| 检查 | 结果 |
| --- | --- |
| 新 revalidation integration | 11 passed / 0 failed / 0 ignored |
| 原完整 capture integration | 11 passed / 0 failed / 0 ignored |
| 原 inventory resource integration | 3 passed / 0 failed / 0 ignored |
| 原确定性 mid-capture drift test | 1 passed / 0 failed / 0 ignored；402 filtered out |
| focused strict Clippy：上述三 integration targets + cc-index lib | exit 0 |
| workspace `cargo fmt --all -- --check` | exit 0 |
| `git diff --check` | exit 0 |

合计 26 个 scoped tests 通过。均为自编临时文件系统 fixture；没有执行其 Python 源码。
重新运行：先确保 cargo/rustup 1.95.0 可用，再执行
`python3 artifacts/checkpoints/python-inventory-revalidation-20261007/validate.py --target-dir /an/isolated/target`。

## 结果边界

收据不是签名、授权或离线声明缓存。owner/scope 和三组预算由本次调用方独立提供，
serialized fields 不能直接构造受信声明，始终只消费成功重建后的 `fresh.outcomes()`。
比对覆盖配置原 bytes/provenance、全部 regular-file inventory、完整 Derived/Unavailable
及空输出；wire/version/owner/scope/三个 digest 错配均有拒绝控制。

收据绑定内容而非历史 root inode/mount 连续性；可信 root alias 和同 owner 显式授权的
内容等价 root 遵循原规则。Linux/native 平台条件、完整 scope、原预算和非原子 snapshot
边界保留。收据应保存在 scope 外，避免其自身成为新 inventory 内容。

未进行非 Linux、public quality、100k、远程 CI、live provider、DB/MCP 接线或新 source
registry 独立接受。本实施仍待独立审查；P7-014 保持 in_progress，其余 parent gates
不随本 scoped 子项自动完成。
