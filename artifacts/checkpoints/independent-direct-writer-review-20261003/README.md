# PR117 DirectWriter 独立审查（2026-10-03）

结论：**boundedpass**。固定候选 `098ebd9c08031e0b652e7b021d8abbc9e8b19c3d` 相对固定 PR113 `ee4c4fc0b41e298bf38b8269310053fa4b355c61`，本审查范围没有发现需 reject 的实现反例。没有关闭中央 gate，也没有给一般 swap/crash 保证背书。

独立新写 `driver/src/main.rs`，通过公开 `IndexDb::admin()/writes()/reads()` 调用固定生产库；未运行作者 13 项 fixtures。canonical 检查使用两份有效 FileWriteUnit、独立 SourceSnapshot/DocumentRecord/EmbeddingInput、symbols/literals，以及纯 synthetic 的 semantic storage rows；没有 encoder/provider 请求。只有指定 checkpoint 有工作文件改动。源码副本、原始失败库、可执行文件及编译缓存保存在被本目录 `.gitignore` 排除的 `local/`，生产 checkout 完全未改。

## 执行结论与有效负对照

最终同一驱动 **6 个 case groups 全部 exit 0**，不是 6 个 Cargo tests，也不累加中间重复成功。每项 binary/source/log 的 SHA256、命令及退出码见 `review-receipt.json` 和 append-only `commands.jsonl`。

| 检查 | 独立断言与结果 |
|---|---|
| 真实 canonical direct → normal 对照 | callback 开始时比较完整非显式-index catalog，载入后检查五 FTS/source 全行与非空 MATCH。两种路径的 callback 数据和最终所有 table rows（含 FTS shadow tables）相等；仅剔除预期变化的 incarnation，fixture 的 indexed_at 固定。user_version=24，index/evidence epochs 41/73→42/74，incarnation 更新，semantic_epoch=None。 |
| reopen 不能掩盖缺失 | production callback 在 reopen 前断言；另 standalone canonical 生成完成文件后用普通 SQLite 打开，未经 IndexDb ensure/migrate，完整 catalog 与 SQLite 执行原 schema 的 oracle 一致。162 objects = 60 tables、95 indexes、7 triggers；68 显式 indexes 恢复、27 自动约束 indexes 保留。canonical 当前无 view/seed SQL，另用独立 synthetic 覆盖它们。 |
| 正常写入后的 FTS | source/symbol/file-path trigger INSERT/UPDATE/DELETE；文件路径 probe 在普通 synthetic 连接 FK OFF 下暂改再恢复，以隔离 trigger。公开 replace/remove API 后五组 FTS 与 source 完整投影、rowid 均一致，旧词消失、新词非空。最后 files/chunks/symbols/literals/documents/semantic_manifest 无残留，integrity_check=ok。 |
| normal helpers | 对 actual canonical schema 执行 drop 后 catalog 必须只少全部 68 显式 indexes，重建后精确恢复。含完整多语句 trigger 的独立 SQL 中，字符串 `prefix;CREATE INDEX forged...` 不能泄漏进 index 集合，且无末尾分号的真实 index 被保留。 |
| synthetic SQL | 多语句 trigger/CASE END、Unicode 和单双/backtick/bracket 引号、注释和名字/字符串内分号、双引号转义、view、无分号尾句；callback seed/audit rows 与 SQLite 按原始顺序执行的 oracle 一致，bulk 后全 catalog/data 一致。quoted explicit unique/index 恢复，automatic UNIQUE 在 callback 内仍约束。 |
| 错误与边界 | 不完整 trigger/string、非法 index SQL、seed automatic/explicit UNIQUE 冲突均报错且 callback 未调用；SQLite 接受的 line-comment/未闭合 block-comment tail 和无分号尾句接受；NUL 在 suffix/comment/string 三处均在打开目标路径前拒绝。 |
| 生产错误隔离 | callback 预期错误和 semantic_outbox partial UNIQUE 重建真实冲突返回错误，live catalog、所有逻辑 rows、完整 generation 不变，旧词仍非空；正常重试成功。另 standalone UNIQUE 重建错误后 `path_exists=true`，这是 standalone 文件构建契约的残留，未称 live 发布或损坏。chunk_document_delete 在 FK OFF 下确实删除 document。 |

固定 base 使用最终同一驱动真实 exit 101，准确重现 `schema tables: no such column: new.rowid`。三个仅作用于本目录源码副本的限定 mutants 全部成功编译，然后被对应断言拒绝（exit 101）：

- `mutant-no-triggers`：完整 schema 执行后删除全部 triggers；在 production callback **写入前**的 catalog 检查失败。
- `mutant-split`：只把 helper splitter 退化为分号切分；把 trigger 字符串中的 `forged` 误认作 index，被实际 drop 集合断言拒绝。
- `mutant-no-nul`：只取消 DirectWriter NUL 拒绝；SQLite 实际返回成功并静默省略 suffix，触发 `NUL silently accepted: ()`。

这三项证明本驱动能抓到丢 trigger、错误边界与静默截断；不外推任意 mutation coverage。负对照源码变异、固定 source hashes、binary hashes 与失败日志全部留证。

## 源码与 FFI 审查

