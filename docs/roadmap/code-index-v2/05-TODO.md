# 05｜逐项重构 TODO（由 tasks.json 派生）

> 任务总数：192；源文件 SHA-256：`776b21dae16b8ab501b0ce185a52b1da46db9128478f7023c0b7f200d099b67f`。
> 状态只改 tasks.json；使用 scripts/code_index_plan.py --write 生成本页。

## 总览

| Phase | 主题 | done / 总数 |
|---|---|---:|
| P0 | 基线与benchmark底座 | 20 / 20 |
| P1 | 检索正确性与范围 | 20 / 20 |
| P2 | 公共表面与增量正确性 | 20 / 20 |
| P3 | 项目模型与模块解析 | 20 / 20 |
| P4 | 源码切块与文档版本 | 20 / 20 |
| P5 | 查询执行与证据装配 | 15 / 20 |
| P6 | 语义持久化与发布底座 | 0 / 20 |
| P7 | provider与dense端到端 | 0 / 20 |
| P8 | 规模、质量与发行认证 | 0 / 20 |
| P9 | 有收益门的可选增强 | 0 / 12 |

## P0｜基线与benchmark底座

### [x] P0-001｜冻结源码与工作区基线

状态：`done`；批次：`P0-A`；优先级：`high`。
范围：`crates/cc-eval/src/benchmark/manifest.rs`；`docs/roadmap/code-index-v2/00-BASELINE.md`
硬依赖：无
步骤：记录 HEAD、dirty、submodule、binary digest 和工具链；将已有1307通过与SDK失败分别保留
交付物：冻结源码与工作区基线的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V01 验证证据（实施时生成）
验收：manifest 不把旧测试日志标为新SHA证据，未提交修改可辨认；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V01
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "artifacts/benchmarks/p0-baseline-4514630/manifest.json"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}]
实施备注：已按P0范围完成工程与本地验收；证据见独立命令记录，不将其他阶段或认证范围视为完成。

### [x] P0-002｜建立隔离冷构建环境

状态：`done`；批次：`P0-A`；优先级：`high`。
范围：`.github/workflows/ci.yml`；`CONTRIBUTING.md`
硬依赖：P0-001
步骤：定位SDK/TAPI阻塞并选局部环境或隔离runner；用新的target目录冷构建而不删除用户target
交付物：建立隔离冷构建环境的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V01 验证证据（实施时生成）
验收：build、doctest、MSRV的退出码分别可复核，未解决平台明确blocked；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V01
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "artifacts/benchmarks/p0-final-runs/platform-probes.json", "scripts/p0-validation.py"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}]
实施备注：隔离环境与本机1.97冷构建已交付；1.95工具链未安装，MSRV认证未通过，其未完成证据阻塞P0-020，不把脚本完成当MSRV成功。

### [x] P0-003｜运行现有回归并归档

状态：`done`；批次：`P0-A`；优先级：`high`。
范围：`crates/cc-eval/src/benchmark/runner.rs`；`artifacts/benchmarks/`
硬依赖：P0-002
步骤：执行现有workspace和stdio测试；记录ignored规模测试与全部命令输出
交付物：运行现有回归并归档的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V01, V04 验证证据（实施时生成）
验收：正常测试、ignored、link failure不混为一份绿色汇总；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V01；V04
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "artifacts/benchmarks/p0-final-validation/command-02.log", "artifacts/benchmarks/p0-final-validation/command-05.log"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}]
实施备注：已按P0范围完成工程与本地验收；证据见独立命令记录，不将其他阶段或认证范围视为完成。

### [x] P0-004｜固定缺陷夹具与已知失败登记

状态：`done`；批次：`P0-A`；优先级：`high`。
范围：`crates/cc-eval/fixtures/index-v2/`；`crates/cc-eval/benchmarks/goldens/`
硬依赖：P0-003
步骤：为BM25、softscope、Rust/Python API变更建最小夹具；绑定已知失败到P1/P2任务
交付物：固定缺陷夹具与已知失败登记的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05, V06, V07 验证证据（实施时生成）
验收：每个缺陷有重现输入与观察，不因基线有bug阻止建立测量；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05；V06；V07
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "crates/cc-eval/benchmarks/KNOWN-FAILURES.md", "artifacts/benchmarks/p0-final-validation/known-defects/B01-bm25.json", "artifacts/benchmarks/p0-final-validation/known-defects/B02-soft-scope.json"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}]
实施备注：观察测试与oracle成功留存已知缺陷；产品错误未修复，不能算P1/P2完成。

### [x] P0-005｜定义版本化benchmark schema

状态：`done`；批次：`P0-A`；优先级：`high`。
范围：`crates/cc-eval/src/benchmark/schema.rs`；`crates/cc-eval/benchmarks/schema/`
硬依赖：P0-004
步骤：定义suite/query/gold/result/status；支持answer groups、span、case family及错误状态
交付物：定义版本化benchmark schema的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V02 验证证据（实施时生成）
验收：坏JSON、重复ID、空gold、非法路径和未知版本全部有负测；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V02
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "crates/cc-eval/benchmarks/schema/suite.schema.json", "crates/cc-eval/benchmarks/schema/query.schema.json", "crates/cc-eval/benchmarks/schema/result.schema.json"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}]
实施备注：已按P0范围完成工程与本地验收；证据见独立命令记录，不将其他阶段或认证范围视为完成。

### [x] P0-006｜设计外部题库导入与许可记录

状态：`done`；批次：`P0-B`；优先级：`high`。
范围：`crates/cc-eval/src/benchmark/importer_oce.rs`；`crates/cc-eval/benchmarks/manifests/`
硬依赖：P0-005
步骤：读取外部JSONL/metadata并保持字段含义；记录上游SHA、数据hash和权限来源
交付物：设计外部题库导入与许可记录的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V02, V03 验证证据（实施时生成）
验收：不直接vendor无明确许可的内容，200题声明与实际计数可对照；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V02；V03
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "artifacts/benchmarks/p0-external-reference/import-validation.json"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}]
实施备注：两份真实外部题库实际各100题，导入exit0；仅格式/计数/摘要核验，不代表全部标准答案语义审阅或服务效果。

### [x] P0-007｜实现OCE兼容评分goldens

状态：`done`；批次：`P0-B`；优先级：`high`。
范围：`crates/cc-eval/src/benchmark/metrics.rs`；`crates/cc-eval/tests/benchmark_scoring.rs`
硬依赖：P0-006
步骤：实现any-expected Top1、linear nDCG、首次路径去重与glob；构造主/支持/重复/空结果金样
交付物：实现OCE兼容评分goldens的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V03 验证证据（实施时生成）
验收：手算与兼容对照逐例一致，不能换成指数gain；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V03
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "crates/cc-eval/benchmarks/goldens/fnmatch.json", "crates/cc-eval/tests/benchmark_scoring.rs"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}]
实施备注：已按P0范围完成工程与本地验收；证据见独立命令记录，不将其他阶段或认证范围视为完成。

### [x] P0-008｜实现原生证据评分骨架

状态：`done`；批次：`P0-B`；优先级：`high`。
范围：`crates/cc-eval/src/benchmark/metrics.rs`；`crates/cc-eval/src/benchmark/schema.rs`
硬依赖：P0-007, P0-005
步骤：增加primary group、符号身份和跨度评分；把文件级call_chain与关系证明分开
交付物：实现原生证据评分骨架的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V03 验证证据（实施时生成）
验收：同名错文件和整文件灌入不能获得精确符号/跨度满分；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V03
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "crates/cc-eval/tests/benchmark_scoring.rs"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}]
实施备注：已实现group/symbol/span骨架；不宣称完整facet与有序graph约束。

### [x] P0-009｜实现严格输入锁

状态：`done`；批次：`P0-B`；优先级：`high`。
范围：`crates/cc-eval/src/benchmark/manifest.rs`；`crates/cc-eval/tests/benchmark_lock.rs`
硬依赖：P0-008, P0-005
步骤：锁源码/题库/配置/评分器/实际文件manifest；single和suite都验dirty与submodule
交付物：实现严格输入锁的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V02 验证证据（实施时生成）
验收：HEAD相同但正文改变也必须拒绝正式可比结果；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V02
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "crates/cc-eval/tests/benchmark_lock.rs"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}]
实施备注：子模块和未解析LFS在P0 fail-closed拒绝；不宣称完整子模块锁支持。

### [x] P0-010｜冻结后端适配接口

状态：`done`；批次：`P0-B`；优先级：`high`。
范围：`crates/cc-eval/src/benchmark/adapters/mod.rs`；`crates/cc-eval/src/benchmark/normalizer.rs`
硬依赖：P0-009, P0-005
步骤：定义prepare/readiness/search/close与有效预算；normalize只处理公开响应
交付物：冻结后端适配接口的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V04 验证证据（实施时生成）
验收：适配器不能访问gold、私有DB或偷偷改变用户查询；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V04
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "crates/cc-eval/src/benchmark/adapters/mod.rs", "crates/cc-eval/src/benchmark/normalizer.rs"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}]
实施备注：已按P0范围完成工程与本地验收；证据见独立命令记录，不将其他阶段或认证范围视为完成。

### [x] P0-011｜实现真实MCP子进程适配

状态：`done`；批次：`P0-C`；优先级：`high`。
范围：`crates/cc-eval/src/benchmark/adapters/mcp_stdio.rs`；`crates/cc-server/tests/mcp_stdio.rs`
硬依赖：P0-010
步骤：启动精确binary并完成握手/调用/取消；区分进程内seam与真实stdio
交付物：实现真实MCP子进程适配的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V04, V18 验证证据（实施时生成）
验收：坏响应、进程崩溃、工具错误、路径过滤都有黑盒证据；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V04；V18
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "artifacts/benchmarks/p0-final-validation/command-05.log", "artifacts/benchmarks/p0-closeout/cancellation.json"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}]
实施备注：已按P0范围完成工程与本地验收；证据见独立命令记录，不将其他阶段或认证范围视为完成。

### [x] P0-012｜实现可选OCE HTTP适配

状态：`done`；批次：`P0-C`；优先级：`high`。
范围：`crates/cc-eval/src/benchmark/adapters/oce_http.rs`；`crates/cc-eval/Cargo.toml`
硬依赖：P0-011, P0-010
步骤：按公开上传/状态/检索契约实现；先用本地stub测试再按授权对照
交付物：实现可选OCE HTTP适配的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V04 验证证据（实施时生成）
验收：eval-http依赖不进入产品默认包，鉴权不写日志；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V04
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "artifacts/benchmarks/p0-final-validation/command-03.log", "artifacts/benchmarks/p0-closeout/product-dependencies.log"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}]
实施备注：仅本地HTTP stub验证；真实OCE服务、模型调用与远端预算对齐未运行。

### [x] P0-013｜实现readiness与失败分母

状态：`done`；批次：`P0-C`；优先级：`high`。
范围：`crates/cc-eval/src/benchmark/readiness.rs`；`crates/cc-eval/src/benchmark/schema.rs`
硬依赖：P0-012, P0-011
步骤：区分unknown/pending/failed/partial/ready；记录覆盖率与总deadline
交付物：实现readiness与失败分母的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V04 验证证据（实施时生成）
验收：错误/超时不误当ready，缺失输入不能悄悄从分母消失；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V04
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "crates/cc-eval/tests/benchmark_adapters.rs", "crates/cc-eval/tests/benchmark_reports.rs"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}]
实施备注：已按P0范围完成工程与本地验收；证据见独立命令记录，不将其他阶段或认证范围视为完成。

### [x] P0-014｜实现原始工件和报告一致性

状态：`done`；批次：`P0-C`；优先级：`high`。
范围：`crates/cc-eval/src/benchmark/report.rs`；`crates/cc-eval/src/benchmark/gate.rs`
硬依赖：P0-013, P0-007, P0-008, P0-011
步骤：保存raw/normalized/metrics/gate；从同一结果生成Markdown和机器输出
交付物：实现原始工件和报告一致性的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V04 验证证据（实施时生成）
验收：失败退出非零，报告可离线重算且脱敏；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V04
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "artifacts/benchmarks/p0-final-runs/replay-0.log", "artifacts/benchmarks/p0-final-runs/replay-1.log"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}]
实施备注：已按P0范围完成工程与本地验收；证据见独立命令记录，不将其他阶段或认证范围视为完成。

### [x] P0-015｜建立资源与延迟采样原语

状态：`done`；批次：`P0-C`；优先级：`high`。
范围：`crates/cc-eval/src/benchmark/sampler.rs`；`crates/cc-eval/src/benchmark/statistics.rs`
硬依赖：P0-014
步骤：单调计时，区分client/server/process tree；校验macOS/Linux RSS单位与高水位
交付物：建立资源与延迟采样原语的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V20 验证证据（实施时生成）
验收：缺失资源显示unavailable，重复采样不取best-of；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V20
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "artifacts/benchmarks/p0-final-runs/mcp-suite/suite-001/resources.jsonl", "artifacts/benchmarks/p0-final-runs/mcp-suite/suite-001/latency-summary.json"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}]
实施备注：阶段边界采样，不声称捕获全部瞬时峰值；小N不认证p95/p99。

### [x] P0-016｜建立增量对全量oracle骨架

状态：`done`；批次：`P0-D`；优先级：`high`。
范围：`crates/cc-eval/src/benchmark/oracle.rs`；`crates/cc-eval/src/benchmark/mutations.rs`
硬依赖：P0-004, P0-005
步骤：生成隔离A/B副本；对比语义事实并保留人工gold独立校验
交付物：建立增量对全量oracle骨架的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V07 验证证据（实施时生成）
验收：rowid可忽略而目标UID/策略/歧义不得忽略；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V07
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "artifacts/benchmarks/p0-final-runs/oracle-python/oracle.json", "artifacts/benchmarks/p0-final-runs/oracle-rust/oracle.json"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}]
实施备注：八张表与公开查询的诊断骨架，A/B目标UID与策略保留；两语言已复现现有失效。

### [x] P0-017｜落第一批真实查询与强干扰样本

状态：`done`；批次：`P0-D`；优先级：`high`。
范围：`crates/cc-eval/benchmarks/native/`；`crates/cc-eval/fixtures/index-v2/`
硬依赖：P0-016, P0-005
步骤：阅读固定源码写主/支持答案；加入同名错文件和无词面重合问题
交付物：落第一批真实查询与强干扰样本的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V02, V19 验证证据（实施时生成）
验收：gold经过独立复核，题库不进入被测索引或生产包；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V02；V19
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "artifacts/benchmarks/p0-closeout/gold-source-review.json", "crates/cc-eval/benchmarks/native/p0-smoke.jsonl", "crates/cc-eval/benchmarks/native/p0-codecortex-subset.jsonl"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}]
实施备注：11题自编smoke与14题真实源码子集，完成来源阅读及独立于排序结果的二次校验；无外部人工评审或holdout认证。

### [x] P0-018｜冻结数据治理和holdout规则

状态：`done`；批次：`P0-D`；优先级：`high`。
范围：`crates/cc-eval/benchmarks/manifests/`；`docs/BENCHMARK.md`
硬依赖：P0-017
步骤：按repo/module/query-family分割；规定答案争议隔离和模型辅助标签流程
交付物：冻结数据治理和holdout规则的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V02, V19 验证证据（实施时生成）
验收：改写/翻译不跨split泄漏，不按失败题写生产特判；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V02；V19
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "crates/cc-eval/benchmarks/README.md", "crates/cc-eval/tests/benchmark_scoring.rs"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}]
实施备注：已按P0范围完成工程与本地验收；证据见独立命令记录，不将其他阶段或认证范围视为完成。

### [x] P0-019｜生成首份基线与门槛配置

状态：`done`；批次：`P0-D`；优先级：`high`。
范围：`crates/cc-eval/src/bin/cc-eval.rs`；`crates/cc-eval/benchmarks/manifests/`
硬依赖：P0-018, P0-009, P0-014, P0-015, P0-016, P0-017
步骤：集成validate/run/compare入口；先跑小基线并预注册quality/perf margins和known failures
交付物：生成首份基线与门槛配置的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V01, V03, V04, V19, V20 验证证据（实施时生成）
验收：配置/模型/输入不一致时拒比较，没有测量不填SLA；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V01；V03；V04；V19；V20
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "artifacts/benchmarks/p0-final-runs/mcp-suite/suite-000/metrics.json", "artifacts/benchmarks/p0-final-runs/mcp-suite/suite-001/metrics.json", "artifacts/benchmarks/p0-final-runs/comparison.json"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}]
实施备注：生成基线，不声称性能提升；smoke no-answer和mutation按已知失败exit1；42样本比较inconclusive。

### [x] P0-020｜P0验收与任务视图维护

状态：`done`；批次：`P0-D`；优先级：`blocking`。
范围：`docs/roadmap/code-index-v2/`；`docs/BENCHMARK.md`
硬依赖：P0-001, P0-002, P0-003, P0-004, P0-005, P0-006, P0-007, P0-008, P0-009, P0-010, P0-011, P0-012, P0-013, P0-014, P0-015, P0-016, P0-017, P0-018, P0-019
步骤：检查任务JSON和Markdown一致；归档G0、实际命令和下一批输入
交付物：P0验收与任务视图维护的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V01, V02, V03, V04 验证证据（实施时生成）
验收：scorer/locks/adapters可用，未复现问题和平台阻塞有明确清单；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V01；V02；V03；V04
回滚：保留原始测量与旧runner入口；停用新评分profile不改生产行为。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "819fe341fbcb941bd704248ef1f18a24487108fd06ecc6a4435a610390a80d53", "run_id": "p0-final-validation / p0-final-runs", "artifacts": ["artifacts/benchmarks/p0-final-validation/validation.json", "artifacts/benchmarks/p0-final-runs/commands.json", "artifacts/benchmarks/p0-closeout/static-checks.json", "docs/roadmap/code-index-v2/P0-IMPLEMENTATION.md", "artifacts/benchmarks/p0-final-runs/platform-probes.json"], "commands_receipt": "artifacts/benchmarks/p0-final-validation/validation.json", "baseline_commands_receipt": "artifacts/benchmarks/p0-final-runs/commands.json", "review": "source/scope/negative cases reviewed; local validation only, see P0-IMPLEMENTATION.md", "rollback_status": "production behavior unchanged; remove new dev runner and revert benchmark-only dependency/config changes without modifying original source/index", "limitations": "MSRV and strict workspace gate not certified"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "de917e20ac7e2e967c2b4a5e00528c1316065b1b5f1dc0f73b51a5493ba1bb4a", "run_id": "p0-g0-20260927/accepted-v2", "artifacts": ["artifacts/benchmarks/p0-g0-20260927/accepted-v2/validation.json", "artifacts/benchmarks/p0-g0-20260927/toolchain-install.json", "artifacts/benchmarks/p0-g0-20260927/binaries.json"], "review": "等价lint变换；未调用接口删除；分析输入结构收口；MSRV新增告警真实修复；两工具链验证；本地限定", "rollback_status": "保留入场source hash、原始P0报告及每次失败日志；按G0变更切片回退，不改变全局工具链/SDK。", "limitations": "GitHub Actions/Linux/付费模型未运行，不计作G0本地通过证据"}]
实施备注：G0已在macOS arm64本地完成Rust1.95与stable严格lint/全量测试/doctest/build/真实stdio验证；未降低warnings；Actions/Linux留待后续发布认证。

## P1｜检索正确性与范围

### [x] P1-001｜补BM25端到端排序回归

状态：`done`；批次：`P1-A`；优先级：`high`。
范围：`crates/cc-search/src/preselect.rs`；`crates/cc-eval/fixtures/index-v2/`
硬依赖：P0-020
步骤：把隔离公式例转成真实FTS/preselect/MCP样例；固定强弱匹配对
交付物：补BM25端到端排序回归的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05 验证证据（实施时生成）
验收：修复前失败、修复后同一题不靠改权重通过；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "24b0d62ea2521ba98312bf5d057eb4403c4b7827c05f944be77c3889607274fc", "run_id": "p1a-20260927", "artifacts": ["artifacts/benchmarks/p1a-20260927/final/validation.json", "artifacts/benchmarks/p1a-20260927/red-v2/commands.json", "artifacts/benchmarks/p1a-20260927/paired/commands.json", "artifacts/benchmarks/p1a-20260927/paired/comparison.json"], "review": "两工具链严格Clippy/全量测试/真实MCP；G0前后证据分开；原始输入评分锁不变；graph/header/SQL/cache范围复核。", "rollback_status": "G0源码哈希和独立二进制保留；按P1-A切片回退，旧gold/比较原始工件不覆盖；无DB schema变化。", "limitations": "小样本比较inconclusive，不声明统计显著提升或p95认证；P1-B..D/P2/live模型/Actions未完成。"}]
实施备注：实际SQLite/preselect与旧G0 MCP二进制红断言；新二进制与公共API转绿。

### [x] P1-002｜就地修正BM25单调映射

状态：`done`；批次：`P1-A`；优先级：`high`。
范围：`crates/cc-search/src/preselect.rs`；`crates/cc-db/src/index_db_retrieval.rs`
硬依赖：P1-001
步骤：选择rank映射或保持单调的变换；保留数据库原始分数诊断
交付物：就地修正BM25单调映射的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05, V19 验证证据（实施时生成）
验收：更好的原始FTS匹配不会获得更低的该层贡献；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05；V19
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "24b0d62ea2521ba98312bf5d057eb4403c4b7827c05f944be77c3889607274fc", "run_id": "p1a-20260927", "artifacts": ["artifacts/benchmarks/p1a-20260927/final/validation.json", "artifacts/benchmarks/p1a-20260927/red-v2/commands.json", "artifacts/benchmarks/p1a-20260927/paired/commands.json", "artifacts/benchmarks/p1a-20260927/paired/comparison.json"], "review": "两工具链严格Clippy/全量测试/真实MCP；G0前后证据分开；原始输入评分锁不变；graph/header/SQL/cache范围复核。", "rollback_status": "G0源码哈希和独立二进制保留；按P1-A切片回退，旧gold/比较原始工件不覆盖；无DB schema变化。", "limitations": "小样本比较inconclusive，不声明统计显著提升或p95认证；P1-B..D/P2/live模型/Actions未完成。"}]
实施备注：负向BM25以x/(1+x)单调映射；独立公式校验图排序旧基线，未改权重。

### [x] P1-003｜定义HardScope与SoftHints

状态：`done`；批次：`P1-A`；优先级：`high`。
范围：`crates/cc-model/src/retrieval.rs`；`crates/cc-search/src/scope.rs`
硬依赖：P1-002
步骤：从request参数分离硬范围和提示；保持现有wire字段可映射
交付物：定义HardScope与SoftHints的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05, V18 验证证据（实施时生成）
验收：Some(empty)和None类型/序列化/语义明确不同；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05；V18
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "24b0d62ea2521ba98312bf5d057eb4403c4b7827c05f944be77c3889607274fc", "run_id": "p1a-20260927", "artifacts": ["artifacts/benchmarks/p1a-20260927/final/validation.json", "artifacts/benchmarks/p1a-20260927/red-v2/commands.json", "artifacts/benchmarks/p1a-20260927/paired/commands.json", "artifacts/benchmarks/p1a-20260927/paired/comparison.json"], "review": "两工具链严格Clippy/全量测试/真实MCP；G0前后证据分开；原始输入评分锁不变；graph/header/SQL/cache范围复核。", "rollback_status": "G0源码哈希和独立二进制保留；按P1-A切片回退，旧gold/比较原始工件不覆盖；无DB schema变化。", "limitations": "小样本比较inconclusive，不声明统计显著提升或p95认证；P1-B..D/P2/live模型/Actions未完成。"}]
实施备注：HardScope/SoftHints、Some(empty)语义贯穿SQL/服务器；必要缓存presence和hint顺序修复同步落地。

### [x] P1-004｜移除预选到硬范围的隐式写回

状态：`done`；批次：`P1-A`；优先级：`high`。
范围：`crates/cc-search/src/plan.rs`；`crates/cc-search/src/preselect.rs`
硬依赖：P1-003
步骤：保留预选打分但不覆盖caller file_paths；增加soft候选集合
交付物：移除预选到硬范围的隐式写回的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05, V19 验证证据（实施时生成）
验收：未预选到的正确精确/词法结果能被召回；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05；V19
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "24b0d62ea2521ba98312bf5d057eb4403c4b7827c05f944be77c3889607274fc", "run_id": "p1a-20260927", "artifacts": ["artifacts/benchmarks/p1a-20260927/final/validation.json", "artifacts/benchmarks/p1a-20260927/red-v2/commands.json", "artifacts/benchmarks/p1a-20260927/paired/commands.json", "artifacts/benchmarks/p1a-20260927/paired/comparison.json"], "review": "两工具链严格Clippy/全量测试/真实MCP；G0前后证据分开；原始输入评分锁不变；graph/header/SQL/cache范围复核。", "rollback_status": "G0源码哈希和独立二进制保留；按P1-A切片回退，旧gold/比较原始工件不覆盖；无DB schema变化。", "limitations": "小样本比较inconclusive，不声明统计显著提升或p95认证；P1-B..D/P2/live模型/Actions未完成。"}]
实施备注：预选不写回硬file_paths；独立合法词法/精确结果可召回，grep仍有既有cap。

### [x] P1-005｜统一DSL与参数范围交集

状态：`done`；批次：`P1-A`；优先级：`high`。
范围：`crates/cc-search/src/dsl.rs`；`crates/cc-search/src/scope.rs`
硬依赖：P1-004
步骤：定义path/language/file约束组合；对冲突输出一致行为
交付物：统一DSL与参数范围交集的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05, V18 验证证据（实施时生成）
验收：多个限制不得因后者覆盖而扩大用户范围；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05；V18
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "24b0d62ea2521ba98312bf5d057eb4403c4b7827c05f944be77c3889607274fc", "run_id": "p1a-20260927", "artifacts": ["artifacts/benchmarks/p1a-20260927/final/validation.json", "artifacts/benchmarks/p1a-20260927/red-v2/commands.json", "artifacts/benchmarks/p1a-20260927/paired/commands.json", "artifacts/benchmarks/p1a-20260927/paired/comparison.json"], "review": "两工具链严格Clippy/全量测试/真实MCP；G0前后证据分开；原始输入评分锁不变；graph/header/SQL/cache范围复核。", "rollback_status": "G0源码哈希和独立二进制保留；按P1-A切片回退，旧gold/比较原始工件不覆盖；无DB schema变化。", "limitations": "小样本比较inconclusive，不声明统计显著提升或p95认证；P1-B..D/P2/live模型/Actions未完成。"}]
实施备注：重复DSL path/lang与显式参数取交集；无效过滤报错；图补充证据同范围筛选。

### [x] P1-006｜完善路径规范与越界测试

状态：`done`；批次：`P1-B`；优先级：`high`。
范围：`crates/cc-server/src/path_guard.rs`；`crates/cc-search/src/scope.rs`
硬依赖：P0-020, P1-003, P1-005
步骤：校验相对路径、分隔符、大小写和symlink边界；保持平台差异可见
交付物：完善路径规范与越界测试的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05 验证证据（实施时生成）
验收：项目外路径、前缀近似同名和大小写别名不泄漏；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "3ac1a68ba1db00de9672b7be5e32395df183c5836cdb55636a7ee609e761e05c", "run_id": "p1b-20260927", "artifacts": ["artifacts/benchmarks/p1b-20260927/final/validation.json", "artifacts/benchmarks/p1b-20260927/paired/summary.json", "artifacts/benchmarks/p1b-20260927/paired/commands.json", "artifacts/benchmarks/p1b-20260927/scan-summary.json", "artifacts/benchmarks/p1b-20260927/evidence-classification.json", "docs/roadmap/code-index-v2/P1-B-IMPLEMENTATION.md"], "review": "两工具链strict/test/doctest/build及真实MCP；坏压缩cap前拒绝、组件/大小写SQL、graph候选/证据与回放审查；最终覆盖文件无漂移。", "rollback_status": "保留上轮P1-A产品、入场hash及原始run；按本批文件改动切片回退，无DB schema改变、不覆盖历史题库报告。", "limitations": "macOS arm64本地；新旧同v2评分，小样本compare不作发行认证；graph/parser/P2已知失败独立保留；无Linux/Windows/Actions/live模型。"}]
实施备注：路径组件/大小写/分隔符/遍历/符号链接边界已实施；Unix原生不可表示名字拒绝；跨平台运行/原子TOCTOU沙箱不在本次认证。

### [x] P1-007｜词法lane独立作用域

状态：`done`；批次：`P1-B`；优先级：`high`。
范围：`crates/cc-search/src/lanes.rs`；`crates/cc-db/src/index_db_retrieval.rs`
硬依赖：P1-006
步骤：只以HardScope限制FTS；soft提示作为排序/候选优先而非全局白名单
交付物：词法lane独立作用域的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05, V19 验证证据（实施时生成）
验收：scope内的预选外命中可见，scope外任何命中不可见；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05；V19
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "3ac1a68ba1db00de9672b7be5e32395df183c5836cdb55636a7ee609e761e05c", "run_id": "p1b-20260927", "artifacts": ["artifacts/benchmarks/p1b-20260927/final/validation.json", "artifacts/benchmarks/p1b-20260927/paired/summary.json", "artifacts/benchmarks/p1b-20260927/paired/commands.json", "artifacts/benchmarks/p1b-20260927/scan-summary.json", "artifacts/benchmarks/p1b-20260927/evidence-classification.json", "docs/roadmap/code-index-v2/P1-B-IMPLEMENTATION.md"], "review": "两工具链strict/test/doctest/build及真实MCP；坏压缩cap前拒绝、组件/大小写SQL、graph候选/证据与回放审查；最终覆盖文件无漂移。", "rollback_status": "保留上轮P1-A产品、入场hash及原始run；按本批文件改动切片回退，无DB schema改变、不覆盖历史题库报告。", "limitations": "macOS arm64本地；新旧同v2评分，小样本compare不作发行认证；graph/parser/P2已知失败独立保留；无Linux/Windows/Actions/live模型。"}]
实施备注：词法HardScope通过二进制SQL过滤在LIMIT之前执行，保留软提示非白名单；大小写强干扰回归与旧14题配对无退化。

### [x] P1-008｜grep分段扫描与诊断

状态：`done`；批次：`P1-B`；优先级：`high`。
范围：`crates/cc-search/src/lanes.rs`；`crates/cc-db/src/index_db_retrieval.rs`
硬依赖：P1-007
步骤：先softscope再有界global回退；去重扫描并报告cap/扫描量
交付物：grep分段扫描与诊断的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05, V20 验证证据（实施时生成）
验收：预算用尽不报告完整no_match，也不解压扫描无上限；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05；V20
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "3ac1a68ba1db00de9672b7be5e32395df183c5836cdb55636a7ee609e761e05c", "run_id": "p1b-20260927", "artifacts": ["artifacts/benchmarks/p1b-20260927/final/validation.json", "artifacts/benchmarks/p1b-20260927/paired/summary.json", "artifacts/benchmarks/p1b-20260927/paired/commands.json", "artifacts/benchmarks/p1b-20260927/scan-summary.json", "artifacts/benchmarks/p1b-20260927/evidence-classification.json", "docs/roadmap/code-index-v2/P1-B-IMPLEMENTATION.md"], "review": "两工具链strict/test/doctest/build及真实MCP；坏压缩cap前拒绝、组件/大小写SQL、graph候选/证据与回放审查；最终覆盖文件无漂移。", "rollback_status": "保留上轮P1-A产品、入场hash及原始run；按本批文件改动切片回退，无DB schema改变、不覆盖历史题库报告。", "limitations": "macOS arm64本地；新旧同v2评分，小样本compare不作发行认证；graph/parser/P2已知失败独立保留；无Linux/Windows/Actions/live模型。"}]
实施备注：soft/FTS/fallback共用解压cap与去重；空结果保留complete/limited/partial诊断；1000文件90次操作计数验证无超cap，不冒充发布p95。

### [x] P1-009｜graph lane与最终过滤对齐

状态：`done`；批次：`P1-B`；优先级：`high`。
范围：`crates/cc-search/src/lanes.rs`；`crates/cc-search/src/plan.rs`
硬依赖：P1-008
步骤：图种子/邻居保留hard过滤；最终hydrate二次守卫
交付物：graph lane与最终过滤对齐的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05 验证证据（实施时生成）
验收：图扩展不会绕过语言/文件范围，软范围外合法邻居保留；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "3ac1a68ba1db00de9672b7be5e32395df183c5836cdb55636a7ee609e761e05c", "run_id": "p1b-20260927", "artifacts": ["artifacts/benchmarks/p1b-20260927/final/validation.json", "artifacts/benchmarks/p1b-20260927/paired/summary.json", "artifacts/benchmarks/p1b-20260927/paired/commands.json", "artifacts/benchmarks/p1b-20260927/scan-summary.json", "artifacts/benchmarks/p1b-20260927/evidence-classification.json", "docs/roadmap/code-index-v2/P1-B-IMPLEMENTATION.md"], "review": "两工具链strict/test/doctest/build及真实MCP；坏压缩cap前拒绝、组件/大小写SQL、graph候选/证据与回放审查；最终覆盖文件无漂移。", "rollback_status": "保留上轮P1-A产品、入场hash及原始run；按本批文件改动切片回退，无DB schema改变、不覆盖历史题库报告。", "limitations": "macOS arm64本地；新旧同v2评分，小样本compare不作发行认证；graph/parser/P2已知失败独立保留；无Linux/Windows/Actions/live模型。"}]
实施备注：图种子/合法邻居在限额前过滤与去重；显式范围下调用点/邻居定义双检查；hydrate二次守卫；用明确邻接夹具核验而非宣称解析器正确。

### [x] P1-010｜精确符号和路径结果保底测试

状态：`done`；批次：`P1-B`；优先级：`high`。
范围：`crates/cc-search/src/engine_lane_tests.rs`；`crates/cc-eval/benchmarks/native/`
硬依赖：P1-009
步骤：加标识符、路径、单文件top1 fixtures；防语义式提示压掉精确目标
交付物：精确符号和路径结果保底测试的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05, V19 验证证据（实施时生成）
验收：完全匹配任务不依赖query字典或特别白名单；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05；V19
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "3ac1a68ba1db00de9672b7be5e32395df183c5836cdb55636a7ee609e761e05c", "run_id": "p1b-20260927", "artifacts": ["artifacts/benchmarks/p1b-20260927/final/validation.json", "artifacts/benchmarks/p1b-20260927/paired/summary.json", "artifacts/benchmarks/p1b-20260927/paired/commands.json", "artifacts/benchmarks/p1b-20260927/scan-summary.json", "artifacts/benchmarks/p1b-20260927/evidence-classification.json", "docs/roadmap/code-index-v2/P1-B-IMPLEMENTATION.md"], "review": "两工具链strict/test/doctest/build及真实MCP；坏压缩cap前拒绝、组件/大小写SQL、graph候选/证据与回放审查；最终覆盖文件无漂移。", "rollback_status": "保留上轮P1-A产品、入场hash及原始run；按本批文件改动切片回退，无DB schema改变、不覆盖历史题库报告。", "limitations": "macOS arm64本地；新旧同v2评分，小样本compare不作发行认证；graph/parser/P2已知失败独立保留；无Linux/Windows/Actions/live模型。"}]
实施备注：精确符号/路径候选预留与exact-target排序层；新8题3个路径漏召回修复，旧14题不退化；symbol-mode已有path_prefix接线。

