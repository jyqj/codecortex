# 真正的 v26 合成迁移输入

`index-v26.dbfixture` 来自原封不动的发布 v5 `9641add263f18dc840562e46cd388afaea6796a4` parser/indexer，而非修改新数据库 user_version 的模拟。SHA256 `1043bf9a862dd93f6f8e2286e7095085d4f383437f41ff98cf12a77153668767`；823296 bytes，schema26，无 cpp_qualified_owner 列。四个合成源文件的原文/纳秒时间戳和真实旧输出预期随附。

生产者：隔离 native worktree 中的窄 `cpp_qualified_owner_v26_fixture` 测试，1 pass；缓存位于任何 Git checkout 之外，co-change rows=0；integrity/FK checks 通过。原始 parser 输出有8个 qualified definitions、两对真实 UID 碰撞，SQL各存活一行。源文件未变时 no-op/reopen 均解析0文件。

完整构建器/工具链/hash/命令证据留在本次实施的 owned v26 证据目录。此副本仅供 test 创建自己的 tempdir 后从实际旧 schema 升级；不得读写用户缓存。没有 C++ 编译或执行。