生产差异仅 `crates/cc-db/src/direct_writer.rs`，另作者 tests 和 checkpoint；固定 diff 不含 FIFO。完整 `execute_batch(schema_sql)` 替换白名单过滤，因此 trigger/view/seed 没有被筛掉，seed 按原始 index/trigger 创建次序运行。随后从 `sqlite_schema` 收集 `type='index' AND sql IS NOT NULL` 的实际名字与 SQL，catalog statement/rows 在 drop 前已释放；自动 UNIQUE/PRIMARY KEY indexes 不进入 drop 集合。名字以双引号包裹，并将名字内 `"` 变成 `""`，本次真实 quoted-index synthetic 成功覆盖。

normal helpers 保留 canonical 范围：`classify_statement/index_name` 仍不是通用 SQL parser，不承诺任意 block-comment 前缀或带空格的 index 名。actual canonical 的全部 drop/create 集合已精确验证。normal 路径先执行完整固定 schema，因此不靠 helper 做合法性判断。

`split_sql_statements` 在 ASCII `;` 字节处取 UTF-8 slice，该位置不会切开多字节字符。每轮 `CString` 是局部 owned 值，`sqlite3_complete(c_sql.as_ptr())` 调用期间存活；返回之后才离开本轮，未保存悬空指针。NUL helper input 保留 tail 的行为不代表任意 helper NUL 契约；normal 实际输入是无 NUL 的 canonical 常量，public DirectWriter 则先显式拒绝 NUL。`sqlite3_complete` 仅决定完整边界，实际 SQL 仍由 SQLite 执行校验。

## 固定环境、重放与失败留存

官方真实 binary `/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/{rustc,cargo,rustfmt}`；rustc 1.95.0 (`59807616e1fa2540724bfbac14d7976d7e4a3860`)、cargo 1.95.0 (`f2d3ce0bd`)；native SQLite 3.53.2。没有调用默认 rustup proxy 或 install.sh，没有改 HOME、权限、身份或凭据。缓存使用既有 `/workspace/.cargo`，缺少的两个固定依赖从正常 crates.io 获取。driver Cargo.lock 的全部 109 个选中 registry package/version/checksum 均属于固定仓库 Cargo.lock。compiler、cargo binary SHA256 在 receipt。

候选 binary SHA256：`2a0373187e56e22845e2806bcc7fc5fd044494a06b3eb1be546f637321cb9a8d`。
固定生产 writer SHA256：`f503106e972535c273bcd1bebb34b844e0f1e3091ae33a9f39f6854590c292be`。
最终驱动 SHA256：`ec99a9453a815147eb9e0e3f142e222b057b1dbbc5dc0bd960a5a1545363750b`。

`prepare.py` 从固定 git objects 抽取 cc-db/cc-model 和 manifests；只调整副本 workspace members，使依赖编译限于这两个 crate，生产 Rust/SQL 文件逐字核 hash。driver 同时编译 actual cc-db 生产库与同字节 writer module（后者仅用于 crate-private helper 检查）。作者 tests 未执行。原 checkout HEAD 保持 `ff458bc591b4e7e444af4464d6eef2513cdb335c`；目标固定 SHA 不由此旧 checkout 推断。

```bash
python3 artifacts/checkpoints/independent-direct-writer-review-20261003/run.py base
python3 artifacts/checkpoints/independent-direct-writer-review-20261003/run.py mutant-no-triggers
python3 artifacts/checkpoints/independent-direct-writer-review-20261003/run.py mutant-split
python3 artifacts/checkpoints/independent-direct-writer-review-20261003/run.py mutant-no-nul
python3 artifacts/checkpoints/independent-direct-writer-review-20261003/run.py candidate
python3 artifacts/checkpoints/independent-direct-writer-review-20261003/audit.py
```

driver/candidate writer 的直接 rustfmt `--check` 均通过。检查 `/AGENTS.md`、workspace/repo AGENTS、`.agents/.codex` 与固定树 AGENTS/SKILL：没有本地指令文件，工作区指导目录为空；已读 CONTRIBUTING。没有引入多 agent。

初期有 5 次驱动失败：旧 facade 入口编译失败、Document policy 长度、file provenance 未对齐、FTS shadow WITHOUT ROWID 查询、parent-key FK probe。每次真实 exit 101 日志及其精确源码副本均保留，hash 在 receipt 可映射，不将它们当实现反例；没有删原失败、降低关键断言或计入最终 pass 数。format 后的最终六组执行与 base/mutant 使用同一驱动 hash。`audit.py` 核对这些日志/source/binary、全部未改的 production 文件及依赖身份。

## 范围与停止点

未测试 FIFO、100k/吞吐、GC/WAL kill/crash/fault、一般 swap/crash/任意 temporary rollback、真实 provider、DEV/heldout、远端全 CI。没有触碰曾拒写 cache、权限/凭据/安装流程，也没有修改生产、版本、DEV、CI 或中央 TODO。journal_mode=OFF 维持原生产约定，本审查只证明普通 synthetic API 错误的 staging 不发布。

本独立单块验证已完成，具体待办见本目录 `TODO.md`；交付证据 commit 与自有 draft PR 后即停，不 merge/forcepush/deploy。