### [x] P1-011｜范围与排序缓存键更新

状态：`done`；批次：`P1-C`；优先级：`high`。
范围：`crates/cc-search/src/engine_cache.rs`；`crates/cc-search/src/engine.rs`
硬依赖：P0-020, P1-005, P1-010
步骤：将hard/soft区分、排序政策和预算纳入hash；校验空范围缓存
交付物：范围与排序缓存键更新的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05, V11 验证证据（实施时生成）
验收：不同scope不共享错误结果，配置变更可失效；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05；V11
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "0d8c117b7ac95d997622de8a9ef09705b7c09b897620155051b471b7bcb2ae3e", "run_id": "p1c-20260927", "artifacts": ["artifacts/benchmarks/p1c-20260927/final/validation.json", "artifacts/benchmarks/p1c-20260927/paired/summary.json", "artifacts/benchmarks/p1c-20260927/mcp-compatibility.json", "artifacts/benchmarks/p1c-20260927/paired/intent-slice-comparison.json", "artifacts/benchmarks/p1c-20260927/ablation-run/ablation.json", "artifacts/benchmarks/p1c-20260927/ablation-frozen-run/ablation.json", "artifacts/benchmarks/p1c-20260927/ablation-frozen-comparisons.json", "docs/roadmap/code-index-v2/P1-C-IMPLEMENTATION.md"], "review": "source/config/negative/actual SDK shapes inspected; frozen stable and 1.95 all-target strict, full tests and real MCP passed; no final source drift", "rollback_status": "entry-source.tar.gz and P1-B binaries retained; revert only P1-C source slices, preserve prior work and raw experiments; no DB schema change", "limitations": ["macOS arm64 local only, no Actions/Linux/Windows/model run", "tiny authored corpora are not general semantic or holdout certification", "P2 two signature mutation oracles remain false/exit1; smoke S11 remains failed", "counterfactual binaries use P1-B scaffold with declared two-factor changes, not historical complete G0", "cached grep diagnostics describe originating work, not repeat work on cache hits"]}]
实施备注：完整检索配置/政策/档位和图预算键控；空范围、显式配置重载与冷热矩阵通过，不宣称已发现跨引擎缓存污染。

### [x] P1-012｜可观测scope解释

状态：`done`；批次：`P1-C`；优先级：`high`。
范围：`crates/cc-model/src/context.rs`；`crates/cc-search/src/score_trace.rs`
硬依赖：P1-011
步骤：输出hard约束、soft贡献和扫描截断原因；避免泄露被排除路径
交付物：可观测scope解释的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05, V12 验证证据（实施时生成）
验收：用户能区分过滤和排序，score_trace可回放；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05；V12
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "0d8c117b7ac95d997622de8a9ef09705b7c09b897620155051b471b7bcb2ae3e", "run_id": "p1c-20260927", "artifacts": ["artifacts/benchmarks/p1c-20260927/final/validation.json", "artifacts/benchmarks/p1c-20260927/paired/summary.json", "artifacts/benchmarks/p1c-20260927/mcp-compatibility.json", "artifacts/benchmarks/p1c-20260927/paired/intent-slice-comparison.json", "artifacts/benchmarks/p1c-20260927/ablation-run/ablation.json", "artifacts/benchmarks/p1c-20260927/ablation-frozen-run/ablation.json", "artifacts/benchmarks/p1c-20260927/ablation-frozen-comparisons.json", "docs/roadmap/code-index-v2/P1-C-IMPLEMENTATION.md"], "review": "source/config/negative/actual SDK shapes inspected; frozen stable and 1.95 all-target strict, full tests and real MCP passed; no final source drift", "rollback_status": "entry-source.tar.gz and P1-B binaries retained; revert only P1-C source slices, preserve prior work and raw experiments; no DB schema change", "limitations": ["macOS arm64 local only, no Actions/Linux/Windows/model run", "tiny authored corpora are not general semantic or holdout certification", "P2 two signature mutation oracles remain false/exit1; smoke S11 remains failed", "counterfactual binaries use P1-B scaffold with declared two-factor changes, not historical complete G0", "cached grep diagnostics describe originating work, not repeat work on cache hits"]}]
实施备注：hard/soft/budget/ordering解释接线到空结果和图缓存，提示仅计数；源trace回放与缓存诊断origin口径明确。

### [x] P1-013｜MCP契约与unknown参数回归

状态：`done`；批次：`P1-C`；优先级：`high`。
范围：`crates/cc-server/src/tools.rs`；`crates/cc-server/src/mcp.rs`
硬依赖：P1-012
步骤：保持旧mode/参数名；验证新内部scope不改变sanitize边界
交付物：MCP契约与unknown参数回归的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V18 验证证据（实施时生成）
验收：14工具旧有效请求继续可用，未知字段仍拒绝；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V18
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "0d8c117b7ac95d997622de8a9ef09705b7c09b897620155051b471b7bcb2ae3e", "run_id": "p1c-20260927", "artifacts": ["artifacts/benchmarks/p1c-20260927/final/validation.json", "artifacts/benchmarks/p1c-20260927/paired/summary.json", "artifacts/benchmarks/p1c-20260927/mcp-compatibility.json", "artifacts/benchmarks/p1c-20260927/paired/intent-slice-comparison.json", "artifacts/benchmarks/p1c-20260927/ablation-run/ablation.json", "artifacts/benchmarks/p1c-20260927/ablation-frozen-run/ablation.json", "artifacts/benchmarks/p1c-20260927/ablation-frozen-comparisons.json", "docs/roadmap/code-index-v2/P1-C-IMPLEMENTATION.md"], "review": "source/config/negative/actual SDK shapes inspected; frozen stable and 1.95 all-target strict, full tests and real MCP passed; no final source drift", "rollback_status": "entry-source.tar.gz and P1-B binaries retained; revert only P1-C source slices, preserve prior work and raw experiments; no DB schema change", "limitations": ["macOS arm64 local only, no Actions/Linux/Windows/model run", "tiny authored corpora are not general semantic or holdout certification", "P2 two signature mutation oracles remain false/exit1; smoke S11 remains failed", "counterfactual binaries use P1-B scaffold with declared two-factor changes, not historical complete G0", "cached grep diagnostics describe originating work, not repeat work on cache hits"]}]
实施备注：十四工具真实有效/unknown/schema矩阵通过；旧P1-B和新产品契约收据一致，保留tool-error与RPC错误的差别。

### [x] P1-014｜小型中文/英文意图回归

状态：`done`；批次：`P1-C`；优先级：`high`。
范围：`crates/cc-eval/benchmarks/native/`；`crates/cc-eval/src/benchmark/runner.rs`
硬依赖：P1-013
步骤：对配置、调用点、定义、错误路径分别跑；记录无词面重合仍局限
交付物：小型中文/英文意图回归的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V19 验证证据（实施时生成）
验收：不夸大无embedding自然语言覆盖，输出每类delta；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V19
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "0d8c117b7ac95d997622de8a9ef09705b7c09b897620155051b471b7bcb2ae3e", "run_id": "p1c-20260927", "artifacts": ["artifacts/benchmarks/p1c-20260927/final/validation.json", "artifacts/benchmarks/p1c-20260927/paired/summary.json", "artifacts/benchmarks/p1c-20260927/mcp-compatibility.json", "artifacts/benchmarks/p1c-20260927/paired/intent-slice-comparison.json", "artifacts/benchmarks/p1c-20260927/ablation-run/ablation.json", "artifacts/benchmarks/p1c-20260927/ablation-frozen-run/ablation.json", "artifacts/benchmarks/p1c-20260927/ablation-frozen-comparisons.json", "docs/roadmap/code-index-v2/P1-C-IMPLEMENTATION.md"], "review": "source/config/negative/actual SDK shapes inspected; frozen stable and 1.95 all-target strict, full tests and real MCP passed; no final source drift", "rollback_status": "entry-source.tar.gz and P1-B binaries retained; revert only P1-C source slices, preserve prior work and raw experiments; no DB schema change", "limitations": ["macOS arm64 local only, no Actions/Linux/Windows/model run", "tiny authored corpora are not general semantic or holdout certification", "P2 two signature mutation oracles remain false/exit1; smoke S11 remains failed", "counterfactual binaries use P1-B scaffold with declared two-factor changes, not historical complete G0", "cached grep diagnostics describe originating work, not repeat work on cache hits"]}]
实施备注：18题EN/ZH固定小集及query-slices报告，旧新四意图类别delta均零；标识符8题全中、纯描述8题Top1=.25；不以词典调分。

### [x] P1-015｜召回质量配对消融

状态：`done`；批次：`P1-C`；优先级：`high`。
范围：`crates/cc-eval/src/benchmark/ablation.rs`；`artifacts/benchmarks/`
硬依赖：P1-014
步骤：单测BM25与softscope两项收益；固定其余配置/输入/预算
交付物：召回质量配对消融的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V19 验证证据（实施时生成）
验收：能归因每项变化，不只报总分提高；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V19
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "0d8c117b7ac95d997622de8a9ef09705b7c09b897620155051b471b7bcb2ae3e", "run_id": "p1c-20260927", "artifacts": ["artifacts/benchmarks/p1c-20260927/final/validation.json", "artifacts/benchmarks/p1c-20260927/paired/summary.json", "artifacts/benchmarks/p1c-20260927/mcp-compatibility.json", "artifacts/benchmarks/p1c-20260927/paired/intent-slice-comparison.json", "artifacts/benchmarks/p1c-20260927/ablation-run/ablation.json", "artifacts/benchmarks/p1c-20260927/ablation-frozen-run/ablation.json", "artifacts/benchmarks/p1c-20260927/ablation-frozen-comparisons.json", "docs/roadmap/code-index-v2/P1-C-IMPLEMENTATION.md"], "review": "source/config/negative/actual SDK shapes inspected; frozen stable and 1.95 all-target strict, full tests and real MCP passed; no final source drift", "rollback_status": "entry-source.tar.gz and P1-B binaries retained; revert only P1-C source slices, preserve prior work and raw experiments; no DB schema change", "limitations": ["macOS arm64 local only, no Actions/Linux/Windows/model run", "tiny authored corpora are not general semantic or holdout certification", "P2 two signature mutation oracles remain false/exit1; smoke S11 remains failed", "counterfactual binaries use P1-B scaffold with declared two-factor changes, not historical complete G0", "cached grep diagnostics describe originating work, not repeat work on cache hits"]}]
实施备注：独立参考源二因素四格与source/binary/build收据；三题机制集+原14题共204次请求，12组回放；BM25和softscope收益分别归因，非发行/总体显著性认证。

### [x] P1-016｜检索解压与SQL成本对照

状态：`done`；批次：`P1-D`；优先级：`high`。
范围：`crates/cc-eval/src/benchmark/sampler.rs`；`crates/cc-search/src/engine.rs`
硬依赖：P0-020, P1-015
步骤：测soft放开后的扫描/候选/解压代价；用扫描cap而非硬漏召回控成本
交付物：检索解压与SQL成本对照的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V20 验证证据（实施时生成）
验收：质量变化附资源账单，不能以无声少搜换速度；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V20
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "aa25c36d70e40dc3233568159caffbdec90cc2262081be4c2b7e20b432b86d73", "run_id": "p1d-20260927", "artifacts": ["artifacts/benchmarks/p1d-20260927/final-v2/validation.json", "artifacts/benchmarks/p1d-20260927/paired/summary.json", "artifacts/benchmarks/p1d-20260927/cost-experiment/summary-v2.json", "artifacts/benchmarks/p1d-20260927/concurrency-summary.json", "artifacts/benchmarks/p1d-20260927/evidence-classification.json", "docs/roadmap/code-index-v2/P1-GATE.json", "docs/roadmap/code-index-v2/P1-D-IMPLEMENTATION.md"], "review": "actual source/SQL/cache generation, negative/real-MCP scope, fixed-input and cost provenance reviewed; independent binary digest check; both toolchains, source rechecked", "rollback_status": "entry-source.tar.gz and old binary receipts retained; no DB schema change, revert only P1-D slice without deleting inherited work/raw failures", "limitations": [{"issue": "B03-Python/B03-Rust", "state": "unresolved_observed_exit1", "owner": "P2", "receipt": "artifacts/benchmarks/p1d-20260927/paired"}, {"issue": "B15/S11", "state": "unresolved_observed_exit1", "owner": "P5-006 QueryPolicy and no-answer/selector calibration; review recorded, no benchmark waiver"}, {"issue": "whole_query_generation_snapshot/deadline/global_queue", "state": "not_implemented", "owner": "P5-008/009/011"}, {"issue": "Linux/Windows/remote Actions/100k/holdout/live OCE and embedding", "state": "not_run", "owner": "later certified profiles"}]}]
实施备注：分部分SQL/文本账单贯穿MCP和costs.jsonl；1k/5k同源独立release目标两变体1800次，公开不足覆盖范围；首次复用binary对照无效并保留。

### [x] P1-017｜并发与读池退化测试

状态：`done`；批次：`P1-D`；优先级：`high`。
范围：`crates/cc-search/src/engine_lane_tests.rs`；`crates/cc-eval/tests/benchmark_adapters.rs`
硬依赖：P1-016
步骤：在1连接池、并发查询/写入下跑修复；记录有界等待
交付物：并发与读池退化测试的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V20 验证证据（实施时生成）
验收：无嵌套连接死锁、无scope串扰；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V20
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "aa25c36d70e40dc3233568159caffbdec90cc2262081be4c2b7e20b432b86d73", "run_id": "p1d-20260927", "artifacts": ["artifacts/benchmarks/p1d-20260927/final-v2/validation.json", "artifacts/benchmarks/p1d-20260927/paired/summary.json", "artifacts/benchmarks/p1d-20260927/cost-experiment/summary-v2.json", "artifacts/benchmarks/p1d-20260927/concurrency-summary.json", "artifacts/benchmarks/p1d-20260927/evidence-classification.json", "docs/roadmap/code-index-v2/P1-GATE.json", "docs/roadmap/code-index-v2/P1-D-IMPLEMENTATION.md"], "review": "actual source/SQL/cache generation, negative/real-MCP scope, fixed-input and cost provenance reviewed; independent binary digest check; both toolchains, source rechecked", "rollback_status": "entry-source.tar.gz and old binary receipts retained; no DB schema change, revert only P1-D slice without deleting inherited work/raw failures", "limitations": [{"issue": "B03-Python/B03-Rust", "state": "unresolved_observed_exit1", "owner": "P2", "receipt": "artifacts/benchmarks/p1d-20260927/paired"}, {"issue": "B15/S11", "state": "unresolved_observed_exit1", "owner": "P5-006 QueryPolicy and no-answer/selector calibration; review recorded, no benchmark waiver"}, {"issue": "whole_query_generation_snapshot/deadline/global_queue", "state": "not_implemented", "owner": "P5-008/009/011"}, {"issue": "Linux/Windows/remote Actions/100k/holdout/live OCE and embedding", "state": "not_run", "owner": "later certified profiles"}]}]
实施备注：核心单读连接C=1/4/8/16与真实MCP64搜索/8增量通过；旧文本晚到回填、stats嵌套checkout均有效红绿修复；未扩大pool/watchdog；watcher强前置且native保留。

### [x] P1-018｜删除重复过滤与旧分支

状态：`done`；批次：`P1-D`；优先级：`high`。
范围：`crates/cc-search/src/plan.rs`；`crates/cc-search/src/preselect.rs`
硬依赖：P1-017
步骤：迁移调用点后移除隐式scope旧行为；保留必要wire映射
交付物：删除重复过滤与旧分支的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05, V18 验证证据（实施时生成）
验收：不长期保留两个可切换的范围真相；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05；V18
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "aa25c36d70e40dc3233568159caffbdec90cc2262081be4c2b7e20b432b86d73", "run_id": "p1d-20260927", "artifacts": ["artifacts/benchmarks/p1d-20260927/final-v2/validation.json", "artifacts/benchmarks/p1d-20260927/paired/summary.json", "artifacts/benchmarks/p1d-20260927/cost-experiment/summary-v2.json", "artifacts/benchmarks/p1d-20260927/concurrency-summary.json", "artifacts/benchmarks/p1d-20260927/evidence-classification.json", "docs/roadmap/code-index-v2/P1-GATE.json", "docs/roadmap/code-index-v2/P1-D-IMPLEMENTATION.md"], "review": "actual source/SQL/cache generation, negative/real-MCP scope, fixed-input and cost provenance reviewed; independent binary digest check; both toolchains, source rechecked", "rollback_status": "entry-source.tar.gz and old binary receipts retained; no DB schema change, revert only P1-D slice without deleting inherited work/raw failures", "limitations": [{"issue": "B03-Python/B03-Rust", "state": "unresolved_observed_exit1", "owner": "P2", "receipt": "artifacts/benchmarks/p1d-20260927/paired"}, {"issue": "B15/S11", "state": "unresolved_observed_exit1", "owner": "P5-006 QueryPolicy and no-answer/selector calibration; review recorded, no benchmark waiver"}, {"issue": "whole_query_generation_snapshot/deadline/global_queue", "state": "not_implemented", "owner": "P5-008/009/011"}, {"issue": "Linux/Windows/remote Actions/100k/holdout/live OCE and embedding", "state": "not_run", "owner": "later certified profiles"}]}]
实施备注：图富化复用同一SearchPlan/HardScope及top_k，删除第二份DSL/参数归一化；保留SQL限额前和hydrate必要守卫及单一路径的公开包装函数。

### [x] P1-019｜同步搜索文档与行为说明

状态：`done`；批次：`P1-D`；优先级：`high`。
范围：`docs/internals/SEARCH.md`；`docs/MCP_TOOLS.md`；`docs/CONFIGURATION.md`
硬依赖：P1-018
步骤：明确硬约束/软提示和budget含义；文档例由契约测试校验
交付物：同步搜索文档与行为说明的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V18, V21 验证证据（实施时生成）
验收：文档不再承诺预选内即全仓完成；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V18；V21
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "aa25c36d70e40dc3233568159caffbdec90cc2262081be4c2b7e20b432b86d73", "run_id": "p1d-20260927", "artifacts": ["artifacts/benchmarks/p1d-20260927/final-v2/validation.json", "artifacts/benchmarks/p1d-20260927/paired/summary.json", "artifacts/benchmarks/p1d-20260927/cost-experiment/summary-v2.json", "artifacts/benchmarks/p1d-20260927/concurrency-summary.json", "artifacts/benchmarks/p1d-20260927/evidence-classification.json", "docs/roadmap/code-index-v2/P1-GATE.json", "docs/roadmap/code-index-v2/P1-D-IMPLEMENTATION.md"], "review": "actual source/SQL/cache generation, negative/real-MCP scope, fixed-input and cost provenance reviewed; independent binary digest check; both toolchains, source rechecked", "rollback_status": "entry-source.tar.gz and old binary receipts retained; no DB schema change, revert only P1-D slice without deleting inherited work/raw failures", "limitations": [{"issue": "B03-Python/B03-Rust", "state": "unresolved_observed_exit1", "owner": "P2", "receipt": "artifacts/benchmarks/p1d-20260927/paired"}, {"issue": "B15/S11", "state": "unresolved_observed_exit1", "owner": "P5-006 QueryPolicy and no-answer/selector calibration; review recorded, no benchmark waiver"}, {"issue": "whole_query_generation_snapshot/deadline/global_queue", "state": "not_implemented", "owner": "P5-008/009/011"}, {"issue": "Linux/Windows/remote Actions/100k/holdout/live OCE and embedding", "state": "not_run", "owner": "later certified profiles"}]}]
实施备注：SEARCH/CONFIGURATION/MCP成本、范围、正确BM25公式、缓存epoch与预算口径统一；Markdown请求示例提取后执行真实MCP，未知字段契约不变。

### [x] P1-020｜P1验收与known-failure关闭

状态：`done`；批次：`P1-D`；优先级：`blocking`。
范围：`docs/roadmap/code-index-v2/`；`artifacts/benchmarks/`
硬依赖：P1-001, P1-002, P1-003, P1-004, P1-005, P1-006, P1-007, P1-008, P1-009, P1-010, P1-011, P1-012, P1-013, P1-014, P1-015, P1-016, P1-017, P1-018, P1-019
步骤：重跑G1和全部旧回归；关BM25/scope对应基线豁免
交付物：P1验收与known-failure关闭的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05, V18, V19, V20 验证证据（实施时生成）
验收：V05全过且可比质量/成本证据完整，未知问题保留；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05；V18；V19；V20
回滚：按行为切片回退代码并恢复冻结排序配置；不得用扩大scope作为回滚。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "aa25c36d70e40dc3233568159caffbdec90cc2262081be4c2b7e20b432b86d73", "run_id": "p1d-20260927", "artifacts": ["artifacts/benchmarks/p1d-20260927/final-v2/validation.json", "artifacts/benchmarks/p1d-20260927/paired/summary.json", "artifacts/benchmarks/p1d-20260927/cost-experiment/summary-v2.json", "artifacts/benchmarks/p1d-20260927/concurrency-summary.json", "artifacts/benchmarks/p1d-20260927/evidence-classification.json", "docs/roadmap/code-index-v2/P1-GATE.json", "docs/roadmap/code-index-v2/P1-D-IMPLEMENTATION.md"], "review": "actual source/SQL/cache generation, negative/real-MCP scope, fixed-input and cost provenance reviewed; independent binary digest check; both toolchains, source rechecked", "rollback_status": "entry-source.tar.gz and old binary receipts retained; no DB schema change, revert only P1-D slice without deleting inherited work/raw failures", "limitations": [{"issue": "B03-Python/B03-Rust", "state": "unresolved_observed_exit1", "owner": "P2", "receipt": "artifacts/benchmarks/p1d-20260927/paired"}, {"issue": "B15/S11", "state": "unresolved_observed_exit1", "owner": "P5-006 QueryPolicy and no-answer/selector calibration; review recorded, no benchmark waiver"}, {"issue": "whole_query_generation_snapshot/deadline/global_queue", "state": "not_implemented", "owner": "P5-008/009/011"}, {"issue": "Linux/Windows/remote Actions/100k/holdout/live OCE and embedding", "state": "not_run", "owner": "later certified profiles"}]}]
实施备注：G1本地限定范围通过：两工具链严格/全测/MCP、B01/B02正向阻断回归、同输入质量和成本证据完整；未声称全部原始benchmark门为绿，B03/S11和P5/P8边界保留。

## P2｜公共表面与增量正确性

### [x] P2-001｜冻结PublicSurface模型

状态：`done`；批次：`P2-A`；优先级：`high`。
范围：`crates/cc-model/src/public_surface.rs`；`crates/cc-model/src/parse.rs`
硬依赖：P1-020
步骤：区分KnownEmpty/Unknown和visibility domain；设计canonical编码
交付物：冻结PublicSurface模型的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V06 验证证据（实施时生成）
验收：未知能力不会自动产生空指纹且版本可追踪；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V06
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "aeb2d29759d4d81b577c493630ef283d37874bf7be23b904b87c864a327acec5", "run_id": "p2a-closeout-20260928", "artifacts": ["artifacts/benchmarks/p2a-closeout-20260928/final-v2/validation.json", "artifacts/benchmarks/p2a-closeout-20260928/paired/summary.json", "artifacts/benchmarks/p2a-closeout-20260928/evidence-classification.json", "docs/roadmap/code-index-v2/P2-A-GATE.json", "docs/roadmap/code-index-v2/P2-A-IMPLEMENTATION.md"], "review": "Recovered exact P2 session/source evidence; reviewed canonical/storage/language/positive dependency paths; added 3 valid red regressions and reran both toolchains/MCP; no gold changes", "rollback_status": "Preserved P1-D and intermediate source/binaries/logs; rollback binary in isolated rebuilt cache, not schema10 reuse", "limitations": ["No G2, compiler semantic completeness or negative-dependency certification", "No model, live OCE, Linux, Windows, remote Actions, holdout or release performance run", "S11 remains failed; four unanchored Chinese authored cases remain misses", "Declaration evidence is not compiler/LSP or runtime evaluation", "Unknown propagation is bounded by existing closure budget; durable frontier and negative/name-bucket dependencies remain later tasks"]}]
实施备注：PublicSurface已接ParseOutcome，Known/Empty/Unknown和可见域、token/字面量、规范编码金样验证；默认缺失为Unknown，非编译器完整接口。

### [x] P2-002｜实现规范指纹与存储

状态：`done`；批次：`P2-A`；优先级：`high`。
范围：`crates/cc-db/src/public_surface_store.rs`；`crates/cc-index/src/incremental/surface.rs`
硬依赖：P2-001
步骤：排序/长度分隔/版本hash；读写端共用规范而不重复公式
交付物：实现规范指纹与存储的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V06, V13 验证证据（实施时生成）
验收：内存与DB按人工gold一致，线程顺序不影响指纹；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V06；V13
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "aeb2d29759d4d81b577c493630ef283d37874bf7be23b904b87c864a327acec5", "run_id": "p2a-closeout-20260928", "artifacts": ["artifacts/benchmarks/p2a-closeout-20260928/final-v2/validation.json", "artifacts/benchmarks/p2a-closeout-20260928/paired/summary.json", "artifacts/benchmarks/p2a-closeout-20260928/evidence-classification.json", "docs/roadmap/code-index-v2/P2-A-GATE.json", "docs/roadmap/code-index-v2/P2-A-IMPLEMENTATION.md"], "review": "Recovered exact P2 session/source evidence; reviewed canonical/storage/language/positive dependency paths; added 3 valid red regressions and reran both toolchains/MCP; no gold changes", "rollback_status": "Preserved P1-D and intermediate source/binaries/logs; rollback binary in isolated rebuilt cache, not schema10 reuse", "limitations": ["No G2, compiler semantic completeness or negative-dependency certification", "No model, live OCE, Linux, Windows, remote Actions, holdout or release performance run", "S11 remains failed; four unanchored Chinese authored cases remain misses", "Declaration evidence is not compiler/LSP or runtime evaluation", "Unknown propagation is bounded by existing closure budget; durable frontier and negative/name-bucket dependencies remain later tasks"]}]
实施备注：共用canonical编码和同事务public_surfaces；正常/增量/staging/direct均接线；schema10，旧7/8/9缓存重建。使用现有dirty.rs而非新建空incremental/surface模块。

### [x] P2-003｜JS/TS公共表面补全

状态：`done`；批次：`P2-A`；优先级：`high`。
范围：`crates/cc-parsers/src/jsts/`；`crates/cc-parsers/src/exports/jsts.rs`
硬依赖：P2-002
步骤：覆盖type/named/default与forwarding；CommonJS静态可识别模式单独声明
交付物：JS/TS公共表面补全的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V06, V07 验证证据（实施时生成）
验收：reexport/alias变化正确传播，动态模式显式unknown；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V06；V07
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "aeb2d29759d4d81b577c493630ef283d37874bf7be23b904b87c864a327acec5", "run_id": "p2a-closeout-20260928", "artifacts": ["artifacts/benchmarks/p2a-closeout-20260928/final-v2/validation.json", "artifacts/benchmarks/p2a-closeout-20260928/paired/summary.json", "artifacts/benchmarks/p2a-closeout-20260928/evidence-classification.json", "docs/roadmap/code-index-v2/P2-A-GATE.json", "docs/roadmap/code-index-v2/P2-A-IMPLEMENTATION.md"], "review": "Recovered exact P2 session/source evidence; reviewed canonical/storage/language/positive dependency paths; added 3 valid red regressions and reran both toolchains/MCP; no gold changes", "rollback_status": "Preserved P1-D and intermediate source/binaries/logs; rollback binary in isolated rebuilt cache, not schema10 reuse", "limitations": ["No G2, compiler semantic completeness or negative-dependency certification", "No model, live OCE, Linux, Windows, remote Actions, holdout or release performance run", "S11 remains failed; four unanchored Chinese authored cases remain misses", "Declaration evidence is not compiler/LSP or runtime evaluation", "Unknown propagation is bounded by existing closure budget; durable frontier and negative/name-bucket dependencies remain later tasks"]}]
实施备注：JS/TS named/default/type、具名/两步/namespace转发与静态CommonJS子集；局部声明依赖与const/let/var；推断/动态/计算键标Unknown；现有已解析转发链回归通过。

### [x] P2-004｜Rust公共接口与可见性

状态：`done`；批次：`P2-A`；优先级：`high`。
范围：`crates/cc-parsers/src/rust.rs`；`crates/cc-parsers/src/exports/rust.rs`
硬依赖：P2-003
步骤：提取pub及受限可见性、签名、pub use；cfg摘要进入surface
交付物：Rust公共接口与可见性的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V06, V07 验证证据（实施时生成）
验收：公开签名变化触发导入者，私有body不无条件全仓重解析；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V06；V07
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "aeb2d29759d4d81b577c493630ef283d37874bf7be23b904b87c864a327acec5", "run_id": "p2a-closeout-20260928", "artifacts": ["artifacts/benchmarks/p2a-closeout-20260928/final-v2/validation.json", "artifacts/benchmarks/p2a-closeout-20260928/paired/summary.json", "artifacts/benchmarks/p2a-closeout-20260928/evidence-classification.json", "docs/roadmap/code-index-v2/P2-A-GATE.json", "docs/roadmap/code-index-v2/P2-A-IMPLEMENTATION.md"], "review": "Recovered exact P2 session/source evidence; reviewed canonical/storage/language/positive dependency paths; added 3 valid red regressions and reran both toolchains/MCP; no gold changes", "rollback_status": "Preserved P1-D and intermediate source/binaries/logs; rollback binary in isolated rebuilt cache, not schema10 reuse", "limitations": ["No G2, compiler semantic completeness or negative-dependency certification", "No model, live OCE, Linux, Windows, remote Actions, holdout or release performance run", "S11 remains failed; four unanchored Chinese authored cases remain misses", "Declaration evidence is not compiler/LSP or runtime evaluation", "Unknown propagation is bounded by existing closure budget; durable frontier and negative/name-bucket dependencies remain later tasks"]}]
实施备注：Rust词法pub/crate/restricted、类型/trait/impl签名、pubuse与cfg文本；嵌套活动属性/宏/const求值不伪装Known。原signature及声明移动目标ID回归通过。

### [x] P2-005｜Python模块表面

状态：`done`；批次：`P2-A`；优先级：`high`。
范围：`crates/cc-parsers/src/python/`；`crates/cc-parsers/src/exports/python.rs`
硬依赖：P2-004
步骤：模块定义、静态__all__、包重导出；动态导出保守标识
交付物：Python模块表面的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V06, V07 验证证据（实施时生成）
验收：__init__/star forwarding变化能触发或明确不完整；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V06；V07
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "aeb2d29759d4d81b577c493630ef283d37874bf7be23b904b87c864a327acec5", "run_id": "p2a-closeout-20260928", "artifacts": ["artifacts/benchmarks/p2a-closeout-20260928/final-v2/validation.json", "artifacts/benchmarks/p2a-closeout-20260928/paired/summary.json", "artifacts/benchmarks/p2a-closeout-20260928/evidence-classification.json", "docs/roadmap/code-index-v2/P2-A-GATE.json", "docs/roadmap/code-index-v2/P2-A-IMPLEMENTATION.md"], "review": "Recovered exact P2 session/source evidence; reviewed canonical/storage/language/positive dependency paths; added 3 valid red regressions and reran both toolchains/MCP; no gold changes", "rollback_status": "Preserved P1-D and intermediate source/binaries/logs; rollback binary in isolated rebuilt cache, not schema10 reuse", "limitations": ["No G2, compiler semantic completeness or negative-dependency certification", "No model, live OCE, Linux, Windows, remote Actions, holdout or release performance run", "S11 remains failed; four unanchored Chinese authored cases remain misses", "Declaration evidence is not compiler/LSP or runtime evaluation", "Unknown propagation is bounded by existing closure budget; durable frontier and negative/name-bucket dependencies remain later tasks"]}]
实施备注：Python模块/类绑定、静态__all__、包具名转发、生成器形态；星号/动态/装饰器/未建模实例明确Unknown；原signature与package回归通过。

### [x] P2-006｜Go package公共接口

状态：`done`；批次：`P2-B`；优先级：`high`。
范围：`crates/cc-parsers/src/go.rs`；`crates/cc-parsers/src/exports/go.rs`
硬依赖：P1-020, P2-001, P2-002
步骤：提取导出名/类型/方法与包贡献；处理同包多文件组合
交付物：Go package公共接口的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V06, V07 验证证据（实施时生成）
验收：同包文件增删影响surface，非导出局部变量不污染；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V06；V07
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "f561180fac26dbbcd5615b7079f22052123b83ac7f452395d35971da707b591d", "run_id": "p2b-20260928", "artifacts": ["artifacts/benchmarks/p2b-20260928/final/validation.json", "artifacts/benchmarks/p2b-20260928/paired/summary.json", "artifacts/benchmarks/p2b-20260928/incremental-summary.json", "artifacts/benchmarks/p2b-20260928/evidence-classification.json", "docs/roadmap/code-index-v2/P2-B-GATE.json", "docs/roadmap/code-index-v2/P2-B-IMPLEMENTATION.md"], "review": "Reviewed declaration/candidate identity, bounded serialization, transactional ownership, package views and dependency consumption; all frozen two-toolchain and real MCP checks passed; no gold rewrites", "rollback_status": "Schema12 vs P2A10/draft11 requires isolated full rebuild; retained source archive, exact predecessor binaries, patch and failed observations; daily index untouched", "limitations": ["P2-C durable remainder/freshness not implemented", "P3 full module/config/build-condition semantics not implemented", "No compiler/runtime/embedding/live OCE/Linux/Windows/Actions/holdout/100k/release certification", "S11 raw gate remains failed; small-sample comparisons inconclusive"]}]
实施备注：Go既有AST、版本化包贡献、生产/内部测试隔离、包级名称共享索引已贯通；方法不误作包函数，build条件Unknown；无编译器或性能认证。

### [x] P2-007｜其余语言保守能力表

