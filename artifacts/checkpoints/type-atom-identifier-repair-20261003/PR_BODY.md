PR103 的 `is_alphanumeric` 类型原子过滤丢弃合法 Python `_`、`__`、`℘`（同时丢 NameBucket），并丢弃 TypeScript `$` 的 USES_TYPE 边。此修复仅改变 `type_atoms` 的过滤边界：排除 ASCII 语法标点原子，保留标识符字符 `_`/`$` 和非 ASCII 名称。`resolution_name_keys` 的非空 leaf 过滤、ellipsis 负例与 strict validator 保留。

同一套 synthetic 测试分别在固定原版 `ace2bc7983be2955831c9384e44d1bdd0749c909`、PR103 `da5b05ee08d84fd336d5a98f25da7cae176daebf` 和修复的独立 Cargo target 执行：原版合法域通过但旧负例失败，PR103 负例通过但合法域失败，修复 7 项全部通过。真实 Python/TypeScript parser/catalog/manifest 测试检查边的 target UID 与 NameBucket；全部 Unicode XID_Start scalar、qualified/generic/union、variadic tuple 及显式空 key 的 normalize/序列化验证均覆盖。

验证：限定 resolver 97 pass/1 ignored；model resolution 3 pass；两 crate `clippy --all-targets -- -D warnings` 与全仓 `fmt --check` 通过。原始失败日志、输入哈希、退出码、重建脚本和 TODO 在 `artifacts/checkpoints/type-atom-identifier-repair-20261003/`。

冻结独立负证据 `7f650a5f8338e0d1ad2d8e8592ae31c89b34047c` 保持原样。本 PR 不认领完整语料验收：原 20 文件/2416 deps 身份仍未确认，不与独立补充的 19 Python/2420 deps 合记；全量测试和旧 cache 迁移留给主集成 owner。请新独立 reviewer 对最终 fixed SHA 复验。
