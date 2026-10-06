# C++ cv/ref 身份与 symbol/FTS 一致性：有界修复

基线为公开 commit `13bd9dbbe3f96b390f6adf34a95bb398da89bf4d`，精确树 `5c4c2d55422e99ea2aa53b4768ffee0af06b6926`。本交付仅包含已复核的定义身份、语义重建与 FTS 派生镜像修复。

## 行为与边界

在既有同文件 B1 owner proof 成立后，仅对普通显式 qualified 的 `ProvenType` 全局定义，读取已选定 function_declarator 参数后的直接 `const` / `volatile` 与 `&` / `&&` 节点。签名前缀逐字保留，后缀按 `const volatile ref` 单空格排序；空后缀不改 UID。未知、重复、缺失或错误 qualifier，以及此语境的 definition-local override/final，均保守弃权，不发布缩短的正身份。

未扩展声明匹配、inline、namespace qualifier、operator、template、wrapper 或 resolver。`cv()` 合法保留原共享 UID；const overload 获得新 UID，ref & / && 各获新 UID。B1 调用/引用仍不参与普通目标绑定。旧 inline Legacy::same 身份碰撞维持既有单 survivor 语义。

schema 27 → 28 触发语义 mismatch/rebuild；源 bytes/hash/mtime 不变也重新解析，不能通过 row backfill 代替。真实合成 v27 fixture 对四个定义保存两个 SQL/public survivor；升级后四个定义均有准确 SQL、source-bound association 与公开 hydration。

## FTS 生命周期阻塞与修复

原始保留条件在 final delete 暴露已有问题：SQLite 默认 recursive_triggers=OFF，INSERT OR REPLACE 的隐式删除不执行原 DELETE trigger，遗留 symbols_fts rowid，之后 dirty rewrite 重用 rowid 导致 constraint failed。真实 v27 与仅 Legacy 的 v27/v28 最小案例均复现；迁移及原失败证据未改写。

修复使用带 UNIQUE 自动索引的派生 symbols_fts_keys 与 AFTER INSERT/UPDATE/DELETE 镜像维护，保留 REPLACE、IGNORE、FAIL、ABORT、NULL 和多键冲突的既有 SQLite 语义。该表不定义 symbol identity，新增 O(n) 派生存储；没有实测大工作区性能声明。

## 本次验证

官方 Rust/Cargo 1.95.0，locked/offline、单 job、debug=0、incremental=0；实际源码、compiler/link 与运行工件重新绑定验证。

- 最终作者限定检查 66/66，0 ignored：生命周期 4、FTS 6、schema 5、B1 DB guard 8、既有 B1 生命周期 2、cv/ref parser 14、namespace parser 17、DirectWriter 10。
- 未放宽的独立原验收 85/85：parser 82 与真实 v27 persistence/lifecycle 3。
- 独立 FTS SQL conflict matrix 与实际重建的 Rust 8 项通过；保留原红条件、补充条件和旧失败记录。
- 原 final delete、scalar/batch dirty、hot no-op、reopen、const→volatile edit、rename/delete、source/proof rollback 均通过。

可复现的真实合成 v27 输入位于 `crates/cc-index/tests/fixtures/cpp-cvref-v27/`，包含固定 bytes/mtime、基线 survivor/public audit 与 DB provenance。交付不携带运行日志、编译 binary 或私有工作区数据。

## 未关闭的范围

192 项任务状态保持原样。source registry v5、CI 与旧证据不改；source integration、完整 public/NL quality、P7/V19、formal eval/100k 仍开放。未运行全 server/workspace、已排除的 semantic-runtime paging test、private/production 或破坏性故障测试；本结果不代表这些范围通过。