状态：`done`；批次：`P2-B`；优先级：`high`。
范围：`crates/cc-parsers/src/exports/conservative.rs`；`docs/LANGUAGES.md`
硬依赖：P2-006
步骤：为Java/C/C++/generic标声明覆盖；未知构造触发保守策略
交付物：其余语言保守能力表的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V06 验证证据（实施时生成）
验收：不宣称完整编译器语义，unsupported不会被当成normal；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V06
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "f561180fac26dbbcd5615b7079f22052123b83ac7f452395d35971da707b591d", "run_id": "p2b-20260928", "artifacts": ["artifacts/benchmarks/p2b-20260928/final/validation.json", "artifacts/benchmarks/p2b-20260928/paired/summary.json", "artifacts/benchmarks/p2b-20260928/incremental-summary.json", "artifacts/benchmarks/p2b-20260928/evidence-classification.json", "docs/roadmap/code-index-v2/P2-B-GATE.json", "docs/roadmap/code-index-v2/P2-B-IMPLEMENTATION.md"], "review": "Reviewed declaration/candidate identity, bounded serialization, transactional ownership, package views and dependency consumption; all frozen two-toolchain and real MCP checks passed; no gold rewrites", "rollback_status": "Schema12 vs P2A10/draft11 requires isolated full rebuild; retained source archive, exact predecessor binaries, patch and failed observations; daily index untouched", "limitations": ["P2-C durable remainder/freshness not implemented", "P3 full module/config/build-condition semantics not implemented", "No compiler/runtime/embedding/live OCE/Linux/Windows/Actions/holdout/100k/release certification", "S11 raw gate remains failed; small-sample comparisons inconclusive"]}]
实施备注：Java/C/C++/spec-driven/generic保守能力表保留具体语言和原因；已有提取不冒充已知完整接口。

### [x] P2-008｜定义ResolutionOutcome与歧义

状态：`done`；批次：`P2-B`；优先级：`high`。
范围：`crates/cc-model/src/resolution.rs`；`crates/cc-index/src/resolver/resolve_outcome.rs`
硬依赖：P2-007
步骤：Resolved/Ambiguous/Unresolved/Unsupported分型；保存必要候选原因
交付物：定义ResolutionOutcome与歧义的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V07 验证证据（实施时生成）
验收：同名候选不能因内部顺序被标成唯一精确目标；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V07
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "f561180fac26dbbcd5615b7079f22052123b83ac7f452395d35971da707b591d", "run_id": "p2b-20260928", "artifacts": ["artifacts/benchmarks/p2b-20260928/final/validation.json", "artifacts/benchmarks/p2b-20260928/paired/summary.json", "artifacts/benchmarks/p2b-20260928/incremental-summary.json", "artifacts/benchmarks/p2b-20260928/evidence-classification.json", "docs/roadmap/code-index-v2/P2-B-GATE.json", "docs/roadmap/code-index-v2/P2-B-IMPLEMENTATION.md"], "review": "Reviewed declaration/candidate identity, bounded serialization, transactional ownership, package views and dependency consumption; all frozen two-toolchain and real MCP checks passed; no gold rewrites", "rollback_status": "Schema12 vs P2A10/draft11 requires isolated full rebuild; retained source archive, exact predecessor binaries, patch and failed observations; daily index untouched", "limitations": ["P2-C durable remainder/freshness not implemented", "P3 full module/config/build-condition semantics not implemented", "No compiler/runtime/embedding/live OCE/Linux/Windows/Actions/holdout/100k/release certification", "S11 raw gate remains failed; small-sample comparisons inconclusive"]}]
实施备注：四类结果、稳定目标身份、有界候选/预算、逐文件原子manifest/覆盖统计已落库；歧义不选任意目标，超限保持incomplete。

### [x] P2-009｜修正warm/cold消歧差异

状态：`done`；批次：`P2-B`；优先级：`high`。
范围：`crates/cc-index/src/resolver/catalog.rs`；`crates/cc-index/src/resolver/catalog_cache.rs`
硬依赖：P2-008
步骤：统一语义评分和stable tie-break；保留目录缓存与tombstone策略
交付物：修正warm/cold消歧差异的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V07 验证证据（实施时生成）
验收：相同最终源码的多次cache历史得到相同规范结果；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V07
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "f561180fac26dbbcd5615b7079f22052123b83ac7f452395d35971da707b591d", "run_id": "p2b-20260928", "artifacts": ["artifacts/benchmarks/p2b-20260928/final/validation.json", "artifacts/benchmarks/p2b-20260928/paired/summary.json", "artifacts/benchmarks/p2b-20260928/incremental-summary.json", "artifacts/benchmarks/p2b-20260928/evidence-classification.json", "docs/roadmap/code-index-v2/P2-B-GATE.json", "docs/roadmap/code-index-v2/P2-B-IMPLEMENTATION.md"], "review": "Reviewed declaration/candidate identity, bounded serialization, transactional ownership, package views and dependency consumption; all frozen two-toolchain and real MCP checks passed; no gold rewrites", "rollback_status": "Schema12 vs P2A10/draft11 requires isolated full rebuild; retained source archive, exact predecessor binaries, patch and failed observations; daily index untouched", "limitations": ["P2-C durable remainder/freshness not implemented", "P3 full module/config/build-condition semantics not implemented", "No compiler/runtime/embedding/live OCE/Linux/Windows/Actions/holdout/100k/release certification", "S11 raw gate remains failed; small-sample comparisons inconclusive"]}]
实施备注：冷热和插入顺序回归、类型/别名冲突、真实增量历史与独立全量规范对照通过；目录seed验证/tombstone保留。

### [x] P2-010｜加入正向与负向解析依赖

状态：`done`；批次：`P2-B`；优先级：`high`。
范围：`crates/cc-db/src/resolution_dependency_store.rs`；`crates/cc-index/src/indexer_phases/dependencies.rs`；`crates/cc-index/src/indexer_phases/dirty.rs`
硬依赖：P2-009
步骤：记录target surface、name bucket、missing path与配置条件；去重并有界存储
交付物：加入正向与负向解析依赖的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V07, V13 验证证据（实施时生成）
验收：新增同名或补全原缺失文件使相关旧解析失效；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V07；V13
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "f561180fac26dbbcd5615b7079f22052123b83ac7f452395d35971da707b591d", "run_id": "p2b-20260928", "artifacts": ["artifacts/benchmarks/p2b-20260928/final/validation.json", "artifacts/benchmarks/p2b-20260928/paired/summary.json", "artifacts/benchmarks/p2b-20260928/incremental-summary.json", "artifacts/benchmarks/p2b-20260928/evidence-classification.json", "docs/roadmap/code-index-v2/P2-B-GATE.json", "docs/roadmap/code-index-v2/P2-B-IMPLEMENTATION.md"], "review": "Reviewed declaration/candidate identity, bounded serialization, transactional ownership, package views and dependency consumption; all frozen two-toolchain and real MCP checks passed; no gold rewrites", "rollback_status": "Schema12 vs P2A10/draft11 requires isolated full rebuild; retained source archive, exact predecessor binaries, patch and failed observations; daily index untouched", "limitations": ["P2-C durable remainder/freshness not implemented", "P3 full module/config/build-condition semantics not implemented", "No compiler/runtime/embedding/live OCE/Linux/Windows/Actions/holdout/100k/release certification", "S11 raw gate remains failed; small-sample comparisons inconclusive"]}]
实施备注：名称/路径/包/配置依赖同事务并被旧closure实际消费；新增同名、补缺路径、事件范围及配置变更通过。超限仍需P2-C持久frontier，未提前完成。

### [x] P2-011｜接入变化分类与DirtyPlanner

状态：`done`；批次：`P2-C`；优先级：`high`。
范围：`crates/cc-index/src/indexer_phases/dirty.rs`；`crates/cc-model/src/freshness.rs`；`crates/cc-index/src/indexer_phases/reconcile.rs`
硬依赖：P1-020, P2-006, P2-010
步骤：拆body/API/config/inventory原因；消费旧dirty closure与reload策略
交付物：接入变化分类与DirtyPlanner的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V06, V07 验证证据（实施时生成）
验收：最小工作量有理由，缺依赖信息时保守扩大而非跳过；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V06；V07
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "c392dc347bcd0a11cb3d7addb6eaf81c5656ec4c513aa9f446038530f2df8a8a", "run_id": "p2c-20260928-OttX/final + paired", "artifacts": ["artifacts/benchmarks/p2c-20260928-OttX/final/validation.json", "artifacts/benchmarks/p2c-20260928-OttX/paired/summary.json", "artifacts/benchmarks/p2c-20260928-OttX/evidence-classification.json", "artifacts/benchmarks/p2c-20260928-OttX/observations/final-stable-real-mcp/p2c-mcp.json", "docs/internals/INCREMENTAL_RECOVERY.md"], "commands_receipt": "artifacts/benchmarks/p2c-20260928-OttX/final/validation.json", "review": "504-file contents and membership frozen; 22 commands exit0 across stable/1.95, old/new real MCP and original mutations checked; independent parser/DB/watcher negatives retained", "rollback_status": "schema13 incompatible with12; preserved entry archive and P2B binary; isolated cache rebuild and existing durable-asset recovery, no daily index touched", "limitations": "passed local batch only, not P2/G2 or M1; bounded declared static semantics; S11 still fails; no 100k/holdout/live semantic/release/platform certification"}]
实施备注：P2-C复用已有surface/地址/依赖事件和有界closure，加入可观测变化原因、预算/处理计数、resumed/rebased。仅有实际候选依赖才把非API变化提升为失效根，原body-only不额外重解析回归保持通过；没有新建第二套依赖引擎。

### [x] P2-012｜处理重导出环与多步收敛

状态：`done`；批次：`P2-C`；优先级：`high`。
范围：`crates/cc-index/src/dirty_closure.rs`；`crates/cc-index/src/indexer_phases/reconcile.rs`；`crates/cc-eval/tests/p2c_reconcile.rs`
硬依赖：P2-011
步骤：对环定义稳定组件贡献；增加ES/Rust/Python链环夹具
交付物：处理重导出环与多步收敛的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V07 验证证据（实施时生成）
验收：无hash递归自激活，固定点有限收敛或明确预算状态；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V07
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "c392dc347bcd0a11cb3d7addb6eaf81c5656ec4c513aa9f446038530f2df8a8a", "run_id": "p2c-20260928-OttX/final + paired", "artifacts": ["artifacts/benchmarks/p2c-20260928-OttX/final/validation.json", "artifacts/benchmarks/p2c-20260928-OttX/paired/summary.json", "artifacts/benchmarks/p2c-20260928-OttX/evidence-classification.json", "artifacts/benchmarks/p2c-20260928-OttX/observations/final-stable-real-mcp/p2c-mcp.json", "docs/internals/INCREMENTAL_RECOVERY.md"], "commands_receipt": "artifacts/benchmarks/p2c-20260928-OttX/final/validation.json", "review": "504-file contents and membership frozen; 22 commands exit0 across stable/1.95, old/new real MCP and original mutations checked; independent parser/DB/watcher negatives retained", "rollback_status": "schema13 incompatible with12; preserved entry archive and P2B binary; isolated cache rebuild and existing durable-asset recovery, no daily index touched", "limitations": "passed local batch only, not P2/G2 or M1; bounded declared static semantics; S11 still fails; no 100k/holdout/live semantic/release/platform certification"}]
实施备注：P2-C以有限文件贡献集合而非递归hash传播；传播根与当前basis下已解析集合分离，预算前缀保留可续跑贡献。TS20层含环及Python/Rust链环实际parser/十四表full-inc和独立目标断言通过；保守转发不是编译器级模块求值。

### [x] P2-013｜完整清理脏重载目标字段

状态：`done`；批次：`P2-C`；优先级：`high`。
范围：`crates/cc-index/src/dirty_reload_policy.rs`；`crates/cc-index/src/indexer.rs`
硬依赖：P2-012
步骤：审视各edge/ref target_id/uid/path/confidence/strategy联动；统一策略
交付物：完整清理脏重载目标字段的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V07 验证证据（实施时生成）
验收：旧目标不因残留id跳过重新解析，无悬空引用；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V07
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "c392dc347bcd0a11cb3d7addb6eaf81c5656ec4c513aa9f446038530f2df8a8a", "run_id": "p2c-20260928-OttX/final + paired", "artifacts": ["artifacts/benchmarks/p2c-20260928-OttX/final/validation.json", "artifacts/benchmarks/p2c-20260928-OttX/paired/summary.json", "artifacts/benchmarks/p2c-20260928-OttX/evidence-classification.json", "artifacts/benchmarks/p2c-20260928-OttX/observations/final-stable-real-mcp/p2c-mcp.json", "docs/internals/INCREMENTAL_RECOVERY.md"], "commands_receipt": "artifacts/benchmarks/p2c-20260928-OttX/final/validation.json", "review": "504-file contents and membership frozen; 22 commands exit0 across stable/1.95, old/new real MCP and original mutations checked; independent parser/DB/watcher negatives retained", "rollback_status": "schema13 incompatible with12; preserved entry archive and P2B binary; isolated cache rebuild and existing durable-asset recovery, no daily index touched", "limitations": "passed local batch only, not P2/G2 or M1; bounded declared static semantics; S11 still fails; no 100k/holdout/live semantic/release/platform certification"}]
实施备注：P2-C完成reload字段矩阵与十四表对照；真实红测试修复持久dispatch handler UID错误保留。本地source身份、关系提取confidence与解析target策略分开；旧call/ref/route目标清理保留。Python AST真值修复移除声明/注释/字符串伪调用，动态词法绑定保留Unsupported而不由全局候选伪造目标。

### [x] P2-014｜持久化未完成闭包frontier

状态：`done`；批次：`P2-C`；优先级：`high`。
范围：`crates/cc-db/src/freshness_store.rs`；`crates/cc-index/src/indexer_phases/reconcile.rs`；`crates/cc-db/tests/p2c_freshness_store.rs`；`crates/cc-server/src/project_session.rs`
硬依赖：P2-013
步骤：保存basis/remainder/reason；预算不足不丢事件
交付物：持久化未完成闭包frontier的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V07, V13 验证证据（实施时生成）
验收：重启或下一tick可以继续，状态仍标incomplete直到闭合；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V07；V13
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "c392dc347bcd0a11cb3d7addb6eaf81c5656ec4c513aa9f446038530f2df8a8a", "run_id": "p2c-20260928-OttX/final + paired", "artifacts": ["artifacts/benchmarks/p2c-20260928-OttX/final/validation.json", "artifacts/benchmarks/p2c-20260928-OttX/paired/summary.json", "artifacts/benchmarks/p2c-20260928-OttX/evidence-classification.json", "artifacts/benchmarks/p2c-20260928-OttX/observations/final-stable-real-mcp/p2c-mcp.json", "docs/internals/INCREMENTAL_RECOVERY.md"], "commands_receipt": "artifacts/benchmarks/p2c-20260928-OttX/final/validation.json", "review": "504-file contents and membership frozen; 22 commands exit0 across stable/1.95, old/new real MCP and original mutations checked; independent parser/DB/watcher negatives retained", "rollback_status": "schema13 incompatible with12; preserved entry archive and P2B binary; isolated cache rebuild and existing durable-asset recovery, no daily index touched", "limitations": "passed local batch only, not P2/G2 or M1; bounded declared static semantics; S11 still fails; no 100k/holdout/live semantic/release/platform certification"}]
实施备注：P2-C新增同事务epoch围栏的resolution_frontier，保存原始失效因果而非截断队列。新事件rebase不丢旧根；无变化/重启/空事件及watcher续跑、删除根、disabled再启用、15消费者超查询窗口、rollback/CAS/损坏负例通过。16MiB/200k状态上限超限失败而不丢事件提交；规模成本归P2-D。

### [x] P2-015｜将freshness传到查询与MCP

状态：`done`；批次：`P2-C`；优先级：`high`。
范围：`crates/cc-model/src/freshness.rs`；`crates/cc-server/src/handlers/freshness.rs`；`crates/cc-server/src/handlers/output_budget.rs`；`crates/cc-server/src/mcp.rs`
硬依赖：P2-014
步骤：暴露不完整范围与原因；完整关系请求不能静默读取旧边
交付物：将freshness传到查询与MCP的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V07, V18 验证证据（实施时生成）
验收：status和关系工具可解释stale与retry，不虚报完整；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V07；V18
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "c392dc347bcd0a11cb3d7addb6eaf81c5656ec4c513aa9f446038530f2df8a8a", "run_id": "p2c-20260928-OttX/final + paired", "artifacts": ["artifacts/benchmarks/p2c-20260928-OttX/final/validation.json", "artifacts/benchmarks/p2c-20260928-OttX/paired/summary.json", "artifacts/benchmarks/p2c-20260928-OttX/evidence-classification.json", "artifacts/benchmarks/p2c-20260928-OttX/observations/final-stable-real-mcp/p2c-mcp.json", "docs/internals/INCREMENTAL_RECOVERY.md"], "commands_receipt": "artifacts/benchmarks/p2c-20260928-OttX/final/validation.json", "review": "504-file contents and membership frozen; 22 commands exit0 across stable/1.95, old/new real MCP and original mutations checked; independent parser/DB/watcher negatives retained", "rollback_status": "schema13 incompatible with12; preserved entry archive and P2B binary; isolated cache rebuild and existing durable-asset recovery, no daily index touched", "limitations": "passed local batch only, not P2/G2 or M1; bounded declared static semantics; S11 still fails; no 100k/holdout/live semantic/release/platform certification"}]
实施备注：P2-C在index/status及MCP对象型查询输出追加resolution_freshness；前后generation变化显式标changed_during_query，旧数组型关系在欠账时明确失败，截断保留状态。仅声明observed_resolution_invalidations范围，不宣称全查询快照/编译器正确。两工具链各13项真实MCP及14工具输入契约保持一致。

### [x] P2-016｜多语言mutation差分扩充

状态：`done`；批次：`P2-D`；优先级：`high`。
范围：`crates/cc-eval/src/benchmark/mutations.rs`；`crates/cc-eval/src/benchmark/mutation_case.rs`；`crates/cc-eval/src/bin/cc-eval.rs`；`crates/cc-eval/tests/p2d_mutations.rs`；`crates/cc-parsers/src/jsts/`；`crates/cc-parsers/tests/jsts_call_truth.rs`
硬依赖：P1-020, P2-015
步骤：覆盖visibility/signature/remove/rename/namecollision和预算场景；失败自动shrink
交付物：多语言mutation差分扩充的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V07 验证证据（实施时生成）
验收：full/inc比较保留目标与策略，最小反例可单独重放；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V07
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "26ef83740ad43a8dc3c3d924199caeb45f77426f12d6fa632860ae24494e0ba6", "run_id": "p2d-20260928-ja0e/final + paired + visibility", "artifacts": ["artifacts/benchmarks/p2d-20260928-ja0e/final/validation.json", "artifacts/benchmarks/p2d-20260928-ja0e/paired/summary.json", "artifacts/benchmarks/p2d-20260928-ja0e/paired/recovery.json", "artifacts/benchmarks/p2d-20260928-ja0e/cost-summary.json", "artifacts/benchmarks/p2d-20260928-ja0e/visibility/summary.json", "artifacts/benchmarks/p2d-20260928-ja0e/evidence-classification.json", "docs/roadmap/code-index-v2/P2-D-GATE.json"], "commands_receipt": "artifacts/benchmarks/p2d-20260928-ja0e/final/validation.json", "review": "512-file complete inventory frozen; 24 final commands exit0, stable/1.95 each1531 workspace and14 actual MCP; 14-table mutation, independent truth, bounded reducer, 306-request fixed-input replay and explicit visibility cases verified", "rollback_status": "schema14 rebuild required; P2C exact binaries and entry archive preserved; no daily index touched", "limitations": "G2 passed only declared static/local scope; V19 authored fixed suites and V20 mechanism costs are not holdout/100k/RSS/tail/platform certification; S11 fails; pair-launcher CLI path typo retained with separate successful replay"}]
实施备注：12条四语言固定变更序列108检查点、交错预算/重启/Unknown与非法输入、同失败签名缩减和独立CLI重放通过；追加4份可见性CLI输入11检查点，hash留证。JS/TS正则伪调用/引用与真实AST重复/漏遍历修复；不声称编译器访问控制或全局最小源码。

### [x] P2-017｜缓存压实和依赖存储性能

状态：`done`；批次：`P2-D`；优先级：`high`。
范围：`crates/cc-index/src/resolver/catalog_cache.rs`；`crates/cc-index/src/build_plan.rs`；`crates/cc-index/src/indexer_phases/reconcile.rs`；`crates/cc-db/src/public_surface_store.rs`；`crates/cc-db/src/resolution_dependency_store.rs`；`crates/cc-db/src/index_db_rebuild.rs`；`crates/cc-db/tests/p2d_dependency_cost.rs`；`crates/cc-eval/tests/p2d_cost.rs`
硬依赖：P2-016
步骤：测tombstone、name bucket和dependency rows增长；限定清理策略
交付物：缓存压实和依赖存储性能的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V20 验证证据（实施时生成）
验收：不能引入每次增量重建全仓目录的性能地板；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V20
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "26ef83740ad43a8dc3c3d924199caeb45f77426f12d6fa632860ae24494e0ba6", "run_id": "p2d-20260928-ja0e/final + paired + visibility", "artifacts": ["artifacts/benchmarks/p2d-20260928-ja0e/final/validation.json", "artifacts/benchmarks/p2d-20260928-ja0e/paired/summary.json", "artifacts/benchmarks/p2d-20260928-ja0e/paired/recovery.json", "artifacts/benchmarks/p2d-20260928-ja0e/cost-summary.json", "artifacts/benchmarks/p2d-20260928-ja0e/visibility/summary.json", "artifacts/benchmarks/p2d-20260928-ja0e/evidence-classification.json", "docs/roadmap/code-index-v2/P2-D-GATE.json"], "commands_receipt": "artifacts/benchmarks/p2d-20260928-ja0e/final/validation.json", "review": "512-file complete inventory frozen; 24 final commands exit0, stable/1.95 each1531 workspace and14 actual MCP; 14-table mutation, independent truth, bounded reducer, 306-request fixed-input replay and explicit visibility cases verified", "rollback_status": "schema14 rebuild required; P2C exact binaries and entry archive preserved; no daily index touched", "limitations": "G2 passed only declared static/local scope; V19 authored fixed suites and V20 mechanism costs are not holdout/100k/RSS/tail/platform certification; S11 fails; pair-launcher CLI path typo retained with separate successful replay"}]
实施备注：反向依赖窗口保留溢出见证并记录SQL work；600相关依赖在100/1k/5k背景中VM步数稳定，替换/删除不增长旧行。160轮catalog churn两次压实，1k背景24次真实增量复用目录；release1k/5k共170观测。修复自定义DB名旧WAL残留；完成前缀/持久payload仍有线性相关成本，不宣称100k/RSS/尾延迟认证。

### [x] P2-018｜删除JS专属dirty入口假设

状态：`done`；批次：`P2-D`；优先级：`high`。
范围：`crates/cc-index/src/indexer_phases/dirty.rs`；`crates/cc-db/src/index_db_graph.rs`
硬依赖：P2-017
步骤：旧export_name指纹迁移到统一surface；语言特定信息仍可保留展示
交付物：删除JS专属dirty入口假设的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V06, V07 验证证据（实施时生成）
验收：没有两套分别驱动失效的指纹规则；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V06；V07
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "26ef83740ad43a8dc3c3d924199caeb45f77426f12d6fa632860ae24494e0ba6", "run_id": "p2d-20260928-ja0e/final + paired + visibility", "artifacts": ["artifacts/benchmarks/p2d-20260928-ja0e/final/validation.json", "artifacts/benchmarks/p2d-20260928-ja0e/paired/summary.json", "artifacts/benchmarks/p2d-20260928-ja0e/paired/recovery.json", "artifacts/benchmarks/p2d-20260928-ja0e/cost-summary.json", "artifacts/benchmarks/p2d-20260928-ja0e/visibility/summary.json", "artifacts/benchmarks/p2d-20260928-ja0e/evidence-classification.json", "docs/roadmap/code-index-v2/P2-D-GATE.json"], "commands_receipt": "artifacts/benchmarks/p2d-20260928-ja0e/final/validation.json", "review": "512-file complete inventory frozen; 24 final commands exit0, stable/1.95 each1531 workspace and14 actual MCP; 14-table mutation, independent truth, bounded reducer, 306-request fixed-input replay and explicit visibility cases verified", "rollback_status": "schema14 rebuild required; P2C exact binaries and entry archive preserved; no daily index touched", "limitations": "G2 passed only declared static/local scope; V19 authored fixed suites and V20 mechanism costs are not holdout/100k/RSS/tail/platform certification; S11 fails; pair-launcher CLI path typo retained with separate successful replay"}]
实施备注：删除SQL与cc-index的旧export_name指纹公式；兼容get_export_fingerprint(s)也委托唯一PublicSurface编码，KnownEmpty和Unknown正确区分。保留语言展示/解析元数据；真实存储/内存一致性回归通过。

### [x] P2-019｜文档与支持范围对齐

状态：`done`；批次：`P2-D`；优先级：`high`。
范围：`docs/internals/INDEXING.md`；`docs/LANGUAGES.md`；`docs/internals/STORAGE.md`
硬依赖：P2-018
步骤：更新能力、预算恢复、消歧确定性与故障说明；保留未支持清单
交付物：文档与支持范围对齐的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V21 验证证据（实施时生成）
验收：文档不再承诺缓存同分可随历史选不同赢家；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V21
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "26ef83740ad43a8dc3c3d924199caeb45f77426f12d6fa632860ae24494e0ba6", "run_id": "p2d-20260928-ja0e/final + paired + visibility", "artifacts": ["artifacts/benchmarks/p2d-20260928-ja0e/final/validation.json", "artifacts/benchmarks/p2d-20260928-ja0e/paired/summary.json", "artifacts/benchmarks/p2d-20260928-ja0e/paired/recovery.json", "artifacts/benchmarks/p2d-20260928-ja0e/cost-summary.json", "artifacts/benchmarks/p2d-20260928-ja0e/visibility/summary.json", "artifacts/benchmarks/p2d-20260928-ja0e/evidence-classification.json", "docs/roadmap/code-index-v2/P2-D-GATE.json"], "commands_receipt": "artifacts/benchmarks/p2d-20260928-ja0e/final/validation.json", "review": "512-file complete inventory frozen; 24 final commands exit0, stable/1.95 each1531 workspace and14 actual MCP; 14-table mutation, independent truth, bounded reducer, 306-request fixed-input replay and explicit visibility cases verified", "rollback_status": "schema14 rebuild required; P2C exact binaries and entry archive preserved; no daily index touched", "limitations": "G2 passed only declared static/local scope; V19 authored fixed suites and V20 mechanism costs are not holdout/100k/RSS/tail/platform certification; S11 fails; pair-launcher CLI path typo retained with separate successful replay"}]
实施备注：更新schema14、恢复/工作量/语法支持、INDEXING消歧确定性和类型贡献说明。新增INCREMENTAL_VERIFICATION，明确声明分析不是编译器，V19/V20发布范围未扩张；当前rolling R09历史查询需后续独立数据审核，历史frozen gold不改。

### [x] P2-020｜P2验收与跨语言基线关闭

状态：`done`；批次：`P2-D`；优先级：`blocking`。
范围：`docs/roadmap/code-index-v2/`；`artifacts/benchmarks/`
硬依赖：P2-001, P2-002, P2-003, P2-004, P2-005, P2-006, P2-007, P2-008, P2-009, P2-010, P2-011, P2-012, P2-013, P2-014, P2-015, P2-016, P2-017, P2-018, P2-019
步骤：重跑G2与真实检索回归；映射修复到B03/B04/B14
交付物：P2验收与跨语言基线关闭的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V06, V07, V19, V20 验证证据（实施时生成）
验收：支持范围内oracle一致，Unknown与incomplete不被误计通过；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V06；V07；V19；V20
回滚：回退binary后在隔离缓存全量重建；不复用新旧不兼容解析表。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "26ef83740ad43a8dc3c3d924199caeb45f77426f12d6fa632860ae24494e0ba6", "run_id": "p2d-20260928-ja0e/final + paired + visibility", "artifacts": ["artifacts/benchmarks/p2d-20260928-ja0e/final/validation.json", "artifacts/benchmarks/p2d-20260928-ja0e/paired/summary.json", "artifacts/benchmarks/p2d-20260928-ja0e/paired/recovery.json", "artifacts/benchmarks/p2d-20260928-ja0e/cost-summary.json", "artifacts/benchmarks/p2d-20260928-ja0e/visibility/summary.json", "artifacts/benchmarks/p2d-20260928-ja0e/evidence-classification.json", "docs/roadmap/code-index-v2/P2-D-GATE.json"], "commands_receipt": "artifacts/benchmarks/p2d-20260928-ja0e/final/validation.json", "review": "512-file complete inventory frozen; 24 final commands exit0, stable/1.95 each1531 workspace and14 actual MCP; 14-table mutation, independent truth, bounded reducer, 306-request fixed-input replay and explicit visibility cases verified", "rollback_status": "schema14 rebuild required; P2C exact binaries and entry archive preserved; no daily index touched", "limitations": "G2 passed only declared static/local scope; V19 authored fixed suites and V20 mechanism costs are not holdout/100k/RSS/tail/platform certification; S11 fails; pair-launcher CLI path typo retained with separate successful replay"}]
实施备注：G2在当前声明静态子集/macOS arm64范围完成：B03统一表面与签名，B04历史/插入顺序无事实分歧，B14正负依赖和持久余量回归。最终双工具链24命令、14工具输入契约和原mutation通过；S11失败/比较inconclusive保留。配对脚本首次exit2仅CLI路径笔误，正确路径另行成功重放并归档，不改历史失败。下一批P3-A。

## P3｜项目模型与模块解析

### [x] P3-001｜定义不可变ProjectModel

状态：`done`；批次：`P3-A`；优先级：`normal`。
范围：`crates/cc-model/src/project_model.rs`；`crates/cc-index/src/project_model/mod.rs`
硬依赖：P2-020
步骤：定义package/config root/condition/FileCatalog；拆detect与resolve
交付物：定义不可变ProjectModel的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V08 验证证据（实施时生成）
验收：解析阶段不需随每条import访问磁盘；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V08
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "48750ff11621439baf933b11fa52ca6d94ae89e5abb2b8a551cc46cf65319b6e", "run_id": "p3a-20260928-KTGS/final-v2 + paired", "artifacts": ["artifacts/benchmarks/p3a-20260928-KTGS/final-v2/validation.json", "artifacts/benchmarks/p3a-20260928-KTGS/paired/summary.json", "artifacts/benchmarks/p3a-20260928-KTGS/cost-summary.json", "artifacts/benchmarks/p3a-20260928-KTGS/scope-check.json", "artifacts/benchmarks/p3a-20260928-KTGS/evidence-classification.json", "docs/roadmap/code-index-v2/P3-A-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-A-GATE.json", "docs/internals/PROJECT_MODEL.md"], "commands_receipt": "artifacts/benchmarks/p3a-20260928-KTGS/final-v2/validation.json", "review": "522-file content and membership freeze;25 final commands exit0;each stable/1.95 workspace1561/MCP15;306 fixed-input requests with zero per-case quality delta;raw failures and first stopped attempt retained", "rollback_status": "schema15 requires isolated rebuild from14/earlier;entry source archive and P2D binaries retained;no daily index mutation", "limitations": "local P3-A subset only;not G3 or compiler/package completeness;146 build+30 pure-resolution release samples not RSS/tail/100k;S11 still fails and compares inconclusive;no publish"}]
实施备注：定义只读ProjectModel/FileCatalog/配置来源及模块证据；prepare一次捕获，parse和dirty-reload共用，不逐import读盘。旧Rust alias loader每次capture一次；package roots目前是发现描述，不代表完整包解析。

### [x] P3-002｜共享目录与配置发现

状态：`done`；批次：`P3-A`；优先级：`normal`。
范围：`crates/cc-index/src/project_model/mod.rs`；`crates/cc-index/src/scanner.rs`
硬依赖：P3-001
步骤：消费WalkManifest与scope事件；配置即使非源码也纳入更新
交付物：共享目录与配置发现的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V08, V20 验证证据（实施时生成）
验收：一次全建不重复全仓walk，scoped不能漏配置变化；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V08；V20
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "48750ff11621439baf933b11fa52ca6d94ae89e5abb2b8a551cc46cf65319b6e", "run_id": "p3a-20260928-KTGS/final-v2 + paired", "artifacts": ["artifacts/benchmarks/p3a-20260928-KTGS/final-v2/validation.json", "artifacts/benchmarks/p3a-20260928-KTGS/paired/summary.json", "artifacts/benchmarks/p3a-20260928-KTGS/cost-summary.json", "artifacts/benchmarks/p3a-20260928-KTGS/scope-check.json", "artifacts/benchmarks/p3a-20260928-KTGS/evidence-classification.json", "docs/roadmap/code-index-v2/P3-A-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-A-GATE.json", "docs/internals/PROJECT_MODEL.md"], "commands_receipt": "artifacts/benchmarks/p3a-20260928-KTGS/final-v2/validation.json", "review": "522-file content and membership freeze;25 final commands exit0;each stable/1.95 workspace1561/MCP15;306 fixed-input requests with zero per-case quality delta;raw failures and first stopped attempt retained", "rollback_status": "schema15 requires isolated rebuild from14/earlier;entry source archive and P2D binaries retained;no daily index mutation", "limitations": "local P3-A subset only;not G3 or compiler/package completeness;146 build+30 pure-resolution release samples not RSS/tail/100k;S11 still fails and compares inconclusive;no publish"}]
实施备注：共享WalkManifest与scoped已对账文件集合；配置即使ignored为源码也产生ModuleConfig失效。补隐藏继承配置事件红绿回归，loader/watcher共用准入与生成缓存排除规则。发现实际位于mod.rs；目录整理仍O(files)，不重复新建扫描引擎。

### [x] P3-003｜配置缓存与继承DAG

