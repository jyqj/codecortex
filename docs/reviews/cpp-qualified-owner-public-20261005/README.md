# C++ 同文件显式限定函数身份修复

产品源：`c9ea86ed80f814f2c4a7686f00deaf8689291fb8`，基于 published v5 `9641add263f18dc840562e46cd388afaea6796a4`。

## 范围

只处理全局位置的普通显式限定函数定义：从同文件、在定义之前完成的 AST 声明证明完整 namespace 或直接 class/struct owner，输出正确 Function/Method、完整 qname 和对应 UID。不同 owner 的同尾函数不再共用 UID。

未知、冲突或无法完整证明的 owner 不发布 qname、UID 或公开声明关联。持久 eligibility 和终态拒绝覆盖普通解析、resolver/type backfill、依赖重解析、重开及通用 synthesis/config/infra 绑定；不据短名恢复 B1 调用目标。准确的 caller/env 源码归属保留。

数据库语义版本为 **27**。真实 v26 缓存需要从源码重建，即使文件内容与时间戳不变。原有 scope-A namespace 函数和 `::f` 这种没有 named owner 的全局函数行为保留。

## 验证

- 作者最终 source/link-bound 复跑：296 项限定测试通过，0 failed/ignored；Rust 1.95，实际库/test artifacts 强制重建并核对链接 hashes。
- 独立 r2：142 项验收测试与 8 组精确基线比较通过。两组统计不相加。
- 真实 owned-v26 →27 验证覆盖原样 source bytes/mtime、旧 UID 清除、跨 owner 双行存活、公开 identity、终态拒绝、no-op/reopen、rename/delete 和 generation。
- 七种其他语言默认 records 与旧版一致；production strict lint 与格式检查通过。
- r1 曾把 `::f` 误纳入 B1；发现后其阶段性接受被取代。最终 r2 独立验证 global identity、SQL/public association 和同文件调用恢复为 v5 行为。

## 迁移测试输入

公开测试输入包含一个 823296-byte 的 SQLite 二进制 fixture：`crates/cc-index/tests/fixtures/cpp-qualified-v26/index-v26.dbfixture`。它是发布 v5 对四个自建合成源文件生成的真实 schema26 数据库，用来验证 unchanged-source 升级，不是构建产物。编译可执行文件、rlib/rmeta、完整本地缓存和原始重复 archives 均不在交付范围。

数据库及随附四份元数据已逐列/逐值和原始页审计：1,379 个 TEXT 值及 87 个 FTS BLOB 均已检查，未发现本地绝对路径或私有项目内容。fixture 未经 VACUUM/逻辑重写；页内空闲区域仍有同一合成运行的旧 schema/索引记录，保留原始字节以证明真实升级。SHA256：`1043bf9a862dd93f6f8e2286e7095085d4f383437f41ff98cf12a77153668767`。

## 保留限制

相对定义、nested type、template owner、friend/anonymous owner、完整 C++ name lookup 和跨文件调用召回不在本次正向支持范围。遇到无法完整识别的冲突来源可能保守省略同文件其他 B1 身份。

**同一 owner 的 cv/ref overload signature 债务未修复。** `cv()`/`cv() const` 与 `ref() &`/`ref() &&` 两侧比较均为两个 parser definitions、一个 SQL survivor、一个公开 chunk；幸存 symbol/chunk ID 与 v5 相同，没有新增公开 association。不能将本次跨 owner 碰撞修复称为完整 overload identity 修复。

未宣称全工作区、服务器、真实用户项目、总体检索质量或规模性能验收。v5 source registry、CI 与旧证据保持不变；后续 source integration 单独进行。P7/V19、任务状态和整体质量验收仍开放。
