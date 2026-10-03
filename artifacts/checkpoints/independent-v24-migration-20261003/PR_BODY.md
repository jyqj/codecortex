独立核查固定生产 `9ebdb155c64e094b3d774be41dbe7ab8c4e222c1` / final `6db4d396e5d994388ada1e97c3d28947ffdb9c81` 的 schema24/manifest3 迁移；仅新增 independent-v24-migration-20261003 证据目录，生产和ledger只读。

代码失效路径和作者证据核查为 BOUNDED_PASS：四构建的源码 hash匹配、71份artifact hash匹配，实际保存的五个v23/m2旧SQLite快照缺边，10迁移路径与独立语法oracle一致。亲跑的是Python模型/证据audit，产品迁移亲跑0。

整体独立迁移验收 BOUNDED_REJECT（证据不足，未认定代码缺陷）：工具链版本探测出现read-only拒绝，按授权限制停止，不换home/路径规避；PR只读查询Forbidden后停止PR操作。普通reopen/regression和更强同query温缓存pinned设计已写，但未跑。全features 2474pass/4fail/68ignore仍未总验收，GC/WAL fault、semantic_runtime写入、heldout、真实provider均未跑。

验证：`python3 artifacts/checkpoints/independent-v24-migration-20261003/audit.py` → EVIDENCE_AND_MODEL_CHECKS_PASS；产品build/reopen/pinned=0。