状态：`done`；批次：`P3-A`；优先级：`normal`。
范围：`crates/cc-index/src/project_model/config_cache.rs`
硬依赖：P3-002
步骤：内容digest键控；检测extends环/越界/超限并记录依赖
交付物：配置缓存与继承DAG的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V08 验证证据（实施时生成）
验收：mtime碰撞不会在strict模式复用错误配置，循环有界失败；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V08
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "48750ff11621439baf933b11fa52ca6d94ae89e5abb2b8a551cc46cf65319b6e", "run_id": "p3a-20260928-KTGS/final-v2 + paired", "artifacts": ["artifacts/benchmarks/p3a-20260928-KTGS/final-v2/validation.json", "artifacts/benchmarks/p3a-20260928-KTGS/paired/summary.json", "artifacts/benchmarks/p3a-20260928-KTGS/cost-summary.json", "artifacts/benchmarks/p3a-20260928-KTGS/scope-check.json", "artifacts/benchmarks/p3a-20260928-KTGS/evidence-classification.json", "docs/roadmap/code-index-v2/P3-A-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-A-GATE.json", "docs/internals/PROJECT_MODEL.md"], "commands_receipt": "artifacts/benchmarks/p3a-20260928-KTGS/final-v2/validation.json", "review": "522-file content and membership freeze;25 final commands exit0;each stable/1.95 workspace1561/MCP15;306 fixed-input requests with zero per-case quality delta;raw failures and first stopped attempt retained", "rollback_status": "schema15 requires isolated rebuild from14/earlier;entry source archive and P2D binaries retained;no daily index mutation", "limitations": "local P3-A subset only;not G3 or compiler/package completeness;146 build+30 pure-resolution release samples not RSS/tail/100k;S11 still fails and compares inconclusive;no publish"}]
实施备注：内容digest缓存不依赖mtime/size；相对extends/DAG、循环/深度32、单配置1MiB/总16MiB、1024输入与effective配置上限。摘要校验与提交前复核通过；输入确认在事实/frontier成功后，崩溃重复失效，不宣称全部状态同事务。

### [x] P3-004｜TS最近配置与JSONC

状态：`done`；批次：`P3-A`；优先级：`normal`。
范围：`crates/cc-index/src/project_model/typescript.rs`
硬依赖：P3-003
步骤：最近ts/jsconfig作用域；字符串安全JSONC与extends继承
交付物：TS最近配置与JSONC的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V08 验证证据（实施时生成）
验收：嵌套包不套用错误根别名，注释不破坏字符串；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V08
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "48750ff11621439baf933b11fa52ca6d94ae89e5abb2b8a551cc46cf65319b6e", "run_id": "p3a-20260928-KTGS/final-v2 + paired", "artifacts": ["artifacts/benchmarks/p3a-20260928-KTGS/final-v2/validation.json", "artifacts/benchmarks/p3a-20260928-KTGS/paired/summary.json", "artifacts/benchmarks/p3a-20260928-KTGS/cost-summary.json", "artifacts/benchmarks/p3a-20260928-KTGS/scope-check.json", "artifacts/benchmarks/p3a-20260928-KTGS/evidence-classification.json", "docs/roadmap/code-index-v2/P3-A-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-A-GATE.json", "docs/internals/PROJECT_MODEL.md"], "commands_receipt": "artifacts/benchmarks/p3a-20260928-KTGS/final-v2/validation.json", "review": "522-file content and membership freeze;25 final commands exit0;each stable/1.95 workspace1561/MCP15;306 fixed-input requests with zero per-case quality delta;raw failures and first stopped attempt retained", "rollback_status": "schema15 requires isolated rebuild from14/earlier;entry source archive and P2D binaries retained;no daily index mutation", "limitations": "local P3-A subset only;not G3 or compiler/package completeness;146 build+30 pure-resolution release samples not RSS/tail/100k;S11 still fails and compares inconclusive;no publish"}]
实施备注：最近目录ts/jsconfig、字符串安全JSONC、有序继承数组和定义配置来源。最近归属是CodeCortex策略，非完整tsc项目选择；嵌套无效配置不静默使用远端有效配置。16模型测试、10产品集成及真实MCP在最终全仓覆盖。

### [x] P3-005｜TS paths/baseUrl与条件

状态：`done`；批次：`P3-A`；优先级：`normal`。
范围：`crates/cc-index/src/module_resolution/typescript.rs`
硬依赖：P3-004
步骤：实现别名通配和定义配置基准目录；固定resolution mode/conditions
交付物：TS paths/baseUrl与条件的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V08 验证证据（实施时生成）
验收：多target按声明规则验证，未支持模式明确返回；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V08
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "48750ff11621439baf933b11fa52ca6d94ae89e5abb2b8a551cc46cf65319b6e", "run_id": "p3a-20260928-KTGS/final-v2 + paired", "artifacts": ["artifacts/benchmarks/p3a-20260928-KTGS/final-v2/validation.json", "artifacts/benchmarks/p3a-20260928-KTGS/paired/summary.json", "artifacts/benchmarks/p3a-20260928-KTGS/cost-summary.json", "artifacts/benchmarks/p3a-20260928-KTGS/scope-check.json", "artifacts/benchmarks/p3a-20260928-KTGS/evidence-classification.json", "docs/roadmap/code-index-v2/P3-A-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-A-GATE.json", "docs/internals/PROJECT_MODEL.md"], "commands_receipt": "artifacts/benchmarks/p3a-20260928-KTGS/final-v2/validation.json", "review": "522-file content and membership freeze;25 final commands exit0;each stable/1.95 workspace1561/MCP15;306 fixed-input requests with zero per-case quality delta;raw failures and first stopped attempt retained", "rollback_status": "schema15 requires isolated rebuild from14/earlier;entry source archive and P2D binaries retained;no daily index mutation", "limitations": "local P3-A subset only;not G3 or compiler/package completeness;146 build+30 pure-resolution release samples not RSS/tail/100k;S11 still fails and compares inconclusive;no publish"}]
实施备注：本地paths/baseUrl精确与最长前缀、多target顺序、扩展名替代及负向探测；同优先级重叠patterns显式Unsupported。node10/bundler/local_compat范围固定，条件记录不冒充package分支求值；Node16/NodeNext/package入口仍后续实现。

### [x] P3-006｜TS workspace与package入口

状态：`done`；批次：`P3-B`；优先级：`normal`。
范围：`crates/cc-model/src/module_inputs.rs`；`crates/cc-index/src/project_model/package.rs`；`crates/cc-index/src/module_resolution/package.rs`；`crates/cc-index/src/project_model/typescript.rs`；`crates/cc-index/src/module_resolution/typescript.rs`
硬依赖：P2-020, P3-004, P3-005
步骤：处理workspace package、imports/exports/subpaths；谨慎区分types/import/require
交付物：TS workspace与package入口的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V08 验证证据（实施时生成）
验收：不把固定优先级猜测标为所有模式正确；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V08
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "a0b93f187d6a287fc87dcd21af0e7494f2de7c4dfc84abda333172c636b8eace", "run_id": "p3b-20260928-oQGF/final-v3 + paired-final-v3", "artifacts": ["docs/roadmap/code-index-v2/P3-B-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-B-GATE.json", "docs/internals/MODULE_RESOLUTION.md", "artifacts/benchmarks/p3b-20260928-oQGF/final-v3/validation.json", "artifacts/benchmarks/p3b-20260928-oQGF/paired-final-v3/summary.json", "artifacts/benchmarks/p3b-20260928-oQGF/scope-check-v3.json", "artifacts/benchmarks/p3b-20260928-oQGF/cost-summary-v3.json", "artifacts/benchmarks/p3b-20260928-oQGF/evidence-classification.json"], "commands_receipt": "artifacts/benchmarks/p3b-20260928-oQGF/final-v3/validation.json", "review": "535-file content and membership fenced;26 commands exit0;each toolchain1589workspace/171HTTP/16realMCP;no negative per-case delta,R08 improved;old/new14-tool inputs and14-table oracles retained", "rollback_status": "schema16 and model payload2 require isolated rebuild from old caches; entry archive and prior/current exact binaries retained, no daily index touched", "limitations": "local static subset only, not G3/compiler/runtime/holdout/100k/RSS/tail/remote/live semantic/release;S11 still fails and performance comparisons inconclusive;initial R04 regression and formula migration failures retained"}]
实施备注：P3-B捕获的workspace/package映射保留JSON条件原顺序，区分static/require/dynamic/type-only并持久化context_json。近workspace约束、重复包名/越界/null/数组/重定向负例及实际MCP预算1重启1→2→3通过；未安装外部依赖，未执行用户脚本。

### [x] P3-007｜TS扩展名/目录/ESM映射

状态：`done`；批次：`P3-B`；优先级：`normal`。
范围：`crates/cc-index/src/module_resolution/typescript.rs`
硬依赖：P3-006
步骤：相对路径、index、js指向ts等按模式验证；构建产物映射单独heuristic
交付物：TS扩展名/目录/ESM映射的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V08 验证证据（实施时生成）
验收：源码/声明/产物选择符合冻结夹具，外部依赖不伪造内部路径；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V08
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "a0b93f187d6a287fc87dcd21af0e7494f2de7c4dfc84abda333172c636b8eace", "run_id": "p3b-20260928-oQGF/final-v3 + paired-final-v3", "artifacts": ["docs/roadmap/code-index-v2/P3-B-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-B-GATE.json", "docs/internals/MODULE_RESOLUTION.md", "artifacts/benchmarks/p3b-20260928-oQGF/final-v3/validation.json", "artifacts/benchmarks/p3b-20260928-oQGF/paired-final-v3/summary.json", "artifacts/benchmarks/p3b-20260928-oQGF/scope-check-v3.json", "artifacts/benchmarks/p3b-20260928-oQGF/cost-summary-v3.json", "artifacts/benchmarks/p3b-20260928-oQGF/evidence-classification.json"], "commands_receipt": "artifacts/benchmarks/p3b-20260928-oQGF/final-v3/validation.json", "review": "535-file content and membership fenced;26 commands exit0;each toolchain1589workspace/171HTTP/16realMCP;no negative per-case delta,R08 improved;old/new14-tool inputs and14-table oracles retained", "rollback_status": "schema16 and model payload2 require isolated rebuild from old caches; entry archive and prior/current exact binaries retained, no daily index touched", "limitations": "local static subset only, not G3/compiler/runtime/holdout/100k/RSS/tail/remote/live semantic/release;S11 still fails and performance comparisons inconclusive;initial R04 regression and formula migration failures retained"}]
实施备注：P3-B按Node16/NodeNext/bundler/node10声明子集处理格式和条件；JS到TS/声明扩展替换、目录package优先于index、ESM相对显式扩展名验证通过。任意dist到src/source-map不伪造，classic/完整interop等仍Unsupported。

### [x] P3-008｜Rust workspace旧能力迁移

状态：`done`；批次：`P3-B`；优先级：`normal`。
范围：`crates/cc-index/src/resolver/cargo_workspace.rs`；`crates/cc-index/src/project_model/rust.rs`；`crates/cc-index/src/project_model/rust_capture.rs`
硬依赖：P3-007
步骤：保留当前workspace别名用例；抽入ProjectModel而不另建平行解析器
交付物：Rust workspace旧能力迁移的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V08 验证证据（实施时生成）
验收：现有cargo_workspace tests全过，目录扫描不重复；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V08
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "a0b93f187d6a287fc87dcd21af0e7494f2de7c4dfc84abda333172c636b8eace", "run_id": "p3b-20260928-oQGF/final-v3 + paired-final-v3", "artifacts": ["docs/roadmap/code-index-v2/P3-B-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-B-GATE.json", "docs/internals/MODULE_RESOLUTION.md", "artifacts/benchmarks/p3b-20260928-oQGF/final-v3/validation.json", "artifacts/benchmarks/p3b-20260928-oQGF/paired-final-v3/summary.json", "artifacts/benchmarks/p3b-20260928-oQGF/scope-check-v3.json", "artifacts/benchmarks/p3b-20260928-oQGF/cost-summary-v3.json", "artifacts/benchmarks/p3b-20260928-oQGF/evidence-classification.json"], "commands_receipt": "artifacts/benchmarks/p3b-20260928-oQGF/final-v3/validation.json", "review": "535-file content and membership fenced;26 commands exit0;each toolchain1589workspace/171HTTP/16realMCP;no negative per-case delta,R08 improved;old/new14-tool inputs and14-table oracles retained", "rollback_status": "schema16 and model payload2 require isolated rebuild from old caches; entry archive and prior/current exact binaries retained, no daily index touched", "limitations": "local static subset only, not G3/compiler/runtime/holdout/100k/RSS/tail/remote/live semantic/release;S11 still fails and performance comparisons inconclusive;initial R04 regression and formula migration failures retained"}]
实施备注：原Cargo workspace用例经test-only兼容入口调用新ProjectModel；生产移除旧目录loader。捕获TOML解析成员/排除/库入口/声明path及workspace继承别名，原dependency rename负例升级为精确目标断言。旧无依赖workspace便利性只标heuristic，未冒充Cargo授权依赖。

### [x] P3-009｜Rust模块路径与cfg

状态：`done`；批次：`P3-B`；优先级：`normal`。
范围：`crates/cc-index/src/module_resolution/rust.rs`；`crates/cc-parsers/src/rust_modules.rs`
硬依赖：P3-008
步骤：crate/self/super、mod文件及路径属性；features/cfg显式进入条件
交付物：Rust模块路径与cfg的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V08 验证证据（实施时生成）
验收：跨模块目标可追踪，宏/复杂cfg局限有状态；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V08
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "a0b93f187d6a287fc87dcd21af0e7494f2de7c4dfc84abda333172c636b8eace", "run_id": "p3b-20260928-oQGF/final-v3 + paired-final-v3", "artifacts": ["docs/roadmap/code-index-v2/P3-B-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-B-GATE.json", "docs/internals/MODULE_RESOLUTION.md", "artifacts/benchmarks/p3b-20260928-oQGF/final-v3/validation.json", "artifacts/benchmarks/p3b-20260928-oQGF/paired-final-v3/summary.json", "artifacts/benchmarks/p3b-20260928-oQGF/scope-check-v3.json", "artifacts/benchmarks/p3b-20260928-oQGF/cost-summary-v3.json", "artifacts/benchmarks/p3b-20260928-oQGF/evidence-classification.json"], "commands_receipt": "artifacts/benchmarks/p3b-20260928-oQGF/final-v3/validation.json", "review": "535-file content and membership fenced;26 commands exit0;each toolchain1589workspace/171HTTP/16realMCP;no negative per-case delta,R08 improved;old/new14-tool inputs and14-table oracles retained", "rollback_status": "schema16 and model payload2 require isolated rebuild from old caches; entry archive and prior/current exact binaries retained, no daily index touched", "limitations": "local static subset only, not G3/compiler/runtime/holdout/100k/RSS/tail/remote/live semantic/release;S11 still fails and performance comparisons inconclusive;initial R04 regression and formula migration failures retained"}]
实施备注：声明模块facts包含inline作用域/path/cfg，crate/self/super跨模块可追踪。默认本地feature条件固定，平台/optional/dev/target/dependency feature override及cfg_attr/宏保守Unsupported。scanner内容hash复用Rustfacts；100/1000模块no-op源码读取0，文件/逻辑模块索引支撑200000纯查询；改动文件额外紧凑AST遍历局限公开。

### [x] P3-010｜Python包与src layout

状态：`done`；批次：`P3-B`；优先级：`normal`。
范围：`crates/cc-index/src/project_model/python.rs`；`crates/cc-index/src/module_resolution/python.rs`
硬依赖：P3-009
步骤：处理项目根/src roots、相对点、__init__和namespace；不执行源码
交付物：Python包与src layout的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V08 验证证据（实施时生成）
验收：相对import不走JS扩展名逻辑，项目外路径受限；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V08
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "a0b93f187d6a287fc87dcd21af0e7494f2de7c4dfc84abda333172c636b8eace", "run_id": "p3b-20260928-oQGF/final-v3 + paired-final-v3", "artifacts": ["docs/roadmap/code-index-v2/P3-B-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-B-GATE.json", "docs/internals/MODULE_RESOLUTION.md", "artifacts/benchmarks/p3b-20260928-oQGF/final-v3/validation.json", "artifacts/benchmarks/p3b-20260928-oQGF/paired-final-v3/summary.json", "artifacts/benchmarks/p3b-20260928-oQGF/scope-check-v3.json", "artifacts/benchmarks/p3b-20260928-oQGF/cost-summary-v3.json", "artifacts/benchmarks/p3b-20260928-oQGF/evidence-classification.json"], "commands_receipt": "artifacts/benchmarks/p3b-20260928-oQGF/final-v3/validation.json", "review": "535-file content and membership fenced;26 commands exit0;each toolchain1589workspace/171HTTP/16realMCP;no negative per-case delta,R08 improved;old/new14-tool inputs and14-table oracles retained", "rollback_status": "schema16 and model payload2 require isolated rebuild from old caches; entry archive and prior/current exact binaries retained, no daily index touched", "limitations": "local static subset only, not G3/compiler/runtime/holdout/100k/RSS/tail/remote/live semantic/release;S11 still fails and performance comparisons inconclusive;initial R04 regression and formula migration failures retained"}]
实施备注：Python root/src、setuptools/Poetry静态根、相对dot边界、__init__及namespace按真实路径解析，不执行源码。namespace先积累，后续普通包优先且限制其子查找；不生成目录假源码。不再用JS探测Python。配置-only切换和14表差分通过；缺失导入阻断全局同名猜测，真实转发路径保持旧多语言链环回归。

### [x] P3-011｜Go module/workspace

状态：`done`；批次：`P3-C`；优先级：`normal`。
范围：`crates/cc-model/src/go_project.rs`；`crates/cc-index/src/project_model/go.rs`；`crates/cc-index/src/project_model/go_capture.rs`；`crates/cc-parsers/src/go_modules.rs`；`crates/cc-index/src/module_resolution/go.rs`
硬依赖：P2-020, P3-001, P3-002
步骤：go.mod/go.work/replace、包目录、多文件集与build条件
交付物：Go module/workspace的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V08 验证证据（实施时生成）
验收：本地replace可解析，远端未索引依赖明确external；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V08
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "2ba69cc87450d109fa33945a402a5f03a4fc783f8aa6733c710e11f06eefb961", "run_id": "p3c-20260928-4eCI/final + paired-final", "artifacts": ["artifacts/benchmarks/p3c-20260928-4eCI/final/validation.json", "artifacts/benchmarks/p3c-20260928-4eCI/paired-final/summary.json", "artifacts/benchmarks/p3c-20260928-4eCI/scope-check.json", "artifacts/benchmarks/p3c-20260928-4eCI/cost-summary.json", "artifacts/benchmarks/p3c-20260928-4eCI/final-oracles-stable/result.json", "artifacts/benchmarks/p3c-20260928-4eCI/final-oracles-1.95.0/result.json", "artifacts/benchmarks/p3c-20260928-4eCI/evidence-classification.json", "docs/roadmap/code-index-v2/P3-C-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-C-GATE.json", "docs/internals/GO_MODULES.md"], "commands_receipt": "artifacts/benchmarks/p3c-20260928-4eCI/final/validation.json", "review": "548-file content/membership fence;27 Cargo commands;each stable/1.95 workspace1611 and realMCP17/watcher17;306 paired requests no negative per-case quality delta;log/binary/fixture hashes checked", "rollback_status": "schema17/model3 require isolated rebuild from16/earlier;entry archive and P3B binaries preserved;no daily index changes", "limitations": "local portable Go/package and typed-result subset;not G3/MVS/full compiler/runtime/100k/RSS/tail/holdout;four Rust/Python cases available under both runs,Go/TypeScript compiler comparisons not_run;S11 fails;compares inconclusive;no publication"}]
实施备注：捕获Go manifest及紧凑语法，workspace/local replace、声明包名和多文件包集合已接实际索引；条件未选/版本选择/编译器缺口保持Unknown/Unsupported，不执行被索引代码。

### [x] P3-012｜统一模块解析返回值

状态：`done`；批次：`P3-C`；优先级：`normal`。
范围：`crates/cc-model/src/project_model.rs`；`crates/cc-model/src/resolution.rs`；`crates/cc-index/src/module_resolution/`；`crates/cc-index/src/indexer_phases/resolve.rs`；`crates/cc-index/src/resolver/`
硬依赖：P3-011
步骤：返回内部文件/包/外部/歧义/unknown；每个结果带规则与依赖
交付物：统一模块解析返回值的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V08 验证证据（实施时生成）
验收：不能用None混合missing和external再静默回退假目标；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V08
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "2ba69cc87450d109fa33945a402a5f03a4fc783f8aa6733c710e11f06eefb961", "run_id": "p3c-20260928-4eCI/final + paired-final", "artifacts": ["artifacts/benchmarks/p3c-20260928-4eCI/final/validation.json", "artifacts/benchmarks/p3c-20260928-4eCI/paired-final/summary.json", "artifacts/benchmarks/p3c-20260928-4eCI/scope-check.json", "artifacts/benchmarks/p3c-20260928-4eCI/cost-summary.json", "artifacts/benchmarks/p3c-20260928-4eCI/final-oracles-stable/result.json", "artifacts/benchmarks/p3c-20260928-4eCI/final-oracles-1.95.0/result.json", "artifacts/benchmarks/p3c-20260928-4eCI/evidence-classification.json", "docs/roadmap/code-index-v2/P3-C-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-C-GATE.json", "docs/internals/GO_MODULES.md"], "commands_receipt": "artifacts/benchmarks/p3c-20260928-4eCI/final/validation.json", "review": "548-file content/membership fence;27 Cargo commands;each stable/1.95 workspace1611 and realMCP17/watcher17;306 paired requests no negative per-case quality delta;log/binary/fixture hashes checked", "rollback_status": "schema17/model3 require isolated rebuild from16/earlier;entry archive and P3B binaries preserved;no daily index changes", "limitations": "local portable Go/package and typed-result subset;not G3/MVS/full compiler/runtime/100k/RSS/tail/holdout;four Rust/Python cases available under both runs,Go/TypeScript compiler comparisons not_run;S11 fails;compares inconclusive;no publication"}]
实施备注：统一文件/包集合/External/Ambiguous/Unknown状态；NULL物理路径不再兼任唯一状态。Go调用与对应引用保留真实包接收者，不误绑同文件同名函数；TS本地缺失和外部、Rust声明registry/git外部、Python环境未知分别表达。

### [x] P3-013｜配置-only变更进入dirty

状态：`done`；批次：`P3-C`；优先级：`normal`。
范围：`crates/cc-index/src/project_model/mod.rs`；`crates/cc-index/src/indexer_phases/dependencies.rs`；`crates/cc-index/src/indexer_phases/dirty.rs`；`crates/cc-eval/tests/p3c_modules.rs`；`crates/cc-eval/tests/p3c_cost.rs`
硬依赖：P3-012, P3-003
步骤：配置、别名、workspace入口变更触发相关解析；无需改源码mtime
交付物：配置-only变更进入dirty的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V07, V08 验证证据（实施时生成）
验收：仅改tsconfig/go.work/Cargo依赖即可收敛到full结果；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V07；V08
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "2ba69cc87450d109fa33945a402a5f03a4fc783f8aa6733c710e11f06eefb961", "run_id": "p3c-20260928-4eCI/final + paired-final", "artifacts": ["artifacts/benchmarks/p3c-20260928-4eCI/final/validation.json", "artifacts/benchmarks/p3c-20260928-4eCI/paired-final/summary.json", "artifacts/benchmarks/p3c-20260928-4eCI/scope-check.json", "artifacts/benchmarks/p3c-20260928-4eCI/cost-summary.json", "artifacts/benchmarks/p3c-20260928-4eCI/final-oracles-stable/result.json", "artifacts/benchmarks/p3c-20260928-4eCI/final-oracles-1.95.0/result.json", "artifacts/benchmarks/p3c-20260928-4eCI/evidence-classification.json", "docs/roadmap/code-index-v2/P3-C-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-C-GATE.json", "docs/internals/GO_MODULES.md"], "commands_receipt": "artifacts/benchmarks/p3c-20260928-4eCI/final/validation.json", "review": "548-file content/membership fence;27 Cargo commands;each stable/1.95 workspace1611 and realMCP17/watcher17;306 paired requests no negative per-case quality delta;log/binary/fixture hashes checked", "rollback_status": "schema17/model3 require isolated rebuild from16/earlier;entry archive and P3B binaries preserved;no daily index changes", "limitations": "local portable Go/package and typed-result subset;not G3/MVS/full compiler/runtime/100k/RSS/tail/holdout;four Rust/Python cases available under both runs,Go/TypeScript compiler comparisons not_run;S11 fails;compares inconclusive;no publication"}]
实施备注：复用已有dependencies/dirty/frontier，未创建计划中的重复incremental引擎。Go配置-only重定向/重启/全量十四表通过，已知本地无条件结果精化配置依赖；38条机制样本中25消费者预算5恰好五批，未知情况保留保守全局失效。

### [x] P3-014｜watcher重命名与溢出对账

状态：`done`；批次：`P3-C`；优先级：`normal`。
范围：`crates/cc-server/src/watcher.rs`；`crates/cc-server/src/project_session.rs`；`crates/cc-index/src/scanner.rs`
硬依赖：P3-013
步骤：配置重命名/删除/目录事件保守识别；overflow回全树
交付物：watcher重命名与溢出对账的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V07, V08 验证证据（实施时生成）
验收：不以scope signature提示误跳过项目模型重建；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V07；V08
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "2ba69cc87450d109fa33945a402a5f03a4fc783f8aa6733c710e11f06eefb961", "run_id": "p3c-20260928-4eCI/final + paired-final", "artifacts": ["artifacts/benchmarks/p3c-20260928-4eCI/final/validation.json", "artifacts/benchmarks/p3c-20260928-4eCI/paired-final/summary.json", "artifacts/benchmarks/p3c-20260928-4eCI/scope-check.json", "artifacts/benchmarks/p3c-20260928-4eCI/cost-summary.json", "artifacts/benchmarks/p3c-20260928-4eCI/final-oracles-stable/result.json", "artifacts/benchmarks/p3c-20260928-4eCI/final-oracles-1.95.0/result.json", "artifacts/benchmarks/p3c-20260928-4eCI/evidence-classification.json", "docs/roadmap/code-index-v2/P3-C-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-C-GATE.json", "docs/internals/GO_MODULES.md"], "commands_receipt": "artifacts/benchmarks/p3c-20260928-4eCI/final/validation.json", "review": "548-file content/membership fence;27 Cargo commands;each stable/1.95 workspace1611 and realMCP17/watcher17;306 paired requests no negative per-case quality delta;log/binary/fixture hashes checked", "rollback_status": "schema17/model3 require isolated rebuild from16/earlier;entry archive and P3B binaries preserved;no daily index changes", "limitations": "local portable Go/package and typed-result subset;not G3/MVS/full compiler/runtime/100k/RSS/tail/holdout;four Rust/Python cases available under both runs,Go/TypeScript compiler comparisons not_run;S11 fails;compares inconclusive;no publication"}]
实施备注：修复watcher记录rescan却未交给consumer的问题；重命名/目录/溢出/错误保守全树，发布失败重排rescan。17项watcher测试通过，包含实际consumer无路径对账和失败重试，不宣称所有OS原生overflow认证。

### [x] P3-015｜模块解析夹具与外部oracle

状态：`done`；批次：`P3-C`；优先级：`normal`。
范围：`crates/cc-index/tests/p3c_modules.rs`；`crates/cc-eval/tests/p3c_modules.rs`；`crates/cc-eval/benchmarks/modules/p3c-fixtures.json`；`scripts/module_oracles.py`
硬依赖：P3-014
步骤：手写gold覆盖每种规则；可选固定编译器结果作独立参照
交付物：模块解析夹具与外部oracle的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V08 验证证据（实施时生成）
验收：oracle版本可追溯，没安装编译器不宣称核验通过；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V08
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "2ba69cc87450d109fa33945a402a5f03a4fc783f8aa6733c710e11f06eefb961", "run_id": "p3c-20260928-4eCI/final + paired-final", "artifacts": ["artifacts/benchmarks/p3c-20260928-4eCI/final/validation.json", "artifacts/benchmarks/p3c-20260928-4eCI/paired-final/summary.json", "artifacts/benchmarks/p3c-20260928-4eCI/scope-check.json", "artifacts/benchmarks/p3c-20260928-4eCI/cost-summary.json", "artifacts/benchmarks/p3c-20260928-4eCI/final-oracles-stable/result.json", "artifacts/benchmarks/p3c-20260928-4eCI/final-oracles-1.95.0/result.json", "artifacts/benchmarks/p3c-20260928-4eCI/evidence-classification.json", "docs/roadmap/code-index-v2/P3-C-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-C-GATE.json", "docs/internals/GO_MODULES.md"], "commands_receipt": "artifacts/benchmarks/p3c-20260928-4eCI/final/validation.json", "review": "548-file content/membership fence;27 Cargo commands;each stable/1.95 workspace1611 and realMCP17/watcher17;306 paired requests no negative per-case quality delta;log/binary/fixture hashes checked", "rollback_status": "schema17/model3 require isolated rebuild from16/earlier;entry archive and P3B binaries preserved;no daily index changes", "limitations": "local portable Go/package and typed-result subset;not G3/MVS/full compiler/runtime/100k/RSS/tail/holdout;four Rust/Python cases available under both runs,Go/TypeScript compiler comparisons not_run;S11 fails;compares inconclusive;no publication"}]
实施备注：本批具名模块测试和六组固定fixture替代尚未存在的matrix/目录设计路径。相同四个Rust/Python案例在两工具链运行中独立参照通过；两个Go外部案例因环境缺少编译器not_run，TypeScript编译器未调用。原命令、版本、输出及fixture/harness哈希留存，不把未运行计为通过。

### [x] P3-016｜配置深度与文件访问防护

状态：`done`；批次：`P3-D`；优先级：`normal`。
范围：`crates/cc-index/src/project_model/`；`crates/cc-model/src/input_file.rs`；`crates/cc-model/src/config.rs`；`crates/cc-server/src/path_guard.rs`；`crates/cc-index/tests/p3d_boundaries.rs`
硬依赖：P2-020, P3-015
步骤：约束深度/字节/路径/符号链接；配置无法触发任意执行或外部抓取
交付物：配置深度与文件访问防护的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V08 验证证据（实施时生成）
验收：恶意extends/超大manifest/循环均有界失败；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V08
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "e7da57859fd5849f8daf8145a37d72d306798fffc374f20ebd2865ddd6815461", "run_id": "p3d-20260928-oO9a/final-v3 + paired-final-v3 + resume-HjT7", "artifacts": ["artifacts/benchmarks/p3d-20260928-oO9a/final-v3/validation.json", "artifacts/benchmarks/p3d-20260928-oO9a/paired-final-v3/summary.json", "artifacts/benchmarks/p3d-20260928-oO9a/scope-check.json", "artifacts/benchmarks/p3d-20260928-oO9a/cost-summary.json", "artifacts/benchmarks/p3d-20260928-oO9a/evidence-classification.json", "artifacts/benchmarks/p3d-20260928-oO9a/checks/resume-HjT7-boundaries.json", "artifacts/benchmarks/p3d-20260928-oO9a/checks/resume-HjT7-mixed.json", "docs/roadmap/code-index-v2/P3-D-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-D-GATE.json", "docs/internals/MODULE_CAPABILITIES.json", "docs/internals/MODULE_INPUT_SAFETY.md"], "commands_receipt": "artifacts/benchmarks/p3d-20260928-oO9a/final-v3/validation.json", "review": "555-file content and membership plus 28 completed command/log/binary receipts verified; current-session pairing306requests and boundary/mixed-MCP checks passed; no negative per-case deltas", "rollback_status": "schema18 isolated rebuild from17/earlier; prior source/binaries retained; daily index untouched", "limitations": "G3 declared local static subset; four Rust/Python external cases, Go/TS compiler not_run; S11 fails; comparisons inconclusive; cost50 samples/40000 lookups not RSS/tail/100k or release"}]
实施备注：共享cc-model/input_file普通文件有界读取；FIFO应用/模块配置阻塞与符号链接防护有失败后通过证据。Unix描述符相对读取不等于全文件系统沙箱，其他平台不承诺同等竞态保证。

### [x] P3-017｜import规模与配置缓存成本

状态：`done`；批次：`P3-D`；优先级：`normal`。
范围：`crates/cc-eval/tests/p3d_cost.rs`；`crates/cc-eval/tests/common/p3d_fixture.rs`；`crates/cc-index/src/module_resolution/`
硬依赖：P3-016
步骤：分别统计detect、resolve、cache reuse和fs reads；扩到多包大仓
交付物：import规模与配置缓存成本的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V20 验证证据（实施时生成）
验收：每import无重复stat/配置parse，缓存更新不牺牲正确性；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V20
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "e7da57859fd5849f8daf8145a37d72d306798fffc374f20ebd2865ddd6815461", "run_id": "p3d-20260928-oO9a/final-v3 + paired-final-v3 + resume-HjT7", "artifacts": ["artifacts/benchmarks/p3d-20260928-oO9a/final-v3/validation.json", "artifacts/benchmarks/p3d-20260928-oO9a/paired-final-v3/summary.json", "artifacts/benchmarks/p3d-20260928-oO9a/scope-check.json", "artifacts/benchmarks/p3d-20260928-oO9a/cost-summary.json", "artifacts/benchmarks/p3d-20260928-oO9a/evidence-classification.json", "artifacts/benchmarks/p3d-20260928-oO9a/checks/resume-HjT7-boundaries.json", "artifacts/benchmarks/p3d-20260928-oO9a/checks/resume-HjT7-mixed.json", "docs/roadmap/code-index-v2/P3-D-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-D-GATE.json", "docs/internals/MODULE_CAPABILITIES.json", "docs/internals/MODULE_INPUT_SAFETY.md"], "commands_receipt": "artifacts/benchmarks/p3d-20260928-oO9a/final-v3/validation.json", "review": "555-file content and membership plus 28 completed command/log/binary receipts verified; current-session pairing306requests and boundary/mixed-MCP checks passed; no negative per-case deltas", "rollback_status": "schema18 isolated rebuild from17/earlier; prior source/binaries retained; daily index untouched", "limitations": "G3 declared local static subset; four Rust/Python external cases, Go/TS compiler not_run; S11 fails; comparisons inconclusive; cost50 samples/40000 lookups not RSS/tail/100k or release"}]
实施备注：实际成本入口p3d_cost/common p3d_fixture，复用现有评测底座；40/400语言项目、50条release样本、40000次删树后纯查找。扫描/捕获/无变化构建/缓存分列，非100k/RSS/尾延迟或全部IO认证。

### [x] P3-018｜清理旧路径解析重复逻辑

