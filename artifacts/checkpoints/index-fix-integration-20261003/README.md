# 索引修复集成验证（验收尚阻塞）

基线 PR101 `574f7598662334c63e020da136c87f4f7281554d`；Go102 `99a2c0426054ddbed7edf42de43593a780a76a49`、Requests103 `da5b05ee08d84fd336d5a98f25da7cae176daebf` 原样组合。固定生产源码 `57bedbaf193e28be34271a290d28c90dc662b613`。

生产新增范围只有 schema 23 与 resolution manifest v2；旧 v21/v22 都重建，旧 additive 例外删除。未修改作者 Go/parser、helpers/type_atoms、resolution_name_keys 函数。

`receipt.json`：实际 PR101 default 产品→新版，4组旧负例/旧缓存，14次 index；两个旧库成功形成 v22/v1，升级重开清空旧 parse/derived 持久行、更新 incarnation，增量再解析，no-op 稳定，最后 nested Go 和 variadic tuple 成功。没有手工改缓存 key/schema。PR92 实际历史旧构建验证另保留于 pr92-migration。

公共源码：Requests 固定提交 `611c6162cbc4ac2020a2f91c7cfa4f3abf9bbb60`，20份源码/配置与作者所有锁定哈希完全一致；Gin 固定 `43fe48e8a0f44af783116cdb010725e6bb50255f`，context.go 原失败负例与新版完整109文件仓库索引成功，见 gin-repository.json（整仓旧二进制为PR92，四组迁移旧二进制为PR101）。旧 Requests 空key及旧 Gin 冲突 site 都复现。新 DB 审计 empty key/punctuation semantic/duplicate site 均0。无真实 provider、gold/holdout读取或调参；index-only公共验证无 search。

`old-pinned-live.json`：PR101 真实 rlibs 编译 driver，保留 v22 旧 query handle 和温缓存，另一新版产品重开同 DB 并重建。旧 pinned handle 根据新 incarnation 返回新源码，旧 manifest reader 明确拒绝新版 v2。该边界 probe 有2次 local search，无provider。正常 pinned handle 跨完整重建 Rust CI 回归 `crates/cc-server/tests/index_fix_pinned_handle.rs`，1/0。

验证：workspace all-targets 2340/0、65原有ignored，146 suite；clippy workspace all-targets -D warnings 通过；fmt通过。原始日志 gzip 保留，SHA见 validation-log-manifest.json。PR101首次链接和clippy首次执行因磁盘满失败，清理生成缓存后重试通过；额外独立eval首次执行同样磁盘满，后按实际lib测试范围重试。不是全V20或发布性能证明。

验收阻塞：父提供 Requests 独立 review BOUNDED_REJECT（合法 `_`/`__`/`$`/`℘` 类型被过滤）。本树不得标 accepted，等待新修复 owner 的精确 SHA 和同一 reviewer 复验。Go 独立 review PR104 `06478892104347a846cdfdb81f843574e188634b` 远端核验一致；范围为 Go 修复，不证明迁移。PR101 cold review 尚待父结论。live/heldout质量及完整性能gate继续 blocked。
