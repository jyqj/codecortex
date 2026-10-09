# 同连接 prepared statement 复用：非作者静态审查

接受范围：`accepted_scoped_for_engineering_CI_admission`。固定候选 `41236c3eb6e43d8c6f0fc489d5192cf14483069c`，parent `921d81174daec7408551f4236767c865c9bf9f5b`，基础 G4 `260f596582f2d82b8d7c707b61a6b8b6a43b069f`。没有发现阻止隔离工程 CI 的生产语义问题。未编译、未执行 Rust 测试、产品或规模测量；此结论不是源码发布审批、性能通过、P8-005/006 完成或新研究授权。

## 实际生产变化

仅两文件、三处原 `Connection::execute` 调用改为既有 `IndexDb::execute_cached`：

| 位置 | SQL 与作用 | 核验 |
|---|---|---|
| resolution_dependency_store::replace_on | DELETE 该 file_path 的 resolution_dependencies | 原 SQL、路径参数、归一化/验证/payload 预算在前、后续执行顺序不变 |
| 同函数 | INSERT OR REPLACE resolution_manifests | 原四个绑定含版本、payload、BLAKE3，参数表达式与错误传播不变 |
| symbol_identity_store::insert_on | INSERT chunk_symbol_identity | 原七参数含文档版本和序列化 identity；全部形状/源/声明/owner 验证、SQL survivor 检查与插入后完整 load_on 回读仍在原位置 |

独立 `verify_static.py` 对 G4 与候选 Git blob 作精确变换比对：除 import、这三处调用方式及格式空白以外，两个完整文件 token 序列相同，SQL 字符串和参数表达式逐字保持。helper、调用者的事务代码、epoch/schema/DDL、Cargo.lock、原 scale driver/capacity/oracle 测试输入均逐 Git blob 不变。

既有 helper 是 `conn.prepare_cached(sql).map_err(db_err)?` 后 `stmt.execute(params).map_err(db_err)`。参数泛型仍是 `rusqlite::Params`，原 `[&file.rel_path]` 和 params! 表达式未改；仍使用传入的同一个 Connection/Transaction。函数返回前局部 statement drop，不把读连接、事务、行快照或应用数据缓存带到下一次 prepare。

我还读取了锁定 rusqlite 0.40.1 的实际本地源码，并将原 `.crate` SHA256 与 Cargo.lock 相等、三个所读成员与 archive 原字节相等核验：Connection::execute 原来也是 prepare 后同一 Statement::execute；execute 会绑定参数并执行/重置；CachedStatement drop 归还当前连接的缓存并清空绑定。不是跨连接或跨库复用。缓存容量仍为原 writer/staging 的 64 条；没有承诺无缓存逐出、零新增内存或 SQL 执行次数减少。

full staging 原协议完全未改：私有 WAL 库、原 pragmas、删除显式索引、同一事务内写入、重建索引、恢复 pragmas、关闭连接、原发布 generation fence。incremental replacement 同样保持 write mutex、IMMEDIATE transaction、错误即返回触发 rollback、commit 后 epoch 语义。未删除每行身份回读，未跨 prepare 持有数据缓存。

## 测试范围与发现的收口

921 的两项现有测试增加了真实 SQLite 行为断言，未删除旧断言：

- resolution：原批次中先写 ok.py、再由 bad.py 的 manifest 验证失败触发 rollback；旧 generation/文件/manifest 保持，然后同 writer 换不同 payload/依赖并 reopen。
- identity：原伪造 qname 拒绝、旧 generation/源文本/identity 保留，然后同 writer 写两个真实解析的不同 identity，reopen 后按对应 chunk_id 查询各自 qname。另一个原 duplicate-symbol SQL-survivor 测试原字节保持。

独立审查与 root 均发现 921 的窄缺口：新 manifest 正确不代表 reverse dependency 表已经移除旧 rows，最后 remove_files 会掩盖这个问题。412 仅增加测试断言，生产两文件与 921 完全相等：reopen 后、remove_files 前，旧 NameBucket(missing) 与 MissingPath(api.py) 反查必须为空，同时新 MissingPath(replacement.py) 必须仍返回 use.py。此位置能检测 DELETE 误绑到先前 rollback 路径而留下旧 reverse rows 的行为。

这些测试检查持久化、事务失败后的复用和可观察的依赖/身份关系，不是检查内部是否调用 prepare_cached。覆盖并非“所有 SQLite 故障类型”；不以静读替代待执行测试。

## 建议最小隔离工程验证

可将固定四文件候选用于一个独立 branch 的一次工程 CI，保留固定源码前后快照、完整命令/退出/测试日志。不要接上正式 150-cell study，也不要修改 PR160 的原 gate。现有 v15 pins 不会自动批准这个新产品源码。

第一层使用作者已列出的十个测试命令，覆盖受影响的 public write/读取证明、原 full-staging fence 和原 oracle：

```sh
cargo test --locked --offline -p cc-db --test p2b_resolution_store
cargo test --locked --offline -p cc-index --test qname_identity_transaction
cargo test --locked --offline -p cc-index --test qname_identity_proof
cargo test --locked --offline -p cc-db --test public_surface_store
cargo test --locked --offline -p cc-db --lib full_rebuild_advances_both_epochs_past_previous_values
cargo test --locked --offline -p cc-db --lib rebuild_generation_exceeds_writes_committed_during_rebuild
cargo test --locked --offline -p cc-db --lib staging_checkpoint_busy_preserves_wal_and_refuses_replacement
cargo test --locked --offline -p cc-db --lib schema_mismatch_rebuild_advances_generation_past_old_values
cargo test --locked --offline -p cc-eval --test p8_scale
cargo test --locked --offline -p cc-eval --test benchmark_oracle_streaming
cargo fmt --all -- --check
cargo clippy --locked --offline --workspace --all-targets -- -D warnings
```

CI 可以先按原 lock fetch 依赖。四个过滤的 lib 命令必须实际各运行其目标测试，不能用零测试 exit0 充数；完整 target 的原测试/ignored 逐项保留。上述命令只是计划，没有执行结果。若以后做 release 性能诊断，需要独立固定 build receipt/source 和原预算，结果不得重标为 G4/D0；此静审没有批准任何新的诊断负载。

157.185535 秒只是既有 D0 10k 的完整 full-staging 观测。没有原件把其中多少时间归因 SQL 编译，所以本次不声称加速比、绝对节约时间、解决 100k 或保证 300 分钟内完成。

新增 TODO 完成 0；剩余 29。