状态：`done`；批次：`P3-D`；优先级：`normal`。
范围：`crates/cc-parsers/src/import_resolver.rs`；`crates/cc-index/src/module_resolution/mod.rs`；`crates/cc-index/src/resolver/evidence.rs`；`scripts/check_module_architecture.py`
硬依赖：P3-017
步骤：将调用点迁至统一模块层；保留语法提取的纯职责
交付物：清理旧路径解析重复逻辑的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V08, V21 验证证据（实施时生成）
验收：旧fallback仅临时适配且有移除任务，不留双真相；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V08；V21
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "e7da57859fd5849f8daf8145a37d72d306798fffc374f20ebd2865ddd6815461", "run_id": "p3d-20260928-oO9a/final-v3 + paired-final-v3 + resume-HjT7", "artifacts": ["artifacts/benchmarks/p3d-20260928-oO9a/final-v3/validation.json", "artifacts/benchmarks/p3d-20260928-oO9a/paired-final-v3/summary.json", "artifacts/benchmarks/p3d-20260928-oO9a/scope-check.json", "artifacts/benchmarks/p3d-20260928-oO9a/cost-summary.json", "artifacts/benchmarks/p3d-20260928-oO9a/evidence-classification.json", "artifacts/benchmarks/p3d-20260928-oO9a/checks/resume-HjT7-boundaries.json", "artifacts/benchmarks/p3d-20260928-oO9a/checks/resume-HjT7-mixed.json", "docs/roadmap/code-index-v2/P3-D-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-D-GATE.json", "docs/internals/MODULE_CAPABILITIES.json", "docs/internals/MODULE_INPUT_SAFETY.md"], "commands_receipt": "artifacts/benchmarks/p3d-20260928-oO9a/final-v3/validation.json", "review": "555-file content and membership plus 28 completed command/log/binary receipts verified; current-session pairing306requests and boundary/mixed-MCP checks passed; no negative per-case deltas", "rollback_status": "schema18 isolated rebuild from17/earlier; prior source/binaries retained; daily index untouched", "limitations": "G3 declared local static subset; four Rust/Python external cases, Go/TS compiler not_run; S11 fails; comparisons inconclusive; cost50 samples/40000 lookups not RSS/tail/100k or release"}]
实施备注：旧cc-parsers/import_resolver.rs已删除，其scope路径为删除落点；生产模块解析唯一入口cc-index/module_resolution，SymbolCatalog不再重算通用后缀。Java误命中TS负例、Vue/Svelte导入与有限架构检查通过。

### [x] P3-019｜更新语言矩阵和配置契约

状态：`done`；批次：`P3-D`；优先级：`normal`。
范围：`docs/LANGUAGES.md`；`docs/CONFIGURATION.md`；`docs/internals/INDEXING.md`
硬依赖：P3-018
步骤：记录按语言/模式的准确与heuristic范围；用户能诊断无法解析原因
交付物：更新语言矩阵和配置契约的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V18, V21 验证证据（实施时生成）
验收：文档不把config_linker误称模块resolver；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V18；V21
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "e7da57859fd5849f8daf8145a37d72d306798fffc374f20ebd2865ddd6815461", "run_id": "p3d-20260928-oO9a/final-v3 + paired-final-v3 + resume-HjT7", "artifacts": ["artifacts/benchmarks/p3d-20260928-oO9a/final-v3/validation.json", "artifacts/benchmarks/p3d-20260928-oO9a/paired-final-v3/summary.json", "artifacts/benchmarks/p3d-20260928-oO9a/scope-check.json", "artifacts/benchmarks/p3d-20260928-oO9a/cost-summary.json", "artifacts/benchmarks/p3d-20260928-oO9a/evidence-classification.json", "artifacts/benchmarks/p3d-20260928-oO9a/checks/resume-HjT7-boundaries.json", "artifacts/benchmarks/p3d-20260928-oO9a/checks/resume-HjT7-mixed.json", "docs/roadmap/code-index-v2/P3-D-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-D-GATE.json", "docs/internals/MODULE_CAPABILITIES.json", "docs/internals/MODULE_INPUT_SAFETY.md"], "commands_receipt": "artifacts/benchmarks/p3d-20260928-oO9a/final-v3/validation.json", "review": "555-file content and membership plus 28 completed command/log/binary receipts verified; current-session pairing306requests and boundary/mixed-MCP checks passed; no negative per-case deltas", "rollback_status": "schema18 isolated rebuild from17/earlier; prior source/binaries retained; daily index untouched", "limitations": "G3 declared local static subset; four Rust/Python external cases, Go/TS compiler not_run; S11 fails; comparisons inconclusive; cost50 samples/40000 lookups not RSS/tail/100k or release"}]
实施备注：MODULE_CAPABILITIES、输入防护、语言/配置/索引文档与schema18/model3一致；14工具输入合同保持。计划检查必须从当前权威JSON生成，不能使用落后的70/122旧记录。

### [x] P3-020｜P3模块解析验收

状态：`done`；批次：`P3-D`；优先级：`blocking`。
范围：`docs/roadmap/code-index-v2/`；`artifacts/benchmarks/`
硬依赖：P3-001, P3-002, P3-003, P3-004, P3-005, P3-006, P3-007, P3-008, P3-009, P3-010, P3-011, P3-012, P3-013, P3-014, P3-015, P3-016, P3-017, P3-018, P3-019
步骤：执行G3、跨语言检索、config-only oracle和成本回归
交付物：P3模块解析验收的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V07, V08, V19, V20 验证证据（实施时生成）
验收：规则覆盖、精度局限和性能证据完整，源/配置输入锁固定；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V07；V08；V19；V20
回滚：恢复旧受支持解析入口并重建项目模型/索引；保留新夹具作为失败证据。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "e7da57859fd5849f8daf8145a37d72d306798fffc374f20ebd2865ddd6815461", "run_id": "p3d-20260928-oO9a/final-v3 + paired-final-v3 + resume-HjT7", "artifacts": ["artifacts/benchmarks/p3d-20260928-oO9a/final-v3/validation.json", "artifacts/benchmarks/p3d-20260928-oO9a/paired-final-v3/summary.json", "artifacts/benchmarks/p3d-20260928-oO9a/scope-check.json", "artifacts/benchmarks/p3d-20260928-oO9a/cost-summary.json", "artifacts/benchmarks/p3d-20260928-oO9a/evidence-classification.json", "artifacts/benchmarks/p3d-20260928-oO9a/checks/resume-HjT7-boundaries.json", "artifacts/benchmarks/p3d-20260928-oO9a/checks/resume-HjT7-mixed.json", "docs/roadmap/code-index-v2/P3-D-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P3-D-GATE.json", "docs/internals/MODULE_CAPABILITIES.json", "docs/internals/MODULE_INPUT_SAFETY.md"], "commands_receipt": "artifacts/benchmarks/p3d-20260928-oO9a/final-v3/validation.json", "review": "555-file content and membership plus 28 completed command/log/binary receipts verified; current-session pairing306requests and boundary/mixed-MCP checks passed; no negative per-case deltas", "rollback_status": "schema18 isolated rebuild from17/earlier; prior source/binaries retained; daily index untouched", "limitations": "G3 declared local static subset; four Rust/Python external cases, Go/TS compiler not_run; S11 fails; comparisons inconclusive; cost50 samples/40000 lookups not RSS/tail/100k or release"}]
实施备注：G3本地声明静态范围通过；续接核验final-v3的555文件及28条已完成命令，补完306请求配对、11边界测试及2混合/MCP测试。P3-A/B/C在同源final-v3重跑；外部缺项、完整编译器、跨平台和发行未认证。

## P4｜源码切块与文档版本

### [x] P4-001｜定义SourceSnapshot与byte span

状态：`done`；批次：`P4-A`；优先级：`normal`。
范围：`crates/cc-model/src/source.rs`；`crates/cc-model/src/chunk.rs`；`crates/cc-parsers/src/chunker.rs`；`crates/cc-index/src/indexer.rs`；`crates/cc-db/src/index_db_write_batch.rs`；`crates/cc-db/src/index_db_multi_insert.rs`；`crates/cc-db/src/index_db_retrieval.rs`；`crates/cc-search/src/plan.rs`；`crates/cc-eval/src/benchmark/normalizer.rs`
硬依赖：P3-020
步骤：原始bytes digest、编码、半开span和line转换；区分展示文本
交付物：定义SourceSnapshot与byte span的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V09 验证证据（实施时生成）
验收：CRLF/UTF8/末尾换行的正文与位置有独立gold；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V09
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "12d564788bcb59f6e841504d5720172ae3d0be7b2e2aef0ec16d94e00e2999bd", "run_id": "artifacts/benchmarks/p4a-20260928-VyxH/final-v2 + paired-final-v2", "artifacts": ["artifacts/benchmarks/p4a-20260928-VyxH/final-v2/validation.json", "artifacts/benchmarks/p4a-20260928-VyxH/paired-final-v2/summary.json", "artifacts/benchmarks/p4a-20260928-VyxH/scope-check.json", "artifacts/benchmarks/p4a-20260928-VyxH/cost-summary.json", "artifacts/benchmarks/p4a-20260928-VyxH/evidence-classification.json", "artifacts/benchmarks/p4a-20260928-VyxH/failed-final-source-proof.json", "docs/roadmap/code-index-v2/P4-A-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-A-GATE.json", "docs/internals/SOURCE_CHUNKS.md"], "commands_receipt": "artifacts/benchmarks/p4a-20260928-VyxH/final-v2/validation.json", "review": "565文件冻结、29条Cargo命令，两工具链各workspace1647passed/46ignored、HTTP180passed/39ignored、显式真实MCP19passed、watcher17passed；固定51题306请求无逐题负差分且源码证据全部有效。字节/签名/父域负例及public-v3严格证明；原失败保留，未修改gold。", "rollback_status": "schema19隔离重建；旧源码/二进制保留；日常索引未动", "limitations": "P4-A本地声明范围，G4、DocKey/Version、完整freshness-aware hydrate、项目chunk_line_budget接线、通用小块合并、公开holdout、100k/RSS/尾延迟、跨平台及发行认证未完成；S11仍失败，性能compare inconclusive；外部Go/TS编译器未运行。"}]
实施备注：SourceSnapshot为原始bytes/encoding/content身份；file rows保留mtime观测。scalar/batch/plain/zstd的source_json和最小公开字节证明投影贯通；为消除实际证据硬失败提前接入hydrate坐标，未宣称DocKey/Version或live磁盘freshness。

### [x] P4-002｜提取紧凑AST边界

状态：`done`；批次：`P4-A`；优先级：`normal`。
范围：`crates/cc-model/src/parse.rs`；`crates/cc-parsers/src/chunker/boundaries.rs`；`crates/cc-parsers/src/parse_common.rs`
硬依赖：P4-001
步骤：在已有解析任务输出symbol/block/statement spans；结束释放tree
交付物：提取紧凑AST边界的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V09, V20 验证证据（实施时生成）
验收：不为chunking进行第二轮全仓parse且输出无悬空节点；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V09；V20
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "12d564788bcb59f6e841504d5720172ae3d0be7b2e2aef0ec16d94e00e2999bd", "run_id": "artifacts/benchmarks/p4a-20260928-VyxH/final-v2 + paired-final-v2", "artifacts": ["artifacts/benchmarks/p4a-20260928-VyxH/final-v2/validation.json", "artifacts/benchmarks/p4a-20260928-VyxH/paired-final-v2/summary.json", "artifacts/benchmarks/p4a-20260928-VyxH/scope-check.json", "artifacts/benchmarks/p4a-20260928-VyxH/cost-summary.json", "artifacts/benchmarks/p4a-20260928-VyxH/evidence-classification.json", "artifacts/benchmarks/p4a-20260928-VyxH/failed-final-source-proof.json", "docs/roadmap/code-index-v2/P4-A-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-A-GATE.json", "docs/internals/SOURCE_CHUNKS.md"], "commands_receipt": "artifacts/benchmarks/p4a-20260928-VyxH/final-v2/validation.json", "review": "565文件冻结、29条Cargo命令，两工具链各workspace1647passed/46ignored、HTTP180passed/39ignored、显式真实MCP19passed、watcher17passed；固定51题306请求无逐题负差分且源码证据全部有效。字节/签名/父域负例及public-v3严格证明；原失败保留，未修改gold。", "rollback_status": "schema19隔离重建；旧源码/二进制保留；日常索引未动", "limitations": "P4-A本地声明范围，G4、DocKey/Version、完整freshness-aware hydrate、项目chunk_line_budget接线、通用小块合并、公开holdout、100k/RSS/尾延迟、跨平台及发行认证未完成；S11仍失败，性能compare inconclusive；外部Go/TS编译器未运行。"}]
实施备注：六个tree-backed生产解析器在已有parse任务输出紧凑owned边界，无Tree/Node逃逸；单语言parse计数不冒充Rust/Go整个索引只做一次AST。其他解析器明确fallback，SFC单次字节对齐projection并返回组件原文。

### [x] P4-003｜方法/类的层级切块

状态：`done`；批次：`P4-A`；优先级：`normal`。
范围：`crates/cc-parsers/src/chunker/split.rs`；`crates/cc-parsers/src/chunker/merge.rs`
硬依赖：P4-002
步骤：小符号整体、大类递归成员；保存父符号breadcrumb
交付物：方法/类的层级切块的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V09 验证证据（实施时生成）
验收：方法不会因只取top-level而全部退化固定行窗；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V09
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "12d564788bcb59f6e841504d5720172ae3d0be7b2e2aef0ec16d94e00e2999bd", "run_id": "artifacts/benchmarks/p4a-20260928-VyxH/final-v2 + paired-final-v2", "artifacts": ["artifacts/benchmarks/p4a-20260928-VyxH/final-v2/validation.json", "artifacts/benchmarks/p4a-20260928-VyxH/paired-final-v2/summary.json", "artifacts/benchmarks/p4a-20260928-VyxH/scope-check.json", "artifacts/benchmarks/p4a-20260928-VyxH/cost-summary.json", "artifacts/benchmarks/p4a-20260928-VyxH/evidence-classification.json", "artifacts/benchmarks/p4a-20260928-VyxH/failed-final-source-proof.json", "docs/roadmap/code-index-v2/P4-A-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-A-GATE.json", "docs/internals/SOURCE_CHUNKS.md"], "commands_receipt": "artifacts/benchmarks/p4a-20260928-VyxH/final-v2/validation.json", "review": "565文件冻结、29条Cargo命令，两工具链各workspace1647passed/46ignored、HTTP180passed/39ignored、显式真实MCP19passed、watcher17passed；固定51题306请求无逐题负差分且源码证据全部有效。字节/签名/父域负例及public-v3严格证明；原失败保留，未修改gold。", "rollback_status": "schema19隔离重建；旧源码/二进制保留；日常索引未动", "limitations": "P4-A本地声明范围，G4、DocKey/Version、完整freshness-aware hydrate、项目chunk_line_budget接线、通用小块合并、公开holdout、100k/RSS/尾延迟、跨平台及发行认证未完成；S11仍失败，性能compare inconclusive；外部Go/TS编译器未运行。"}]
实施备注：小叶符号整体，容器成员按独立检索身份切块；breadcrumb保留父域及有限真实成员标签、不复制父body。修复旧log/withdraw/Logger语料退化，未更改gold。

### [x] P4-004｜长函数语句块分割

状态：`done`；批次：`P4-A`；优先级：`normal`。
范围：`crates/cc-parsers/src/chunker/split.rs`
硬依赖：P4-003
步骤：按statement/block边界拆；必要fallback保持来源范围
交付物：长函数语句块分割的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V09 验证证据（实施时生成）
验收：块长度有界，签名/控制结构上下文可定位；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V09
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "12d564788bcb59f6e841504d5720172ae3d0be7b2e2aef0ec16d94e00e2999bd", "run_id": "artifacts/benchmarks/p4a-20260928-VyxH/final-v2 + paired-final-v2", "artifacts": ["artifacts/benchmarks/p4a-20260928-VyxH/final-v2/validation.json", "artifacts/benchmarks/p4a-20260928-VyxH/paired-final-v2/summary.json", "artifacts/benchmarks/p4a-20260928-VyxH/scope-check.json", "artifacts/benchmarks/p4a-20260928-VyxH/cost-summary.json", "artifacts/benchmarks/p4a-20260928-VyxH/evidence-classification.json", "artifacts/benchmarks/p4a-20260928-VyxH/failed-final-source-proof.json", "docs/roadmap/code-index-v2/P4-A-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-A-GATE.json", "docs/internals/SOURCE_CHUNKS.md"], "commands_receipt": "artifacts/benchmarks/p4a-20260928-VyxH/final-v2/validation.json", "review": "565文件冻结、29条Cargo命令，两工具链各workspace1647passed/46ignored、HTTP180passed/39ignored、显式真实MCP19passed、watcher17passed；固定51题306请求无逐题负差分且源码证据全部有效。字节/签名/父域负例及public-v3严格证明；原失败保留，未修改gold。", "rollback_status": "schema19隔离重建；旧源码/二进制保留；日常索引未动", "limitations": "P4-A本地声明范围，G4、DocKey/Version、完整freshness-aware hydrate、项目chunk_line_budget接线、通用小块合并、公开holdout、100k/RSS/尾延迟、跨平台及发行认证未完成；S11仍失败，性能compare inconclusive；外部Go/TS编译器未运行。"}]
实施备注：长函数按已有statement/block边界递归；必要fallback保留精确原文span，UTF8/CRLF/零预算进度有独立测试。项目chunk_line_budget到Registry的既有接线缺口及可配置组合预算仍属P4-B。

### [x] P4-005｜注释签名关联

状态：`done`；批次：`P4-A`；优先级：`normal`。
范围：`crates/cc-parsers/src/chunker/boundaries.rs`；`crates/cc-parsers/src/chunker/merge.rs`；`crates/cc-parsers/src/chunker/split.rs`
硬依赖：P4-004
步骤：doc comment优先贴近定义；不跨不相关父域合并
交付物：注释签名关联的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V09 验证证据（实施时生成）
验收：解释性注释与实现关联，独立配置注释不误绑；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V09
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "12d564788bcb59f6e841504d5720172ae3d0be7b2e2aef0ec16d94e00e2999bd", "run_id": "artifacts/benchmarks/p4a-20260928-VyxH/final-v2 + paired-final-v2", "artifacts": ["artifacts/benchmarks/p4a-20260928-VyxH/final-v2/validation.json", "artifacts/benchmarks/p4a-20260928-VyxH/paired-final-v2/summary.json", "artifacts/benchmarks/p4a-20260928-VyxH/scope-check.json", "artifacts/benchmarks/p4a-20260928-VyxH/cost-summary.json", "artifacts/benchmarks/p4a-20260928-VyxH/evidence-classification.json", "artifacts/benchmarks/p4a-20260928-VyxH/failed-final-source-proof.json", "docs/roadmap/code-index-v2/P4-A-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-A-GATE.json", "docs/internals/SOURCE_CHUNKS.md"], "commands_receipt": "artifacts/benchmarks/p4a-20260928-VyxH/final-v2/validation.json", "review": "565文件冻结、29条Cargo命令，两工具链各workspace1647passed/46ignored、HTTP180passed/39ignored、显式真实MCP19passed、watcher17passed；固定51题306请求无逐题负差分且源码证据全部有效。字节/签名/父域负例及public-v3严格证明；原失败保留，未修改gold。", "rollback_status": "schema19隔离重建；旧源码/二进制保留；日常索引未动", "limitations": "P4-A本地声明范围，G4、DocKey/Version、完整freshness-aware hydrate、项目chunk_line_budget接线、通用小块合并、公开holdout、100k/RSS/尾延迟、跨平台及发行认证未完成；S11仍失败，性能compare inconclusive；外部Go/TS编译器未运行。"}]
实施备注：相邻doc不跨空行/父域误绑；长Python literal docstring与装饰签名前缀关联，箭头函数signature不包含body；连续文件文档形成预算内完整块，R14恢复。通用碎片合并未提前标done。

### [x] P4-006｜小碎片合并与重复区间消除

状态：`done`；批次：`P4-B`；优先级：`normal`。
范围：`crates/cc-parsers/src/chunker/merge.rs`
硬依赖：P3-020, P4-003, P4-005
步骤：处理括号尾/重复起点/包含span；合并仍遵守大小和结构域
交付物：小碎片合并与重复区间消除的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V09, V12 验证证据（实施时生成）
验收：不丢源码覆盖、不制造重叠刷分候选；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V09；V12
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "ccb702775561ad50b332047b6d248f1c31e94b6a41be17cd45ee4aac9c06dbcb", "run_id": "p4b-20260928-tuNL/final + paired-final", "artifacts": ["artifacts/benchmarks/p4b-20260928-tuNL/final/validation.json", "artifacts/benchmarks/p4b-20260928-tuNL/paired-final/summary.json", "artifacts/benchmarks/p4b-20260928-tuNL/scope-check.json", "artifacts/benchmarks/p4b-20260928-tuNL/cost-summary.json", "artifacts/benchmarks/p4b-20260928-tuNL/evidence-classification.json", "docs/roadmap/code-index-v2/P4-B-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-B-GATE.json", "docs/internals/CHUNK_POLICY.md"], "commands_receipt": "artifacts/benchmarks/p4b-20260928-tuNL/final/validation.json", "review": "Frozen source membership/content, command logs and binary receipts verified; independent raw-source, policy transition, render and per-case quality gates passed. Author review, not external review.", "rollback_status": "schema20 isolated rebuild from19/earlier; prior sources/binaries retained; daily index untouched", "limitations": "G4、DocKey/Version、持久document manifest、provider、完整查询时磁盘freshness、公开holdout、100k/RSS/尾延迟、跨平台与发行认证未完成。S11仍失败；性能比较按原始结果保留，模块外部Go/TS参照未运行。"}]
实施备注：复用原Partition的排序/裁剪，合并严格同owner/同结构父域的相邻普通小片段；Control/Block/声明/文档为边界，包装statement传播Control约束。独立分支负例和跨重叠span覆盖保留；R12开发退化原始结果保留。

### [x] P4-007｜统一gap与尾部预算

状态：`done`；批次：`P4-B`；优先级：`normal`。
范围：`crates/cc-parsers/src/chunker/mod.rs`；`crates/cc-parsers/src/chunker/fallback.rs`
硬依赖：P4-006
步骤：所有非符号区间走同一budget；长配置/文档有fallback
交付物：统一gap与尾部预算的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V09 验证证据（实施时生成）
验收：不存在无限gap块或不受预算的尾块；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V09
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "ccb702775561ad50b332047b6d248f1c31e94b6a41be17cd45ee4aac9c06dbcb", "run_id": "p4b-20260928-tuNL/final + paired-final", "artifacts": ["artifacts/benchmarks/p4b-20260928-tuNL/final/validation.json", "artifacts/benchmarks/p4b-20260928-tuNL/paired-final/summary.json", "artifacts/benchmarks/p4b-20260928-tuNL/scope-check.json", "artifacts/benchmarks/p4b-20260928-tuNL/cost-summary.json", "artifacts/benchmarks/p4b-20260928-tuNL/evidence-classification.json", "docs/roadmap/code-index-v2/P4-B-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-B-GATE.json", "docs/internals/CHUNK_POLICY.md"], "commands_receipt": "artifacts/benchmarks/p4b-20260928-tuNL/final/validation.json", "review": "Frozen source membership/content, command logs and binary receipts verified; independent raw-source, policy transition, render and per-case quality gates passed. Author review, not external review.", "rollback_status": "schema20 isolated rebuild from19/earlier; prior sources/binaries retained; daily index untouched", "limitations": "G4、DocKey/Version、持久document manifest、provider、完整查询时磁盘freshness、公开holdout、100k/RSS/尾延迟、跨平台与发行认证未完成。S11仍失败；性能比较按原始结果保留，模块外部Go/TS参照未运行。"}]
实施备注：实际入口沿用chunker.rs（不是新建mod.rs），共享budget.rs/fallback.rs服务所有gap/head/tail/symbol；与源码连续字节覆盖和行/字节/标量/估算token限制一起验证。

### [x] P4-008｜长行与不可解析文件fallback

状态：`done`；批次：`P4-B`；优先级：`normal`。
范围：`crates/cc-parsers/src/chunker/fallback.rs`；`crates/cc-parsers/src/chunker/source_slice.rs`
硬依赖：P4-007
步骤：UTF8安全分byte span或显式skip；parse失败与无内容区分
交付物：长行与不可解析文件fallback的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V09 验证证据（实施时生成）
验收：不把截短正文标成完整行，压缩单行有明确处置；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V09
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "ccb702775561ad50b332047b6d248f1c31e94b6a41be17cd45ee4aac9c06dbcb", "run_id": "p4b-20260928-tuNL/final + paired-final", "artifacts": ["artifacts/benchmarks/p4b-20260928-tuNL/final/validation.json", "artifacts/benchmarks/p4b-20260928-tuNL/paired-final/summary.json", "artifacts/benchmarks/p4b-20260928-tuNL/scope-check.json", "artifacts/benchmarks/p4b-20260928-tuNL/cost-summary.json", "artifacts/benchmarks/p4b-20260928-tuNL/evidence-classification.json", "docs/roadmap/code-index-v2/P4-B-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-B-GATE.json", "docs/internals/CHUNK_POLICY.md"], "commands_receipt": "artifacts/benchmarks/p4b-20260928-tuNL/final/validation.json", "review": "Frozen source membership/content, command logs and binary receipts verified; independent raw-source, policy transition, render and per-case quality gates passed. Author review, not external review.", "rollback_status": "schema20 isolated rebuild from19/earlier; prior sources/binaries retained; daily index untouched", "limitations": "G4、DocKey/Version、持久document manifest、provider、完整查询时磁盘freshness、公开holdout、100k/RSS/尾延迟、跨平台与发行认证未完成。S11仍失败；性能比较按原始结果保留，模块外部Go/TS参照未运行。"}]
实施备注：UTF8/CRLF安全前缀和显式SourceSnapshot byte span复用而非再建source_slice.rs；畸形AST标partial，致命编码/解析错误保留错误，不将跳过伪装成功空文件。

### [x] P4-009｜token与字符/字节预算

状态：`done`；批次：`P4-B`；优先级：`normal`。
范围：`crates/cc-model/src/config.rs`；`crates/cc-parsers/src/chunker/`
硬依赖：P4-008
步骤：定义单位/上限/估算字段；精确tokenizer为可选而非默认假精确
交付物：token与字符/字节预算的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V09 验证证据（实施时生成）
验收：首块和所有fallback都不超过硬byte cap；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V09
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "ccb702775561ad50b332047b6d248f1c31e94b6a41be17cd45ee4aac9c06dbcb", "run_id": "p4b-20260928-tuNL/final + paired-final", "artifacts": ["artifacts/benchmarks/p4b-20260928-tuNL/final/validation.json", "artifacts/benchmarks/p4b-20260928-tuNL/paired-final/summary.json", "artifacts/benchmarks/p4b-20260928-tuNL/scope-check.json", "artifacts/benchmarks/p4b-20260928-tuNL/cost-summary.json", "artifacts/benchmarks/p4b-20260928-tuNL/evidence-classification.json", "docs/roadmap/code-index-v2/P4-B-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-B-GATE.json", "docs/internals/CHUNK_POLICY.md"], "commands_receipt": "artifacts/benchmarks/p4b-20260928-tuNL/final/validation.json", "review": "Frozen source membership/content, command logs and binary receipts verified; independent raw-source, policy transition, render and per-case quality gates passed. Author review, not external review.", "rollback_status": "schema20 isolated rebuild from19/earlier; prior sources/binaries retained; daily index untouched", "limitations": "G4、DocKey/Version、持久document manifest、provider、完整查询时磁盘freshness、公开holdout、100k/RSS/尾延迟、跨平台与发行认证未完成。S11仍失败；性能比较按原始结果保留，模块外部Go/TS参照未运行。"}]
实施备注：项目所有Registry解析器和SFC接入ChunkPolicy；每build读取indexing配置。逐文件stamp同事务确认、aggregate4缓存一致、事件外旧policy扫描、失败文件恢复、PreparedBuild policy校验与原报告carry均已测试。估算token=ceil(bytes/4)，不是精确tokenizer。

### [x] P4-010｜分离源码文本与embedding渲染

状态：`done`；批次：`P4-B`；优先级：`normal`。
范围：`crates/cc-index/src/documents/render.rs`；`crates/cc-model/src/retrieval.rs`
硬依赖：P4-009
步骤：render路径/语言/父签名元数据；源片段保持原样
交付物：分离源码文本与embedding渲染的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V09, V10 验证证据（实施时生成）
验收：模型输入变化可hash，证据正文不含伪造元数据；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V09；V10
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "ccb702775561ad50b332047b6d248f1c31e94b6a41be17cd45ee4aac9c06dbcb", "run_id": "p4b-20260928-tuNL/final + paired-final", "artifacts": ["artifacts/benchmarks/p4b-20260928-tuNL/final/validation.json", "artifacts/benchmarks/p4b-20260928-tuNL/paired-final/summary.json", "artifacts/benchmarks/p4b-20260928-tuNL/scope-check.json", "artifacts/benchmarks/p4b-20260928-tuNL/cost-summary.json", "artifacts/benchmarks/p4b-20260928-tuNL/evidence-classification.json", "docs/roadmap/code-index-v2/P4-B-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-B-GATE.json", "docs/internals/CHUNK_POLICY.md"], "commands_receipt": "artifacts/benchmarks/p4b-20260928-tuNL/final/validation.json", "review": "Frozen source membership/content, command logs and binary receipts verified; independent raw-source, policy transition, render and per-case quality gates passed. Author review, not external review.", "rollback_status": "schema20 isolated rebuild from19/earlier; prior sources/binaries retained; daily index untouched", "limitations": "G4、DocKey/Version、持久document manifest、provider、完整查询时磁盘freshness、公开holdout、100k/RSS/尾延迟、跨平台与发行认证未完成。S11仍失败；性能比较按原始结果保留，模块外部Go/TS参照未运行。"}]
实施备注：documents/render.rs纯API由真实parser chunk和SourceSnapshot验证，独立EmbeddingInput保持source_range、actual-input hash和render_key；元数据截断显式，源正文预算不足则报错。尚未连接provider/outbox/持久文档，按后续阶段推进。

### [x] P4-011｜定义DocKey与DocVersion

状态：`done`；批次：`P4-C`；优先级：`normal`。
范围：`crates/cc-model/src/identity.rs`；`crates/cc-model/src/id.rs`
硬依赖：P3-020, P4-001, P4-010
步骤：明确实体/文档/输入hash边界；legacy chunk_id只作兼容
交付物：定义DocKey与DocVersion的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V10 验证证据（实施时生成）
验收：路径序号不是异步向量发布依据，rename不虚称稳定；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V10
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "fe81b8e4dac7fae17d9ba2d5ac317386cc8114abf1523c154bd3caedb0e6915a", "run_id": "p4c-20260928-pHaf/final-v2 + paired-final-v2", "artifacts": ["artifacts/benchmarks/p4c-20260928-pHaf/final-v2/validation.json", "artifacts/benchmarks/p4c-20260928-pHaf/paired-final-v2/summary.json", "artifacts/benchmarks/p4c-20260928-pHaf/scope-check.json", "artifacts/benchmarks/p4c-20260928-pHaf/cost-summary.json", "artifacts/benchmarks/p4c-20260928-pHaf/evidence-classification.json", "artifacts/benchmarks/p4c-20260928-pHaf/accepted-source.tar.gz", "docs/roadmap/code-index-v2/P4-C-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-C-GATE.json", "docs/internals/DOCUMENTS.md"], "commands_receipt": "artifacts/benchmarks/p4c-20260928-pHaf/final-v2/validation.json", "review": "584 complete source membership/content hashes,31 completed command/log receipts,306 paired requests,14 input contracts,version-specific14/15-table oracles and public lifecycle validated", "rollback_status": "schema21 isolated rebuild; accepted current source archive and prior accepted binaries preserved; daily index untouched", "limitations": "P4-C local scope only; G4, asynchronous publish/incarnation/provider, global filesystem/query snapshot, public holdout, 100k/RSS/tail, cross-platform and release not certified. S11 fails; formal performance comparisons retained."}]
实施备注：Index-local key/version/entity/encoding分离；原文字节/位置/kind/policy/spec/input进入版本，rename不假称稳定；重复来源不去重丢失。

### [x] P4-012｜文档清单存储与delta

状态：`done`；批次：`P4-C`；优先级：`normal`。
范围：`crates/cc-db/src/document_store.rs`；`crates/cc-index/src/documents/delta.rs`
硬依赖：P4-011
步骤：upsert/remove/current版本映射；比较内容/渲染规格复用
交付物：文档清单存储与delta的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V10, V13 验证证据（实施时生成）
验收：删块移除当前清单，同输入不同来源仍保留各自出处；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V10；V13
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "fe81b8e4dac7fae17d9ba2d5ac317386cc8114abf1523c154bd3caedb0e6915a", "run_id": "p4c-20260928-pHaf/final-v2 + paired-final-v2", "artifacts": ["artifacts/benchmarks/p4c-20260928-pHaf/final-v2/validation.json", "artifacts/benchmarks/p4c-20260928-pHaf/paired-final-v2/summary.json", "artifacts/benchmarks/p4c-20260928-pHaf/scope-check.json", "artifacts/benchmarks/p4c-20260928-pHaf/cost-summary.json", "artifacts/benchmarks/p4c-20260928-pHaf/evidence-classification.json", "artifacts/benchmarks/p4c-20260928-pHaf/accepted-source.tar.gz", "docs/roadmap/code-index-v2/P4-C-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-C-GATE.json", "docs/internals/DOCUMENTS.md"], "commands_receipt": "artifacts/benchmarks/p4c-20260928-pHaf/final-v2/validation.json", "review": "584 complete source membership/content hashes,31 completed command/log receipts,306 paired requests,14 input contracts,version-specific14/15-table oracles and public lifecycle validated", "rollback_status": "schema21 isolated rebuild; accepted current source archive and prior accepted binaries preserved; daily index untouched", "limitations": "P4-C local scope only; G4, asynchronous publish/incarnation/provider, global filesystem/query snapshot, public holdout, 100k/RSS/tail, cross-platform and release not certified. S11 fails; formal performance comparisons retained."}]
实施备注：document_manifest当前索引清单与有界逐文件delta，模型投影输入或显式render_error；同事务写入/删除/回滚，非history/outbox。

### [x] P4-013｜接入构建与staging

