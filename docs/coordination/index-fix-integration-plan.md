# 唯一索引修复集成计划

基线：`ace2bc7983be2955831c9384e44d1bdd0749c909`（PR92）。输入：Requests103 `da5b05ee08d84fd336d5a98f25da7cae176daebf`，Go102 `99a2c0426054ddbed7edf42de43593a780a76a49`。远端核验通过；组合无冲突，首个组合 SHA `c2074b43aa58e75c9a569c021f05e7d2cd11028f`。

本任务新增生产写权只有 `crates/cc-db/src/index_migrate.rs` 和 `crates/cc-model/src/resolution.rs` 的版本边界。前者 schema 22→23，复用现有重建失效，不再让 v21 additive 绕过算法升级；后者 manifest 1→2。旧 parse/derived 数据实际为 DB 内持久化行，基线没有独立 parse/derived artifact cache API。不引入假缓存 key 或新 unsupported metric。

迁移测试写权：`crates/cc-db/tests/ci_schema_guard_contract.rs`、`crates/cc-db/tests/semantic_schema.rs`、必要的独立升级测试/脚本。保留真实历史 v21 SQL，另用基线二进制实际生成 v22，禁止改 user_version 假造旧构建。

验证：两个固定公共源码仓库的旧负例、新 full index；可成功形成旧缓存的子集 old→new reopen→incremental reindex→no-op；manifest 版本、generation/旧查询 handle 边界；格式、clippy、workspace 回归及适用 CI。只读两个修复自带开发证据，禁止读取隔离 holdout。真实 provider 调用数必须为 0。

不改 `index_db_edges.rs`、作者三个生产函数。2026-10-03 正式移交 ledger；本轮只写可证实的集成验收，不关闭 live/heldout 或性能质量 gate。两项独立 review 待其他新 session 提供。只创建综合 draft PR，不 merge/force-push/deploy。

正式移交后基线改为 PR101 `574f7598662334c63e020da136c87f4f7281554d`，新组合 SHA `fb73dd7fed5fadbd1b0b2fab54f754968505749e`，保留 PR101 `c70c68f` 原优化函数。旧基线 worktree 专用于实际旧负例与旧库生成。

当前验收阻塞：Requests 独立 review BOUNDED_REJECT，合法符号类型过滤回归；两个函数已经临时交新修复owner，本任务不改。Go独立review PR104 `06478892104347a846cdfdb81f843574e188634b` 远端核验一致。旧pinned handle真实跨版本验证完成，但不替代独立review和完整性能/质量gate。
