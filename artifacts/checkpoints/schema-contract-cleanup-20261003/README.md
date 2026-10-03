# schema 声明契约清理（2026-10-03）

基线：`29b03a0fec6bfde92aac9c41dce996ebb2d08cc7`（statusv2 为作者交付，不代表独立通过）。本块只修 MODULE_CAPABILITIES 与 declaration checker 的过期 adjacent-additive 假设。PR105 的 CI215 job111147110042 在此 checker 失败、后续步骤跳过；此问题与原 all-features 四失败分开。

## 核心取舍与具体变化

- `database_schema: 22` → `24`，与生产 `CURRENT_SCHEMA_VERSION` 对齐。
- compatibility 的 `additive_migration_from: 21` 与旧 `introduced_by` 删除，改为唯一合法 `policy: rebuild_on_mismatch`；保留真实 Rust guard 路径。
- checker 删除 predecessor+1 与已移除 `ADDITIVE_MIGRATION_FROM` 常量的强制要求，改为校验整数版本与生产常量相等、compatibility 恰含 policy/contract_tests、policy 合法且 Rust guard 引用准确。拒绝缺失/未知/互相矛盾的声明，包括正确 policy 下残留 additive 字段。不预设当前未使用的 policy enum 或兼容分支。
- 声明负向控制覆盖错误/缺失 schema、错误/缺失 policy、残留 additive 假定、错误/缺失 Rust guard 引用及缺失生产版本常量。测试仅变更内存副本。

这是声明一致性检查，不是动态迁移证明。生产迁移源、schema24 版本、Rust guard/fixture 均未修改。`ci_schema_guard_contract.rs` 与 `fixtures/p7-ci-schema-v21.sql` 路径已核实；guard 保留 fresh v24、真实历史 v21 文件重建、incarnation 更新、epoch 提升、semantic manifest 对象及 reopen 稳定性断言。v22/v23 mismatch 与其他版本仍由 guard 和迁移单元测试检查。

## 有限验证

所有以下命令退出 0，日志均在本目录（Rust 日志仅去除终端末尾空行；implementation.diff 用零行上下文，避免证据文件自身引入空白告警）：

```sh
python3 scripts/check_module_architecture.py
python3 -m unittest scripts.tests.test_p7_ci_architecture_guards.SchemaDeclarationMutations -v
python3 -B -m unittest scripts.tests.test_p7_ci_architecture_guards -v
CARGO_HOME=/workspace/.cargo RUSTC=/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/rustc /workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/cargo test --locked -p cc-db --test ci_schema_guard_contract
CARGO_HOME=/workspace/.cargo RUSTC=/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/rustc /workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/cargo test --locked -p cc-db --lib index_migrate::tests
git diff --check
```

结果：checker passed；声明测试 5 passed；整个 Python guard 文件 9 passed（包含该 5 项）；Rust guard 3 passed；迁移单元测试 5 passed / 177 filtered out。默认 features，Rust/Cargo 1.95.0，locked 依赖，官方 crates.io 与已有可写 `/workspace/.cargo` cache。直接使用已有真实工具链 binary，无权限修改、registry 换源或持久凭据。

未发现 `/workspace` 或仓库的 AGENTS.md/本地 SKILL.md；`.agents`/`.codex` 为空。已读 CONTRIBUTING.md，按中文文档约定记录；现有云 skills 没有直接适用本限定源码修复者。全仓检查、文档基线重写与跨 crate 运行受本任务明确有限范围约束，未执行。

不改 statusv2、query、cache/GC/WAL 或根 ledger。不运行 fault、拒写 semanticruntime、live/heldout 或全量测试。原 **2474 passed / 4 failed / 68 ignored** 不被本块通过结果覆盖。本块有限验证不代表整体 CI/发布通过。仅独立 draft PR；不 merge/forcepush/deploy。远端提交与 PR 核验结果在交付回复中给出。