状态：`done`；批次：`P4-C`；优先级：`normal`。
范围：`crates/cc-index/src/build_plan.rs`；`crates/cc-index/src/indexer_phases/write.rs`
硬依赖：P4-012
步骤：文档投影随PreparedBuild传递；full/inc共用delta规范
交付物：接入构建与staging的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V09, V10 验证证据（实施时生成）
验收：两条构建路径产出同文档集合，事务回滚不半写；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V09；V10
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "fe81b8e4dac7fae17d9ba2d5ac317386cc8114abf1523c154bd3caedb0e6915a", "run_id": "p4c-20260928-pHaf/final-v2 + paired-final-v2", "artifacts": ["artifacts/benchmarks/p4c-20260928-pHaf/final-v2/validation.json", "artifacts/benchmarks/p4c-20260928-pHaf/paired-final-v2/summary.json", "artifacts/benchmarks/p4c-20260928-pHaf/scope-check.json", "artifacts/benchmarks/p4c-20260928-pHaf/cost-summary.json", "artifacts/benchmarks/p4c-20260928-pHaf/evidence-classification.json", "artifacts/benchmarks/p4c-20260928-pHaf/accepted-source.tar.gz", "docs/roadmap/code-index-v2/P4-C-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-C-GATE.json", "docs/internals/DOCUMENTS.md"], "commands_receipt": "artifacts/benchmarks/p4c-20260928-pHaf/final-v2/validation.json", "review": "584 complete source membership/content hashes,31 completed command/log receipts,306 paired requests,14 input contracts,version-specific14/15-table oracles and public lifecycle validated", "rollback_status": "schema21 isolated rebuild; accepted current source archive and prior accepted binaries preserved; daily index untouched", "limitations": "P4-C local scope only; G4, asynchronous publish/incarnation/provider, global filesystem/query snapshot, public holdout, 100k/RSS/tail, cross-platform and release not certified. S11 fails; formal performance comparisons retained."}]
实施备注：已有prepare/FileWriteUnit/full-staging/scalar/table-major管线接线；document_spec独立参与file-state/cache失效，dirty-only不重写；15表差分。

### [x] P4-014｜FTS与chunk兼容迁移

状态：`done`；批次：`P4-C`；优先级：`normal`。
范围：`crates/cc-db/src/index_db_write_batch.rs`；`crates/cc-search/src/plan.rs`
硬依赖：P4-013
步骤：把新span/doc映射接入FTS和读面；保持旧wire可读
交付物：FTS与chunk兼容迁移的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V10, V18 验证证据（实施时生成）
验收：旧工具不依赖新向量功能，FTS镜像一致；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V10；V18
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "fe81b8e4dac7fae17d9ba2d5ac317386cc8114abf1523c154bd3caedb0e6915a", "run_id": "p4c-20260928-pHaf/final-v2 + paired-final-v2", "artifacts": ["artifacts/benchmarks/p4c-20260928-pHaf/final-v2/validation.json", "artifacts/benchmarks/p4c-20260928-pHaf/paired-final-v2/summary.json", "artifacts/benchmarks/p4c-20260928-pHaf/scope-check.json", "artifacts/benchmarks/p4c-20260928-pHaf/cost-summary.json", "artifacts/benchmarks/p4c-20260928-pHaf/evidence-classification.json", "artifacts/benchmarks/p4c-20260928-pHaf/accepted-source.tar.gz", "docs/roadmap/code-index-v2/P4-C-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-C-GATE.json", "docs/internals/DOCUMENTS.md"], "commands_receipt": "artifacts/benchmarks/p4c-20260928-pHaf/final-v2/validation.json", "review": "584 complete source membership/content hashes,31 completed command/log receipts,306 paired requests,14 input contracts,version-specific14/15-table oracles and public lifecycle validated", "rollback_status": "schema21 isolated rebuild; accepted current source archive and prior accepted binaries preserved; daily index untouched", "limitations": "P4-C local scope only; G4, asynchronous publish/incarnation/provider, global filesystem/query snapshot, public holdout, 100k/RSS/tail, cross-platform and release not certified. S11 fails; formal performance comparisons retained."}]
实施备注：FTS原文与rowid/legacy chunk_id保留，hydration输出document ref并拒绝坏/缺失的必需manifest；输出截断保留有界预览与诊断；14工具输入不变。

### [x] P4-015｜源码hydration一致性

状态：`done`；批次：`P4-C`；优先级：`normal`。
范围：`crates/cc-search/src/evidence.rs`；`crates/cc-search/src/evidence_path.rs`；`crates/cc-server/src/engine_query.rs`；`crates/cc-server/src/handlers/context.rs`；`crates/cc-server/src/graph_trace.rs`；`crates/cc-server/src/graph_flow.rs`
硬依赖：P4-014
步骤：索引快照读或磁盘hash核验；失配返回freshness诊断
交付物：源码hydration一致性的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V09, V12 验证证据（实施时生成）
验收：旧行号不能解释新文件正文，删除文件不伪造当前证据；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V09；V12
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "fe81b8e4dac7fae17d9ba2d5ac317386cc8114abf1523c154bd3caedb0e6915a", "run_id": "p4c-20260928-pHaf/final-v2 + paired-final-v2", "artifacts": ["artifacts/benchmarks/p4c-20260928-pHaf/final-v2/validation.json", "artifacts/benchmarks/p4c-20260928-pHaf/paired-final-v2/summary.json", "artifacts/benchmarks/p4c-20260928-pHaf/scope-check.json", "artifacts/benchmarks/p4c-20260928-pHaf/cost-summary.json", "artifacts/benchmarks/p4c-20260928-pHaf/evidence-classification.json", "artifacts/benchmarks/p4c-20260928-pHaf/accepted-source.tar.gz", "docs/roadmap/code-index-v2/P4-C-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-C-GATE.json", "docs/internals/DOCUMENTS.md"], "commands_receipt": "artifacts/benchmarks/p4c-20260928-pHaf/final-v2/validation.json", "review": "584 complete source membership/content hashes,31 completed command/log receipts,306 paired requests,14 input contracts,version-specific14/15-table oracles and public lifecycle validated", "rollback_status": "schema21 isolated rebuild; accepted current source archive and prior accepted binaries preserved; daily index untouched", "limitations": "P4-C local scope only; G4, asynchronous publish/incarnation/provider, global filesystem/query snapshot, public holdout, 100k/RSS/tail, cross-platform and release not certified. S11 fails; formal performance comparisons retained."}]
实施备注：实际源码入口为cc-search/evidence、engine_query、handlers/context及trace/flow；原symbol_extract只抽用户查询名称，scope据实修正。缓存命中也核验磁盘，省略stale/deleted并partial；非OS/全查询原子快照。

### [x] P4-016｜身份扰动mutation tests

状态：`done`；批次：`P4-D`；优先级：`normal`。
范围：`crates/cc-eval/tests/document_identity.rs`；`crates/cc-eval/benchmarks/mutations/p4d-document-identity.json`
硬依赖：P3-020, P4-015
步骤：顶部插入、重复函数、换行变更、rename、模板更新；比较复用和撤销
交付物：身份扰动mutation tests的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V10 验证证据（实施时生成）
验收：不错误共享embedding input，稳定片段复用可量化；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V10
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "worktree_digest": "78a88e6ad2955e488900670fe92ff74d0b02d4d83eaa481f9785cbd4e3007773", "covered_source_digest_sha256": "78a88e6ad2955e488900670fe92ff74d0b02d4d83eaa481f9785cbd4e3007773", "run_id": "p4d-20260929-g4/final + paired-final", "artifacts": ["artifacts/benchmarks/p4d-20260929-g4/final/validation.json", "artifacts/benchmarks/p4d-20260929-g4/paired-final/summary.json", "artifacts/benchmarks/p4d-20260929-g4/scope-check.json", "artifacts/benchmarks/p4d-20260929-g4/cost-summary.json", "artifacts/benchmarks/p4d-20260929-g4/evidence-classification.json", "artifacts/benchmarks/p4d-20260929-g4/accepted-source.tar.gz", "docs/roadmap/code-index-v2/P4-D-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-D-GATE.json", "docs/roadmap/code-index-v2/P4-GATE.json", "docs/internals/CHUNK_POLICY.md"], "commands_receipt": "artifacts/benchmarks/p4d-20260929-g4/final/validation.json", "review": "586 complete source membership/content hashes; 19 Cargo receipts, 2 architecture checks, two toolchain oracle runs, 306 paired requests, 14 MCP input contracts, 15-table mutation oracles and P4-D V09/V10/V19/V20 evidence reviewed", "rollback_status": "schema21 unchanged; accepted P4-D source archive and exact P4-C binaries preserved; remove P4-D policy/test additions and rebuild isolated index; daily index untouched", "limitations": "P4/G4 local macOS arm64 declared scope only; 51 authored development questions are not public holdout; S11 no-answer remains failed for P5; no provider/vector publication/global query snapshot/100k/transient peak RSS/tail/cross-platform/release certification; Go external compiler oracles remain not_run."}]
实施备注：真实parser/SQLite增量夹具覆盖尾部扩展、顶部插入、重复函数、LF↔CRLF、rename、撤销及renderer spec变化；旧引用撤销、精确恢复、合法复用计数及encoding key不别名均通过。

### [x] P4-017｜切块检索质量消融

状态：`done`；批次：`P4-D`；优先级：`normal`。
范围：`crates/cc-eval/src/benchmark/ablation.rs`；`crates/cc-eval/benchmarks/native/p4d-chunk-ablation.json`；`crates/cc-eval/tests/p4d_quality.rs`
硬依赖：P4-016
步骤：旧/新chunker固定检索器比较；查看symbol/span/facet与duplication
交付物：切块检索质量消融的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V19 验证证据（实施时生成）
验收：不能只以块数减少声称质量改善；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V19
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "worktree_digest": "78a88e6ad2955e488900670fe92ff74d0b02d4d83eaa481f9785cbd4e3007773", "covered_source_digest_sha256": "78a88e6ad2955e488900670fe92ff74d0b02d4d83eaa481f9785cbd4e3007773", "run_id": "p4d-20260929-g4/final + paired-final", "artifacts": ["artifacts/benchmarks/p4d-20260929-g4/final/validation.json", "artifacts/benchmarks/p4d-20260929-g4/paired-final/summary.json", "artifacts/benchmarks/p4d-20260929-g4/scope-check.json", "artifacts/benchmarks/p4d-20260929-g4/cost-summary.json", "artifacts/benchmarks/p4d-20260929-g4/evidence-classification.json", "artifacts/benchmarks/p4d-20260929-g4/accepted-source.tar.gz", "docs/roadmap/code-index-v2/P4-D-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-D-GATE.json", "docs/roadmap/code-index-v2/P4-GATE.json", "docs/internals/CHUNK_POLICY.md"], "commands_receipt": "artifacts/benchmarks/p4d-20260929-g4/final/validation.json", "review": "586 complete source membership/content hashes; 19 Cargo receipts, 2 architecture checks, two toolchain oracle runs, 306 paired requests, 14 MCP input contracts, 15-table mutation oracles and P4-D V09/V10/V19/V20 evidence reviewed", "rollback_status": "schema21 unchanged; accepted P4-D source archive and exact P4-C binaries preserved; remove P4-D policy/test additions and rebuild isolated index; daily index untouched", "limitations": "P4/G4 local macOS arm64 declared scope only; 51 authored development questions are not public holdout; S11 no-answer remains failed for P5; no provider/vector publication/global query snapshot/100k/transient peak RSS/tail/cross-platform/release certification; Go external compiler oracles remain not_run."}]
实施备注：同一parser/SQLite/公开hybrid仅切换merge_min_bytes；4题路径/符号/facet/span/digest/freshness/duplication全核验，逐题零退化、source failure 0，documents 25→20、model input 7417→6156。

### [x] P4-018｜切块内存和构建成本

状态：`done`；批次：`P4-D`；优先级：`normal`。
范围：`crates/cc-eval/src/benchmark/sampler.rs`；`crates/cc-index/src/memory_budget.rs`；`crates/cc-eval/tests/p4d_cost.rs`
硬依赖：P4-017
步骤：记录每文件parse次数、边界内存、chunk数量和最大块；测大文件
交付物：切块内存和构建成本的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V20 验证证据（实施时生成）
验收：没有全仓AST驻留，新增文档存储成本明确；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V20
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "worktree_digest": "78a88e6ad2955e488900670fe92ff74d0b02d4d83eaa481f9785cbd4e3007773", "covered_source_digest_sha256": "78a88e6ad2955e488900670fe92ff74d0b02d4d83eaa481f9785cbd4e3007773", "run_id": "p4d-20260929-g4/final + paired-final", "artifacts": ["artifacts/benchmarks/p4d-20260929-g4/final/validation.json", "artifacts/benchmarks/p4d-20260929-g4/paired-final/summary.json", "artifacts/benchmarks/p4d-20260929-g4/scope-check.json", "artifacts/benchmarks/p4d-20260929-g4/cost-summary.json", "artifacts/benchmarks/p4d-20260929-g4/evidence-classification.json", "artifacts/benchmarks/p4d-20260929-g4/accepted-source.tar.gz", "docs/roadmap/code-index-v2/P4-D-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-D-GATE.json", "docs/roadmap/code-index-v2/P4-GATE.json", "docs/internals/CHUNK_POLICY.md"], "commands_receipt": "artifacts/benchmarks/p4d-20260929-g4/final/validation.json", "review": "586 complete source membership/content hashes; 19 Cargo receipts, 2 architecture checks, two toolchain oracle runs, 306 paired requests, 14 MCP input contracts, 15-table mutation oracles and P4-D V09/V10/V19/V20 evidence reviewed", "rollback_status": "schema21 unchanged; accepted P4-D source archive and exact P4-C binaries preserved; remove P4-D policy/test additions and rebuild isolated index; daily index untouched", "limitations": "P4/G4 local macOS arm64 declared scope only; 51 authored development questions are not public holdout; S11 no-answer remains failed for P5; no provider/vector publication/global query snapshot/100k/transient peak RSS/tail/cross-platform/release certification; Go external compiler oracles remain not_run."}]
实施备注：release记录32/256文件各3次全建、30次no-op、24次单文件更新及1000/5000语句各3次；native/ps RSS阶段快照、phase timing、chunk/manifest/DB成本和无持久AST表均留证，不冒充peak/100k/tail。

### [x] P4-019｜移除旧切块双实现并更新文档

状态：`done`；批次：`P4-D`；优先级：`normal`。
范围：`crates/cc-parsers/src/chunker.rs`；`crates/cc-parsers/src/lib.rs`；`docs/internals/CHUNK_POLICY.md`；`docs/internals/INDEXING.md`；`docs/internals/STORAGE.md`；`scripts/check_source_architecture.py`
硬依赖：P4-018
步骤：在行为通过后拆文件/移除旧默认；记录fallback和schema影响
交付物：移除旧切块双实现并更新文档的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V09, V21 验证证据（实施时生成）
验收：只有一个生产切块策略入口，旧策略仅benchmark对照；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V09；V21
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "worktree_digest": "78a88e6ad2955e488900670fe92ff74d0b02d4d83eaa481f9785cbd4e3007773", "covered_source_digest_sha256": "78a88e6ad2955e488900670fe92ff74d0b02d4d83eaa481f9785cbd4e3007773", "run_id": "p4d-20260929-g4/final + paired-final", "artifacts": ["artifacts/benchmarks/p4d-20260929-g4/final/validation.json", "artifacts/benchmarks/p4d-20260929-g4/paired-final/summary.json", "artifacts/benchmarks/p4d-20260929-g4/scope-check.json", "artifacts/benchmarks/p4d-20260929-g4/cost-summary.json", "artifacts/benchmarks/p4d-20260929-g4/evidence-classification.json", "artifacts/benchmarks/p4d-20260929-g4/accepted-source.tar.gz", "docs/roadmap/code-index-v2/P4-D-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-D-GATE.json", "docs/roadmap/code-index-v2/P4-GATE.json", "docs/internals/CHUNK_POLICY.md"], "commands_receipt": "artifacts/benchmarks/p4d-20260929-g4/final/validation.json", "review": "586 complete source membership/content hashes; 19 Cargo receipts, 2 architecture checks, two toolchain oracle runs, 306 paired requests, 14 MCP input contracts, 15-table mutation oracles and P4-D V09/V10/V19/V20 evidence reviewed", "rollback_status": "schema21 unchanged; accepted P4-D source archive and exact P4-C binaries preserved; remove P4-D policy/test additions and rebuild isolated index; daily index untouched", "limitations": "P4/G4 local macOS arm64 declared scope only; 51 authored development questions are not public holdout; S11 no-answer remains failed for P5; no provider/vector publication/global query snapshot/100k/transient peak RSS/tail/cross-platform/release certification; Go external compiler oracles remain not_run."}]
实施备注：删除line-only Chunker::new及静默bounded，统一完整ChunkPolicy显式校验；AST/heuristic/line仅产生SourceStructure，生产唯一核心为from_structure→Partition→coalesce；schema仍21。

### [x] P4-020｜P4文档与源码证据验收

状态：`done`；批次：`P4-D`；优先级：`blocking`。
范围：`docs/roadmap/code-index-v2/`；`artifacts/benchmarks/p4d-20260929-g4/`
硬依赖：P4-001, P4-002, P4-003, P4-004, P4-005, P4-006, P4-007, P4-008, P4-009, P4-010, P4-011, P4-012, P4-013, P4-014, P4-015, P4-016, P4-017, P4-018, P4-019
步骤：执行G4并归档身份、覆盖、召回和资源报告
交付物：P4文档与源码证据验收的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V09, V10, V19, V20 验证证据（实施时生成）
验收：所有source spans可核验，embedding尚未接入也有独立收益；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V09；V10；V19；V20
回滚：回退chunker/document版本并重建索引；旧wire身份映射不得冒充新文档。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "worktree_digest": "78a88e6ad2955e488900670fe92ff74d0b02d4d83eaa481f9785cbd4e3007773", "covered_source_digest_sha256": "78a88e6ad2955e488900670fe92ff74d0b02d4d83eaa481f9785cbd4e3007773", "run_id": "p4d-20260929-g4/final + paired-final", "artifacts": ["artifacts/benchmarks/p4d-20260929-g4/final/validation.json", "artifacts/benchmarks/p4d-20260929-g4/paired-final/summary.json", "artifacts/benchmarks/p4d-20260929-g4/scope-check.json", "artifacts/benchmarks/p4d-20260929-g4/cost-summary.json", "artifacts/benchmarks/p4d-20260929-g4/evidence-classification.json", "artifacts/benchmarks/p4d-20260929-g4/accepted-source.tar.gz", "docs/roadmap/code-index-v2/P4-D-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P4-D-GATE.json", "docs/roadmap/code-index-v2/P4-GATE.json", "docs/internals/CHUNK_POLICY.md"], "commands_receipt": "artifacts/benchmarks/p4d-20260929-g4/final/validation.json", "review": "586 complete source membership/content hashes; 19 Cargo receipts, 2 architecture checks, two toolchain oracle runs, 306 paired requests, 14 MCP input contracts, 15-table mutation oracles and P4-D V09/V10/V19/V20 evidence reviewed", "rollback_status": "schema21 unchanged; accepted P4-D source archive and exact P4-C binaries preserved; remove P4-D policy/test additions and rebuild isolated index; daily index untouched", "limitations": "P4/G4 local macOS arm64 declared scope only; 51 authored development questions are not public holdout; S11 no-answer remains failed for P5; no provider/vector publication/global query snapshot/100k/transient peak RSS/tail/cross-platform/release certification; Go external compiler oracles remain not_run."}]
实施备注：G4本地声明范围完成：586文件冻结、双工具链workspace1678/51、HTTP200/44、P4-D2、真实MCP21、watcher17；51题306请求零逐题退化，14工具输入契约一致。下一批P5-A，M2仍需P5。

## P5｜查询执行与证据装配

### [x] P5-001｜统一Candidate与LaneOutcome

状态：`done`；批次：`P5-A`；优先级：`normal`。
范围：`crates/cc-model/src/retrieval.rs`；`crates/cc-search/src/lanes.rs`
硬依赖：P4-020
步骤：加入doc版本、rank/raw score、状态、耗时和coverage；保持schema映射
交付物：统一Candidate与LaneOutcome的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V18 验证证据（实施时生成）
验收：disabled/empty/error/partial在公开结果可区分；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V18
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "status": "partial_not_accepted", "artifacts": ["docs/roadmap/code-index-v2/P5-A-PROGRESS.md", "artifacts/benchmarks/p5a-20260929-resume/development/verified.log", "artifacts/benchmarks/p5a-20260929-resume/final/validation.json"], "limitations": "局部通过不代表当前工作树全仓通过；失败冻结后又有源码变更。"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "cdc8a168987e005fdf8f36539525343c2531ba90e4b282938117642663919c8a", "status": "passed_declared_local_scope", "run_id": "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2", "artifacts": ["artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/validation.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/paired/summary.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/closure-audit.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/cost-summary.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/source-manifest.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/accepted-source.tar.gz", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/completeness-review.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/runner-restoration.json", "docs/roadmap/code-index-v2/P5-A-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P5-A-GATE.json"], "commands_receipt": "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/validation.json", "review": "592-file frozen membership/content and archive audited; dual toolchain regressions, version/scope/score negative cases, 14-tool input compatibility, 306 paired requests and 60 release observations verified", "rollback_status": "prior accepted P4-D source archive preserved; current schema remains21; local sources only, no deployment or daily-index mutation", "limitations": "P5-A local macOS arm64 scope only; not G5/M2 or a release certificate; S11 no-answer failure retained; no gold or scoring-formula changes; Three-attempt index/evidence fence is not filesystem atomicity or rebuild-incarnation safety; No QueryHandle, cross-request bounded executor, deadlines/cancellation or semantic provider in this batch; 60 in-process local cost observations are not holdout, 100k, peak RSS, p95/p99 or cross-platform certification; Full retrieval-completeness gates remain failed: 42 source and 12 intent candidate requests are explicitly Partial; 75 source reason occurrences and 12 intent occurrences reviewed. S11 also remains. No full green retrieval claim."}]
实施备注：CandidateRef/LaneOutcome跨层接线、状态/原因/coverage强校验与public-v5解释已按当前冻结源码验收；完整异步查询契约由P5-B/C/D继续。

### [x] P5-002｜收拢本地lane注册

状态：`done`；批次：`P5-A`；优先级：`normal`。
范围：`crates/cc-search/src/lanes/`；`crates/cc-search/src/engine.rs`
硬依赖：P5-001
步骤：在新契约下迁移lexical/grep/graph；不直接重写其已验证算法
交付物：收拢本地lane注册的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05, V11 验证证据（实施时生成）
验收：旧本地排序除声明变化外可对照，错误不静默吞掉；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05；V11
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "status": "blocked_validation_failed", "artifacts": ["docs/roadmap/code-index-v2/P5-A-PROGRESS.md", "artifacts/benchmarks/p5a-20260929-resume/development/migration1-receipts.json", "artifacts/benchmarks/p5a-20260929-resume/development/red-cached-text.log"], "limitations": "缓存修复未落地，红测仍失败，不接受本批。"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "cdc8a168987e005fdf8f36539525343c2531ba90e4b282938117642663919c8a", "status": "passed_declared_local_scope", "run_id": "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2", "artifacts": ["artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/validation.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/paired/summary.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/closure-audit.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/cost-summary.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/source-manifest.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/accepted-source.tar.gz", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/completeness-review.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/runner-restoration.json", "docs/roadmap/code-index-v2/P5-A-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P5-A-GATE.json"], "commands_receipt": "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/validation.json", "review": "592-file frozen membership/content and archive audited; dual toolchain regressions, version/scope/score negative cases, 14-tool input compatibility, 306 paired requests and 60 release observations verified", "rollback_status": "prior accepted P4-D source archive preserved; current schema remains21; local sources only, no deployment or daily-index mutation", "limitations": "P5-A local macOS arm64 scope only; not G5/M2 or a release certificate; S11 no-answer failure retained; no gold or scoring-formula changes; Three-attempt index/evidence fence is not filesystem atomicity or rebuild-incarnation safety; No QueryHandle, cross-request bounded executor, deadlines/cancellation or semantic provider in this batch; 60 in-process local cost observations are not holdout, 100k, peak RSS, p95/p99 or cross-platform certification; Full retrieval-completeness gates remain failed: 42 source and 12 intent candidate requests are explicitly Partial; 75 source reason occurrences and 12 intent occurrences reviewed. S11 also remains. No full green retrieval claim."}]
实施备注：五条本地通道统一注册，保留原算法与明确错误/空范围语义；旧缓存提示核验与三次乐观代检查修复并发失配，稳定损坏不被重试掩盖，结果缓存仅接纳通过检查的版本。

### [x] P5-003｜独立exact-symbol召回

状态：`done`；批次：`P5-A`；优先级：`normal`。
范围：`crates/cc-search/src/lanes/exact_symbol.rs`；`crates/cc-db/src/index_db_retrieval.rs`
硬依赖：P5-002
步骤：按名称/签名/限定路径取候选；保留歧义而非强选
交付物：独立exact-symbol召回的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05, V19 验证证据（实施时生成）
验收：被预选排除的定义可找回，同名错目标不拿高置信度；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05；V19
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "status": "partial_not_accepted", "artifacts": ["docs/roadmap/code-index-v2/P5-A-PROGRESS.md", "artifacts/benchmarks/p5a-20260929-resume/development/eval-fixtures-migration1.log"], "limitations": "前置依赖及当前源码固定题库配对未收口。"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "cdc8a168987e005fdf8f36539525343c2531ba90e4b282938117642663919c8a", "status": "passed_declared_local_scope", "run_id": "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2", "artifacts": ["artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/validation.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/paired/summary.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/closure-audit.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/cost-summary.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/source-manifest.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/accepted-source.tar.gz", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/completeness-review.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/runner-restoration.json", "docs/roadmap/code-index-v2/P5-A-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P5-A-GATE.json"], "commands_receipt": "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/validation.json", "review": "592-file frozen membership/content and archive audited; dual toolchain regressions, version/scope/score negative cases, 14-tool input compatibility, 306 paired requests and 60 release observations verified", "rollback_status": "prior accepted P4-D source archive preserved; current schema remains21; local sources only, no deployment or daily-index mutation", "limitations": "P5-A local macOS arm64 scope only; not G5/M2 or a release certificate; S11 no-answer failure retained; no gold or scoring-formula changes; Three-attempt index/evidence fence is not filesystem atomicity or rebuild-incarnation safety; No QueryHandle, cross-request bounded executor, deadlines/cancellation or semantic provider in this batch; 60 in-process local cost observations are not holdout, 100k, peak RSS, p95/p99 or cross-platform certification; Full retrieval-completeness gates remain failed: 42 source and 12 intent candidate requests are explicitly Partial; 75 source reason occurrences and 12 intent occurrences reviewed. S11 also remains. No full green retrieval claim."}]
实施备注：独立name/qname/实际字面签名召回；按定义字节位置映射，保留同名歧义、scope-before-limit、长定义和同一行所有者回归；51题固定对照无退化。

### [x] P5-004｜独立path召回

状态：`done`；批次：`P5-A`；优先级：`normal`。
范围：`crates/cc-search/src/lanes/path.rs`；`crates/cc-db/src/index_db_retrieval.rs`
硬依赖：P5-003
步骤：使用已有路径FTS/trigram而非另建引擎；路径类intent中立文档类型
交付物：独立path召回的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05, V19 验证证据（实施时生成）
验收：文件定位不被源码偏好统一降权；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05；V19
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "status": "partial_not_accepted", "artifacts": ["docs/roadmap/code-index-v2/P5-A-PROGRESS.md", "artifacts/benchmarks/p5a-20260929-resume/development/eval-fixtures-migration1.log"], "limitations": "当前源码完整集成/配对验收未通过。"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "cdc8a168987e005fdf8f36539525343c2531ba90e4b282938117642663919c8a", "status": "passed_declared_local_scope", "run_id": "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2", "artifacts": ["artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/validation.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/paired/summary.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/closure-audit.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/cost-summary.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/source-manifest.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/accepted-source.tar.gz", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/completeness-review.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/runner-restoration.json", "docs/roadmap/code-index-v2/P5-A-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P5-A-GATE.json"], "commands_receipt": "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/validation.json", "review": "592-file frozen membership/content and archive audited; dual toolchain regressions, version/scope/score negative cases, 14-tool input compatibility, 306 paired requests and 60 release observations verified", "rollback_status": "prior accepted P4-D source archive preserved; current schema remains21; local sources only, no deployment or daily-index mutation", "limitations": "P5-A local macOS arm64 scope only; not G5/M2 or a release certificate; S11 no-answer failure retained; no gold or scoring-formula changes; Three-attempt index/evidence fence is not filesystem atomicity or rebuild-incarnation safety; No QueryHandle, cross-request bounded executor, deadlines/cancellation or semantic provider in this batch; 60 in-process local cost observations are not holdout, 100k, peak RSS, p95/p99 or cross-platform certification; Full retrieval-completeness gates remain failed: 42 source and 12 intent candidate requests are explicitly Partial; 75 source reason occurrences and 12 intent occurrences reviewed. S11 also remains. No full green retrieval claim."}]
实施备注：独立精确路径与有界token召回，空文件不占文档预算、短词资格与截断状态明确，路径类型中立，HardScope在候选上限前执行。

### [x] P5-005｜RRF与trace通用化

状态：`done`；批次：`P5-A`；优先级：`normal`。
范围：`crates/cc-search/src/fusion.rs`；`crates/cc-search/src/score_trace.rs`
硬依赖：P5-004
步骤：移用现有rrf；按lane固定序和doc去重融合
交付物：RRF与trace通用化的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V03, V11 验证证据（实施时生成）
验收：不混原始量纲，NaN/重复候选有防护，trace可回放；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V03；V11
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "status": "partial_not_accepted", "artifacts": ["docs/roadmap/code-index-v2/P5-A-PROGRESS.md", "artifacts/benchmarks/p5a-20260929-resume/final/validation.json"], "limitations": "完整V11仍失败；不得从已有实现推定验收完成。"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "covered_source_digest_sha256": "cdc8a168987e005fdf8f36539525343c2531ba90e4b282938117642663919c8a", "status": "passed_declared_local_scope", "run_id": "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2", "artifacts": ["artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/validation.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/paired/summary.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/closure-audit.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/cost-summary.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/source-manifest.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/accepted-source.tar.gz", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/completeness-review.json", "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/runner-restoration.json", "docs/roadmap/code-index-v2/P5-A-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P5-A-GATE.json"], "commands_receipt": "artifacts/benchmarks/p5a-20260929-cache-fix/final-v2/validation.json", "review": "592-file frozen membership/content and archive audited; dual toolchain regressions, version/scope/score negative cases, 14-tool input compatibility, 306 paired requests and 60 release observations verified", "rollback_status": "prior accepted P4-D source archive preserved; current schema remains21; local sources only, no deployment or daily-index mutation", "limitations": "P5-A local macOS arm64 scope only; not G5/M2 or a release certificate; S11 no-answer failure retained; no gold or scoring-formula changes; Three-attempt index/evidence fence is not filesystem atomicity or rebuild-incarnation safety; No QueryHandle, cross-request bounded executor, deadlines/cancellation or semantic provider in this batch; 60 in-process local cost observations are not holdout, 100k, peak RSS, p95/p99 or cross-platform certification; Full retrieval-completeness gates remain failed: 42 source and 12 intent candidate requests are explicitly Partial; 75 source reason occurrences and 12 intent occurrences reviewed. S11 also remains. No full green retrieval claim."}]
实施备注：rank-only RRF按固定lane序与文档版本融合，拒绝重复票/NaN/版本冲突；score trace可回放，候选到hydration版本受检查，三入口持续变化返回可重试错误。

### [x] P5-006｜QueryPolicy与预算规划

状态：`done`；批次：`P5-B`；优先级：`normal`。
范围：`crates/cc-search/src/query_policy.rs`；`crates/cc-search/src/plan.rs`
硬依赖：P4-020, P5-001, P5-005
步骤：冻结local/auto/semantic、intent与子预算；明确disabled路线
交付物：QueryPolicy与预算规划的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V18 验证证据（实施时生成）
验收：默认local不探测网络或等待未配置服务；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V18
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "status": "development_passed_final_validation_in_progress", "artifacts": ["artifacts/benchmarks/p5b-20260929-query-execution/development/integration-r3.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v1/validation.json", "docs/internals/QUERY_EXECUTION.md"], "limitations": "开发期通过不代表最终冻结验收完成；G5/M2和完整检索认证未完成。"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "worktree_digest": "a6d20460f180c774eade845e241c22da42c045d183d5926ea02674a67b2bc8d9", "status": "passed_declared_local_scope", "run_id": "final-v3", "artifacts": ["artifacts/benchmarks/p5b-20260929-query-execution/final-v3/validation.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/paired/summary.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/closure-audit.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/cost-summary.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/evidence-classification.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/source-review.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/source-manifest.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/source.tar.gz", "docs/internals/QUERY_EXECUTION.md", "docs/roadmap/code-index-v2/P5-B-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P5-B-GATE.json"], "commands_receipt": "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/validation.json", "review": "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/closure-audit.json", "rollback_status": "source archive retained; schema unchanged; no live rollback or daily index mutation", "limitations": ["Local macOS arm64 implementation/contract/regression scope, not G5/M2 or release certification", "Retained Partial and S11 gates are not green; fixed answer/scoring data unchanged", "Query clock begins at admission/capture; preceding cold ProjectSession loading is not covered", "Synchronous SQL or noncooperative work cannot be force-stopped; permits stay owned until actual exit", "Optional port is a fake-tested interface; no real provider, embeddings, vectors or semantic epoch", "Pure captured search survives idle close; optional legacy graph-source expansion still uses short locks and rejects changed DB instance", "No persistent incarnation, filesystem snapshot, holdout, 100k, peak RSS, tail latency or cross-platform certification", "Before project configuration can be captured, requests without supplied QueryControl use a provisional 600-second admission cap; shorter configured budgets are applied from the original start after capture, not enforced before that unknown configuration is available."]}]
实施备注：QueryPolicy统一local/auto/semantic、intent召回职责与总/子预算；配置和intent进入缓存域，未配置auto等价local，显式semantic报不可用。既有Partial/S11不隐藏，无答案及selector完整校准仍由P5后续承担。

### [x] P5-007｜提取无锁QueryHandle

状态：`done`；批次：`P5-B`；优先级：`normal`。
范围：`crates/cc-server/src/query_handle.rs`；`crates/cc-server/src/handlers/context.rs`
硬依赖：P5-006
步骤：短锁复制Arc上下文；跨网络前释放CodeIndex guard和连接
交付物：提取无锁QueryHandle的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11 验证证据（实施时生成）
验收：fake慢provider期间index/status仍可进展；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "status": "development_passed_final_validation_in_progress", "artifacts": ["artifacts/benchmarks/p5b-20260929-query-execution/development/integration-r3.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v1/validation.json", "docs/internals/QUERY_EXECUTION.md"], "limitations": "开发期通过不代表最终冻结验收完成；G5/M2和完整检索认证未完成。"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "worktree_digest": "a6d20460f180c774eade845e241c22da42c045d183d5926ea02674a67b2bc8d9", "status": "passed_declared_local_scope", "run_id": "final-v3", "artifacts": ["artifacts/benchmarks/p5b-20260929-query-execution/final-v3/validation.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/paired/summary.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/closure-audit.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/cost-summary.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/evidence-classification.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/source-review.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/source-manifest.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/source.tar.gz", "docs/internals/QUERY_EXECUTION.md", "docs/roadmap/code-index-v2/P5-B-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P5-B-GATE.json"], "commands_receipt": "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/validation.json", "review": "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/closure-audit.json", "rollback_status": "source archive retained; schema unchanged; no live rollback or daily index mutation", "limitations": ["Local macOS arm64 implementation/contract/regression scope, not G5/M2 or release certification", "Retained Partial and S11 gates are not green; fixed answer/scoring data unchanged", "Query clock begins at admission/capture; preceding cold ProjectSession loading is not covered", "Synchronous SQL or noncooperative work cannot be force-stopped; permits stay owned until actual exit", "Optional port is a fake-tested interface; no real provider, embeddings, vectors or semantic epoch", "Pure captured search survives idle close; optional legacy graph-source expansion still uses short locks and rejects changed DB instance", "No persistent incarnation, filesystem snapshot, holdout, 100k, peak RSS, tail latency or cross-platform certification", "Before project configuration can be captured, requests without supplied QueryControl use a provisional 600-second admission cap; shorter configured budgets are applied from the original start after capture, not enforced before that unknown configuration is available."]}]
实施备注：Arc-owned QueryHandle接入MCP search/context，短锁只复制捕获的项目/DB/engine/services；慢fake期间index/status及单读连接可用。纯检索保活idle close；附加旧图源码扩展仍短锁并校验实例，未声称所有图工具无锁。

### [x] P5-008｜建立有界执行器

状态：`done`；批次：`P5-B`；优先级：`normal`。
范围：`crates/cc-search/src/execution.rs`；`crates/cc-server/src/service_factory.rs`
硬依赖：P5-007
步骤：CPU/DB任务与async端口分开调度；避免每请求无界开线程
交付物：建立有界执行器的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V20 验证证据（实施时生成）
验收：高并发线程数/queue受控，1连接池无死锁；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V20
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "status": "development_passed_final_validation_in_progress", "artifacts": ["artifacts/benchmarks/p5b-20260929-query-execution/development/integration-r3.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v1/validation.json", "docs/internals/QUERY_EXECUTION.md"], "limitations": "开发期通过不代表最终冻结验收完成；G5/M2和完整检索认证未完成。"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "worktree_digest": "a6d20460f180c774eade845e241c22da42c045d183d5926ea02674a67b2bc8d9", "status": "passed_declared_local_scope", "run_id": "final-v3", "artifacts": ["artifacts/benchmarks/p5b-20260929-query-execution/final-v3/validation.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/paired/summary.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/closure-audit.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/cost-summary.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/evidence-classification.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/source-review.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/source-manifest.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/source.tar.gz", "docs/internals/QUERY_EXECUTION.md", "docs/roadmap/code-index-v2/P5-B-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P5-B-GATE.json"], "commands_receipt": "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/validation.json", "review": "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/closure-audit.json", "rollback_status": "source archive retained; schema unchanged; no live rollback or daily index mutation", "limitations": ["Local macOS arm64 implementation/contract/regression scope, not G5/M2 or release certification", "Retained Partial and S11 gates are not green; fixed answer/scoring data unchanged", "Query clock begins at admission/capture; preceding cold ProjectSession loading is not covered", "Synchronous SQL or noncooperative work cannot be force-stopped; permits stay owned until actual exit", "Optional port is a fake-tested interface; no real provider, embeddings, vectors or semantic epoch", "Pure captured search survives idle close; optional legacy graph-source expansion still uses short locks and rejects changed DB instance", "No persistent incarnation, filesystem snapshot, holdout, 100k, peak RSS, tail latency or cross-platform certification", "Before project configuration can be captured, requests without supplied QueryControl use a provisional 600-second admission cap; shorter configured budgets are applied from the original start after capture, not enforced before that unknown configuration is available."]}]
实施备注：进程共享CPU4/queue32、async8/queue32，固定4线程本地lane池；先准入再spawn，运行闭包持permit至真实退出。3轮64请求各36准入/28拒绝，peak<=4且恢复零占用；不称全进程线程或吞吐认证。

