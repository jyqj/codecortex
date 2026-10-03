# Direct writer 完整 schema 修复（2026-10-03）

实质结果：PR113 固定基线 `ee4c4fc0b41e298bf38b8269310053fa4b355c61` 的真实 canonical `IndexDb::rebuild_with_direct_writer` 原先返回 `schema tables: no such column: new.rowid`；修复后正常生成临时库并完成 swap/reopen。完整 schema 的触发器、视图和 seed 初始化按原始顺序执行，批量写入期间仅移除显式 indexes，完成后按 SQLite catalog 中的原始定义重建。自动 UNIQUE/PRIMARY KEY 约束 indexes 始终保留。

## 修改与约定

生产仅修改 `crates/cc-db/src/direct_writer.rs`。移除丢弃 CREATE TRIGGER 的 table 白名单。DirectWriter 将完整 schema 交给 rusqlite `execute_batch`（SQLite prepare 边界），从 `sqlite_schema` 获取显式 index 名称和 SQL，引用/转义名称后 drop，载入后重建。这样 seed 执行时的显式 UNIQUE index 和触发器语义与标准 normal temp path 相同，不用匹配 `new.rowid`。

仍被标准临时库路径使用的 `extract_index_statements` / `drop_index_statements` 保留 `String` 返回约定；调用者先执行完整 canonical schema，因此 SQL 错误先由 SQLite 返回。这两个 canonical-schema helpers 的语句切分改用 [SQLite 官方 sqlite3_complete](https://www.sqlite.org/c3ref/complete.html)，保留未加分号的最后输入及不完整 tail。其 index 分类/名称 helper 仍限于既有 canonical schema 语法；DirectWriter 的任意合法 quoted index 名称由 catalog 处理，不增加通用 SQL parser。`sqlite3_complete` 仅识别完整语句边界，不是语法校验；实际执行仍由 SQLite 验证。

public `write_db` / `verify` 的签名、CcResult 和原有 stage 错误传播保留；新增 catalog/drop 阶段错误标签。显式拒绝 embedded NUL：rusqlite/SQLite 原调用会在 NUL 处截断，否则 suffix 会静默遗漏。SQLite 接受的无末尾分号 SQL 与纯注释 tail 继续接受（包括 SQLite 本身允许的未闭合 block-comment tail）；不完整 trigger、字符串、语法错误、seed UNIQUE 冲突在执行完整 schema 时失败，callback 不被调用。

已亲读 `index_db_rebuild.rs` 与 generation finalization：direct callback 在事务内设置当前 `user_version`；原公共 rebuild 协议在 swap 前以 `max(floor, live)+1` 写 index/evidence epochs 并 renew incarnation；新临时库的 optional semantic clock 与标准路径一致为空。未更改迁移、版本、缓存格式或 swap 协议。

## 可重放限定验证

运行 `python3 artifacts/checkpoints/direct-writer-schema-repair-20261003/validate.py`。使用官方 `/workspace/.cloud-setup/install.sh` 已有配置：Rust/Cargo 1.95.0、`/workspace/.cargo`、`/workspace/.rustup`；未更换 HOME、权限或凭据。查找了 AGENTS.md / .agents / SKILL.md：仓库没有本地文件，工作区指导目录为空；已读 CONTRIBUTING.md。按用户限定范围执行测试与 cc-db scoped clippy，未委派子 agent。

`validation-receipt.json` 固定生产/测试文件 SHA256，记录逐项命令、真实退出码、非零测试计数及日志。最终 **13 passed / 0 failed / 0 ignored**：

- DirectWriter 与 canonical schema helpers：10 tests。
- 真实 canonical direct 与 normal temp rebuild 对照：1 test。用生产 `insert_file_data` 和有效 DocumentRecord 构建 files/chunks/document/symbols/literals，并写入 semantic 表。callback 内比较所有非 index schema 对象，检查 trigger FTS 已生效、显式 indexes 已移除，防止 reopen 补建 schema 掩盖缺陷。完成正常 swap 后比较完整 schema catalog（含所有 indexes）、13 个逻辑表的 rowid/rows、除 incarnation 外的 metadata；所有 canonical table 均可 prepare。五种 FTS MATCH 各有非空一致命中；schema version 当前值、epochs 7/11→8/12、新 incarnation 与 optional semantic clock 规则均有断言。
- 上述 canonical test 在完成 DB 上直接 INSERT/UPDATE/DELETE symbols/file paths，验证旧词消失、新词出现；在 FK 关闭的普通连接上证明 chunk_document_delete 触发器自身移除 document。生产 replacement/remove API 维护 application FTS，五组 source/FTS rows 全量相等，删除后 files/chunks/documents/semantic_manifest/symbols/literals 无 dangling rows，integrity_check=ok。
- 普通 callback error 和 canonical partial UNIQUE index 重建失败：1 test。错误准确返回，live rows/incarnation/epochs 不变，随后常规重试成功。这里验证的是正常 API 的 staging 隔离/不发布失败库，不宣称既有 journal_mode=OFF 对任意临时库故障有 rollback 保证。
- 原 standard temp rebuild epochs 回归：1 test。
- `cargo fmt --all -- --check` 通过；`cargo clippy -p cc-db --all-targets --locked -- -D warnings` 通过。

synthetic SQL 含多条 trigger body、CASE END、双引号/反引号/bracket identifiers、quoted index 中的空格/双引号/分号、strings/line/block comments 内分号、seed INSERT、view 和 UNIQUE index；逐条完整边界执行与完整 schema 执行一致，seed/audit rows、views、自动约束和最终 schema catalog 都有断言。独立错误用例包括不完整 trigger/字符串、完整非法 SQL/VIEW、NUL suffix、seed UNIQUE 冲突与 bulk-load 后 UNIQUE index 冲突。

原基线失败见 `baseline-reproduction.log`，最初复现 fixture 见 `baseline-fixture.rs.txt`；当时生产代码未修改，HEAD 就是指定 PR113。`repaired-tests.log` 是中间 direct_ filter 的 15/0（包含三个名称意外匹配的既有无关测试），不与最终 13 项重复计数。开发阶段纠正过 EmbeddingInput import、synthetic statement 计数和 probe 父表更新的 FK 约束测试设置；NUL success 则导致上述实质防截断修复。最终权威日志为 `validation-*.log`。

## 本块范围与状态

本块代码和限定验证完成。独立分支从固定 PR113 起步，不含尚未 accepted 的 FIFO PR114，也不导入 PR115 的 review 文件。只改上述生产文件、限定 `index_db_tests.rs` 和本 checkpoint。index_v1.sql/index_migrate/semantic_queue/cache/版本/DEV/CI/中央 ledger 未改；不关闭任何中央 gate。

没有运行固定 100k 实验、GC/WAL kill/crash/fault 注入、曾拒写 runtime 路径重试、merge、force push 或 deploy。不推断新 head 远端全 CI 已绿；远端 draft PR 的实际 CI 另由其 run/head 证明。本块完成交付即停。
