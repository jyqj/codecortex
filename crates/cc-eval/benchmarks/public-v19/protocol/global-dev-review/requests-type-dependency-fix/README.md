# Requests 类型依赖最小修复 A

固定 PR92 基线 `ace2bc7983be2955831c9384e44d1bdd0749c909`，只修改授权的两个生产函数及其文件内专属测试。已提交生产源码 `ee1988521e2125f86d2ff0aff8559dccc2a417b0` 的实际 default 产品构建，将旧 variadic tuple 最小失败转为成功，并成功索引原 Requests 完整20文件域。没有排名、provider、gold/scorer修改或删除source。

## 行为与边界

`cc-index/src/resolver/helpers.rs::type_atoms` 排除没有任何 Unicode 字母/数字的纯标点 token（包含 ellipsis、分隔符与 `_` 类型占位符），保留未知但含名称字符的拼写，不充当语言类型语法校验器。已有 qualified/generic/union/pointer/Unicode 类型继续保留；空白分隔覆盖 tab/newline。没有 Requests 符号 hardcode。

`cc-model/src/resolution.rs::resolution_name_keys` 保留非空 exact spelling，只有非空 leaf 才加入依赖集合。未知/无效查询也不悄悄丢失非空 exact key；`ResolutionManifest::validate` 未变，显式空 dependency 仍严格失败。类型提取同时修复，避免仅过滤空 key 后仍留下伪 ellipsis USES_TYPE edge。

6项新 Rust 回归覆盖 variadic tuple、纯标点、qualified/generic/union/Unicode、未知拼写、trailing separator 和严格校验。没有改 Go parser、Cargo、共享版本或 ledger。

## 固定构建及实际验证

- 执行源码：`ee1988521e2125f86d2ff0aff8559dccc2a417b0`，隔离 sparse source-only worktree；375个源码/Cargo文件哈希运行前后一致。
- 实际 codecortex SHA256：`65c15d75419a13925956146a747b4457ce04c2742197061e3e265ba2634075e7`。
- compiler/build receipt SHA256：`84129be229b9c1d061aa497bc2300676725edafeb8b9d5e6bbe70dec59630142`，见 evidence/build/。Rust1.95、offline/locked/default，无semantic；dev构建，不是release性能证明。
- 5个保留的真实 MCP index-only 验证全部成功：旧最小复现、保留命名类型的 variadic tuple、nonvariadic 对照、原 exceptions 单文件、完整 Requests 20文件。
- 完整域没有 parse errors，20个持久化 resolution manifests、2416个 dependencies；空 key、纯标点 semantic record、重复site均0。证据来自只读查询实际生成的SQLite，不是模拟结果。
- 所有20份source读取前核验旧固定准入receipt中的哈希；未读 query/gold body。完整source域未减小。完整索引返回与哈希在 evidence/index/。
- 一个前期验证调用已成功index，但诊断脚本只寻找 `.db` 而未找到实际 `index.sqlite3`，因此DB审计断言失败；修正证据脚本路径后完成5个保留验证。总计6次index、search/provider调用0，未调整产品代码或检索参数。

验证命令与结果（环境 `CARGO_TARGET_DIR=/tmp/p7-017-build CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0`）：

```sh
cargo test -p cc-model -p cc-index --lib --locked --offline
# 473 passed，1既有ignored；其后为跨模块/持久化契约补跑package integration tests：
cargo test -p cc-model -p cc-index --tests --locked --offline
# 563 passed，3既有ignored，无failed；不要把两次测试数相加。
cargo clippy -p cc-model -p cc-index --lib --tests --locked --offline -- -D warnings
cargo fmt --all -- --check
```

上述均退出0。3项ignored为catalog/type_catalog微基准及由父用例启动的child-process harness；保留原状态，没有新增ignored。全仓Rust tests/clippy、cc-eval旧语料、release性能、semantic/provider、正式/heldout及排名not_run。所有原始日志保留；测试日志以gzip保存原字节（含尾部空行），解压SHA见 evidence/raw-test-log-sha256.json。

## 旧证据、复现与集成建议

`pr99-diagnosis.tar.gz` 是PR99 SHA `b25723458a77edd2207ed1a52dceb0ea1009cb93` 诊断目录的字节保留归档，包括旧78b0失败raw/fixture/receipt；原baseline raw与PR99未修改。旧脚本断言旧行为，不能用修复binary冒充旧回放。既有15份license/NOTICE按原字节归档带入，出处为PR95 SHA `5d3d9e101198cebdfa1a959f74ad6ef5da4a064c`。没有复制历史/隔离question/gold。

`build.py` 强制固定上述源码SHA，offline实际构建产品并保存compiler-artifact/完整源摘要。`verify_index.py` 验证binary/build/source/20份准入source哈希，在隔离临时项目执行index，无search。原许可source可从已有准入工作流恢复；默认路径与覆盖参数见脚本。`verify_evidence.py` 仅做离线证据与旧archive字节检查，不调用产品或网络。

```sh
python3 <this-directory>/build.py --worktree <clean-ee19885-sparse-source> --output <new-build-receipt> --binaries <new-binaries> --target <cargo-cache>
python3 <this-directory>/verify_index.py --build <new-build-receipt>/build-receipt.json --inputs <current-admitted-inputs> --output <new-index-evidence>
python3 <this-directory>/verify_evidence.py
```

**版本/cache由集成者处理，本PR未越权修改。** 这是生成算法变更，serialized manifest shape未变；直接 bump `RESOLUTION_VERSION` 并不会自动重建旧索引，还可能使旧manifest读失败。`cc-index/src/indexer.rs::diff_scanned_files` 的Skip条件仅比较mtime/size/content_hash、chunk_policy和document_spec；`scoped_scan_and_diff`亦无 resolver算法版本检查。已持久化的旧伪类型edge/依赖可能在升级后的no-op index中保留。本次验证全部fresh/full，不证明warm migration。集成者至少安排升级后的完整reindex（`index full:true`）；若要求自动升级，应由主任务在这两处及持久化metadata引入生成算法失效标记，或按既定schema升级策略重建，不要仅清除运行时catalog cache或随意改schema格式版本。该集成工作不属于本次两个函数独占范围。

清单建议：Requests空dependency producer bug记为“最小修复与完整source index已验证”；完整V19 ranking gate仍未关闭，Gin修复、资源矩阵与升级失效策略由各owner独立处理，后续整合新源码SHA再冻结/重跑完整baseline。不得将本index成功换算为检索质量提升。
