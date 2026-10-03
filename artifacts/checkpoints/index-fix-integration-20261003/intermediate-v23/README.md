# 中间 v23/v2 缓存边界：已保存，迁移假设待验证

生产执行树：`57bedbaf193e28be34271a290d28c90dc662b613`。default features，无 semantic/semantic-http；二进制 SHA256 `3f9b5fd6cec0f231131961cb125576fb5f2f462a264c147c7306ab6a87103fc5`。当前 HEAD 的生产 src/Cargo 与该执行树相同。

五份源码分别包含 `_`、`__`、`$`、`℘`、`℮` 合法类型；四个 Python 用例本地 ast.parse 通过。通过真实 MCP full index 各生成 schema23、manifest2 数据库；uses_type 都为0。同一中间二进制重新打开、full:false 索引后审计状态完全相同。raw RPC、generation、file hash/mtime、依赖和缺边计数见 receipt.json。

每个 `.sqlite3.gz` 是成功 no-op 后用 SQLite 在线 backup 只读导出的实际数据库，压缩前SHA见 receipt。运行目录 `/workspace/index-fix-intermediate-cache/<label>` 保留原源码mtime与原数据库，供后续新build原地升级；未删除缓存、改缓存key、PRAGMA version 或依赖。生成脚本 capture.py；首次诊断 SQL 把relation_kind误写kind，只有审计失败，未改变库；纠正后完整捕获五组。

核查到的现有契约：`cc-db/src/index_migrate.rs::migrate_index_db` 在 stored==CURRENT_SCHEMA_VERSION 时 UpToDate；`cc-model/src/resolution.rs` 只严格校验 manifest 固定版本；`cc-index/src/indexer.rs::diff_scanned_files` 和 scoped 路径只比较源码状态、chunk_policy、document_spec，没有解析器/ resolver 生成算法 fingerprint。

因此，当前证据支持验证“同v23/v2修复后，未改源码的缓存是否no-op保留”这一假设；尚未执行通过独立复验的修复版，不能称修复版迁移已失败。等待 `671063b11af8cb40a0d526098de82e684dd24aca` 独立复验通过后，实际验证中间树→修复版、PR101→修复版；schema/manifest升级或其他语义失效机制的最终选择由父判断。

全部10次调用只有index，无search/live provider/holdout。未尝试已Forbidden的PR发布动作。