### [x] P5-009｜统一deadline与取消

状态：`done`；批次：`P5-B`；优先级：`normal`。
范围：`crates/cc-search/src/execution.rs`；`crates/cc-server/src/mcp.rs`
硬依赖：P5-008
步骤：总deadline下发各lane；阻塞任务无法中断时限制并拒绝迟到发布
交付物：统一deadline与取消的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11 验证证据（实施时生成）
验收：请求取消后无永久后台积压，迟到结果不进正常缓存；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "status": "development_passed_final_validation_in_progress", "artifacts": ["artifacts/benchmarks/p5b-20260929-query-execution/development/integration-r3.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v1/validation.json", "docs/internals/QUERY_EXECUTION.md"], "limitations": "开发期通过不代表最终冻结验收完成；G5/M2和完整检索认证未完成。"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "worktree_digest": "a6d20460f180c774eade845e241c22da42c045d183d5926ea02674a67b2bc8d9", "status": "passed_declared_local_scope", "run_id": "final-v3", "artifacts": ["artifacts/benchmarks/p5b-20260929-query-execution/final-v3/validation.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/paired/summary.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/closure-audit.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/cost-summary.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/evidence-classification.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/source-review.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/source-manifest.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/source.tar.gz", "docs/internals/QUERY_EXECUTION.md", "docs/roadmap/code-index-v2/P5-B-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P5-B-GATE.json"], "commands_receipt": "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/validation.json", "review": "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/closure-audit.json", "rollback_status": "source archive retained; schema unchanged; no live rollback or daily index mutation", "limitations": ["Local macOS arm64 implementation/contract/regression scope, not G5/M2 or release certification", "Retained Partial and S11 gates are not green; fixed answer/scoring data unchanged", "Query clock begins at admission/capture; preceding cold ProjectSession loading is not covered", "Synchronous SQL or noncooperative work cannot be force-stopped; permits stay owned until actual exit", "Optional port is a fake-tested interface; no real provider, embeddings, vectors or semantic epoch", "Pure captured search survives idle close; optional legacy graph-source expansion still uses short locks and rejects changed DB instance", "No persistent incarnation, filesystem snapshot, holdout, 100k, peak RSS, tail latency or cross-platform certification", "Before project configuration can be captured, requests without supplied QueryControl use a provisional 600-second admission cap; shorter configured budgets are applied from the original start after capture, not enforced before that unknown configuration is available."]}]
实施备注：查询准入起总deadline贯穿等待、端口、三次epoch重试、CPU组装及JSON转换；MCP取消和Future Drop下传控制，结果/正文缓存发布受短栅栏保护。不可中断SQL不伪称立即终止；冷项目路由阶段不纳入此声明。

### [x] P5-010｜可选语义端口和fake适配

状态：`done`；批次：`P5-B`；优先级：`normal`。
范围：`crates/cc-model/src/semantic.rs`；`crates/cc-search/src/lanes/semantic_adapter.rs`
硬依赖：P5-009
步骤：定义SemanticRecall不依赖provider；fake注入timeout/partial
交付物：可选语义端口和fake适配的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V18 验证证据（实施时生成）
验收：cc-search不编译依赖cc-semantic，端口缺失默认正常；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V18
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "status": "development_passed_final_validation_in_progress", "artifacts": ["artifacts/benchmarks/p5b-20260929-query-execution/development/integration-r3.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v1/validation.json", "docs/internals/QUERY_EXECUTION.md"], "limitations": "开发期通过不代表最终冻结验收完成；G5/M2和完整检索认证未完成。"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "worktree_digest": "a6d20460f180c774eade845e241c22da42c045d183d5926ea02674a67b2bc8d9", "status": "passed_declared_local_scope", "run_id": "final-v3", "artifacts": ["artifacts/benchmarks/p5b-20260929-query-execution/final-v3/validation.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/paired/summary.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/closure-audit.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/cost-summary.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/evidence-classification.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/source-review.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/source-manifest.json", "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/source.tar.gz", "docs/internals/QUERY_EXECUTION.md", "docs/roadmap/code-index-v2/P5-B-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P5-B-GATE.json"], "commands_receipt": "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/validation.json", "review": "artifacts/benchmarks/p5b-20260929-query-execution/final-v3/closure-audit.json", "rollback_status": "source archive retained; schema unchanged; no live rollback or daily index mutation", "limitations": ["Local macOS arm64 implementation/contract/regression scope, not G5/M2 or release certification", "Retained Partial and S11 gates are not green; fixed answer/scoring data unchanged", "Query clock begins at admission/capture; preceding cold ProjectSession loading is not covered", "Synchronous SQL or noncooperative work cannot be force-stopped; permits stay owned until actual exit", "Optional port is a fake-tested interface; no real provider, embeddings, vectors or semantic epoch", "Pure captured search survives idle close; optional legacy graph-source expansion still uses short locks and rejects changed DB instance", "No persistent incarnation, filesystem snapshot, holdout, 100k, peak RSS, tail latency or cross-platform certification", "Before project configuration can be captured, requests without supplied QueryControl use a provisional 600-second admission cap; shorter configured budgets are applied from the original start after capture, not enforced before that unknown configuration is available."]}]
实施备注：无provider依赖SemanticRecall端口/fake接入现有rank-only融合；scope/version/span/NaN/重复/假exact和过多候选拒收，timeout/partial/error/panic显式降级，语义结果不进普通结果缓存。无真实模型/vector认证。

### [x] P5-011｜generation读面与缓存守卫

状态：`done`；批次：`P5-C`；优先级：`normal`。
范围：`crates/cc-model/src/generation.rs`；`crates/cc-search/src/engine_cache.rs`
硬依赖：P4-020, P5-007, P5-010
步骤：incarnation/epoch/policy/spec入key；读前后验证和有限重试
交付物：generation读面与缓存守卫的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V13 验证证据（实施时生成）
验收：持续写入下不缓存混代结果，重试耗尽可见；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V13
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "status": "development_only_not_accepted", "artifacts": ["docs/roadmap/code-index-v2/P5-C-PROGRESS.md", "artifacts/benchmarks/p5c-20260929-evidence-assembly/development/focused-v2.log"], "limitations": "最新代码未冻结验收；预算红测保留；P5-015被拦截未落地。"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "worktree_digest": "0a1a1a2fbe857303304a75d0bc1797c0169c2e2d2e0d93b0a464ff0539062585", "status": "passed_declared_local_subset", "run_id": "final-subset-v2", "artifacts": ["artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/validation.json", "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/paired/summary.json", "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/closure-audit.json", "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/source-review.json", "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/source-manifest.json", "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/source.tar.gz", "docs/roadmap/code-index-v2/P5-C-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P5-C-PARTIAL-GATE.json"], "commands_receipt": "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/validation.json", "review": "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/closure-audit.json", "rollback_status": "source archive retained; no live rollback or daily index mutation", "limitations": ["P5-011..013 local implementation/contract/regression scope only; P5-C incomplete.", "P5-014 prototype is not enabled in default responses; five source Top-1 regressions preserved in development/pair-v2; two integration tests remain pending.", "P5-015 final hydrator write was blocked and did not execute; existing SourceVerifier is unchanged.", "A diagnostic tool call was also blocked; no alternate-route retry.", "Original full retrieval Partial and S11 gates remain failed.", "No 100k, holdout, tail latency, cross-platform, live provider, filesystem atomicity or G5/M2/release certification."]}]
实施备注：持久化incarnation与ReadGeneration已落地，正常重开保持、staging换库更新；缓存与读前后核验及可选语义依据纳入代际；非查询心跳不改变读代际。

### [x] P5-012｜selector任务覆盖模型

状态：`done`；批次：`P5-C`；优先级：`normal`。
范围：`crates/cc-search/src/selection/coverage.rs`；`crates/cc-model/src/context.rs`
硬依赖：P5-011
步骤：locate与change/trace使用不同facet配额；相关性排序和集合选择分开
交付物：selector任务覆盖模型的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V12, V19 验证证据（实施时生成）
验收：不为多样性强行排除最重要单文件证据；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V12；V19
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "status": "development_only_not_accepted", "artifacts": ["docs/roadmap/code-index-v2/P5-C-PROGRESS.md", "artifacts/benchmarks/p5c-20260929-evidence-assembly/development/focused-v2.log"], "limitations": "最新代码未冻结验收；预算红测保留；P5-015被拦截未落地。"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "worktree_digest": "0a1a1a2fbe857303304a75d0bc1797c0169c2e2d2e0d93b0a464ff0539062585", "status": "passed_declared_local_subset", "run_id": "final-subset-v2", "artifacts": ["artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/validation.json", "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/paired/summary.json", "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/closure-audit.json", "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/source-review.json", "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/source-manifest.json", "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/source.tar.gz", "docs/roadmap/code-index-v2/P5-C-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P5-C-PARTIAL-GATE.json"], "commands_receipt": "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/validation.json", "review": "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/closure-audit.json", "rollback_status": "source archive retained; no live rollback or daily index mutation", "limitations": ["P5-011..013 local implementation/contract/regression scope only; P5-C incomplete.", "P5-014 prototype is not enabled in default responses; five source Top-1 regressions preserved in development/pair-v2; two integration tests remain pending.", "P5-015 final hydrator write was blocked and did not execute; existing SourceVerifier is unchanged.", "A diagnostic tool call was also blocked; no alternate-route retry.", "Original full retrieval Partial and S11 gates remain failed.", "No 100k, holdout, tail latency, cross-platform, live provider, filesystem atomicity or G5/M2/release certification."]}]
实施备注：独立selector接入原有bounded rerank window，保留最高排名锚点并按intent补测试/接口facet，输出仍按原排名且不改score trace。

### [x] P5-013｜重复/重叠证据抑制

状态：`done`；批次：`P5-C`；优先级：`normal`。
范围：`crates/cc-search/src/selection/overlap.rs`
硬依赖：P5-012
步骤：同source version byte spans去重；区别相同文本不同文件来源
交付物：重复/重叠证据抑制的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V12 验证证据（实施时生成）
验收：不会因去重丢失跨语言接口两端，重复率可复算；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V12
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "status": "development_only_not_accepted", "artifacts": ["docs/roadmap/code-index-v2/P5-C-PROGRESS.md", "artifacts/benchmarks/p5c-20260929-evidence-assembly/development/focused-v2.log"], "limitations": "最新代码未冻结验收；预算红测保留；P5-015被拦截未落地。"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "worktree_digest": "0a1a1a2fbe857303304a75d0bc1797c0169c2e2d2e0d93b0a464ff0539062585", "status": "passed_declared_local_subset", "run_id": "final-subset-v2", "artifacts": ["artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/validation.json", "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/paired/summary.json", "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/closure-audit.json", "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/source-review.json", "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/source-manifest.json", "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/source.tar.gz", "docs/roadmap/code-index-v2/P5-C-IMPLEMENTATION.md", "docs/roadmap/code-index-v2/P5-C-PARTIAL-GATE.json"], "commands_receipt": "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/validation.json", "review": "artifacts/benchmarks/p5c-20260929-evidence-assembly/final-subset-v2/closure-audit.json", "rollback_status": "source archive retained; no live rollback or daily index mutation", "limitations": ["P5-011..013 local implementation/contract/regression scope only; P5-C incomplete.", "P5-014 prototype is not enabled in default responses; five source Top-1 regressions preserved in development/pair-v2; two integration tests remain pending.", "P5-015 final hydrator write was blocked and did not execute; existing SourceVerifier is unchanged.", "A diagnostic tool call was also blocked; no alternate-route retry.", "Original full retrieval Partial and S11 gates remain failed.", "No 100k, holdout, tail latency, cross-platform, live provider, filesystem atomicity or G5/M2/release certification."]}]
实施备注：按文件及source snapshot计算byte区间并集；只由实际选中的来源抑制重复，跨文件或版本的相同文本不合并。输入/选中冗余与名额省略分别可复算。

### [x] P5-014｜结构化BudgetPacker

状态：`done`；批次：`P5-C`；优先级：`normal`。
范围：`crates/cc-search/src/selection/budget.rs`；`crates/cc-server/src/handlers/output_budget.rs`
硬依赖：P5-013
步骤：预算包含JSON/metadata；过大节点降为合法slice/outline/ref
交付物：结构化BudgetPacker的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V12, V18 验证证据（实施时生成）
验收：首项超大也受限，最终响应不是截断JSON前缀；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V12；V18
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "status": "development_only_not_accepted", "artifacts": ["docs/roadmap/code-index-v2/P5-C-PROGRESS.md", "artifacts/benchmarks/p5c-20260929-evidence-assembly/development/focused-v2.log"], "limitations": "最新代码未冻结验收；预算红测保留；P5-015被拦截未落地。"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "status": "not_accepted", "artifacts": ["docs/roadmap/code-index-v2/P5-C-PROGRESS.md", "artifacts/benchmarks/p5c-20260929-evidence-assembly/tool-blockers.json", "artifacts/benchmarks/p5c-20260929-evidence-assembly/development/pair-v2/summary.json", "artifacts/benchmarks/p5c-20260929-evidence-assembly/development/pair-v3-rollback/summary.json"], "limitations": "本项未完成；通过的011-013子集不能替代本项验收。"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "status": "development_passed_pending_frozen_acceptance", "artifacts": ["artifacts/benchmarks/p5c-20260929-completion/development/legacy-94-v5.log", "artifacts/benchmarks/p5c-20260929-completion/development/assembly-v6.log", "artifacts/benchmarks/p5c-20260929-completion/development/pair-v1/summary.json", "docs/internals/EVIDENCE_ASSEMBLY.md"], "limitations": "本条不是完成Gate；完整性/无答案/尾延迟/跨平台/真实provider仍未认证。"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "worktree_digest": "471919b1d73828db2938d314e3319dd34827a7c1828388925c45b1db7372f473", "status": "passed_declared_local_implementation_scope", "run_id": "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930", "artifacts": ["artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/validation.json", "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/paired/summary.json", "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/closure-audit.json", "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/cost-summary.json", "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/evidence-classification.json", "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/source-review.json", "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/wire-budget-audit.json", "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/source-manifest.json", "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/source.tar.gz", "docs/roadmap/code-index-v2/P5-C-GATE.json", "docs/roadmap/code-index-v2/P5-C-COMPLETION.md", "docs/internals/EVIDENCE_ASSEMBLY.md"], "commands_receipt": "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/validation.json", "limitations": ["Local P5-C implementation/contract/ranking/source/bounded-output scope; not G5/M2 or release certification.", "Full retrieval gates still fail: prior Partial/S11 and explicitly counted new budget omissions. No completeness-green claim.", "Budget covers the compact code_index_context result object, not JSON-RPC framing or other legacy tool shapes.", "References and outlines have no source body and are never scored as source hits. File retrieval reasons are diagnostics, not exact identity.", "Optimistic generation and per-file disk checks are not an atomic filesystem snapshot. Three outer attempts, bounded inner retries, same original deadline.", "No provider/vector publication, persistent semantic epoch, holdout, 100k, peak RSS, tail-latency or cross-platform certification.", "No commit, push, PR, merge or daily-index mutation. Inherited changes and prior failed evidence are retained."]}]
实施备注：默认完整JSON预算已恢复并验收：正文优先，文档/文件引用仅占剩余空间，有界召回诊断不冒充精确身份；真实stdio和原94用例通过。新增真实预算Partial单独记录，不宣称完整检索全绿。

### [x] P5-015｜EvidenceHydrator与最终验证

状态：`done`；批次：`P5-C`；优先级：`normal`。
范围：`crates/cc-search/src/evidence.rs`；`crates/cc-server/src/symbol_extract.rs`
硬依赖：P5-014
步骤：验证scope/manifest/source hash/span；按状态处理旧快照
交付物：EvidenceHydrator与最终验证的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V09, V11, V12 验证证据（实施时生成）
验收：任何lane都不能绕开来源和范围检查；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V09；V11；V12
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "status": "development_only_not_accepted", "artifacts": ["docs/roadmap/code-index-v2/P5-C-PROGRESS.md", "artifacts/benchmarks/p5c-20260929-evidence-assembly/development/focused-v2.log"], "limitations": "最新代码未冻结验收；预算红测保留；P5-015被拦截未落地。"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "status": "not_accepted", "artifacts": ["docs/roadmap/code-index-v2/P5-C-PROGRESS.md", "artifacts/benchmarks/p5c-20260929-evidence-assembly/tool-blockers.json", "artifacts/benchmarks/p5c-20260929-evidence-assembly/development/pair-v2/summary.json", "artifacts/benchmarks/p5c-20260929-evidence-assembly/development/pair-v3-rollback/summary.json"], "limitations": "本项未完成；通过的011-013子集不能替代本项验收。"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "status": "development_passed_pending_frozen_acceptance", "artifacts": ["artifacts/benchmarks/p5c-20260929-completion/development/legacy-94-v5.log", "artifacts/benchmarks/p5c-20260929-completion/development/assembly-v6.log", "artifacts/benchmarks/p5c-20260929-completion/development/pair-v1/summary.json", "docs/internals/EVIDENCE_ASSEMBLY.md"], "limitations": "本条不是完成Gate；完整性/无答案/尾延迟/跨平台/真实provider仍未认证。"}, {"target_sha": "4514630dcd26481cf6dbc2aff38824ed71ef06da", "worktree_digest": "471919b1d73828db2938d314e3319dd34827a7c1828388925c45b1db7372f473", "status": "passed_declared_local_implementation_scope", "run_id": "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930", "artifacts": ["artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/validation.json", "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/paired/summary.json", "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/closure-audit.json", "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/cost-summary.json", "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/evidence-classification.json", "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/source-review.json", "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/wire-budget-audit.json", "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/source-manifest.json", "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/source.tar.gz", "docs/roadmap/code-index-v2/P5-C-GATE.json", "docs/roadmap/code-index-v2/P5-C-COMPLETION.md", "docs/internals/EVIDENCE_ASSEMBLY.md"], "commands_receipt": "artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/validation.json", "limitations": ["Local P5-C implementation/contract/ranking/source/bounded-output scope; not G5/M2 or release certification.", "Full retrieval gates still fail: prior Partial/S11 and explicitly counted new budget omissions. No completeness-green claim.", "Budget covers the compact code_index_context result object, not JSON-RPC framing or other legacy tool shapes.", "References and outlines have no source body and are never scored as source hits. File retrieval reasons are diagnostics, not exact identity.", "Optimistic generation and per-file disk checks are not an atomic filesystem snapshot. Three outer attempts, bounded inner retries, same original deadline.", "No provider/vector publication, persistent semantic epoch, holdout, 100k, peak RSS, tail-latency or cross-platform certification.", "No commit, push, PR, merge or daily-index mutation. Inherited changes and prior failed evidence are retained."]}]
实施备注：实际组合根接入EvidenceHydrator，批量核对完整manifest、scope、原字节/跨度/显示坐标和持久化代际，暖缓存损坏与取消负例通过。复用symbol_extract已有名称提取，不伪造该规划路径的源码改动。

### [ ] P5-016｜诊断与能力状态收口

状态：`in_progress`；批次：`P5-D`；优先级：`normal`。
范围：`crates/cc-server/src/capability_status.rs`；`crates/cc-model/src/context.rs`
硬依赖：P4-020, P5-015
步骤：复用GraphExplain/BuildExplain；增加lane/freshness/coverage状态
交付物：诊断与能力状态收口的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V18 验证证据（实施时生成）
验收：不会出现status ready但所有dense因未配置跳过的假象；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V18
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：尚无
实施备注：实现已进入冻结复验，但仍未验收完成：final-v3的Rust 1.95 workspace退出101，project_session::tests::close_idle_instances_closes_cached_non_active_projects预期关闭2实例、实际1；audit.json未生成。保持in_progress，不以stable通过替代最低工具链验收。轻量原始收据见artifacts/checkpoints/20260930-git-sync/p5d-final-v3/；P5-019/020保持todo，原完整性失败保留。

### [ ] P5-017｜项目驱逐与查询资源生命周期

状态：`in_progress`；批次：`P5-D`；优先级：`normal`。
范围：`crates/cc-server/src/project_session.rs`；`crates/cc-server/src/query_handle.rs`
硬依赖：P5-016
步骤：pin活跃查询；idle close有界且不持网络锁
交付物：项目驱逐与查询资源生命周期的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V20 验证证据（实施时生成）
验收：LRU驱逐不中断合法inflight，释放后资源可回收；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V20
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：尚无
实施备注：实现已进入冻结复验，但仍未验收完成：final-v3的Rust 1.95 workspace退出101，project_session::tests::close_idle_instances_closes_cached_non_active_projects预期关闭2实例、实际1；audit.json未生成。保持in_progress，不以stable通过替代最低工具链验收。轻量原始收据见artifacts/checkpoints/20260930-git-sync/p5d-final-v3/；P5-019/020保持todo，原完整性失败保留。

### [ ] P5-018｜MCP旧新契约和文档一体迁移

状态：`in_progress`；批次：`P5-D`；优先级：`normal`。
范围：`crates/cc-server/src/tools.rs`；`docs/MCP_TOOLS.md`；`docs/CONFIGURATION.md`
硬依赖：P5-017
步骤：新策略走schema到handler全链；保留旧mode与错误形态
交付物：MCP旧新契约和文档一体迁移的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V18, V21 验证证据（实施时生成）
验收：14工具旧调用回归过，未实现字段不先对外广告；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V18；V21
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：尚无
实施备注：实现已进入冻结复验，但仍未验收完成：final-v3的Rust 1.95 workspace退出101，project_session::tests::close_idle_instances_closes_cached_non_active_projects预期关闭2实例、实际1；audit.json未生成。保持in_progress，不以stable通过替代最低工具链验收。轻量原始收据见artifacts/checkpoints/20260930-git-sync/p5d-final-v3/；P5-019/020保持todo，原完整性失败保留。

### [ ] P5-019｜查询质量/成本/并发消融

状态：`todo`；批次：`P5-D`；优先级：`normal`。
范围：`crates/cc-eval/src/benchmark/ablation.rs`；`artifacts/benchmarks/`
硬依赖：P5-018
步骤：独立比较path/exact/selector；混合构建下测延迟与线程
交付物：查询质量/成本/并发消融的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V19, V20 验证证据（实施时生成）
验收：无best-of，性能提升不靠削掉所需facet；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V19；V20
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：尚无
实施备注：P5-A交接：完整性门禁未通过，source42/intents12请求为Partial，原因见P5-A-GATE与completeness-review.json；须明确lane需求/子预算及图源码映射覆盖，保留S11，不以压掉状态或查询ID特判修绿。

### [ ] P5-020｜P5本地增强版验收

状态：`todo`；批次：`P5-D`；优先级：`blocking`。
范围：`docs/roadmap/code-index-v2/`；`artifacts/benchmarks/`
硬依赖：P5-001, P5-002, P5-003, P5-004, P5-005, P5-006, P5-007, P5-008, P5-009, P5-010, P5-011, P5-012, P5-013, P5-014, P5-015, P5-016, P5-017, P5-018, P5-019
步骤：G5冻结source/配置和public schema；形成M2本地版本证据
交付物：P5本地增强版验收的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V12, V18, V19, V20 验证证据（实施时生成）
验收：不需要真实embedding即可交付，待实现semantic仅显示disabled；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V12；V18；V19；V20
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：尚无
实施备注：P5-A交接：完整性门禁未通过，source42/intents12请求为Partial，原因见P5-A-GATE与completeness-review.json；须明确lane需求/子预算及图源码映射覆盖，保留S11，不以压掉状态或查询ID特判修绿。

## P6｜语义持久化与发布底座

### [ ] P6-001｜正式确定单库边界修订ADR

状态：`todo`；批次：`P6-A`；优先级：`normal`。
范围：`docs/adr/`；`DESIGN.md`；`docs/internals/STORAGE.md`
硬依赖：P5-020
步骤：审定权威index与派生cache职责；说明队列可靠性数据不是通用runtime
交付物：正式确定单库边界修订ADR的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V21 验证证据（实施时生成）
验收：修改章程有明确理由、默认仍单库且无隐式新服务；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V21
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

### [ ] P6-002｜新增可选cc-semantic骨架

状态：`todo`；批次：`P6-A`；优先级：`normal`。
范围：`crates/cc-semantic/Cargo.toml`；`Cargo.toml`；`crates/cc-server/Cargo.toml`
硬依赖：P6-001
步骤：只依赖cc-model/cc-db；feature和组合根延迟初始化
交付物：新增可选cc-semantic骨架的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V18, V21 验证证据（实施时生成）
验收：默认编译/启动不拉网络模型实现、不生成空缓存；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V18；V21
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

### [ ] P6-003｜冻结编码空间与输入规范

状态：`todo`；批次：`P6-A`；优先级：`normal`。
范围：`crates/cc-semantic/src/spec.rs`；`crates/cc-model/src/identity.rs`
硬依赖：P6-002
步骤：区分VectorSpace/DocumentEncoding/QueryEncoding；定义完整digest
交付物：冻结编码空间与输入规范的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V10, V16 验证证据（实施时生成）
验收：同维度不同模型不能混用，query-only变化不必重嵌文档；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V10；V16
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

### [ ] P6-004｜扩展类型化write effects

状态：`todo`；批次：`P6-A`；优先级：`normal`。
范围：`crates/cc-db/src/epoch_rules.rs`；`crates/cc-db/src/unit_of_work.rs`
硬依赖：P6-003
步骤：Index/Evidence/Semantic/Auxiliary封闭枚举；默认旧行为保留
交付物：扩展类型化write effects的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V13 验证证据（实施时生成）
验收：heartbeat不刷index，commit/rollback恰好推进预期epoch；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V13
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

### [ ] P6-005｜新表与schema初始化

状态：`todo`；批次：`P6-A`；优先级：`normal`。
范围：`crates/cc-db/src/sql/`；`crates/cc-db/src/index_migrate.rs`
硬依赖：P6-004
步骤：加入document/manifest/outbox所需表与索引；按发布节点合并schema版本
交付物：新表与schema初始化的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V13, V21 验证证据（实施时生成）
验收：新旧DB有明确重建路径，FTS旧数据不半升级；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V13；V21
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

### [ ] P6-006｜源码事务原子写outbox

状态：`todo`；批次：`P6-B`；优先级：`normal`。
范围：`crates/cc-index/src/documents/delta.rs`；`crates/cc-db/src/semantic_outbox.rs`
硬依赖：P5-020, P6-004, P6-005
步骤：文档变化撤旧manifest并写desired任务；删除不发embedding
交付物：源码事务原子写outbox的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V13, V14 验证证据（实施时生成）
验收：提交后不会有新文档却无任务，rollback不泄露半个任务；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V13；V14
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

### [ ] P6-007｜实现claim与lease fencing

状态：`todo`；批次：`P6-B`；优先级：`normal`。
范围：`crates/cc-db/src/semantic_outbox.rs`；`crates/cc-semantic/src/queue.rs`
硬依赖：P6-006
步骤：短事务claim/renew/retry；每attempt独立token
交付物：实现claim与lease fencing的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V14 验证证据（实施时生成）
验收：两进程不能同时发布相同lease，过期worker无法ack新lease；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V14
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

### [ ] P6-008｜构建内容寻址artifact cache

状态：`todo`；批次：`P6-B`；优先级：`normal`。
范围：`crates/cc-semantic/src/cache.rs`；`crates/cc-semantic/src/spec.rs`
硬依赖：P6-007
步骤：按namespace+input/spec存validated vector与checksum；无秘密字段
交付物：构建内容寻址artifact cache的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V10, V16 验证证据（实施时生成）
验收：同输入可复用，跨项目默认隔离且cache损坏可检测；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V10；V16
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

### [ ] P6-009｜实现deterministic fake provider

状态：`todo`；批次：`P6-B`；优先级：`normal`。
范围：`crates/cc-semantic/src/providers/fake.rs`；`crates/cc-semantic/src/ports.rs`
硬依赖：P6-008
步骤：注入固定向量、延迟、次数和故障；不发送网络
交付物：实现deterministic fake provider的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V15 验证证据（实施时生成）
验收：测试可确定重现全部状态转移，fake结果不算真实语义质量；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V15
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

### [ ] P6-010｜实现filtered exact向量backend

状态：`todo`；批次：`P6-B`；优先级：`normal`。
范围：`crates/cc-semantic/src/vector/exact.rs`
硬依赖：P6-009
步骤：限定空间、范围先过滤、bounded batch、稳定top-k；保留数值gold
交付物：实现filtered exact向量backend的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V16 验证证据（实施时生成）
验收：手算cosine一致，删除/不同空间不可返回，内存受控；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V16
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

### [ ] P6-011｜artifact到manifest发布CAS

状态：`todo`；批次：`P6-C`；优先级：`normal`。
范围：`crates/cc-semantic/src/publish.rs`；`crates/cc-db/src/semantic_outbox.rs`
硬依赖：P5-020, P6-006, P6-007, P6-008, P6-010
步骤：先持久化artifact，再校验incarnation/lease/doc/input/space写manifest
交付物：artifact到manifest发布CAS的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V14 验证证据（实施时生成）
验收：慢旧结果不能挂到同路径新版本，发布幂等；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V14
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

### [ ] P6-012｜覆盖率与semantic epoch

状态：`todo`；批次：`P6-C`；优先级：`normal`。
范围：`crates/cc-server/src/capability_status.rs`；`crates/cc-db/src/epoch_rules.rs`
硬依赖：P6-011
步骤：统计eligible/published/failed/stale并分母明确；只可见集合变化bump
交付物：覆盖率与semantic epoch的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V13, V18 验证证据（实施时生成）
验收：零eligible有原因，aux重试不冲刷完整查询缓存；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V13；V18
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

### [ ] P6-013｜worker资源与连续编辑合并

状态：`todo`；批次：`P6-C`；优先级：`normal`。
范围：`crates/cc-semantic/src/worker.rs`；`crates/cc-semantic/src/admission.rs`
硬依赖：P6-012
步骤：pending合并、队列上限、公平批次和有界关闭；旧任务supersede
交付物：worker资源与连续编辑合并的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V14, V20 验证证据（实施时生成）
验收：大量保存不导致无限排队，任何时刻local查询可用；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V14；V20
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

### [ ] P6-014｜换库incarnation与缓存重用

状态：`todo`；批次：`P6-C`；优先级：`normal`。
范围：`crates/cc-db/src/index_db_rebuild.rs`；`crates/cc-semantic/src/reconcile.rs`
硬依赖：P6-013
步骤：staging重建更换身份；从artifact补manifest；旧worker fenced
交付物：换库incarnation与缓存重用的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V13, V17 验证证据（实施时生成）
验收：索引重建不误删已付费向量、不接受旧DB时代回包；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V13；V17
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

### [ ] P6-015｜崩溃恢复扫描

