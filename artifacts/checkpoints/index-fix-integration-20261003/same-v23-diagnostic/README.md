# 修复后同版本 v23/v2 对照：已确认no-op缺边

执行生产树 `bf10b6477612f32d991bbfea3ed7791f468acc78`：组合独立复验通过的修复 `671063b11af8cb40a0d526098de82e684dd24aca`，暂保留schema23/manifest2。target `/workspace/index-fix-target-same-v23` 初始不存在，default/offline/locked/debug0/incremental0；实际binary/core artifacts与源码哈希见../final-v24/build-receipts.json，core fresh=false。

对已保存的五份真实中间v23库，在原路径、原源码hash和mtime上执行 full:false：5/5均parsed0、skipped1，缺失uses_type仍0；同一二进制对相同源码fresh/full：5/5各1条uses_type且target UID非空。原始RPC、数据库审计及fresh对照见receipt.json。没有先删除数据库、修改缓存key/schema/deps或覆盖旧反例。初始旧库快照仍保留于../intermediate-v23。

结论只限于这五个可复现语义升级：同v23/v2没有自动识别生成算法变化，会保留已公开生成的缺边。因此按父条件决定，用现有成熟rebuild-on-schema-mismatch升schema24，并用manifest3拒绝旧payload；不扩建算法fingerprint系统。新版本两路径实测见../final-v24。

run.py记录原始一次性实验；原运行目录后来已真实升级为v24，重复实验应从保存的实际快照/源码在新副本重建，不能改旧反例或手工改key冒充迁移。
