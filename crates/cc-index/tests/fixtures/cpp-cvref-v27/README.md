# 真实合成 v27 cv/ref 迁移 fixture

此 fixture 由未修改的公开基线 `13bd9dbbe3f96b390f6adf34a95bb398da89bf4d`（树 `5c4c2d55422e99ea2aa53b4768ffee0af06b6926`）通过 ParserRegistry、Indexer、IndexDb 正常 API 创建；未通过修改 user_version 伪造。

DB 为 778240 bytes，SHA-256 `fc40ed95fe7b3f961f6e57798e1cf4fd7b0b83dc3df060a886578d450cbcc464`。source-manifest.json 仅含两个合成 C++ 文件、原内容/hash/size 与固定 mtime 1720000000.123456789。baseline-audit.json 保存真实 schema/generation、两个旧 UID survivor、完整公开 source owner 证据和控制记录。

测试先在自有临时目录复制 DB/source 并核对旧 schema/hash/mtime，再由当前 version28 正常 mismatch/rebuild。勿直接用新 IndexDb 打开本文件；勿将新版本 DB 的 user_version 降为27来替代本输入。

可重做语义等价基线：检出上述公开 commit，在独立临时目录按 manifest 写入两文件及 mtime，用 max_concurrent_parse=1、dispatch_synthesis=false 的 IndexingConfig 正常创建/构建 v27 IndexDb；验证四个 parser 定义、两个 SQL/stored/public overload survivor，hot/reopen 均0 parse，再完整关闭 DB 取得文件。incarnation 随机，重新产生的 DB 不要求 byte SHA 相同；本固定原件与 producer receipt SHA 见 provenance.json。

隐私检查：仅两份合成源，freelist=0；16 个内部 B-tree freeblock 共1536 bytes，fragmented=0。全文件未发现 workspace/tmp/home 或 credential 标记；未对原 bytes 执行 VACUUM/修改。此检查不冒称通用秘密扫描。