状态：`todo`；批次：`P6-C`；优先级：`normal`。
范围：`crates/cc-semantic/src/reconcile.rs`；`crates/cc-eval/tests/semantic_lifecycle.rs`
硬依赖：P6-014
步骤：对每个persist边界kill/restart；重认过期lease与缺失artifact
交付物：崩溃恢复扫描的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V17 验证证据（实施时生成）
验收：恢复有界且可复算，已存artifact优先复用；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V17
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

### [ ] P6-016｜GC与发布协调

状态：`todo`；批次：`P6-D`；优先级：`normal`。
范围：`crates/cc-semantic/src/cache.rs`；`crates/cc-semantic/src/publish.rs`
硬依赖：P5-020, P6-011, P6-015
步骤：mark/sweep、活跃引用/lease和最短保留期；GC跟publish共同协调
交付物：GC与发布协调的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V17 验证证据（实施时生成）
验收：没有manifest引用刚GC删除产物的竞态，孤儿最终可回收；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V17
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

### [ ] P6-017｜model space切换规划

状态：`todo`；批次：`P6-D`；优先级：`normal`。
范围：`crates/cc-semantic/src/spec.rs`；`crates/cc-semantic/src/reconcile.rs`
硬依赖：P6-016
步骤：新空间回填/切active/撤销；记录用户revision与未pin限制
交付物：model space切换规划的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V16, V17 验证证据（实施时生成）
验收：切换时不把不同空间分数混排，旧cache可回滚复用；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V16；V17
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

### [ ] P6-018｜cache缺失/损坏降级

状态：`todo`；批次：`P6-D`；优先级：`normal`。
范围：`crates/cc-semantic/src/cache.rs`；`crates/cc-server/src/capability_status.rs`
硬依赖：P6-017
步骤：隔离坏记录、语义degraded、本地继续；补嵌受费用策略控制
交付物：cache缺失/损坏降级的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V17, V18 验证证据（实施时生成）
验收：不把缺向量当完整空结果，不静默无界重费；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V17；V18
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

### [ ] P6-019｜更新存储/恢复/配置文档

状态：`todo`；批次：`P6-D`；优先级：`normal`。
范围：`docs/internals/STORAGE.md`；`docs/internals/CONCURRENCY.md`；`docs/TROUBLESHOOTING.md`
硬依赖：P6-018
步骤：写清at-least-once、两库顺序、恢复步骤与namespace
交付物：更新存储/恢复/配置文档的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V21 验证证据（实施时生成）
验收：不宣称跨模型/两库exactly-once或零重复收费；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V21
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

### [ ] P6-020｜P6无网络语义底座验收

状态：`todo`；批次：`P6-D`；优先级：`blocking`。
范围：`docs/roadmap/code-index-v2/`；`artifacts/benchmarks/`
硬依赖：P6-001, P6-002, P6-003, P6-004, P6-005, P6-006, P6-007, P6-008, P6-009, P6-010, P6-011, P6-012, P6-013, P6-014, P6-015, P6-016, P6-017, P6-018, P6-019
步骤：G6使用fake执行故障矩阵和exact oracle；检查依赖图/默认包
交付物：P6无网络语义底座验收的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V13, V14, V16, V17, V18 验证证据（实施时生成）
验收：publish/fencing/GC/rebuild闭环，仍未冒充真实provider效果；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V13；V14；V16；V17；V18
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：尚无

## P7｜provider与dense端到端

### [ ] P7-001｜实现OpenAI-compatible provider适配

状态：`todo`；批次：`P7-A`；优先级：`normal`。
范围：`crates/cc-semantic/src/providers/openai_compatible.rs`；`crates/cc-semantic/src/ports.rs`
硬依赖：P6-020
步骤：明确endpoint/base路径和认证；trait不暴露具体客户端类型
交付物：实现OpenAI-compatible provider适配的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V15 验证证据（实施时生成）
验收：协议stub覆盖成功/错误，provider可替换；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V15
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：尚无

### [ ] P7-002｜模型参数与能力验证

状态：`todo`；批次：`P7-A`；优先级：`normal`。
范围：`crates/cc-semantic/src/spec.rs`；`crates/cc-model/src/config.rs`
硬依赖：P7-001
步骤：模型revision、dimensions、metric、query instruction显式校验；支持差异不能吞
交付物：模型参数与能力验证的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V15, V18 验证证据（实施时生成）
验收：供应商不支持dimensions时给错误/配置路径，不伪成功；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V15；V18
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：尚无

### [ ] P7-003｜真实输入尺寸与批次规划

状态：`todo`；批次：`P7-A`；优先级：`normal`。
范围：`crates/cc-semantic/src/admission.rs`；`crates/cc-index/src/documents/render.rs`
硬依赖：P7-002
步骤：按最终输入计token/bytes/batch；超限重切或明确skip而非平均池化掩盖
交付物：真实输入尺寸与批次规划的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V09, V15 验证证据（实施时生成）
验收：每项和总batch受限，文本与向量所代表文档版本一致；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V09；V15
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：尚无

### [ ] P7-004｜响应强校验

状态：`todo`；批次：`P7-A`；优先级：`normal`。
范围：`crates/cc-semantic/src/providers/openai_compatible.rs`
硬依赖：P7-003
步骤：数量/index完整且唯一、dim、finite、norm检查；错误向量不缓存
交付物：响应强校验的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V15, V16 验证证据（实施时生成）
验收：乱序可正确恢复，重复index/NaN/zero必被拒绝；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V15；V16
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：尚无

### [ ] P7-005｜全局与项目并发限流

状态：`todo`；批次：`P7-A`；优先级：`normal`。
范围：`crates/cc-semantic/src/admission.rs`；`crates/cc-server/src/service_factory.rs`
硬依赖：P7-004
步骤：共享provider级限额和项目公平队列；避免每调用创建独立无限信号量
交付物：全局与项目并发限流的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V15, V20 验证证据（实施时生成）
验收：多项目总并发仍受限，单项目不能饿死其他索引；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V15；V20
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：尚无

### [ ] P7-006｜有界重试与断路器

状态：`todo`；批次：`P7-B`；优先级：`normal`。
范围：`crates/cc-semantic/src/providers/openai_compatible.rs`；`crates/cc-semantic/src/worker.rs`
硬依赖：P6-020, P7-001, P7-005
步骤：429/5xx/timeout退避与Retry-After；auth/永久错误暂停
交付物：有界重试与断路器的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V15 验证证据（实施时生成）
验收：重试次数、deadline和费用封顶，失败原因公开脱敏；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V15
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：尚无

### [ ] P7-007｜代码外发与凭据政策

状态：`todo`；批次：`P7-B`；优先级：`normal`。
范围：`crates/cc-semantic/src/policy.rs`；`crates/cc-model/src/config.rs`
硬依赖：P7-006
步骤：显式opt-in/敏感文件/endpoint协议/redirect；密钥只外部引用
交付物：代码外发与凭据政策的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V15, V18 验证证据（实施时生成）
验收：默认无网络、日志无key/源码、重定向不泄露认证；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V15；V18
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：尚无

### [ ] P7-008｜费用与不确定尝试收据

状态：`todo`；批次：`P7-B`；优先级：`normal`。
范围：`crates/cc-semantic/src/admission.rs`；`crates/cc-semantic/src/worker.rs`
硬依赖：P7-007
步骤：区分reported/estimated tokens、cache reuse和未知重复费用；停机阈值
交付物：费用与不确定尝试收据的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V15, V20 验证证据（实施时生成）
验收：无usage不填0费用，重启重试能看见费用不确定性；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V15；V20
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：尚无

### [ ] P7-009｜查询编码与缓存

状态：`todo`；批次：`P7-B`；优先级：`normal`。
范围：`crates/cc-semantic/src/spec.rs`；`crates/cc-semantic/src/cache.rs`
硬依赖：P7-008
步骤：实现QueryEncodingSpec键和有界query cache；instruction变更失效
交付物：查询编码与缓存的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V15 验证证据（实施时生成）
验收：不需无谓重嵌文档，不跨空间复用query向量；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V15
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：尚无

### [ ] P7-010｜dense召回端口接线

状态：`todo`；批次：`P7-B`；优先级：`normal`。
范围：`crates/cc-search/src/lanes/semantic_adapter.rs`；`crates/cc-server/src/service_factory.rs`
硬依赖：P7-009
步骤：将语义服务注入SemanticRecall；返回doc版本/空间/coverage
交付物：dense召回端口接线的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V16 验证证据（实施时生成）
验收：搜索不依赖具体HTTP客户端，local策略不调用端口；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V16
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：尚无

### [ ] P7-011｜dense范围与hydrate守卫

状态：`todo`；批次：`P7-C`；优先级：`normal`。
范围：`crates/cc-semantic/src/vector/exact.rs`；`crates/cc-search/src/evidence.rs`
硬依赖：P6-020, P7-009, P7-010
步骤：过滤在topk前且最终二次检验manifest/source；删除和scope测试
交付物：dense范围与hydrate守卫的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05, V16 验证证据（实施时生成）
验收：semantic找回结果也不会被softscope误删或越过hard范围；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05；V16
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：尚无

### [ ] P7-012｜融合与部分覆盖语义

状态：`todo`；批次：`P7-C`；优先级：`normal`。
范围：`crates/cc-search/src/fusion.rs`；`crates/cc-model/src/context.rs`
硬依赖：P7-011
步骤：独立dense rank RRF，partial/unavailable透出；不混cosine/BM25
交付物：融合与部分覆盖语义的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V19 验证证据（实施时生成）
验收：timeout与无命中可区分，declared full coverage有证据；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V19
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：尚无

### [ ] P7-013｜查询总deadline和模型故障退化

状态：`todo`；批次：`P7-C`；优先级：`normal`。
范围：`crates/cc-search/src/execution.rs`；`crates/cc-server/src/handlers/context.rs`
硬依赖：P7-012
步骤：fake/HTTP慢请求测取消；auto回本地、explicit semantic明确不足
交付物：查询总deadline和模型故障退化的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V15 验证证据（实施时生成）
验收：网络不占读写锁，故障结果不缓存成完整成功；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V15
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：尚无

### [ ] P7-014｜配置/status/MCP全链贯通

状态：`todo`；批次：`P7-C`；优先级：`normal`。
范围：`crates/cc-server/src/tools.rs`；`crates/cc-server/src/capability_status.rs`；`docs/MCP_TOOLS.md`
硬依赖：P7-013
步骤：新字段schema/sanitize/handler/doc/E2E一体；原mode语义不变
交付物：配置/status/MCP全链贯通的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V18 验证证据（实施时生成）
验收：未配置、关闭、回填、失败、就绪状态真实一致；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V18
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：尚无

### [ ] P7-015｜后台回填与前台查询竞争测试

状态：`todo`；批次：`P7-C`；优先级：`normal`。
范围：`crates/cc-eval/tests/semantic_lifecycle.rs`；`crates/cc-eval/src/benchmark/sampler.rs`
硬依赖：P7-014
步骤：partial backfill时查询/写入/删除/切模型；测queue和CPU/DB占用
交付物：后台回填与前台查询竞争测试的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V14, V17, V20 验证证据（实施时生成）
验收：慢模型不会饿死local索引/查询，过期发布0；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V14；V17；V20
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：尚无

### [ ] P7-016｜fake全故障矩阵回归

状态：`todo`；批次：`P7-D`；优先级：`normal`。
范围：`crates/cc-eval/tests/semantic_lifecycle.rs`；`crates/cc-eval/tests/mcp_v2_contract.rs`
硬依赖：P6-020, P7-015
步骤：组合重试/close/rebuild/delete/lease/GC；每条保留可重放seed
交付物：fake全故障矩阵回归的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V14, V15, V17, V18 验证证据（实施时生成）
验收：依赖独立实验证明恢复，不把fake分数解释成语义效果；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V14；V15；V17；V18
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：尚无

### [ ] P7-017｜离线默认包和未启用测试

状态：`todo`；批次：`P7-D`；优先级：`normal`。
范围：`crates/cc-server/Cargo.toml`；`crates/cc-eval/tests/benchmark_adapters.rs`
硬依赖：P7-016
步骤：禁网络环境启动所有旧工具；检查不创建语义cache文件
交付物：离线默认包和未启用测试的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V18, V21 验证证据（实施时生成）
验收：零key、feature关、semantic.enabled=false都保持本地功能；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V18；V21
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：尚无

### [ ] P7-018｜受授权的真实provider小集认证

状态：`todo`；批次：`P7-D`；优先级：`normal`。
范围：`crates/cc-eval/benchmarks/manifests/`；`artifacts/benchmarks/`
硬依赖：P7-017
步骤：选公开固定输入显式批准费用；验证真实返回、用量和partial behavior
交付物：受授权的真实provider小集认证的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V15, V19, V20 验证证据（实施时生成）
验收：live与fake分栏；无授权则blocked/deferred而不假称通过；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V15；V19；V20
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
条件：仅在真实provider/LLM调用获得明确授权及预算时执行；缺证据阻止相应live效果声明，不阻止已满足的local发布范围。
证据：尚无

### [ ] P7-019｜本地加dense的质量/成本消融

状态：`todo`；批次：`P7-D`；优先级：`normal`。
范围：`crates/cc-eval/src/benchmark/ablation.rs`；`artifacts/benchmarks/`
硬依赖：P7-017
步骤：同输入同budget比较local/dense/hybrid；报告heldout、exact退化和费用
交付物：本地加dense的质量/成本消融的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V19, V20 验证证据（实施时生成）
验收：收益CI与能力差异可解释，不为单题写特殊权重；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V19；V20
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
条件依赖：[{"task": "P7-018", "when": "live semantic-effect certification; not required for engineering/fake profile"}]
证据：尚无

### [ ] P7-020｜P7语义闭环与发布范围验收

状态：`todo`；批次：`P7-D`；优先级：`blocking`。
范围：`docs/roadmap/code-index-v2/`；`artifacts/benchmarks/`
硬依赖：P7-001, P7-002, P7-003, P7-004, P7-005, P7-006, P7-007, P7-008, P7-009, P7-010, P7-011, P7-012, P7-013, P7-014, P7-015, P7-016, P7-017, P7-019
步骤：G7分别出工程/fake与live效果结论；列出认证不足
交付物：P7语义闭环与发布范围验收的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V15, V16, V17, V18, V19, V20 验证证据（实施时生成）
验收：无live证据不声称已证明真实语义收益，本地发布不被模型密钥强绑；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V15；V16；V17；V18；V19；V20
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
条件依赖：[{"task": "P7-018", "when": "live semantic-effect certification; not required for engineering/fake profile"}]
证据：尚无

## P8｜规模、质量与发行认证

### [ ] P8-001｜锁定release候选与证据输入

状态：`todo`；批次：`P8-A`；优先级：`normal`。
范围：`crates/cc-eval/src/benchmark/manifest.rs`；`artifacts/benchmarks/`
硬依赖：P7-020
步骤：冻结binary/config/corpus/scoring/model/环境；停止覆盖被测源码
交付物：锁定release候选与证据输入的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V01, V02 验证证据（实施时生成）
验收：每份报告精确回到同一候选，后续改动使对应证据失效；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V01；V02
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
证据：尚无

### [ ] P8-002｜完成真实多仓native语料认证

状态：`todo`；批次：`P8-A`；优先级：`normal`。
范围：`crates/cc-eval/benchmarks/native/`；`crates/cc-eval/benchmarks/manifests/`
硬依赖：P8-001
步骤：扩至规划语言/仓库覆盖并复核gold；公开/私有数据分离
交付物：完成真实多仓native语料认证的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V02, V19 验证证据（实施时生成）
验收：题数、类别、语言、能力缺口和实际审阅范围透明；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V02；V19
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
证据：尚无

### [ ] P8-003｜运行外部兼容套件

状态：`todo`；批次：`P8-A`；优先级：`normal`。
范围：`crates/cc-eval/src/benchmark/adapters/`；`artifacts/benchmarks/`
硬依赖：P8-002
步骤：同输入锁跑cc-switch/Flask；compat与native独立分报
交付物：运行外部兼容套件的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V03, V04, V19 验证证据（实施时生成）
验收：无锁/输入不同/平台glob差异不能直接对比排行榜；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V03；V04；V19
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
证据：尚无

### [ ] P8-004｜封存holdout与反过拟合检查

状态：`todo`；批次：`P8-A`；优先级：`normal`。
范围：`crates/cc-eval/benchmarks/manifests/`；`crates/cc-search/`
硬依赖：P8-003
步骤：审查生产无gold路径/题词典；一次冻结配置运行heldout
交付物：封存holdout与反过拟合检查的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V19 验证证据（实施时生成）
验收：translation/paraphrase不泄漏，回归后不直接改标签保分；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V19
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
证据：尚无

### [ ] P8-005｜完整规模1k到100k

状态：`todo`；批次：`P8-A`；优先级：`normal`。
范围：`crates/cc-eval/src/benchmark/sampler.rs`；`crates/cc-eval/tests/index_v2_scale.rs`
硬依赖：P8-004
步骤：固定seed与release构建测1k/5k/10k/50k/100k；同时报chunk/edge/vector
交付物：完整规模1k到100k的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V20 验证证据（实施时生成）
验收：不存在只报文件数或把不同工作量直接算倍数；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V20
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
证据：尚无

### [ ] P8-006｜增量规模与fanout曲线

状态：`todo`；批次：`P8-B`；优先级：`normal`。
范围：`crates/cc-eval/src/benchmark/mutations.rs`；`crates/cc-index/`
硬依赖：P7-020, P8-001, P8-005
步骤：测no-op/body/API/config/batch和超预算闭包；输出各phase计数
交付物：增量规模与fanout曲线的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V07, V20 验证证据（实施时生成）
验收：时间可归因且闭包完成后full parity，未完成有显式status；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V07；V20
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
证据：尚无

### [ ] P8-007｜多并发与混合负载

状态：`todo`；批次：`P8-B`；优先级：`normal`。
范围：`crates/cc-eval/src/benchmark/runner.rs`；`crates/cc-server/`
硬依赖：P8-006
步骤：C1/4/8/16 mixed read/build/backfill；记录offered load和排队
交付物：多并发与混合负载的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V20 验证证据（实施时生成）
验收：无死锁饥饿，吞吐不能隐藏timeout与尾部；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V20
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
证据：尚无

### [ ] P8-008｜冷建/重开/热查分层

状态：`todo`；批次：`P8-B`；优先级：`normal`。
范围：`crates/cc-eval/src/benchmark/runner.rs`；`crates/cc-eval/src/benchmark/statistics.rs`
硬依赖：P8-007
步骤：分离OS cache、process cold、result-cache hit与uncached warm；采全部样本
交付物：冷建/重开/热查分层的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V20 验证证据（实施时生成）
验收：无best-of，sample N/分布/CI齐全；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V20
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
证据：尚无

### [ ] P8-009｜内存/磁盘/费用总账

状态：`todo`；批次：`P8-B`；优先级：`normal`。
范围：`crates/cc-eval/src/benchmark/sampler.rs`；`crates/cc-semantic/`
硬依赖：P8-008
步骤：分别client/server/process-tree和artifact/FTS大小；成本reported/estimated分栏
交付物：内存/磁盘/费用总账的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V20 验证证据（实施时生成）
验收：单位和归属正确，不把unavailable填0或把runner当server；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V20
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
证据：尚无

### [ ] P8-010｜长时soak与连续修改

状态：`todo`；批次：`P8-B`；优先级：`normal`。
范围：`crates/cc-eval/tests/soak.rs`；`crates/cc-eval/src/benchmark/mutations.rs`
硬依赖：P8-009
步骤：持续编辑、删除、切分支、catalog压实、cache/worker复用；终点完整对账
交付物：长时soak与连续修改的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V07, V17, V20 验证证据（实施时生成）
验收：内存/队列不无界增长，长期索引和全量一致；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V07；V17；V20
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
证据：尚无

### [ ] P8-011｜端到端故障与恢复认证

状态：`todo`；批次：`P8-C`；优先级：`normal`。
范围：`crates/cc-eval/tests/semantic_lifecycle.rs`；`crates/cc-server/tests/mcp_stdio.rs`
硬依赖：P7-020, P8-007, P8-010
步骤：kill进程/网络断开/缓存损坏/数据库忙/换库；保持raw故障工件
交付物：端到端故障与恢复认证的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V14, V17, V18 验证证据（实施时生成）
验收：没有假ready或删除复活，恢复与费用影响清楚；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V14；V17；V18
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
证据：尚无

### [ ] P8-012｜MSRV与平台冷构建矩阵

状态：`todo`；批次：`P8-C`；优先级：`normal`。
范围：`.github/workflows/ci.yml`；`CONTRIBUTING.md`
硬依赖：P8-011
步骤：Linux/macOS、当前声明MSRV与stable、默认/semantic features；全新target
交付物：MSRV与平台冷构建矩阵的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V01, V21 验证证据（实施时生成）
验收：SDK blocker解决或发布平台范围明确，缓存测试不替冷构建；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V01；V21
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
证据：尚无

### [ ] P8-013｜指标/门槛与失败退出最终认证

状态：`todo`；批次：`P8-C`；优先级：`normal`。
范围：`crates/cc-eval/src/benchmark/gate.rs`；`crates/cc-eval/tests/benchmark_cli.rs`
硬依赖：P8-012
步骤：故意注入quality/perf/lock失败；验证退出和raw报告仍保留
交付物：指标/门槛与失败退出最终认证的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V03, V04, V20 验证证据（实施时生成）
验收：红线失败必非零，inconclusive不被自动passed；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V03；V04；V20
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
证据：尚无

### [ ] P8-014｜可选LLM评审旁证流程

状态：`todo`；批次：`P8-C`；优先级：`normal`。
范围：`crates/cc-eval/benchmarks/manifests/`；`docs/BENCHMARK.md`
硬依赖：P8-013
步骤：冻结judge prompt/model并盲化系统名；复核争议答案/证据充分性
交付物：可选LLM评审旁证流程的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V19 验证证据（实施时生成）
验收：不替代确定性gold，提示注入文本只作数据，费用需授权；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V19
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
条件：可选 LLM 旁证；需要明确授权和预算。未执行不阻塞确定性评分或本地/语义发布，但不能声称完成 LLM 复核。
证据：尚无

### [ ] P8-015｜真实语义效果发布认证

状态：`todo`；批次：`P8-C`；优先级：`normal`。
范围：`crates/cc-eval/benchmarks/manifests/`；`artifacts/benchmarks/`
硬依赖：P8-013, P7-018
步骤：在已授权live条件复跑holdout与费用/错误分布；核对模型revision
交付物：真实语义效果发布认证的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V15, V19, V20 验证证据（实施时生成）
验收：缺live证据只发布已验证的能力范围，不伪造M4-semantic通过；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V15；V19；V20
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
条件：仅在真实provider/LLM调用获得明确授权及预算时执行；缺证据阻止相应live效果声明，不阻止已满足的local发布范围。
证据：尚无

### [ ] P8-016｜数据库/配置/包回滚演练

状态：`todo`；批次：`P8-D`；优先级：`normal`。
范围：`crates/cc-db/src/index_migrate.rs`；`crates/cc-semantic/src/cache.rs`；`docs/TROUBLESHOOTING.md`
硬依赖：P7-020, P8-012, P8-013
步骤：旧binary开新schema的受控重建、cache版本隔离、disable语义回退
交付物：数据库/配置/包回滚演练的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V13, V17, V21 验证证据（实施时生成）
验收：回滚不误读新向量/丢用户源码，恢复步骤实测；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V13；V17；V21
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
证据：尚无

### [ ] P8-017｜删除临时兼容和重复模块

状态：`todo`；批次：`P8-D`；优先级：`normal`。
范围：`crates/cc-index/`；`crates/cc-search/`；`crates/cc-eval/`
硬依赖：P8-016
步骤：清临时旧branch/重复评分器/多份schema来源；保留必要外部wire兼容
交付物：删除临时兼容和重复模块的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V18, V21 验证证据（实施时生成）
验收：运行路径只有一个事实与算法所有者，删除有回归证据；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V18；V21
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
证据：尚无

### [ ] P8-018｜文档事实与安装契约同步

状态：`todo`；批次：`P8-D`；优先级：`normal`。
范围：`docs/ARCHITECTURE.md`；`docs/BENCHMARK.md`；`docs/MCP_TOOLS.md`；`docs/CONFIGURATION.md`
硬依赖：P8-017
步骤：从schema/capabilities生成可核查事实；更新安装/故障/默认离线说明
交付物：文档事实与安装契约同步的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V18, V21 验证证据（实施时生成）
验收：文档表数/schema/工具数不再漂移，设计与已实现标识分开；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V18；V21
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
证据：尚无

### [ ] P8-019｜发布工件与完整报告归档

状态：`todo`；批次：`P8-D`；优先级：`normal`。
范围：`.github/workflows/ci.yml`；`artifacts/benchmarks/`；`docs/benchmarks/`
硬依赖：P8-018
步骤：归档精确binary/checksum/manifest/raw/gates；latest只指向run
交付物：发布工件与完整报告归档的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V01, V04, V21 验证证据（实施时生成）
验收：报告可重算，历史原始run不被下一轮覆盖；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V01；V04；V21
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
证据：尚无

### [ ] P8-020｜P8发布评审与遗留关闭

状态：`todo`；批次：`P8-D`；优先级：`blocking`。
范围：`docs/roadmap/code-index-v2/`；`artifacts/benchmarks/`
硬依赖：P8-001, P8-002, P8-003, P8-004, P8-005, P8-006, P8-007, P8-008, P8-009, P8-010, P8-011, P8-012, P8-013, P8-016, P8-017, P8-018, P8-019
步骤：G8按local/semantic能力分别审核；汇总风险、回滚和后续决策
交付物：P8发布评审与遗留关闭的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V18, V19, V20, V21 验证证据（实施时生成）
验收：本次选择发布范围内blocker为0，其他证据不足明确列出；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V18；V19；V20；V21
回滚：不发布未通过候选；恢复上个已验证binary/config，保留本轮raw报告。
条件依赖：[{"task": "P8-015", "when": "M4-semantic release; not required for M4-local"}]
证据：尚无

## P9｜有收益门的可选增强

### [ ] P9-001｜ANN收益与选型决策

状态：`todo`；批次：`P9-A`；优先级：`normal`。
范围：`crates/cc-eval/src/benchmark/ablation.rs`；`docs/adr/`
硬依赖：P8-020
步骤：依据exact延迟/内存与doc规模比较候选backend；记录不选理由
交付物：ANN收益与选型决策的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V22 验证证据（实施时生成）
验收：没有实测压力不先引入服务/依赖，决定可为deferred；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V22
回滚：关闭该可选增强并回exact/local；清理未选择依赖，不改变主线事实。
条件：本phase为分项收益决策后的可选增强；决策可关闭实现分支，状态必须显式deferred并说明证据。
证据：尚无

### [ ] P9-002｜ANN适配与过滤更新闭环

状态：`todo`；批次：`P9-A`；优先级：`normal`。
范围：`crates/cc-semantic/src/vector/`；`crates/cc-eval/tests/semantic_lifecycle.rs`
硬依赖：P9-001
步骤：仅在选型通过后实现适配；验证过滤recall、insert/delete/tombstone
交付物：ANN适配与过滤更新闭环的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V16, V22 验证证据（实施时生成）
验收：不牺牲hard scope/版本正确性，近似不足有声明；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V16；V22
回滚：关闭该可选增强并回exact/local；清理未选择依赖，不改变主线事实。
条件：本phase为分项收益决策后的可选增强；决策可关闭实现分支，状态必须显式deferred并说明证据。
证据：尚无

### [ ] P9-003｜ANN量化和规模认证

状态：`todo`；批次：`P9-A`；优先级：`normal`。
范围：`crates/cc-eval/src/benchmark/ablation.rs`；`crates/cc-semantic/src/vector/`
硬依赖：P9-002
步骤：量化独立实验不更改exact oracle；测不同filter selectivity和cold加载
交付物：ANN量化和规模认证的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V20, V22 验证证据（实施时生成）
验收：与exact比较质量/资源/更新成本全报，不能只比unfiltered topk；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V20；V22
回滚：关闭该可选增强并回exact/local；清理未选择依赖，不改变主线事实。
条件：本phase为分项收益决策后的可选增强；决策可关闭实现分支，状态必须显式deferred并说明证据。
证据：尚无

### [ ] P9-004｜ANN决策收口

状态：`todo`；批次：`P9-A`；优先级：`normal`。
范围：`docs/adr/`；`docs/CONFIGURATION.md`
硬依赖：P9-003
步骤：保留一种选定生产backend及exact；清试验依赖
交付物：ANN决策收口的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V21, V22 验证证据（实施时生成）
验收：收益门未过恢复exact，未落地不标完成；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V21；V22
回滚：关闭该可选增强并回exact/local；清理未选择依赖，不改变主线事实。
条件：本phase为分项收益决策后的可选增强；决策可关闭实现分支，状态必须显式deferred并说明证据。
证据：尚无

### [ ] P9-005｜LSP按需生命周期

状态：`todo`；批次：`P9-B`；优先级：`normal`。
范围：`crates/cc-server/src/precise_queries/`
硬依赖：P8-020
步骤：参考Astrolabe pool的lazy/cooldown/idle/RSS/inflight generation；不默认全仓启动
交付物：LSP按需生命周期的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V22 验证证据（实施时生成）
验收：没有LSP仍全功能local可用，回收不打断健康请求；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V22
回滚：关闭该可选增强并回exact/local；清理未选择依赖，不改变主线事实。
条件：本phase为分项收益决策后的可选增强；决策可关闭实现分支，状态必须显式deferred并说明证据。
证据：尚无

### [ ] P9-006｜LSP源码快照和冲突表达

状态：`todo`；批次：`P9-B`；优先级：`normal`。
范围：`crates/cc-server/src/precise_queries/`；`crates/cc-model/src/context.rs`
硬依赖：P9-005
步骤：绑定document version/workspace；静态与LSP冲突并列记录
交付物：LSP源码快照和冲突表达的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V09, V22 验证证据（实施时生成）
验收：不以LSP observation无条件覆盖持久化结构事实；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V09；V22
回滚：关闭该可选增强并回exact/local；清理未选择依赖，不改变主线事实。
条件：本phase为分项收益决策后的可选增强；决策可关闭实现分支，状态必须显式deferred并说明证据。
证据：尚无

### [ ] P9-007｜LSP精度/内存收益认证

状态：`todo`；批次：`P9-B`；优先级：`normal`。
范围：`crates/cc-eval/src/benchmark/ablation.rs`；`crates/cc-eval/benchmarks/native/`
硬依赖：P9-006
步骤：固定server版本，对歧义样本测精度、启动/idle成本与失败降级
交付物：LSP精度/内存收益认证的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V20, V22 验证证据（实施时生成）
验收：缓存/外部进程内存全计，收益不足可不开启；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V20；V22
回滚：关闭该可选增强并回exact/local；清理未选择依赖，不改变主线事实。
条件：本phase为分项收益决策后的可选增强；决策可关闭实现分支，状态必须显式deferred并说明证据。
证据：尚无

### [ ] P9-008｜LSP工具面和安装边界收口

状态：`todo`；批次：`P9-B`；优先级：`normal`。
范围：`docs/MCP_TOOLS.md`；`docs/CONFIGURATION.md`；`crates/cc-server/src/precise_queries/`
硬依赖：P9-007
步骤：避免工具爆炸，优先precision选项；不自动下载执行未知server
交付物：LSP工具面和安装边界收口的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V18, V21, V22 验证证据（实施时生成）
验收：行为有明确capability/许可/配置，缺失可诊断；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V18；V21；V22
回滚：关闭该可选增强并回exact/local；清理未选择依赖，不改变主线事实。
条件：本phase为分项收益决策后的可选增强；决策可关闭实现分支，状态必须显式deferred并说明证据。
证据：尚无

### [ ] P9-009｜可选rerank端口与降级

状态：`todo`；批次：`P9-C`；优先级：`normal`。
范围：`crates/cc-search/src/rerank/`；`crates/cc-semantic/src/providers/`
硬依赖：P8-020
步骤：限定候选/预算，保留原始相关性证据；错误回本地确定性排序
交付物：可选rerank端口与降级的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V22 验证证据（实施时生成）
验收：模型不能捏造新文件/边，取消和fallback有状态；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V22
回滚：关闭该可选增强并回exact/local；清理未选择依赖，不改变主线事实。
条件：本phase为分项收益决策后的可选增强；决策可关闭实现分支，状态必须显式deferred并说明证据。
证据：尚无

### [ ] P9-010｜rerank盲测与费用消融

状态：`todo`；批次：`P9-C`；优先级：`normal`。
范围：`crates/cc-eval/src/benchmark/ablation.rs`；`crates/cc-eval/benchmarks/manifests/`
硬依赖：P9-009
步骤：固定候选池/模型/prompt，重复配对/holdout；source注入只当数据
交付物：rerank盲测与费用消融的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V19, V20, V22 验证证据（实施时生成）
验收：收益CI/成本/latency全报，不用单次分数启用默认模型；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V19；V20；V22
回滚：关闭该可选增强并回exact/local；清理未选择依赖，不改变主线事实。
条件：本phase为分项收益决策后的可选增强；决策可关闭实现分支，状态必须显式deferred并说明证据。
证据：尚无

### [ ] P9-011｜按需查询分解收益决策

状态：`todo`；批次：`P9-C`；优先级：`normal`。
范围：`crates/cc-search/src/query_policy.rs`；`crates/cc-eval/src/benchmark/ablation.rs`
硬依赖：P9-010
步骤：只对复杂任务测分解/facet召回；保持scope和总deadline
交付物：按需查询分解收益决策的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V19, V22 验证证据（实施时生成）
验收：不为每个查询调用LLM，收益不足维持原路径；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V19；V22
回滚：关闭该可选增强并回exact/local；清理未选择依赖，不改变主线事实。
条件：本phase为分项收益决策后的可选增强；决策可关闭实现分支，状态必须显式deferred并说明证据。
证据：尚无

### [ ] P9-012｜P9可选增强逐项结项

状态：`todo`；批次：`P9-C`；优先级：`blocking`。
范围：`docs/roadmap/code-index-v2/`；`docs/adr/`
硬依赖：P9-001, P9-002, P9-003, P9-004, P9-005, P9-006, P9-007, P9-008, P9-009, P9-010, P9-011
步骤：分别记录ANN/LSP/rerank启用或deferred；删除未选试验债务
交付物：P9可选增强逐项结项的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V21, V22 验证证据（实施时生成）
验收：不将未选功能算实现完成，不阻断已通过主线发行；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V21；V22
回滚：关闭该可选增强并回exact/local；清理未选择依赖，不改变主线事实。
条件：本phase为分项收益决策后的可选增强；决策可关闭实现分支，状态必须显式deferred并说明证据。
证据：尚无
