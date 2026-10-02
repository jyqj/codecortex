# 两个 CI 架构阻塞的有证据修复

本分支基线 `c38f07803a9d262b2ee83ff72e19b0a1409ae730`（PR29 receipt 接线，叠于 PR26 lint 修复）；生产来自 `993a64e`。只改对应声明文档、两个 guard 和专属验证，不改 DB 迁移、runtime、query 路径、Cargo、权限或主账本。PR25 三条冻结反例与 PR30 修复后复核均不受影响。

## schema 22 的实际来源与升级契约

- 旧声明 `docs/internals/MODULE_CAPABILITIES.json database_schema=21` 来自已提交 P5 检查点 `0a56a257f9a92c54d06ea5be0ce1d1763917a527`。
- 已提交 P6/P7 检查点 `ff458bc591b4e7e444af4464d6eef2513cdb335c` 的 `crates/cc-db/src/index_migrate.rs` 定义 `CURRENT_SCHEMA_VERSION=22`、`ADDITIVE_MIGRATION_FROM=21`。`crates/cc-db/src/sql/index_v1.sql` 新增 semantic manifest/outbox/space 三表及索引；v21 在原库应用 additive DDL，其他非匹配版本仍要求 rebuild。
- 专属 `crates/cc-db/tests/ci_schema_guard_contract.rs` 实际运行 **3 passed / 0 ignored**：新 IndexDb 初始化 v22；真实历史 v21 schema 升级为 `Migrated { from:21 }`，保留旧 sqlite_master 对象、行、FTS、incarnation 和 epochs，重开为 UpToDate；v20/v23 仍为 Mismatch。
- 历史 fixture 是从上述 `0a56a25` 原 SQL 原样提取，而非把当前 schema 换个 user_version。文件 `crates/cc-db/tests/fixtures/p7-ci-schema-v21.sql` SHA256 为 `00a7d1fc249c562160fc0cdc7978ba2198b4ee063f8c6cf77cc564dd3f27407f`，receipt 验证字节相等。
- 声明同步为 22，并记录 additive predecessor 21 和实际契约测试。module guard 同时校验当前版本、生产 predecessor 与契约测试存在；负测证明错填版本或 predecessor 会失败。不是只把数字改到当前。

## generation guard 的实际迁移与保护意图

旧 `scripts/check_source_architecture.py:48` 要求 `handlers/context.rs` 出现 `validate_envelope_generation`。P5→已提交 `ff458bc` 的 diff 已删除 dispatch 后单次硬检查，改用 assembly 内 fence 与 accepted-generation freshness 注解：

1. `crates/cc-server/src/engine.rs` 中 QueryHandle 的 `search_in_context_with` 在 `with_stable_generation` callback 内调用 `assemble_context_once`。
2. `crates/cc-search/src/engine_cache.rs` 的 fence 读取完整 before/after generation，只接受相同 generation 的工作，变化时丢弃/有限重试。
3. assembly 的 hydrate、selection、pack 和 `EvidenceHydrator::finish()` 在同一 generation 内；`crates/cc-search/src/evidence_hydrator.rs` 的 finish 比较完整持久 generation。
4. async handler 经 `QueryHandle::search_async` 进入上述路径；`handlers/context.rs::finalize_search_response` 从 envelope 提取 accepted generation，交给 `handlers/freshness.rs::attach_observed`，最后施加输出预算。

保护意图仍是拒绝混合代际结果，但**并非旧错误语义逐字等价**：fence 接受后的序列化窗口变化由 freshness 显式注解，不再制造单次硬失败。已有 accepted-window 单元测试实际通过；本块不恢复旧 helper 或改生产。

新的 source guard 按这些具体 production 函数体、callback 和调用顺序检查，不凭全文件泛化字符串；token reader 排除注释和 cfg(test) 项，字符串中的括号不污染函数范围。专属负测覆盖真实 fence/finish 缺失、比较反转、错误 hook/路径、注释与同名测试函数假补位。`scripts/tests/test_p7_ci_architecture_guards.py` 的 **5 个测试通过**，source guard CLI 会执行这些负测。该窄结构检查仍不冒充全程序控制流证明。

新增 `crates/cc-eval/tests/ci_generation_guard_contract.rs` 实际 **2 passed / 0 ignored**：稳定 async handler 返回单一 accepted generation，源码证据按 fixture 原文验证；另一查询在受控 recall 等待中真实写文件并通过 typed rebuild 提交新 generation，恢复后拒绝旧 recall generation 为 RetrievalChanged，未拼接混代结果。另执行 4 个 engine generation-fence 回归及 1 个 serialized-window 回归，全部通过。

## 完整重放与边界

按 checked-in CI stdio 步骤实际重放 **17/17 passed，blocked=0、not_run=0**，包含原 P3-D/P4-A 架构检查与各自 stdio。`ci-stdio-replay.json` 保留原命令、实际命令、退出码、时间；仅将默认 binary 路径映射至 `/tmp/p7-017-build`，保留 `RUSTFLAGS=-D warnings`，缓存环境离线。默认及 semantic 构建 receipt/观察分别在 `product-*`；没有提交 ELF。

专属 DB/eval clippy `-D warnings`、全仓 fmt check、diff check通过。没有重复全仓 test/clippy或认证整个远端 Actions job；这些不是本块 17 stdio 步骤的通过证明。全部 fixture 合成，HTTP 用例只 loopback，无真实/付费 provider；普通执行不宣称 network namespace 成功。更广 P7-014/017、V18/V21 状态由主集成者决定，本块不修改主账本。

重放入口：`python3 scripts/check_module_architecture.py`、`python3 scripts/check_source_architecture.py`；Rust 专属 target 为 `ci_schema_guard_contract`（cc-db）和 `ci_generation_guard_contract`（cc-eval）。
