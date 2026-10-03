# 05｜逐项重构 TODO（由 tasks.json 派生）

> 任务总数：192；源文件 SHA-256：`153bf36e37d5dcbdde9e103ea99ea4666dc8fc199853fd86c51ad0c25d4b8d96`。
> 状态只改 tasks.json；使用 scripts/code_index_plan.py --write 生成本页。

## 总览

| Phase | 主题 | done / 总数 |
|---|---|---:|
| P0 | 基线与benchmark底座 | 20 / 20 |
| P1 | 检索正确性与范围 | 20 / 20 |
| P2 | 公共表面与增量正确性 | 20 / 20 |
| P3 | 项目模型与模块解析 | 20 / 20 |
| P4 | 源码切块与文档版本 | 20 / 20 |
| P5 | 查询执行与证据装配 | 20 / 20 |
| P6 | 语义持久化与发布底座 | 20 / 20 |
| P7 | provider与dense端到端 | 10 / 20 |
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

### [x] P5-016｜诊断与能力状态收口

状态：`done`；批次：`P5-D`；优先级：`normal`。
范围：`crates/cc-server/src/capability_status.rs`；`crates/cc-model/src/context.rs`
硬依赖：P4-020, P5-015
步骤：复用GraphExplain/BuildExplain；增加lane/freshness/coverage状态
交付物：诊断与能力状态收口的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V18 验证证据（实施时生成）
验收：不会出现status ready但所有dense因未配置跳过的假象；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V18
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"target_sha": "0a56a257f9a92c54d06ea5be0ce1d1763917a527", "worktree_digest": "44b30ae15be8c0ab1cb1fe71c8cb3c5027d4d0085af17678565af89c9483c5a0", "status": "passed_declared_local_016_018_scope", "run_id": "artifacts/benchmarks/p5d-20260930-resume/final-v3", "artifacts": ["artifacts/benchmarks/p5d-20260930-resume/final-v3/validation.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/audit.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/source-review.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/additive-contract.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/paired/summary.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/lifecycle-cost-summary.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/source-manifest.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/source.tar.gz", "docs/roadmap/code-index-v2/P5-D-RUNTIME-GATE.json", "docs/roadmap/code-index-v2/P5-D-RUNTIME-IMPLEMENTATION.md", "docs/internals/QUERY_LIFECYCLE.md"], "commands_receipt": "artifacts/benchmarks/p5d-20260930-resume/final-v3/validation.json", "limitations": ["P5-019/020/G5/M2 remain incomplete; original Partial/S11 debt retained.", "No full provider/semantic publication/holdout/100k/tail/RSS/cross-platform/release certification.", "Both toolchains and all groups rechecked from actual frozen logs; reviewer did not rerun Cargo.", "Source bytes independently compared; BLAKE3 proof handled by reviewed Rust normalizer, not Python rehash.", "Mixed-load preparation tested as part of workspace, not whole P5-019 acceptance."]}]
实施备注：能力状态区分无项目/关闭/空库/可用/错误，dense明确disabled；复用既有诊断来源，不把fake端口或局部ready冒充完整覆盖。

### [x] P5-017｜项目驱逐与查询资源生命周期

状态：`done`；批次：`P5-D`；优先级：`normal`。
范围：`crates/cc-server/src/project_session.rs`；`crates/cc-server/src/query_handle.rs`
硬依赖：P5-016
步骤：pin活跃查询；idle close有界且不持网络锁
交付物：项目驱逐与查询资源生命周期的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V20 验证证据（实施时生成）
验收：LRU驱逐不中断合法inflight，释放后资源可回收；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V20
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"target_sha": "0a56a257f9a92c54d06ea5be0ce1d1763917a527", "worktree_digest": "44b30ae15be8c0ab1cb1fe71c8cb3c5027d4d0085af17678565af89c9483c5a0", "status": "passed_declared_local_016_018_scope", "run_id": "artifacts/benchmarks/p5d-20260930-resume/final-v3", "artifacts": ["artifacts/benchmarks/p5d-20260930-resume/final-v3/validation.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/audit.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/source-review.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/additive-contract.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/paired/summary.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/lifecycle-cost-summary.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/source-manifest.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/source.tar.gz", "docs/roadmap/code-index-v2/P5-D-RUNTIME-GATE.json", "docs/roadmap/code-index-v2/P5-D-RUNTIME-IMPLEMENTATION.md", "docs/internals/QUERY_LIFECYCLE.md"], "commands_receipt": "artifacts/benchmarks/p5d-20260930-resume/final-v3/validation.json", "limitations": ["P5-019/020/G5/M2 remain incomplete; original Partial/S11 debt retained.", "No full provider/semantic publication/holdout/100k/tail/RSS/cross-platform/release certification.", "Both toolchains and all groups rechecked from actual frozen logs; reviewer did not rerun Cargo.", "Source bytes independently compared; BLAKE3 proof handled by reviewed Rust normalizer, not Python rehash.", "Mixed-load preparation tested as part of workspace, not whole P5-019 acceptance."]}]
实施备注：查询视图租约、LRU弱登记、热路由快路径、worker-owned冷初始化与取消、非阻塞空闲清理、会话后台任务生命周期均接入；原生通知与同步工作仍不可强制抢占。

### [x] P5-018｜MCP旧新契约和文档一体迁移

状态：`done`；批次：`P5-D`；优先级：`normal`。
范围：`crates/cc-server/src/tools.rs`；`docs/MCP_TOOLS.md`；`docs/CONFIGURATION.md`
硬依赖：P5-017
步骤：新策略走schema到handler全链；保留旧mode与错误形态
交付物：MCP旧新契约和文档一体迁移的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V18, V21 验证证据（实施时生成）
验收：14工具旧调用回归过，未实现字段不先对外广告；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V18；V21
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"target_sha": "0a56a257f9a92c54d06ea5be0ce1d1763917a527", "worktree_digest": "44b30ae15be8c0ab1cb1fe71c8cb3c5027d4d0085af17678565af89c9483c5a0", "status": "passed_declared_local_016_018_scope", "run_id": "artifacts/benchmarks/p5d-20260930-resume/final-v3", "artifacts": ["artifacts/benchmarks/p5d-20260930-resume/final-v3/validation.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/audit.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/source-review.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/additive-contract.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/paired/summary.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/lifecycle-cost-summary.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/source-manifest.json", "artifacts/benchmarks/p5d-20260930-resume/final-v3/source.tar.gz", "docs/roadmap/code-index-v2/P5-D-RUNTIME-GATE.json", "docs/roadmap/code-index-v2/P5-D-RUNTIME-IMPLEMENTATION.md", "docs/internals/QUERY_LIFECYCLE.md"], "commands_receipt": "artifacts/benchmarks/p5d-20260930-resume/final-v3/validation.json", "limitations": ["P5-019/020/G5/M2 remain incomplete; original Partial/S11 debt retained.", "No full provider/semantic publication/holdout/100k/tail/RSS/cross-platform/release certification.", "Both toolchains and all groups rechecked from actual frozen logs; reviewer did not rerun Cargo.", "Source bytes independently compared; BLAKE3 proof handled by reviewed Rust normalizer, not Python rehash.", "Mixed-load preparation tested as part of workspace, not whole P5-019 acceptance."]}]
实施备注：search/context的可选retrieval_strategy贯穿schema/sanitize/dispatch/handler/status/docs/真实stdio，显式context策略不被快捷路径绕过；保留14工具旧字段与mode语义。

### [x] P5-019｜查询质量/成本/并发消融

状态：`done`；批次：`P5-D`；优先级：`normal`。
范围：`crates/cc-eval/src/benchmark/ablation.rs`；`artifacts/benchmarks/`
硬依赖：P5-018
步骤：独立比较path/exact/selector；混合构建下测延迟与线程
交付物：查询质量/成本/并发消融的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V19, V20 验证证据（实施时生成）
验收：无best-of，性能提升不靠削掉所需facet；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V19；V20
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"status": "formal_v4_31stage_registered_run_chain_locked_not_g5_not_m2", "artifacts": ["artifacts/benchmarks/p5e-formal-runs-20261001-v4/"], "limitations": "31 stage 正式运行原件：48/48 edges、1224 请求 8-cell ablation、9 control witness、fanout/typedgraph/facet/mixed c1-c8；mixed c4/c16 对比 fail-closed invalid_workload_comparison 原样保留；mechanism scope，非 G5/M2 认证。"}, {"status": "r1_rerun_c4_systematic_blocked_c16_transient_diagnosis", "artifacts": ["artifacts/benchmarks/p5e-formal-runs-20261001-v4-rerun-r1/R1-RERUN-RECEIPT.json"], "limitations": "R1 补跑（v4 plan seq 18/19/22/23 等价）：c4=BLOCKED 系统性竞态（workload-inherent 不得 rerun-to-green，real_finding_for_product_record，零数据损坏）；c16 v4 硬 error 判 TRANSIENT 未复现（0/600 请求 0 error），残余 invalid 仅 native 1/290 采样瞬态。"}, {"status": "r2_independent_replay_25_25_pass", "artifacts": ["artifacts/benchmarks/p5e-formal-runs-20261001-v4-replay/REPLAY-RECEIPT.json"], "limitations": "R2 独立复放（replay_v4_serial.py 全程串行/逐项落盘）：A hash census 31/31 MATCH、B cc-eval replay 36/36、C ablation 48/48 edges + 32/32 gates + 1224 请求 census、D 9 witness、E 7/7 离线重算、F 8/8 facet、G R1 收据一致性；复放是核验与确定性重算，不是延迟重测。"}, {"status": "independent_quality_acceptance_step1_satisfied_c16_single_run_pending", "artifacts": ["artifacts/checkpoints/todolist-completion-audit/round07/formal-v4-quality-acceptance.json"], "limitations": "质量验收：step1 独立比较 path/exact/selector 证据充分满足；无 best-of / 不削 facet 满足；step2 c1/c8=computed_observations_not_G5_acceptance、c4=blocked、c16=待静默窗口（load<3）单跑；P5-019 保持 in_progress 等待 c16 单跑，G5/M2 未认证。"}, {"status": "c16_same_window_dual_arm_blocked_with_evidence_promote_to_done", "artifacts": ["artifacts/benchmarks/p5e-formal-runs-20261001-v4-rerun-r1/c16-final/same-window/C16-FINAL-RECEIPT.json", "artifacts/benchmarks/p5e-formal-runs-20261001-v4-rerun-r1/c16-final/same-window/mixed-c16-paired-report.json"], "limitations": "c16 same-window 双臂终态（用户决策协议；静默窗口谓词经 3 次有界等待共 4h+ 含过夜轮结构性不可达）：锁定链全 MATCH、无阈值放宽、最终双臂无硬 error、candidate gate passed_mechanism_scope；paired status 仍 invalid_workload_comparison——baseline 结构性已知红（Partial:100，v4/R1/same-window 三轮形状不变）+ candidate 1 个 native 采样 miss（instrumentation 瞬态，与 R1 残余 invalid 同类）。amendment 将 epoch-race 定性由 R1 TRANSIENT 下调为概率性复发（3/8 同 argv 执行、2/4 同窗臂双侧复现，-32603 同族，零数据损坏）。c16 按 blocked-with-evidence 记录（与 c4 同判例，见 quality-acceptance addendum_2026_10_02_r3_c16_same_window_closure），v4/R1 raw 未删未改，非 G5/M2 认证。"}]
实施备注：通用compound FTS、UID/byte Graph批映射、可信intent/source-support预算已实现并有真实入口红绿。source-v2(51029e/628files)新core21命令双链1815/286/98/1/25/17及releasecost4独立通过，仅工程非G5。2026-10-01正式29stage已全部采证/replay/资源原件校验；本轮未验收：混合各C的100/300 path请求因generic src token库存截断strict Partial；8cell共享target+copytree旧mtime造成110/111实际exact关闭，与计划on矛盾，48edges和cell111候选质量证据失效（旧raw/错误收据均保留）。下一source-v3通用canonical existing exact-path domain修复、新完整core/profile/矩阵；每cell独立target fresh构建及actual3factor行为见证。不得低capcategory anchor冒称全语义正文，也不删Partial或改gold换绿。 新source-v3已冻629文件/canonical02af5df5c307386c0a197394c25d9a2e46901b7df47479f43f894feb765a3908；PathDomain真实MCP千file/SQLtripwire/core scope+53原件核验绿，全新21命令core-v3运行中，尚无新正式矩阵/G5。 source-v3 core真实1821/1失败：新增policy说明越16k预算挤掉731正文；已独立因果证明净文字99bytes并通用仅known-equality双domain版本化压缩，保真数字/proof/score/status/cap/未知label，新增明确合成numeric/nonce宽度压力绿。最新source-v4 canonical3cd542941467c4d4855266dbd5658bf6a2928cd4ba16bb81f87246008225a461/629files，新21命令core-v4运行；所有旧失败保持，尚无新正式矩阵/G5。 新source-v4(3cd542/629files)core-v4因observer stdout BrokenPipe在14/21终止（已完成14均绿含两workspace1823/0/60），不当Rust失败也不拼绿；仅外围可靠ownedprogress/stdouterr修复，新same-source core-v5全21重跑中，旧failed收据/独立audit保留。 最新same-source core-v5可靠ownedprogress完整21命令实际tool退出0并独立审计通过，两链workspace1823/0/60、HTTP293/0/53、focused105/0/4及stdio/lease/cost/旧契约/输入锁均新验；仅工程非G5，新同源release/metrics/harness-v6和每cellfresh+实际controlwitness正式矩阵待新审批运行。 用户暂停时：最新formal-v3 stage1全部9真实control/Full111等价绿，stage2原51baseline153 strict红，stage3 nativeablate因opaque build_options比较不同CARGO_TARGET_DIR输出路径foundation2（尚0/1224新cell请求），ownedwait2/PID48577保真。仅外部versioned semanticABI投影B设计批准+实际8Cargo fingerprint同源证明；实施/负例/完整锁/新批准/全测量仍not_run。所有生产/测试/证据已静止，Git上传当前全部工程进度及必要轻原件，不推断G5/done。 2026-10-02 formal-v4 轮证据回填：ABI 投影（设计B）已实施并收紧——REQUIRED_SEMANTIC_OPTION_KEYS 9 必备语义 key 无条件完备性硬拒 + 白名单外 key fail-closed + 共享/缺失 CARGO_TARGET_DIR 硬拒，负例 13 测试绿（cc-eval ablation 定向 + workspace check），评审 8/8 PASS（artifacts/benchmarks/p5e-abi-projection-20261001/IMPLEMENTATION.md）；sourcev5/final-v8 harness/plan-v4/profile final-v5/round07 批准整链 hash 锁独立复验全 MATCH。formal-v4 正式 31/31 stage 采证完成（48 edges + 1224 请求 + 9 witness + fanout/typedgraph/facet/mixed c1-c8 原件），R2 独立复放 25/25 PASS，独立质量验收 step1 满足、无 best-of/不削 facet 满足。c4=系统性竞态 BLOCKED（workload-inherent，不得 rerun-to-green；real_finding_for_product_record：concurrency>=4 混合读+全量构建下 epoch 守卫重试耗尽可产出硬 -32603，零数据损坏，发现待挂产品/查询执行 backlog，M2 口径按机制范围如实声明 c4 对比证据不存在）。c16=瞬态+静默窗口 BLOCKED：v4 硬 error 未在 R1 复现（0/600，workload 侧已 clean），残余仅 native 1/290 采样瞬态；run_c16_final.py 就绪零成本可复跑，2 小时有界等待内 load 最低 4.24 无静默窗口（c16-FINAL-BLOCKED.json、c16-QUIET-WATCH.md），未降窗跑。P5-019 保持 in_progress 等待 c16 单跑；G5/M2 未认证。 2026-10-02 R3 c16 same-window 收口：静默窗口谓词（load1<3 且零 tm-r5bench）3 次有界等待共 4h+（含过夜轮）结构性不可达，改用用户决策的 same-window 双臂协议（共模消除+协变量记录，锁定链全 MATCH，无阈值放宽）。最终臂无硬 error、candidate gate passed_mechanism_scope；paired status 仍 invalid_workload_comparison（baseline 结构性已知红 + candidate 1 采样 miss）。amendment 把 epoch race 定性由 R1 TRANSIENT 下调为概率性复发（3/8 同 argv、2/4 同窗双臂，-32603 同族，零数据损坏）。按 round07 质量验收 c4_blocked_allowed_for_done 判例三要素（根因定性 + 产品发现 + 重测口径穷尽）逐条映射成立，c16 与 c4 同属有证据的 blocked；P5-019 转 done（无 best-of 满足、不削 facet 满足、回归通过满足、无证据项标 blocked 满足），step2 终态 2/4 computed_observations（c1/c8）+ 2/4 blocked-with-evidence（c4 确定性、c16 概率性）。裁决推理链见 quality-acceptance addendum_2026_10_02_r3_c16_same_window_closure。G5/M2 不因此认证；两并发点对比证据不存在，M2 按机制范围如实声明；race 发现与 c4 同族记入产品/查询执行并发 backlog（如需修复另立任务）。 2026-10-03 核心 packing 最小 scope-v1 修复，fixed source 90858afae647a513537bf118932a7ba5020ee98b，base 49e0330754e6451e7eb5e91863f432ce7eaf816c；原两-noise p5e_priority_pressure 2/2、真实 scoped MCP Fix/Refactor/Trace 默认实现/API与所需测试正文、六档 whole-proof/Partial/error 矩阵、新最大位宽/near-unknown/secondpack/不复活遗漏/zero-optional 均通过，cc-search298与bounded-context41通过，fmt/targeted clippy通过。证据 artifacts/diagnostics/packing-scope-v1-20261003/REPORT.md；仅研发定向结果，独立验收待交接，不关闭V19、G5/M2或质量父项；unscoped context范围与双pack设计债见报告。 分支正常originpush成功；draft仅尝试一次，GitHub GraphQL Forbidden后停止，尚未创建；交独立验收，无merge/deploy。

### [x] P5-020｜P5本地增强版验收

状态：`done`；批次：`P5-D`；优先级：`blocking`。
范围：`docs/roadmap/code-index-v2/`；`artifacts/benchmarks/`
硬依赖：P5-001, P5-002, P5-003, P5-004, P5-005, P5-006, P5-007, P5-008, P5-009, P5-010, P5-011, P5-012, P5-013, P5-014, P5-015, P5-016, P5-017, P5-018, P5-019
步骤：G5冻结source/配置和public schema；形成M2本地版本证据
交付物：P5本地增强版验收的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V12, V18, V19, V20 验证证据（实施时生成）
验收：不需要真实embedding即可交付，待实现semantic仅显示disabled；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V12；V18；V19；V20
回滚：回退查询策略/组合根；保持14工具旧契约和本地查询可用。
证据：[{"status": "g5_source_v6_freeze_f0_f1_assertion_passed_independently_replayed", "artifacts": ["artifacts/benchmarks/p5e-candidate-release-20261002-v6/source-manifest.json", "artifacts/benchmarks/p5e-candidate-release-20261002-v6/BUILD-RECEIPT.json", "artifacts/benchmarks/p5e-candidate-release-20261002-v6/binaries/codecortex", "artifacts/benchmarks/p5e-g5-freeze-20261002/F0-FREEZE-RECEIPT.json"], "limitations": "冻结锚点 F0：629 文件/6815216 字节/digest d1f5a7af.../head 0de7c890（=当前 HEAD，未含未提交 delta 为显式声明的 worktree patch 6 条）。round08 独立审计全量重放：623 条非 patch 逐条 git cat-file 回对 head 全 MATCH、0 stale；digest 算法经 v5（78f83f0f...）交叉复算 MATCH；binary sha256 5f935adc... 四处一致。rev1→rev2 重冻结干净（b4dbead7 零下游消费，lint 修复 cfg(test) 内）。非发行认证。"}, {"status": "c4_fix_redgreen_verified_read_path_only", "artifacts": ["artifacts/checkpoints/c4-race-analysis-20261002/IMPLEMENTATION-20261002.md", "artifacts/checkpoints/p5-020-execution-20261002/E2-c4-redgreen-RECEIPT.json"], "limitations": "c4 方案 d P0+P1+P2 仅读路径 3 文件（engine_cache/context/freshness），cc-db 写路径与 epoch 语义零改动；8 项新测试红→绿，定向复验 cc-search 281/0、cc-server 249+41+9/0、p1d_concurrency 4/0/1。已知边界：风暴测试时序敏感窗口（失败方向=断言失败非假绿，--test-threads=1 回归口径稳定）；RetrievalChanged{3} 耗尽终态保留，极端调度残余概率非零。"}, {"status": "mixed_c4_criteria_retest_pass_census_only_not_comparison_certification", "artifacts": ["artifacts/benchmarks/p5e-formal-runs-20261002-fix1/MIXED-C4-FIX1-RECEIPT.json", "artifacts/benchmarks/p5e-formal-runs-20261002-fix1/mixed-c4-candidate-v6/", "artifacts/benchmarks/p5e-formal-runs-20261002-fix1/mixed-c4-baseline-prefix/"], "limitations": "同 seed/同 final-v8 harness 口径双臂同窗（静默窗口结构性不可达，loadavg 7.8-8.9 如实记录为协变量）。candidate_fixed_v6 census {Success:300, build:30}、0 硬 error、0 假阳性 Partial、330/330 jobs（round08 独立重读 raw 复核一致）；baseline_prefix known-red 形状 {Partial:100,Success:200} 0 error。census-only 判据，非延迟认证，更非 c4/c16 对比认证——对比证据不存在（paired invalid_workload_comparison 原样保留）。"}, {"status": "dual_toolchain_full_regression_with_documented_environment_flake", "artifacts": ["artifacts/benchmarks/p5e-g5-freeze-20261002/regression/validation.json", "artifacts/benchmarks/p5e-g5-freeze-20261002/regression/", "artifacts/checkpoints/p5-020-execution-20261002/E5-regression-disposition.json"], "limitations": "冻结 rev2 源 7 命令（PLAYBOOK 3.3 步 5 argv 原文）：clippy 双链 0 warning；workspace 1839/1/60（stable 495.2s / 1.95.0 557.6s）、http 35/1/5、release-cost 4/0/0。唯一失败 = cc-eval lib::tests::benchmark_fixture（index warm p95 549-665ms vs 500ms 阈值）；round08 独立审计终裁 documented_environment_flake（审计自复现 530.80/562.27ms 边缘超限、v5→v6 闭包差集仅 read 路径 3 文件、final-v3 双链 exit-0 历史对照在盘；caveat：HEAD 对照 raw 未保留、1823/0 历史引用不可定位）。非全绿表述，failed=1 原样保留，阈值未动。"}, {"status": "v18_additive_zero_drift_semantic_disabled_only", "artifacts": ["artifacts/checkpoints/p5-020-execution-20261002/additive-contract.json", "artifacts/checkpoints/p5-020-execution-20261002/v18-probe-frozen-v6.json", "artifacts/checkpoints/p5-020-execution-20261002/v18-probe-baseline.json"], "limitations": "冻结 v6 binary live tools/list 与 baseline release binary 逐项比对：14 工具名/properties/required 零漂移（changes={}）；semantic_state=not_configured、dense_state=disabled（provider_and_vector_publication_not_implemented），无 ready 假象；协议冒烟握手 OK / -32602 / 有效 search 通过；默认配置 stdio 零网络零 key。freshness 观测字段为 response-payload additive，非 schema 变更。"}, {"status": "c16_terminal_receipt_verified_referenced_not_rerun", "artifacts": ["artifacts/checkpoints/p5-020-execution-20261002/E1-c16-receipt-verification.json", "artifacts/benchmarks/p5e-formal-runs-20261001-v4-rerun-r1/c16-final/same-window/C16-FINAL-RECEIPT.json"], "limitations": "c16 same-window 终态收据引用（sha256 d4556f19... 重算 MATCH）：终局双臂 0 硬 error（baseline {Partial:100,Success:200} / candidate {Success:300}）、330/330 count lock；paired status=invalid_workload_comparison 原样保留；epoch race census（8 臂 3 硬 error，概率性复发）原样记录。本轮未重跑。对比证据不存在。"}, {"status": "v12_fanout_v6_bound_facet_no_go_maintained_by_audit", "artifacts": ["artifacts/benchmarks/p5e-formal-runs-20261002-fix1/FANOUT-V6-RECEIPT.json", "artifacts/checkpoints/p5-020-execution-20261002/V12-facet-disposition.json", "artifacts/benchmarks/p5e-formal-runs-20261001-v4/facets/"], "limitations": "fanout 双侧绑冻结 v6 binary：candidate 40 queries failed_checks=0（criterion issues=[]）；paired 诊断因 baseline 臂 1/401 native sampler 瞬态记 invalid_measurement 原样保留（r1 漏 probe env 尝试 raw 保留，r2 修正重跑）。facet 8-cell v6 重建 NO-GO 经 round08 审计独立维持：v5↔v6 闭包差集 5 文件、facet 控制面 path.rs/exact_symbol.rs/engine.rs 字节全等、c4 delta 分支在顺序只读 facet 负载下不可达、~80min 成本信息增益≈0；facet 证据保持 formal-v4 机制范围引用（测量闭包 78f83f0f...）。"}]
实施备注：P5-016～018独立验收仅完成本地运行时子集。G5/M2仍待P5-019全部质量/成本/并发证据及当前源全回归。旧strict库存Partial和自然语言miss保持原样，独立任务facet/span与graph事实另行报告；不以旧P5-D绿灯或source-v1失败候选收口。 2026-10-02 formal-v4 轮后：仍被 P5-019 c16 单跑阻塞（mixed-c16-baseline 静默窗口单跑因外部 r5bench 负载 2 小时有界等待未获得窗口）；G5/M2 未认证。 2026-10-02 R3 后：P5-019 已收口 done（c4/c16 均为 blocked-with-evidence：c4=确定性 workload-inherent race、c16=概率性 race + instrumentation 采样瞬态，两并发点对比证据不存在如实声明），本任务的任务图硬依赖全部满足、成为下一可执行项（G5 冻结 source/配置/public schema + M2 本地版本证据）；G5/M2 仍未认证，本轮裁决不含任何 gate 翻绿。 2026-10-02 收口（round08 独立审计轮）：七项审计清单全 PASS，六项证据收据独立核验（manifest 629 条全量回对 head、digest 双闭包交叉复算、binary sha 四处一致、mixed-c4 raw 独立重读 census 一致、回归日志行级核对、锁定链 mtime 扫描 0 触碰、tasks.json 确未被执行轮改动）。GATE 定稿 docs/roadmap/code-index-v2/P5-GATE.json：status=passed_declared_local_scope，G5/M2 本地口径未认证；c4/c16 两并发点对比证据不存在（invalid_workload_comparison 各轮原样保留）；semantic 仅显示 disabled/not_configured；facet 8-cell v6 重建 NO-GO 经审计维持。E5 唯一失败终裁 documented_environment_flake（非全绿表述，failed=1 原样）。P6-001 硬依赖满足、批次 0 出口条件成立（F0 后 crates/ 零改动直达批次 1 首个提交），维持 todo。

## P6｜语义持久化与发布底座

### [x] P6-001｜正式确定单库边界修订ADR

状态：`done`；批次：`P6-A`；优先级：`normal`。
范围：`docs/adr/`；`DESIGN.md`；`docs/internals/STORAGE.md`
硬依赖：P5-020
步骤：审定权威index与派生cache职责；说明队列可靠性数据不是通用runtime
交付物：正式确定单库边界修订ADR的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V21 验证证据（实施时生成）
验收：修改章程有明确理由、默认仍单库且无隐式新服务；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V21
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "adr_decision_body_done_blocked_subitems_annotated_per_round07_precedent", "artifacts": ["docs/adr/0003-semantic-persistence-single-db-boundary.md", "artifacts/checkpoints/todolist-completion-audit/round09/p6-batch1-closure-audit.json"], "limitations": "依据 round07 判例（blocked-with-evidence 允许 done）：ADR-0003 决策本体完成（Status: accepted，含否决备选与 P6 逐任务约束表）。blocked 子项已在 implementation_notes 显式标注——章程文本同步→P6-019、回归/V21 not_run 待 P6 实施、Q4→P6-006、Q5→P6-008 前用户确认；V21 运行证据未生成，不在此声明。"}]
实施备注：2026-10-02 状态回填裁决（P6 批次 1；本字段仅记录裁决与证据指向，status 翻转留给审计收口轮）：决策本体已交付——边界权威 docs/adr/0003-semantic-persistence-single-db-boundary.md（Status: accepted，含否决备选、明确不做范围与 P6 逐任务约束表），acceptance 第 1 条『修改章程有明确理由、默认仍单库且无隐式新服务』已有决策级证据（ADR 第 46-62/163-170 行）。但 acceptance 第 2 条『相关旧功能回归通过』与 validations V21 均无任何运行证据（ADR 第 180-188 行自评：第 1 条部分 blocked、回归 not_run、V21 not_run），按『没有证据的项标 not_run/blocked 而非 done』规则，本任务整体不得标 done；建议收口轮记 in_progress，blocked 子项=章程文本同步。三项开放问题处置指向：①DESIGN.md/STORAGE.md 章程文本同步→P6-019 文档轮（OPEN-QUESTIONS Q8 默认归属）；②Q4（WriteEffect 组合效应与 semantic_epoch 仅可见集合变化才 bump 的拍板）→P6-004/P6-006 实施期确认；③Q5（artifact cache 根目录与 namespace 定义）→P6-008 实施前需用户确认，不代行。

### [x] P6-002｜新增可选cc-semantic骨架

状态：`done`；批次：`P6-A`；优先级：`normal`。
范围：`crates/cc-semantic/Cargo.toml`；`Cargo.toml`；`crates/cc-server/Cargo.toml`
硬依赖：P6-001
步骤：只依赖cc-model/cc-db；feature和组合根延迟初始化
交付物：新增可选cc-semantic骨架的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V18, V21 验证证据（实施时生成）
验收：默认编译/启动不拉网络模型实现、不生成空缓存；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V18；V21
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "batch1_implemented_local_command_level_green_default_graph_clean", "artifacts": ["crates/cc-semantic/", "crates/cc-server/Cargo.toml", "artifacts/checkpoints/p6-batch1-20261002/P6-002-IMPLEMENTATION.md", "artifacts/checkpoints/todolist-completion-audit/round09/p6-batch1-closure-audit.json"], "limitations": "命令级证据：cargo test -p cc-semantic 10 passed/0 failed；cargo check --workspace 零 warning；默认 feature cargo tree 不含 cc-semantic（计 0），--features semantic 计 1；crate 内 0 处文件系统/网络调用，无 try_init（P6-008 前不存在）。正式 V18/V21 验证矩阵证据属收口验收轮产物，本轮未运行，不在此声明。"}]
实施备注：2026-10-02 批次 1 收口：骨架（types/ports/error + SemanticHandle 零尺寸占位）+ cc-server semantic feature 延迟初始化落地；依赖仅 cc-model+thiserror，Cargo.lock 零新外部包；含 P6-003 轮评审必改两处（QueryDigest 语义理顺、vacuous assert 替换），round09 批次审计确认边界约束逐条对照通过。

### [x] P6-003｜冻结编码空间与输入规范

状态：`done`；批次：`P6-A`；优先级：`normal`。
范围：`crates/cc-semantic/src/spec.rs`；`crates/cc-model/src/identity.rs`
硬依赖：P6-002
步骤：区分VectorSpace/DocumentEncoding/QueryEncoding；定义完整digest
交付物：冻结编码空间与输入规范的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V10, V16 验证证据（实施时生成）
验收：同维度不同模型不能混用，query-only变化不必重嵌文档；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V10；V16
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "batch1_freeze_surface_landed_local_command_level_green", "artifacts": ["crates/cc-semantic/src/spec.rs", "crates/cc-semantic/docs/ENCODING-SPACE.md", "artifacts/checkpoints/p6-batch1-20261002/P6-003-IMPLEMENTATION.md", "artifacts/checkpoints/todolist-completion-audit/round09/p6-batch1-closure-audit.json"], "limitations": "命令级证据：cc-semantic 全套件 ok, 0 failed（含 spec 14 单测与红绿测试 input_constructors_bind_digest_to_exact_bytes）。同维度不同模型不可混用已有单元契约；正式 V10/V16 验证矩阵证据属收口验收轮产物，本轮未运行。"}]
实施备注：2026-10-02 批次 1 收口：ENCODING_SPEC_VERSION=1 冻结面（常量/校验/三分 digest 公式）落地并以 ENCODING-SPACE.md 为权威规格文档；types.rs 收口构造器 + QuerySpecDigest 新类型；round09 批次审计确认冻结面签名层面无未声明能力。

### [x] P6-004｜扩展类型化write effects

状态：`done`；批次：`P6-A`；优先级：`normal`。
范围：`crates/cc-db/src/epoch_rules.rs`；`crates/cc-db/src/unit_of_work.rs`
硬依赖：P6-003
步骤：Index/Evidence/Semantic/Auxiliary封闭枚举；默认旧行为保留
交付物：扩展类型化write effects的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V13 验证证据（实施时生成）
验收：heartbeat不刷index，commit/rollback恰好推进预期epoch；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V13
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "batch1_typed_effects_landed_zero_assertion_change_green", "artifacts": ["crates/cc-db/src/epoch_rules.rs", "crates/cc-db/src/unit_of_work.rs", "artifacts/checkpoints/p6-batch1-20261002/P6-004-IMPLEMENTATION.md", "artifacts/checkpoints/todolist-completion-audit/round09/p6-batch1-closure-audit.json"], "limitations": "命令级证据：cc-db 全套件 ok（lib 155 passed + 1 ignored），epoch_rules 10 passed、unit_of_work 8 passed；cc-index 全量 377 lib 绿（唯一生产消费方）。既有测试零断言修改即绿（默认旧行为保留）。effect 级审计测试已固化；正式 V13 bench 证据属收口验收轮产物，本轮未运行。生产调用方计数经 P6-005 轮订正：唯一真实调用方 synthesis_pipeline.rs:110/125，其余为测试调用点。"}]
实施备注：2026-10-02 批次 1 收口：Index/Evidence/Semantic/Auxiliary 封闭枚举 EffectSet（commit_with）+ bump_semantic_epoch_on/rollback 不推进落地；round09 批次审计确认 heartbeat 不刷 index、commit/rollback 恰好推进预期 epoch 的命令级证据成立。

### [x] P6-005｜新表与schema初始化

状态：`done`；批次：`P6-A`；优先级：`normal`。
范围：`crates/cc-db/src/sql/`；`crates/cc-db/src/index_migrate.rs`
硬依赖：P6-004
步骤：加入document/manifest/outbox所需表与索引；按发布节点合并schema版本
交付物：新表与schema初始化的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V13, V21 验证证据（实施时生成）
验收：新旧DB有明确重建路径，FTS旧数据不半升级；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V13；V21
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "batch1_v22_schema_additive_migration_independently_reviewed_pass", "artifacts": ["crates/cc-db/src/sql/index_v1.sql", "crates/cc-db/src/index_migrate.rs", "crates/cc-db/tests/semantic_schema.rs", "artifacts/checkpoints/p6-batch1-20261002/P6-005-IMPLEMENTATION.md", "artifacts/checkpoints/todolist-completion-audit/round09/p6-batch1-closure-audit.json"], "limitations": "round09 独立评审 PASS：DDL 简报草案 9 语句逐句一致（3 表 7 索引）；git diff 确认 index_v1.sql 仅头注释+尾部追加、全文件 DDL 均带 IF NOT EXISTS（红线零旧对象改动）；幂等有测试覆盖（模块测试二次 migrate→UpToDate、集成测试文件库重开→UpToDate）并经临时库 python sqlite3 双重放实测（10 对象无重复、旧行保留）；SchemaStatus 消费点全覆盖（index_db 三态直通、cc-server matches!、cc-index 丢弃 status）经 cargo check --workspace 零错误。cc-db 复跑 158/157+1 ignored、0 failed；semantic_schema 6/6。迁移策略相对简报有一处已记录偏差（相邻 v21 原位迁移）。正式 V13/V21 验收矩阵与 V21 降级文档归 P6-019/验收轮，本轮不推断。"}]
实施备注：2026-10-02 批次 1 收口：v22 三表七索引 + CURRENT_SCHEMA_VERSION 22 + ADDITIVE_MIGRATION_FROM=21 原位加法迁移（SchemaStatus::Migrated 新变体）落地；round09 独立评审 PASS（DDL 一致/幂等实测/消费点覆盖/红线 diff/测试复跑五项全过）。

### [x] P6-006｜源码事务原子写outbox

状态：`done`；批次：`P6-B`；优先级：`normal`。
范围：`crates/cc-index/src/documents/delta.rs`；`crates/cc-db/src/semantic_outbox.rs`
硬依赖：P5-020, P6-004, P6-005
步骤：文档变化撤旧manifest并写desired任务；删除不发embedding
交付物：源码事务原子写outbox的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V13, V14 验证证据（实施时生成）
验收：提交后不会有新文档却无任务，rollback不泄露半个任务；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V13；V14
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "batch2_atomic_outbox_single_txn_local_green_effectset_wired", "artifacts": ["crates/cc-db/src/semantic_outbox.rs", "crates/cc-db/src/lib.rs", "crates/cc-db/src/index_db.rs", "crates/cc-db/src/index_db_write_batch.rs", "crates/cc-db/tests/semantic_outbox.rs", "artifacts/checkpoints/p6-batch2-20261002/P6-006-IMPLEMENTATION.md"], "limitations": "命令级证据：cargo test -p cc-db -p cc-index passed=666 failed=0（semantic_outbox 17 全绿：生产路径失败回滚零残留、同 doc 重写恰一 live embed、无 active 空间零变化零 bump、EXPLAIN QUERY PLAN 命中 semantic_out_ready 索引）；cargo test -p cc-server 249/41/9 全绿；cargo check --workspace 零 warning；EffectSet 首个生产调用方（{Index,Semantic} 组合 bump 恰一次实测）。评审留边界：写放大 50k 实测归验证轮（V14 正式证据归收口验收轮，本轮未运行不声明）；全量重建路径不挂接 outbox 归 P6-011/P6-014 换库协议；semantic_manifest 生产写入方归 P6-011。"}]
实施备注：2026-10-02 批次 2 收口（评审已 PASS，round10 落账）：单事务原子 outbox + EffectSet 生产接线落地；评审留边界——写放大 50k 实测归验证轮（V14 正式证据归收口验收轮），全量重建路径挂接归 P6-011/P6-014 换库协议，claim/lease 消费闭环见 P6-007。

### [x] P6-007｜实现claim与lease fencing

状态：`done`；批次：`P6-B`；优先级：`normal`。
范围：`crates/cc-db/src/semantic_outbox.rs`；`crates/cc-semantic/src/queue.rs`
硬依赖：P6-006
步骤：短事务claim/renew/retry；每attempt独立token
交付物：实现claim与lease fencing的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V14 验证证据（实施时生成）
验收：两进程不能同时发布相同lease，过期worker无法ack新lease；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V14
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "batch2_claim_cas_lease_fencing_local_green", "artifacts": ["crates/cc-db/src/semantic_outbox.rs", "crates/cc-db/tests/semantic_lease.rs", "artifacts/checkpoints/p6-batch2-20261002/P6-007-IMPLEMENTATION.md"], "limitations": "命令级证据：cargo test -p cc-db -p cc-index passed=675 failed=0（semantic_lease 9 个 fencing 不变式全绿：双连接 CAS 竞争恰一者胜出、过期 token 的 renew/ack/retry 全拒 Ok(false) 零写入、lease 生命周期 Auxiliary 零 epoch bump、retry 原子折叠 pending/failed）；cargo check --workspace 零 warning；clippy/fmt 干净。评审留边界：reclaim_expired_on 无界扫描的周期/有界编排归 P6-015；worker 侧封装（queue.rs LeaseGuard）与 IndexDb 写门面归 P6-013；真实 kill/restart 残态矩阵归 P6-015；V14 正式矩阵证据归验收轮。"}]
实施备注：2026-10-02 批次 2 收口（评审已 PASS，round10 落账）：claim CAS 单语句 + token fencing（renew/ack/retry/reclaim）落地，封闭表显式扩展 (Claimed,Pending)；评审留边界——reclaim_expired_on 无界扫描的周期/有界编排归 P6-015，worker 封装（queue.rs）与 IndexDb 写门面归 P6-013，真实 kill/restart 残态矩阵归 P6-015。

### [x] P6-008｜构建内容寻址artifact cache

状态：`done`；批次：`P6-B`；优先级：`normal`。
范围：`crates/cc-semantic/src/cache.rs`；`crates/cc-semantic/src/spec.rs`
硬依赖：P6-007
步骤：按namespace+input/spec存validated vector与checksum；无秘密字段
交付物：构建内容寻址artifact cache的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V10, V16 验证证据（实施时生成）
验收：同输入可复用，跨项目默认隔离且cache损坏可检测；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V10；V16
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "batch2_content_addressed_cache_local_green_depgraph_reverified", "artifacts": ["crates/cc-semantic/src/cache.rs", "crates/cc-semantic/src/types.rs", "crates/cc-semantic/src/lib.rs", "crates/cc-semantic/Cargo.toml", "crates/cc-semantic/tests/artifact_cache.rs", "Cargo.lock", "artifacts/checkpoints/p6-batch2-20261002/P6-008-IMPLEMENTATION.md"], "limitations": "命令级证据：cargo test -p cc-semantic lib 22 + artifact_cache 17 全绿；cargo check --workspace 零 warning；17 测试覆盖简报布局/跨克隆共享/跨项目隔离/损坏检测五分支/并发写收敛/无秘密键集恰等。依赖图证据 2026-10-02 收口轮订正：原命令 --no-dev-deps 非 cargo tree 有效 flag，经有效口径 cargo tree --workspace -e normal / cargo tree -p cc-server -e normal 复验，cc-semantic 依赖边计数 0 结论成立（workspace 树中唯一出现为成员根行）。评审留边界：Corrupt 检测不自动隔离（quarantine 搬移归 P6-018）；try_init 与组合根接线归后续接线轮；V10/V16 正式矩阵证据归验收轮。"}]
实施备注：2026-10-02 批次 2 收口（评审已 PASS，round10 落账）：内容寻址 cache（namespace+三 digest 寻址、读时验证链、原子写、可丢弃语义）落地；依赖图证据经收口轮订正复验（原 --no-dev-deps flag 无效，-e normal 有效口径下依赖边计数 0 成立）；评审留边界——Corrupt 不自动隔离（quarantine 归 P6-018），try_init/组合根接线归后续接线轮。

### [x] P6-009｜实现deterministic fake provider

状态：`done`；批次：`P6-B`；优先级：`normal`。
范围：`crates/cc-semantic/src/providers/fake.rs`；`crates/cc-semantic/src/ports.rs`
硬依赖：P6-008
步骤：注入固定向量、延迟、次数和故障；不发送网络
交付物：实现deterministic fake provider的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V15 验证证据（实施时生成）
验收：测试可确定重现全部状态转移，fake结果不算真实语义质量；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V15
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "batch2_deterministic_fake_provider_local_green_golden_pinned", "artifacts": ["crates/cc-semantic/src/providers.rs", "crates/cc-semantic/src/providers/fake.rs", "crates/cc-semantic/src/lib.rs", "crates/cc-semantic/Cargo.toml", "Cargo.lock", "artifacts/checkpoints/p6-batch2-20261002/P6-009-IMPLEMENTATION.md"], "limitations": "命令级证据：cargo test -p cc-semantic lib 38（含本轮 16）+ artifact_cache 17 零回归全绿；cargo check --workspace 零 warning；golden fixture 钉死算法 v1 前四分量 to_bits，流水线漂移即强制版本 bump；六变体 ProviderError 全部可注入原样返回（P6-016 故障矩阵复用面）；输出门禁拒 NaN/Inf/全零（与 P6-008 cache 侧双层设防）。永不触网（实现容器零网络调用）。评审留边界：fake 结果不算真实语义质量（模块文档明示）；crash 断点钩子归 P6-015；真实 provider 与双轨报告归 P7 线（用户 2026-10-02 决策）；V15 正式矩阵证据归验收轮。"}]
实施备注：2026-10-02 批次 2 收口（评审已 PASS，round10 落账）：deterministic fake provider（blake3 XOF 确定性向量 + 五字段故障脚本 + 输出门禁）落地，golden fixture 钉死 v1 算法；评审留边界——fake 结果不算真实语义质量，crash 断点钩子归 P6-015，真实 provider 与双轨报告归 P7 线（用户 2026-10-02 决策）。

### [x] P6-010｜实现filtered exact向量backend

状态：`done`；批次：`P6-B`；优先级：`normal`。
范围：`crates/cc-semantic/src/vector/exact.rs`
硬依赖：P6-009
步骤：限定空间、范围先过滤、bounded batch、稳定top-k；保留数值gold
交付物：实现filtered exact向量backend的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V16 验证证据（实施时生成）
验收：手算cosine一致，删除/不同空间不可返回，内存受控；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V16
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "batch2_filtered_exact_vector_backend_local_green_cosine_only", "artifacts": ["crates/cc-db/src/semantic_manifest_reads.rs", "crates/cc-db/src/lib.rs", "crates/cc-semantic/src/vector.rs", "crates/cc-semantic/src/vector/exact.rs", "crates/cc-semantic/src/lib.rs", "crates/cc-semantic/Cargo.toml", "crates/cc-semantic/tests/manifest_exact_integration.rs", "Cargo.lock", "artifacts/checkpoints/p6-batch2-20261002/P6-010-IMPLEMENTATION.md"], "limitations": "命令级证据：cargo test -p cc-semantic 54 lib + artifact_cache 17 + manifest_exact_integration 4 全绿；cargo test -p cc-db 162 lib 零回归；cargo check --workspace 零 warning；手算 cosine gold（含 0.96 手算值）与 f64 顺序累加参考实现对拍一致；删除/异空间不可返回、Some(empty) 永不退化全仓、(desirability,doc_key) tie-break 稳定、bounded batch 内存上界均测试固化；TDD 期修出 TopK 双分量排序写反与手算对拍 2 处真缺陷。评审留边界：Cosine-only（冻结 spec v1 仅 admit Cosine，L2/内积未实现，新变体编译失败强制显式扩展）；exact 仅小规模 oracle backend，ANN 归 V22 可选轨道；V16 正式矩阵证据归验收轮。"}]
实施备注：2026-10-02 批次 2 收口（评审已 PASS，round10 落账）：filtered exact 向量 backend（范围先过滤、f64 cosine、bounded batch、稳定 tie-break、cc-db 只读适配器）落地，手算 gold 对拍一致；评审留边界——Cosine-only（冻结 spec v1 仅 admit Cosine，L2/内积未实现），exact 仅小规模 oracle backend，ANN 归 V22 可选轨道。

### [x] P6-011｜artifact到manifest发布CAS

状态：`done`；批次：`P6-C`；优先级：`normal`。
范围：`crates/cc-semantic/src/publish.rs`；`crates/cc-db/src/semantic_outbox.rs`
硬依赖：P5-020, P6-006, P6-007, P6-008, P6-010
步骤：先持久化artifact，再校验incarnation/lease/doc/input/space写manifest
交付物：artifact到manifest发布CAS的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V14 验证证据（实施时生成）
验收：慢旧结果不能挂到同路径新版本，发布幂等；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V14
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "batch3_publish_cas_five_fence_q4_local_green", "artifacts": ["crates/cc-db/src/semantic_publish.rs", "crates/cc-db/src/lib.rs", "crates/cc-semantic/src/publish.rs", "crates/cc-semantic/src/lib.rs", "crates/cc-db/tests/semantic_publish.rs", "crates/cc-semantic/tests/publish_cas.rs", "artifacts/checkpoints/p6-batch3-20261002/P6-011-IMPLEMENTATION.md"], "limitations": "命令级证据：批次 3 实施轮 cargo test -p cc-db -p cc-semantic 全绿（semantic_publish 11 + publish_cas 4 = 15 测试：五 fence 逐条拒绝语义、Q4 重复发布零 bump、fenced retry 兜底、Auxiliary 三钟全静）；收口轮复跑 cargo test -p cc-db -p cc-semantic 24 个 test result 全 ok + cargo check --workspace 干净。评审留边界：V14 正式验证矩阵证据不推断，归验收轮；发布编排（claim→embed→publish 周期）归 P6-013。"}]
实施备注：2026-10-02 批次 3 收口（评审已 PASS，round11 落账）：五 fence 发布 CAS + Q4 可见集合判定 + artifact durability 先后序落地（cc-db semantic_publish 原语 + cc-semantic Publisher 三步编排，15 测试全绿）；评审留边界——V14 正式矩阵证据归验收轮、发布 worker 编排归 P6-013、跨实例 ghost fence 归 P6-014（已落地）。

### [x] P6-012｜覆盖率与semantic epoch

状态：`done`；批次：`P6-C`；优先级：`normal`。
范围：`crates/cc-server/src/capability_status.rs`；`crates/cc-db/src/epoch_rules.rs`
硬依赖：P6-011
步骤：统计eligible/published/failed/stale并分母明确；只可见集合变化bump
交付物：覆盖率与semantic epoch的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V13, V18 验证证据（实施时生成）
验收：零eligible有原因，aux重试不冲刷完整查询缓存；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V13；V18
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "batch3_coverage_readmodel_epoch_local_green", "artifacts": ["crates/cc-db/src/semantic_coverage.rs", "crates/cc-db/src/lib.rs", "crates/cc-db/tests/semantic_coverage.rs", "crates/cc-server/src/handlers/context.rs", "crates/cc-server/src/handlers/freshness.rs", "artifacts/checkpoints/p6-batch3-20261002/P6-012-IMPLEMENTATION.md"], "limitations": "命令级证据：批次 3 实施轮 cargo test -p cc-db -p cc-semantic 全绿（semantic_coverage 8 模块级 + 3 门面级：口径分母、epoch 读侧一致、零 eligible 有因、uncovered keyset 分页）；收口轮复跑全绿。评审留边界：V13/V18 正式验证矩阵证据归验收轮；dense lane 对覆盖率的消费（范围声明/查询守卫）归后续接线轮；收口动作①（模块文档措辞统一）移交 P6-019 文档轮。"}]
实施备注：2026-10-02 批次 3 收口（评审已 PASS，round11 落账）：覆盖率口径（eligible/published/failed/stale 分母明确）+ semantic epoch 读侧一致性 + 只可见集合变化 bump 落地（semantic_coverage 真模块 + ReadOps facet，11 测试全绿）；收口动作①移交——模块文档措辞统一归 P6-019 文档轮；stale 恒 0 守卫面与 eligible 50k 基准边界见实施记录 §8；V13/V18 正式证据归验收轮。

### [x] P6-013｜worker资源与连续编辑合并

状态：`done`；批次：`P6-C`；优先级：`normal`。
范围：`crates/cc-semantic/src/worker.rs`；`crates/cc-semantic/src/admission.rs`
硬依赖：P6-012
步骤：pending合并、队列上限、公平批次和有界关闭；旧任务supersede
交付物：worker资源与连续编辑合并的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V14, V20 验证证据（实施时生成）
验收：大量保存不导致无限排队，任何时刻local查询可用；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V14；V20
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "batch3_worker_primitives_leaseline_local_green", "artifacts": ["crates/cc-db/src/semantic_queue.rs", "crates/cc-db/src/lib.rs", "crates/cc-db/tests/semantic_queue.rs", "crates/cc-semantic/src/queue.rs", "crates/cc-semantic/src/lib.rs", "crates/cc-semantic/tests/queue_worker.rs", "artifacts/checkpoints/p6-batch3-20261002/P6-013-IMPLEMENTATION.md"], "limitations": "命令级证据：批次 3 实施轮 cargo test -p cc-db -p cc-semantic 全绿（semantic_queue 门面 2 + queue_worker 端到端 8 + 模块级 8：合并语义、liveness 门、LeaseGuard renew、Drop=零 DB I/O kill 等价）；收口轮复跑全绿。评审留边界：组合根接线（cc-server 调用点）不做归接线轮；V14/V20 正式验证矩阵证据归验收轮。"}]
实施备注：2026-10-02 批次 3 收口（评审已 PASS，round11 落账）：worker 原语全量落地（cc-db semantic_queue 写门面 claim/renew/retry/reclaim + cc-semantic LeaseGuard/drain_pending/EmbedHandler，18 测试全绿）；评审留边界——组合根接线与公平批次 ORDER BY 注入（偏差 3）、机会性 reclaim 收窄（偏差 7）归接线轮一并评审；V14/V20 正式证据归验收轮。

### [x] P6-014｜换库incarnation与缓存重用

状态：`done`；批次：`P6-C`；优先级：`normal`。
范围：`crates/cc-db/src/index_db_rebuild.rs`；`crates/cc-semantic/src/reconcile.rs`
硬依赖：P6-013
步骤：staging重建更换身份；从artifact补manifest；旧worker fenced
交付物：换库incarnation与缓存重用的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V13, V17 验证证据（实施时生成）
验收：索引重建不误删已付费向量、不接受旧DB时代回包；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V13；V17
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "batch3_incarnation_fence_reconcile_local_green", "artifacts": ["crates/cc-db/src/semantic_rebuild.rs", "crates/cc-db/src/lib.rs", "crates/cc-db/tests/semantic_rebuild.rs", "crates/cc-semantic/src/reconcile.rs", "crates/cc-semantic/src/lib.rs", "crates/cc-semantic/tests/reconcile_rebuild.rs", "artifacts/checkpoints/p6-batch3-20261002/P6-014-IMPLEMENTATION.md"], "limitations": "命令级证据：批次 3 实施轮 cargo test -p cc-db -p cc-semantic 全绿（semantic_rebuild 3 + reconcile_rebuild 3 + 模块级：幽灵 fenced 零写入、付费向量只付一次、desired 投影）；收口轮复跑全绿。评审留边界：V13/V17 正式验证矩阵证据归验收轮；DirectWriter 重建路径未单测（fence 对路径不敏感）；组合根接线归接线轮。"}]
实施备注：2026-10-02 批次 3 收口（评审已 PASS，round11 落账）：换库 incarnation 权威路径 fence（generation_at_path fresh 只读连接 strict ReadGeneration）+ reconcile 三步补齐 + cache 重用落地（9 测试全绿）；收口轮已补 TOCTOU 边界声明（publish_semantic_fenced，owner 串行集成模型外需外层串行化）与 generation_at_path busy_timeout fail-stop 方向注明；重入队 bump 口径张力（偏差 5）移交后续裁决；V13/V17 正式证据归验收轮。

### [x] P6-015｜崩溃恢复扫描

状态：`done`；批次：`P6-C`；优先级：`normal`。
范围：`crates/cc-semantic/src/reconcile.rs`；`crates/cc-eval/tests/semantic_lifecycle.rs`
硬依赖：P6-014
步骤：对每个persist边界kill/restart；重认过期lease与缺失artifact
交付物：崩溃恢复扫描的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V17 验证证据（实施时生成）
验收：恢复有界且可复算，已存artifact优先复用；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V17
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "batch3_bounded_recovery_handback_primitive_local_green", "artifacts": ["crates/cc-db/src/semantic_recovery.rs", "crates/cc-db/src/lib.rs", "crates/cc-db/tests/semantic_recovery.rs", "crates/cc-semantic/src/recovery.rs", "crates/cc-semantic/src/lib.rs", "crates/cc-semantic/tests/semantic_recovery.rs", "artifacts/checkpoints/p6-batch3-20261002/P6-015-IMPLEMENTATION.md"], "limitations": "命令级证据：批次 3 实施轮 cargo test -p cc-db -p cc-semantic 全绿（bounded reclaim 分页收敛、dead-letter 只清点、desired keyset 分页、5 个 kill 模拟端到端）；收口动作②新增原语单测 hand_back_primitive_fences_and_never_consumes_an_attempt（stale token/未知 id 拒绝零写入、成功回 pending attempt 不变、终态 fence、Auxiliary）与集成测试 cache_miss_hand_back_spends_no_attempt_budget（max_attempts=1 下 miss hand-back 不死信、预算完整留给 worker）全绿；收口轮复跑 cc-db+cc-semantic 24 个 test result 全 ok + cargo check --workspace 干净。评审留边界：V17 真实子进程 SIGKILL 正式化归验收轮（当前为进程内 kill 模拟）；dead_letter 页帽化（偏差 6）移交。"}]
实施备注：2026-10-02 批次 3 收口（评审已 PASS，round11 落账）：有界恢复原语 + recover_scan 编排落地（kill 模拟多轮收敛、已存 artifact 优先复用、死信只清点、ghost fence，11 测试全绿）；收口动作②已落地——cc-db 新增 hand_back_semantic_task 不计 attempt 的 fenced claimed→pending 直写原语，cache-miss hand-back 切换至该原语（原语单测 + 集成测试不烧 attempt）；desired_set_bounded 不变式前提文档声明已加；V17 真实子进程 SIGKILL 正式化归验收轮。

### [x] P6-016｜GC与发布协调

状态：`done`；批次：`P6-D`；优先级：`normal`。
范围：`crates/cc-semantic/src/cache.rs`；`crates/cc-semantic/src/publish.rs`
硬依赖：P5-020, P6-011, P6-015
步骤：mark/sweep、活跃引用/lease和最短保留期；GC跟publish共同协调
交付物：GC与发布协调的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V17 验证证据（实施时生成）
验收：没有manifest引用刚GC删除产物的竞态，孤儿最终可回收；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V17
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "batch4_gc_publish_coordination_local_green", "artifacts": ["crates/cc-db/src/semantic_gc_reads.rs", "crates/cc-db/src/lib.rs", "crates/cc-semantic/src/gc.rs", "crates/cc-semantic/src/lib.rs", "crates/cc-semantic/tests/semantic_gc.rs", "artifacts/checkpoints/p6-batch4-20261002/P6-016-IMPLEMENTATION.md"], "limitations": "命令级证据：批次 4 实施轮 cargo test -p cc-semantic -p cc-db 全绿（semantic_gc 8 集成：引用保护/宽限回收/收集-清扫交错快照负测试/对偶宽限窗口/有界收敛/corrupt 清扫/temp-半文件+空目录修剪且 quarantine 零触碰/revoked space 保护；semantic_gc_reads 3 内联单测），收口轮复跑全绿。偏差：sweep 落新模块 gc.rs、同步点快照落 cc-db semantic_gc_mark（不改 P6-008/011 密封交付物）。评审留边界：min_retention_secs 配置面与非零下限校验、GC 审计计数落库归接线轮（接线轮 13 项待办表见 artifacts/checkpoints/todolist-completion-audit/round12/p6-batch4-closure-audit.json）；V17 正式验证矩阵证据归验收轮。"}]
实施备注：2026-10-02 批次 4 收口（评审已 PASS，round12 落账）：GC mark/sweep + 一次短读快照同步点 + 3600s 宽限落地（8 集成 + 3 内联单测全绿，quarantine 结构性不可触及有测试守护）；收口边界——宽限参数配置面与非零下限校验、GC 审计落库归接线轮（13 项待办表见 round12 审计文件），V17 正式证据归验收轮。

### [x] P6-017｜model space切换规划

状态：`done`；批次：`P6-D`；优先级：`normal`。
范围：`crates/cc-semantic/src/spec.rs`；`crates/cc-semantic/src/reconcile.rs`
硬依赖：P6-016
步骤：新空间回填/切active/撤销；记录用户revision与未pin限制
交付物：model space切换规划的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V16, V17 验证证据（实施时生成）
验收：切换时不把不同空间分数混排，旧cache可回滚复用；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V16；V17
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "batch4_space_switch_protocol_local_green", "artifacts": ["crates/cc-db/src/semantic_space_switch.rs", "crates/cc-db/src/lib.rs", "crates/cc-db/tests/semantic_space_switch.rs", "crates/cc-semantic/src/space_switch.rs", "crates/cc-semantic/src/lib.rs", "crates/cc-semantic/tests/space_switch.rs", "artifacts/checkpoints/p6-batch4-20261002/P6-017-IMPLEMENTATION.md"], "limitations": "命令级证据：批次 4 实施轮 cargo test -p cc-db -p cc-semantic 全绿（cc-db semantic_space_switch 10：三边闭转换表/单事务五步切换/revoke 生产消费 own-space 守卫/pinned:false 审计键/epoch 守卫；cc-semantic space_switch 2 端到端：回滚复用 provider 0 调用、revoke 后 GC 衔接），收口轮复跑全绿。偏差：新模块 semantic_space_switch/space_switch（不改简报归属的密封交付物）、首次激活不 bump（P6-004 None=not ready 红线优先）。评审留边界：切换组合根触发面与 semantic_space_switch_log 上限归接线轮（13 项待办表见 artifacts/checkpoints/todolist-completion-audit/round12/p6-batch4-closure-audit.json）；V16/V17 正式证据归验收轮。"}]
实施备注：2026-10-02 批次 4 收口（评审已 PASS，round12 落账）：model space 三段切换（回填/单事务切 active/撤销回滚复用）+ revoke 第二生产消费者 + metadata 审计键落地（12 测试全绿，V16 分数不混排/V17 回滚零付费有端到端断言）；收口边界——组合根触发面与切换日志上限归接线轮（13 项待办表见 round12 审计文件），V16/V17 正式证据归验收轮。

### [x] P6-018｜cache缺失/损坏降级

状态：`done`；批次：`P6-D`；优先级：`normal`。
范围：`crates/cc-semantic/src/cache.rs`；`crates/cc-server/src/capability_status.rs`
硬依赖：P6-017
步骤：隔离坏记录、语义degraded、本地继续；补嵌受费用策略控制
交付物：cache缺失/损坏降级的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V17, V18 验证证据（实施时生成）
验收：不把缺向量当完整空结果，不静默无界重费；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V17；V18
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "batch4_cache_degrade_budget_local_green", "artifacts": ["crates/cc-semantic/src/degrade.rs", "crates/cc-semantic/src/lib.rs", "crates/cc-semantic/tests/semantic_degrade.rs", "crates/cc-server/src/service_factory.rs", "crates/cc-server/src/capability_status.rs", "artifacts/checkpoints/p6-batch4-20261002/P6-018-IMPLEMENTATION.md"], "limitations": "命令级证据：批次 4 实施轮 cargo test -p cc-semantic -p cc-server -p cc-db 全绿（semantic_degrade 5 端到端：腐坏隔离→预算内补嵌→put 覆盖自愈回路、claimed 交还零 attempt 消耗、预算耗尽死信且 provider 调用停在拒绝点、GC 多轮 quarantine 字节级零触碰、检索降级矩阵；degrade 4 内联 + capability_status 2 单测），收口轮复跑全绿。偏差：quarantine 落新模块 degrade.rs（P6-008 封存红线）、degraded 透出经 QueryServices 可选槽（默认构建零依赖 cc-semantic）。评审留边界：组合根接线（degrade 门面调用点、降级快照转写）归接线轮（13 项待办表见 artifacts/checkpoints/todolist-completion-audit/round12/p6-batch4-closure-audit.json）；DegradationLedger 跨进程持久化如需亦归接线轮；V17/V18 正式证据归验收轮。"}]
实施备注：2026-10-02 批次 4 收口（评审已 PASS，round12 落账）：cache 缺失/损坏降级落地——quarantine 双半隔离+诊断 sidecar、re-embed 预算整批准入（拒绝先于 provider 调用、超限 failed 死信带 reason）、degraded 判据与透出槽（11 测试全绿，自愈回路端到端固化）；收口边界——组合根接线（门面调用点/快照转写）归接线轮（13 项待办表见 round12 审计文件），V17/V18 正式证据归验收轮。

### [x] P6-019｜更新存储/恢复/配置文档

状态：`done`；批次：`P6-D`；优先级：`normal`。
范围：`docs/internals/STORAGE.md`；`docs/internals/CONCURRENCY.md`；`docs/TROUBLESHOOTING.md`
硬依赖：P6-018
步骤：写清at-least-once、两库顺序、恢复步骤与namespace
交付物：更新存储/恢复/配置文档的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V21 验证证据（实施时生成）
验收：不宣称跨模型/两库exactly-once或零重复收费；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V21
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "batch4_doc_charter_sync_troubleshooting_backfilled", "artifacts": ["DESIGN.md", "docs/internals/STORAGE.md", "docs/internals/CONCURRENCY.md", "docs/internals/INCREMENTAL_RECOVERY.md", "docs/CONFIGURATION.md", "docs/ARCHITECTURE.md", "docs/TROUBLESHOOTING.md", "crates/cc-db/src/semantic_coverage.rs", "crates/cc-db/tests/semantic_space_switch.rs", "artifacts/checkpoints/p6-batch4-20261002/P6-019-IMPLEMENTATION.md"], "limitations": "命令级证据：实施轮 cargo test -p cc-db 全绿（TempDirGuard pid+单调计数修复后 semantic_space_switch 10/10 含两 consume_revoke 用例）+ cargo check --workspace 干净；文档锚点逐条 grep 复核为当前行号、表数 29 基表+5 FTS5 经 CREATE TABLE 计数复核。交付：章程限定式单库表述（DESIGN/STORAGE，ADR-0003 限定修订）+ schema v22 事实订正、CONCURRENCY 两小节、INCREMENTAL_RECOVERY 语义恢复事实节、CONFIGURATION 语义缓存与降级节、semantic_coverage 措辞统一（epoch 声明式口径）、TempDirGuard flaky 修复。TROUBLESHOOTING 偏差 1 移交已由批次 4 收口轮兑现：新增语义缓存损坏/缺失自愈、CODECORTEX_SEMANTIC_CACHE_ROOT 排查、re-embed 预算死信识别与处置、换库语义重建与付费向量复用四条用户排障条目（自 STORAGE crash 点表与 CONFIGURATION 降级语义摘编，未接线处如实注明）。epoch_rules 表→钟映射为声明式口径已在 STORAGE 如实区分（语义三表效应归属为提交点声明，非静态枚举行）。评审留边界：组合根接线如实标注三处；V21 正式验证矩阵证据归验收轮。"}]
实施备注：2026-10-02 批次 4 收口（评审已 PASS，round12 落账）：章程同步（DESIGN/STORAGE 限定式单库 + schema v22 事实）、CONCURRENCY/INCREMENTAL_RECOVERY/CONFIGURATION 更新、coverage 措辞统一（epoch 随语义状态推进、Auxiliary 永不推进的声明式口径）、TempDirGuard flaky 修复落地；TROUBLESHOOTING 缺口已由收口轮补齐（四条用户排障条目，组合根未接线处如实注明）；V21 正式证据归验收轮。

### [x] P6-020｜P6无网络语义底座验收

状态：`done`；批次：`P6-D`；优先级：`blocking`。
范围：`docs/roadmap/code-index-v2/`；`artifacts/benchmarks/`
硬依赖：P6-001, P6-002, P6-003, P6-004, P6-005, P6-006, P6-007, P6-008, P6-009, P6-010, P6-011, P6-012, P6-013, P6-014, P6-015, P6-016, P6-017, P6-018, P6-019
步骤：G6使用fake执行故障矩阵和exact oracle；检查依赖图/默认包
交付物：P6无网络语义底座验收的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V13, V14, V16, V17, V18 验证证据（实施时生成）
验收：publish/fencing/GC/rebuild闭环，仍未冒充真实provider效果；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V13；V14；V16；V17；V18
回滚：禁用语义worker并撤销新manifest；保留派生cache，按incarnation重建索引。
证据：[{"status": "batch5_acceptance_library_layer_g6_passed_declared_local_scope", "artifacts": ["docs/roadmap/code-index-v2/P6-GATE.json", "artifacts/checkpoints/p6-acceptance-20261002/E1-dependency-default-package.json", "artifacts/checkpoints/p6-acceptance-20261002/E2-fault-matrix-exact-oracle-RECEIPT.json", "artifacts/checkpoints/p6-acceptance-20261002/v18-probe-default-binary.json", "artifacts/checkpoints/p6-acceptance-20261002/regression-workspace.log", "artifacts/checkpoints/p6-acceptance-20261002/EXECUTION-PROGRESS.md", "artifacts/checkpoints/todolist-completion-audit/round12/p6-batch4-closure-audit.json", "artifacts/checkpoints/todolist-completion-audit/round13/p6-acceptance-audit.json"], "limitations": "命令级证据（2026-10-02，head 0de7c890）：①依赖图/默认包——cargo tree -p cc-server -e normal 默认 cc-semantic 边计数 0、--features semantic 计 1（optional+feature，P6-002 允许语义）；cc-semantic 传递闭包零网络客户端 crate；默认/semantic 双口径 cargo build --locked --offline 均 exit 0；默认构建无 cache 目录无第二库文件。②故障矩阵+exact oracle 复放——cargo test -p cc-semantic --locked --offline 两轮 117 passed/0 failed（lib 60 含 fake 六变体 ProviderError 故障矩阵与 exact oracle 16 单测；9 集成套件 57 覆盖 publish CAS/fencing/GC/recovery/degrade/space switch）。③V18 探针——默认二进制 stdio：14 工具契约在册、semantic_state=not_configured、dense_state=disabled、零网络零 key。④全量回归——SDKROOT=… cargo test --workspace --locked --offline exit 0：2051 passed/0 failed/60 ignored（118 套件）。裁决=G6 passed_declared_local_scope_library_layer；边界如实声明：库层完成、组合根未接线（接线轮 13 项待办表，权威位置 round12 审计）、semantic 默认 disabled；带可重放 seed 的全故障矩阵正式回归与真实子进程 SIGKILL 级正式化归属 P7-016（本轮 not_run 进程级，不越权）；fake 完成不冒充真实 provider 效果；不推断 P7+ 任何门。"}]
实施备注：2026-10-02 批次 5 验收（G6）：fake 故障矩阵+exact oracle 全量复放（117/0 两轮）、依赖图/默认包双口径检查（默认树 0 边、无网络 crate、双构建 exit 0）、V18 默认二进制探针（14 工具、semantic disabled/not_configured）、workspace 全量回归 2051/0/60 exit 0。G6=passed_declared_local_scope_library_layer，定稿于 docs/roadmap/code-index-v2/P6-GATE.json。边界：组合根未接线（13 项待办表）、进程级故障正式化归 P7-016、真实 provider 归 P7 线。

## P7｜provider与dense端到端

### [x] P7-001｜实现OpenAI-compatible provider适配

状态：`done`；批次：`P7-A`；优先级：`normal`。
范围：`crates/cc-semantic/src/providers/openai_compatible.rs`；`crates/cc-semantic/src/ports.rs`
硬依赖：P6-020
步骤：明确endpoint/base路径和认证；trait不暴露具体客户端类型
交付物：实现OpenAI-compatible provider适配的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V15 验证证据（实施时生成）
验收：协议stub覆盖成功/错误，provider可替换；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V15
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：[{"status": "batch1_openai_compatible_provider_mock_leg_green", "artifacts": ["crates/cc-semantic/src/providers/openai_compatible.rs", "crates/cc-semantic/src/providers.rs", "artifacts/checkpoints/p7-impl-20261002/P7-001-IMPLEMENTATION.md"], "limitations": "命令级证据：批次 1 实施轮 cargo test -p cc-semantic --locked --offline 全绿（lib 84 passed 含 24 项适配层测试 + 集成套件全绿）+ cargo check --workspace --locked --offline 干净；零网络——cc-semantic 依赖表未动（无 HTTP crate），唯一 transport 为内存 mock，未注入 transport 时不存在任何网络代码路径。双轨口径：mock 腿 done；live 腿 blocked（本轮不授权真实 provider），V15 正式验证矩阵证据 not_run，归 live 解封/验收轮。评审遗留：openai_compatible.rs:218 lint 顺延 hygiene 轮。"}]
实施备注：2026-10-02 批次 1 收口（评审已 PASS，round14 落账）：OpenAI-compatible 适配层落地——注入式 EmbeddingHttpTransport seam、EmbeddingApiKey 凭据 newtype（Debug 脱敏/无 Display）、六变体 ProviderError 映射、严格响应结构校验、默认 None=fail-closed disabled，24 项 mock 测试全绿；双轨口径——mock 腿 done、live 腿 blocked（用户 2026-10-02 D1/D2 不授权真实 provider），V15 证据 not_run 归 live 解封；评审遗留：openai_compatible.rs:218 lint 顺延 hygiene 轮。

### [x] P7-002｜模型参数与能力验证

状态：`done`；批次：`P7-A`；优先级：`normal`。
范围：`crates/cc-semantic/src/spec.rs`；`crates/cc-model/src/config.rs`
硬依赖：P7-001
步骤：模型revision、dimensions、metric、query instruction显式校验；支持差异不能吞
交付物：模型参数与能力验证的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V15, V18 验证证据（实施时生成）
验收：供应商不支持dimensions时给错误/配置路径，不伪成功；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V15；V18
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：[{"status": "batch1_model_capability_probe_mock_leg_green", "artifacts": ["crates/cc-semantic/src/capability.rs", "crates/cc-model/src/config.rs", "artifacts/checkpoints/p7-impl-20261002/P7-002-IMPLEMENTATION.md"], "limitations": "命令级证据：批次 1 实施轮 cargo test -p cc-semantic 全绿（lib 108 passed 含 24 项 capability）+ cargo test -p cc-model 全绿（82 passed 含 2 项 semantic 配置节）+ cargo check --workspace 干净；零网络——全部 offline 锁定运行，唯一 transport 为内存 mock。偏差：SemanticProviderConfig 落 cc-model/config.rs 而非 spec.rs，spec.rs 仅引用零改动。双轨口径：mock 腿 done；live 腿 blocked（本轮不授权真实 provider），V15/V18 证据 not_run 归 live 解封/验收轮。评审遗留观察：capability classify 结构化标记维持现状，精化归 hygiene 轮。"}]
实施备注：2026-10-02 批次 1 收口（评审已 PASS，round14 落账）：能力探测协议落地——capability.rs ModelCapability/DimensionsMode/PROBE_PROTOCOL_VERSION + validate_capability（revision/dimensions/metric/query instruction 显式校验，不伪成功）+ cc-model 首组语义配置键，24 项 capability 测试全绿；偏差如实入账——SemanticProviderConfig 落 cc-model/config.rs，spec.rs 冻结面零改动（新增类型扩展）；双轨口径——mock 腿 done、live 腿 blocked，V15/V18 证据 not_run 归 live 解封；评审遗留观察：capability classify 结构化标记（Mismatch/Unavailable 归类）维持现状，精化归 hygiene 轮。

### [x] P7-003｜真实输入尺寸与批次规划

状态：`done`；批次：`P7-A`；优先级：`normal`。
范围：`crates/cc-semantic/src/admission.rs`；`crates/cc-index/src/documents/render.rs`
硬依赖：P7-002
步骤：按最终输入计token/bytes/batch；超限重切或明确skip而非平均池化掩盖
交付物：真实输入尺寸与批次规划的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V09, V15 验证证据（实施时生成）
验收：每项和总batch受限，文本与向量所代表文档版本一致；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V09；V15
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：[{"status": "batch1_input_budget_batch_planning_mock_leg_green", "artifacts": ["crates/cc-semantic/src/admission.rs", "crates/cc-index/src/documents/render.rs", "artifacts/checkpoints/p7-impl-20261002/P7-003-IMPLEMENTATION.md"], "limitations": "命令级证据：批次 1 实施轮 cargo test -p cc-semantic 全绿（lib 124 passed 含 16 项 admission）+ cargo test -p cc-index 全绿（lib 379 passed 含 2 项 render manifest 测试）+ cargo check --workspace 干净；零网络——依赖表零改动，适配层集成测试唯一 transport 为内存 RecordingTransport。口径声明：admission 全部度量基于 render 产物最终 bytes；admission 层零截断（超限产出显式 Skipped{key,reason}）；token 为声明估算器估算而非精确分词。双轨口径：mock 腿 done；live 腿 blocked（本轮不授权真实 provider），V09/V15 证据 not_run 归 live 解封/验收轮。评审遗留：admission.rs:291 clippy loop-index 顺延 hygiene 轮。"}]
实施备注：2026-10-02 批次 1 收口（评审已 PASS，round14 落账）：批次规划器落地——InputBudget 三界（items/硬 bytes/估算 tokens）+ render 最终输入 bytes manifest 消费 + first-fit 确定性切分（plan_order）+ 超限显式 Skipped 不掩盖，16 项 admission + 2 项 render manifest 测试全绿；估算口径如实声明（无 tokenizer 不冒充精确 token 数）；双轨口径——mock 腿 done、live 腿 blocked，V09/V15 证据 not_run 归 live 解封；评审遗留：admission.rs:291 clippy loop-index（P7-003 段 plan_order）顺延 hygiene 轮。

### [x] P7-004｜响应强校验

状态：`done`；批次：`P7-A`；优先级：`normal`。
范围：`crates/cc-semantic/src/providers/openai_compatible.rs`
硬依赖：P7-003
步骤：数量/index完整且唯一、dim、finite、norm检查；错误向量不缓存
交付物：响应强校验的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V15, V16 验证证据（实施时生成）
验收：乱序可正确恢复，重复index/NaN/zero必被拒绝；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V15；V16
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：[{"status": "batch1_strict_response_validation_mock_leg_green", "artifacts": ["crates/cc-semantic/src/providers/openai_compatible.rs", "artifacts/checkpoints/p7-impl-20261002/P7-004-IMPLEMENTATION.md"], "limitations": "命令级证据：批次 1 实施轮 cargo test -p cc-semantic --locked --offline 全绿（lib 140 passed = 前 124 + 新增 16，适配层合计 40 项）+ cargo check --workspace 干净；零网络——依赖表零改动，唯一 transport 为内存 mock；响应体/请求体/凭据零泄漏由测试固化。评审遗留：redirect 验收条款（live 供应商重定向行为验证）归 live 腿解封——适配层现行语义为 3xx 一律非重试 InvalidInput、永不跟随重定向、不泄露凭据；V15/V16 正式证据 not_run 归 live 解封/验收轮。"}]
实施备注：2026-10-02 批次 1 收口（评审已 PASS，round14 落账）：响应四层强校验门落地——NormPolicy 可配 norm 策略 + 数量/index 恰 0..n（乱序重排/缺口/重复拒绝）+ 维度/finite/零向量拒绝 + model 回显必需比对，新增 16 项强校验测试（适配层合计 40，P7-001 24 项断言零放松）；双轨口径——mock 腿 done、live 腿 blocked，V15/V16 证据 not_run 归 live 解封；评审遗留：redirect 验收条款归 live 解封（适配层 3xx 一律拒绝不跟随，live 供应商 redirect 行为验证 blocked），openai_compatible.rs:218 lint 顺延 hygiene 轮。

### [x] P7-005｜全局与项目并发限流

状态：`done`；批次：`P7-A`；优先级：`normal`。
范围：`crates/cc-semantic/src/admission.rs`；`crates/cc-server/src/service_factory.rs`
硬依赖：P7-004
步骤：共享provider级限额和项目公平队列；避免每调用创建独立无限信号量
交付物：全局与项目并发限流的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V15, V20 验证证据（实施时生成）
验收：多项目总并发仍受限，单项目不能饿死其他索引；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V15；V20
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：[{"status": "batch1_concurrency_gate_fairness_mock_leg_green", "artifacts": ["crates/cc-semantic/src/admission.rs", "crates/cc-server/src/service_factory.rs", "artifacts/checkpoints/p7-impl-20261002/P7-005-IMPLEMENTATION.md"], "limitations": "命令级证据：批次 1 实施轮 cargo test -p cc-semantic -p cc-model --locked --offline 全绿（307 passed/0 failed，含 16 项 admission gate 测试：单项目不可饿死、全局上限约束总并发、公平队列防插队、pause 语义、超时显式化、Send+Sync 共享）；clippy 本轮新增告警零；组合根单例挂点默认构建维持 no-network closure。双轨口径：mock 腿 done；live 腿 blocked（本轮不授权真实 provider），V15/V20 证据 not_run 归 live 解封/验收轮。评审遗留：既有 lint 5 条顺延 hygiene 轮（cache.rs:102/:126/:127 P6-008 历轮在案、openai_compatible.rs:218、admission.rs:291 clippy loop-index），本轮零代码改动。"}]
实施备注：2026-10-02 批次 1 收口（评审已 PASS，round14 落账）：并发限流 gate 落地——GateLimits 两级上限（per_project 严格小于全局，cap≥max 拒启防饿死）+ 公平队列（FIFO 防插队）+ rate-limit pause + 超时显式化 + 共享单例挂点（service_factory，cfg(feature="semantic") 默认构建零 cc-semantic 依赖），16 项 gate 测试全绿；Q4 线程模型定案入档（零线程/零定时器，被调用组件）；双轨口径——mock 腿 done、live 腿 blocked，V15/V20 证据 not_run 归 live 解封；评审遗留：既有 lint 5 条顺延 hygiene 轮（本轮新增告警零）。

### [x] P7-006｜有界重试与断路器

状态：`done`；批次：`P7-B`；优先级：`normal`。
范围：`crates/cc-semantic/src/providers/openai_compatible.rs`；`crates/cc-semantic/src/worker.rs`
硬依赖：P6-020, P7-001, P7-005
步骤：429/5xx/timeout退避与Retry-After；auth/永久错误暂停
交付物：有界重试与断路器的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V15 验证证据（实施时生成）
验收：重试次数、deadline和费用封顶，失败原因公开脱敏；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V15
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：[{"status": "batch2_bounded_retry_breaker_mock_leg_green", "artifacts": ["crates/cc-semantic/src/providers/openai_compatible.rs", "crates/cc-semantic/tests/retry_worker_layering.rs", "crates/cc-model/src/config.rs", "crates/cc-server/src/service_factory.rs", "docs/CONFIGURATION.md", "artifacts/checkpoints/p7-impl-20261002/P7-006-IMPLEMENTATION.md"], "limitations": "命令级证据：批次 2 实施轮 cargo test -p cc-semantic -p cc-model --locked --offline 全绿（cc-semantic lib 176 含本任务 20 项单测 + retry_worker_layering 集成 5 项；cc-model lib 84 含 8 新配置键测试）；cc-server --features semantic lib 253 passed；cargo check --workspace 干净；clippy 本轮新增告警零。分层口径测试固化：1 outbox attempt = 1 完整调用层重试序列（attempt 只 +1）、调用层零队列写入（结构性）、C11 重试等待不持任何 DB 锁。双轨口径：mock 腿 done；live 腿 blocked（用户 2026-10-02 D1/D2 不授权真实 provider），V15 正式验证矩阵证据 not_run 归 live 解封/验收轮。评审边界如实入账：失败率阈值变体未预实现（取连续失败一支）；Suspended 消费时序为等待后（等待即冷却语义，双测固化）；断路器开路暴露为 ServerError（冻结六变体无专用变体）；worker 侧 auth/degraded 透出归接线轮（P7-010/P7-014）；retry_max_cost_units 占位费率已由 P7-008 收据层接管；cc-server 首轮 1 例无关 flaky 重跑两次 253/253 全绿。"}]
实施备注：2026-10-03 批次 2 收口（评审已 PASS，round15 落账）：有界重试 + 三态断路器落地——RetryClock 注入式时钟零线程零定时器、RetryPolicy 五上界（attempts/backoff/deadline/cost cap，重试默认关）+ 指数退避确定性抖动（只缩不涨）、CircuitBreaker 三态（半开单探测槽、AuthError ×10 长窗、429/InvalidInput/Cancelled neutral）+ RetryingProvider 装饰器（429 上报共享 gate、等待后消费 Suspended），分层口径测试固化（1 outbox attempt = 1 完整序列、调用层零队列写入、C11 零 DB 锁）；cc-model 8 新配置键 + 组合根断路器单例（与 gate 同点 first-wins）；双轨口径——mock 腿 done、live 腿 blocked（D1/D2 不授权），V15 证据 not_run 归 live 解封；评审边界如实入账（失败率变体未预实现、开路错误形态 ServerError、worker 侧透出归接线轮）。

### [x] P7-007｜代码外发与凭据政策

状态：`done`；批次：`P7-B`；优先级：`normal`。
范围：`crates/cc-semantic/src/policy.rs`；`crates/cc-model/src/config.rs`
硬依赖：P7-006
步骤：显式opt-in/敏感文件/endpoint协议/redirect；密钥只外部引用
交付物：代码外发与凭据政策的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V15, V18 验证证据（实施时生成）
验收：默认无网络、日志无key/源码、重定向不泄露认证；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V15；V18
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：[{"status": "batch2_egress_credential_policy_mechanism_leg_green", "artifacts": ["crates/cc-semantic/src/policy.rs", "crates/cc-semantic/src/providers/openai_compatible.rs", "crates/cc-model/src/config.rs", "docs/CONFIGURATION.md", "artifacts/checkpoints/p7-impl-20261002/P7-007-IMPLEMENTATION.md"], "limitations": "命令级证据：批次 2 实施轮 cargo test -p cc-semantic -p cc-model --locked --offline 全绿（cc-semantic lib 197 含 policy 20 项 + capability 探针外发审计；cc-model lib 85 含 egress 双键测试）；零网络——cc-semantic 依赖表未动（无 HTTP crate），唯一 transport 为内存 mock。机制五层强制：gate_transport_assembly 装配门（未 opt-in 不构造）/ 构造器 fail-fast / GuardedTransport 运行时守卫（scheme+超时前置）/ EmbeddingHttpTransport trait 契约四条（重定向永不跟随+跳前剥认证）/ audit_egress 声明面恰好等于审计；api_key_ref 仅 env:/file:（内联拒绝不回显）+ 14 断言面泄漏扫描 + redact_for_log；重定向 3xx 非重试 + seam 恰一次请求断言（认证绝不重发）。双轨口径：机制腿 done；live 腿 conditional blocked 归 P7-018（生产 transport 真实重定向/超时/TLS 行为核验、敏感文件外发分类矩阵未开放），V15/V18 正式证据 not_run。评审边界如实入账：zero-on-drop 未实现如实声明（无 static/无缓存/无持久化兜底）；守卫层不拦 3xx（保住更优错误分类，拒收紧守在适配层门）；build_request 私有转 pub 供政策级审计（纯可见性放宽）。"}]
实施备注：2026-10-03 批次 2 收口（评审已 PASS，round15 落账）：代码外发与凭据政策执行机制腿落地（用户 2026-10-02 D1/D2 决策口径）——新 policy.rs 模块五层强制（装配门/构造器 fail-fast/GuardedTransport/trait 契约四条含重定向永不跟随+剥认证/audit_egress 声明面恰好等于审计）、api_key_ref 仅 env:/file: 内联拒绝不回显 + 14 断言面泄漏扫描 + redact_for_log、egress 双配置键默认闭合态；双轨口径——机制腿 done、live 腿 conditional blocked 归 P7-018（真实 transport 行为核验、敏感文件外发分类矩阵），V15/V18 证据 not_run；评审边界如实入账（zero-on-drop 未实现、守卫不拦 3xx 保住更优错误分类）。

### [x] P7-008｜费用与不确定尝试收据

状态：`done`；批次：`P7-B`；优先级：`normal`。
范围：`crates/cc-semantic/src/admission.rs`；`crates/cc-semantic/src/worker.rs`
硬依赖：P7-007
步骤：区分reported/estimated tokens、cache reuse和未知重复费用；停机阈值
交付物：费用与不确定尝试收据的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V15, V20 验证证据（实施时生成）
验收：无usage不填0费用，重启重试能看见费用不确定性；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V15；V20
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：[{"status": "batch2_cost_receipts_mock_leg_green", "artifacts": ["crates/cc-semantic/src/admission.rs", "crates/cc-semantic/src/providers/openai_compatible.rs", "artifacts/checkpoints/p7-impl-20261002/P7-008-IMPLEMENTATION.md"], "limitations": "命令级证据：批次 2 实施轮 cargo test -p cc-semantic -p cc-model --locked --offline 全绿（cc-semantic lib 212 含本任务 15 项新增：admission 收据层 7 + RetryingProvider 挂接 8）。核心不变式测试固化：reported usage 恒 None 不填 0（missing_usage_is_unknown_and_never_zero_filled）；unknown_duplicate_risk = attempt>1 且 reported 缺（重启重试费用不确定性可见）；UncertainReason 两变体（BreakerOpen/TimeoutIndeterminate）；CostBudget 调用前拒绝（Some(0) 全拒）；ReceiptLedger 有界环形容器 + 生命周期总额与保留窗聚合双口径分离；收据零敏感内容（零泄漏测试）。双轨口径：mock 腿 done；live 腿 blocked（D1/D2 不授权真实 provider），V15 正式证据 not_run，V20 归接线轮。评审边界如实入账：收据只在内存聚合零落盘（跨进程退化为 outbox attempt_count + unknown_duplicate_risk 组合可观测，落库取舍留 P7-016）；挂接点在 RetryingProvider::run 唯一漏斗而非新建 worker.rs（该文件不存在，P7-006 偏差 4 已裁定）；reported 真实值待端口解冻；零配置键新增（程序化装配面）。"}]
实施备注：2026-10-03 批次 2 收口（评审已 PASS，round15 落账）：费用与不确定尝试收据落地——admission 收据层（UsageReceipt reported/estimated/cache_reuse 三拆分、reported 恒 None 不填 0、unknown_duplicate_risk 重复计费风险、UncertainReason BreakerOpen/TimeoutIndeterminate、CostBudget 调用前拒绝、ReceiptLedger 有界环形账本生命周期总额与窗口聚合双口径）+ RetryingProvider::run 唯一漏斗挂接（断路器拒/预算拒/每次实际尝试三类收据，占位费率接管 P7-006 常量）；双轨口径——mock 腿 done、live 腿 blocked，V15 证据 not_run、V20 归接线轮；评审边界如实入账（内存聚合零落盘、跨进程退化为 outbox attempt_count 组合可观测、落库取舍留 P7-016、零配置键新增）。

### [x] P7-009｜查询编码与缓存

状态：`done`；批次：`P7-B`；优先级：`normal`。
范围：`crates/cc-semantic/src/spec.rs`；`crates/cc-semantic/src/cache.rs`
硬依赖：P7-008
步骤：实现QueryEncodingSpec键和有界query cache；instruction变更失效
交付物：查询编码与缓存的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V15 验证证据（实施时生成）
验收：不需无谓重嵌文档，不跨空间复用query向量；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V15
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：[{"status": "batch2_query_encoding_cache_mock_leg_green", "artifacts": ["crates/cc-semantic/src/cache.rs", "crates/cc-semantic/tests/query_cache.rs", "artifacts/checkpoints/p7-impl-20261002/P7-009-IMPLEMENTATION.md"], "limitations": "命令级证据：批次 2 实施轮 cargo test -p cc-semantic --locked --offline 全绿（lib 212 零回归 + query_cache 新集成套件 17 项）。Q5 裁决入账：缓存键 = namespace + QuerySpecDigest + QueryDigest，不含 semantic_epoch（查询向量为 (spec, 文本) 纯函数、C12 dense 行语义射程不含编码缓存、保守入键为纯浪费；owner 可一键推翻，加字段即全量失效零迁移）；instruction/tokenizer/space 变更 ⇒ spec digest 变 ⇒ 新键，跨空间复用结构性不可达（键级 + 行为级双测固化）；文档路径零接触（query_path_never_touches_the_document_cache 字节级固化）；查询文本零泄漏专项双测；QueryVectorCache 容量 + 字节双硬上限。双轨口径：mock 腿 done；live 腿 blocked（D1/D2 不授权真实 provider），V11/V15 正式证据 not_run（V11 另依赖 P7-010/013 查询执行接线）。评审边界如实入账：键类型落 cache.rs 而非 scope 所列 spec.rs（冻结面零 diff，键无需新 digest 类型）；内存 LRU 非磁盘落位（简报接口草案即 LRU）；零配置键新增（4096 entries / 64 MiB 程序化装配，操作者可调归 P7-014）。"}, {"target_sha": "8b6d9a1d9a7c05700d2de8611dbc20918647e527", "artifact_paths": ["artifacts/checkpoints/cloud-p7-cache-identity-integration-20261003/receipt.json", "artifacts/checkpoints/cloud-p7-cache-identity-integration-20261003/distinct-query-consumption.json", "artifacts/checkpoints/cloud-p7-cache-identity-integration-20261003/schema-reopen.json"], "scope": "bounded independent distinct consumer identity/schema integration; full C12/V11 not complete", "review": "PR55 original dual-Rust evidence unchanged; combined tree1.95 five tests5/0 plus spec12/0/readiness8/0"}]
实施备注：2026-10-03 批次 2 收口（评审已 PASS，round15 落账）：查询编码与缓存落地——QueryCacheKey（namespace + QuerySpecDigest + QueryDigest，Q5 裁决不含 semantic_epoch：查询向量为 (spec,文本) 纯函数、C12 dense 行不覆盖编码缓存，owner 可一键推翻零迁移）+ QueryVectorCache 容量/字节双硬上限 LRU + encode_queries 全链（P7-003 query 批次口径、只重批 miss、错误向量不缓存、Skipped 显式透出）+ query_provider_failure 错误映射；跨空间复用结构性不可达、文档路径零接触、查询文本零泄漏均测试固化；双轨口径——mock 腿 done、live 腿 blocked，V11/V15 证据 not_run；评审边界如实入账（键落 cache.rs 非 spec.rs、内存 LRU 非磁盘、零配置键新增归 P7-014）。 2026-10-03 PR55两提交原样纳入新recovery cache-identity-review分支；8b6d9a1新树independent2+原cache矩阵3共5/0，11distinct消费值/9真实adapterPOST与schema真实reopen复核；spec12/0、status8/0、strictclippy/fmt通过。ADR0003完整digest及既有v1四字段序列化权威核验，三处dimension不进身份文字订正为完整tuple；去除整行注释后的Rust内容逐字不变，无公式/布局/版本兼容变更。作者1.99证据另计；原review脚本针对旧字节基线并写自身receipt，保留未改，不冒充新树检查。full C12/V11及014仍开放。

### [x] P7-010｜dense召回端口接线

状态：`done`；批次：`P7-B`；优先级：`normal`。
范围：`crates/cc-search/src/lanes/semantic_adapter.rs`；`crates/cc-server/src/service_factory.rs`
硬依赖：P7-009
步骤：将语义服务注入SemanticRecall；返回doc版本/空间/coverage
交付物：dense召回端口接线的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V16 验证证据（实施时生成）
验收：搜索不依赖具体HTTP客户端，local策略不调用端口；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V16
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：[{"status": "batch2_dense_recall_port_wiring_mock_leg_green", "artifacts": ["crates/cc-server/src/semantic_wiring.rs", "crates/cc-server/src/engine.rs", "crates/cc-server/src/lib.rs", "crates/cc-db/src/index_db_retrieval.rs", "artifacts/checkpoints/p7-impl-20261002/P7-010-IMPLEMENTATION.md"], "limitations": "命令级证据：批次 2 实施轮 cargo test -p cc-server --features semantic -p cc-semantic --locked --offline 全绿（cc-server lib 263 含 semantic_wiring 10 项；cc-semantic 212 + 集成零回归）；默认构建回归 251 passed；cargo tree 双口径默认 0 边 / feature 1 边（默认构建零 cc-semantic 依赖）；V18 disabled 口径零漂移复跑（cc-eval p5d 三套件 21 passed）；cc-db 173 passed（chunk_ids_by_doc_keys additive 零回归）。最小装配腿：SemanticSubsystem assemble/wire（注入/对称摘除/装配 Err 先于槽写入）、ExactRecallService 生产 recall 供给（C09 filter-before-top-k、CandidateRef.document.doc_version + LaneCoverage.complete、scoring_spec=cosine-exact-v1）、engine.rs set_project 接线 fail-closed、DegradationSnapshot 一行桥接。双轨口径：mock 腿 done；live 腿 blocked（D1/D2 不授权真实 provider），V11/V16 正式证据 not_run。评审边界如实入账：查询路径不内联编码（miss → Unavailable(\"query_vector_not_encoded\")，C10 没执行≠没结果；内联决策归 P7-012/013）；诚实边界——生产侧当前无查询编码调用方，enabled 下 dense lane 恒 Unavailable 直至该决策落地；完整 try_init/调度时机/预算键归 P7-014。"}]
实施备注：2026-10-03 批次 2 收口（评审已 PASS，round15 落账）：dense 召回端口接线 + 组合根最小装配腿落地——semantic_wiring.rs 新模块（SemanticSubsystem 装配 / wire 注入摘除 / ExactRecallService 生产 recall 供给，产出 doc_version/coverage、scoring_spec=cosine-exact-v1）、engine.rs set_project 接线 fail-closed、cc-db additive 只读 chunk_ids_by_doc_keys、cargo tree 双口径默认零依赖、V18 disabled 口径零漂移；双轨口径——mock 腿 done、live 腿 blocked，V11/V16 证据 not_run；接线账目——已消 3（#4/#5/#8）/部分 1（#1）/待 9 归 P7-011/012/014/015/016。【漏记偏差补录】recall 实现落 crates/cc-server/src/semantic_wiring.rs 而非 tasks.json scope 所列 crates/cc-search/src/lanes/semantic_adapter.rs——架构理由：recall 供给是组合根装配产物（与 gate/breaker 单例同点集中装配，产出 SemanticSubsystem 供 P7-014 全量接线消费），semantic_adapter 是消费侧收据门（panic 捕获/取消/超时/容量分类既有实现），保持零改动即接线不触碰既有检索链；实施记录 §6.1 已申报 cc-db 越界但漏记此条，本条为补录。【随 P7-014 消纳的低风险发现三条】①断路器半开单探测槽与调用层重试预算/收据的时序交互口径需接线轮对齐；②本条漏记偏差本身（scope 按实质兑现，文件名口径已补录）；③engine.rs set_project 接线的表述精度（wire 在字段赋值之后、返回 Err 即整体失败且 self 未被污染的语义，配置/文档面表述需更精确）。

### [ ] P7-011｜dense范围与hydrate守卫

状态：`todo`；批次：`P7-C`；优先级：`normal`。
范围：`crates/cc-semantic/src/vector/exact.rs`；`crates/cc-search/src/evidence.rs`
硬依赖：P6-020, P7-009, P7-010
步骤：过滤在topk前且最终二次检验manifest/source；删除和scope测试
交付物：dense范围与hydrate守卫的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V05, V16 验证证据（实施时生成）
验收：semantic找回结果也不会被softscope误删或越过hard范围；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V05；V16
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：[{"target_sha": "c4dfa324143a8406ee7f54e1337c54c1e1700118", "worktree_digest": "dbf3939146b6ca532972e999de3e94f16438b608", "artifact_paths": ["artifacts/checkpoints/cloud-p7-014-20261002/integration-receipt.json"], "review": "integration self-review; prior independent fixes tracked by source SHA; full task review pending", "rollback_status": "revert individual integration/format commits; schemas/dependencies unchanged", "scope": "partial integration evidence only; full task acceptance remains pending"}, {"target_sha": "24db8bbe14e7b47bc5cc413bbf7c66a0be8f2710", "worktree_digest": "5721d0d639e0cfc66437c9ff48af15a016ee4bb7", "artifact_paths": ["artifacts/benchmarks/p7-acceptance-20261002/combined-v2/matrix.json", "artifacts/benchmarks/p7-acceptance-20261002/combined-v2/receipt.json"], "review": "independent offline matrix owner; integration review; declared subchecks only", "rollback_status": "revert new tests/evidence commits independently", "scope": "V05/V16 L1-L2 scope/exact, V11/V15 fault/cache/cancellation mechanisms; no unconditional full validation closure"}, {"target_sha": "2d48f2628ae7c745fcab1a21dd784eae4582a193", "artifact_paths": ["artifacts/benchmarks/p7-v16-2d48f26-9bdab23f-cosine-v1-20261002/report.md", "artifacts/benchmarks/p7-v16-2d48f26-9bdab23f-cosine-v1-20261002/integration-receipt.json", "artifacts/benchmarks/p7-v16-2d48f26-9bdab23f-cosine-v1-20261002/offline-gaps.md"], "scope": "V16 independent hand-vector production L2 subset five runs 5/0/0, 90 gold cases; independent integration targets20/0/0. Bounded-memory/full V05/V11/V16 pending; no holdout/live quality claim", "review": "integration hand oracle; PR43 six independent production cases and PR44 actual product stdio 155/0/0 preserved unchanged"}, {"target_sha": "715ab33e83ecb6c65228c18ed55e0fa6604ca0a6", "artifact_paths": ["artifacts/benchmarks/p7-v11-715ab33-ff968c63-generation-v1-20261002/report.md", "artifacts/benchmarks/p7-v11-715ab33-ff968c63-generation-v1-20261002/gate-assertions.json"], "scope": "V11 production mixed-generation L3 and finite-retry L2 subset; five fixed-source executions10/0/0, no full V05/V11 closure", "review": "integration owner; independent review pending; PR43/44 and V16 files untouched"}, {"target_sha": "32ce36ad424f5279fc0d2c37191e1f3dbb2eddb8", "artifact_paths": ["artifacts/benchmarks/p7-v05-32ce36a-a34ddca2-scope-v1-20261003/report.md", "artifacts/benchmarks/p7-v05-32ce36a-a34ddca2-scope-v1-20261003/gate-assertions-updated.json", "artifacts/benchmarks/p7-v16-independent-review-20261002/README.md", "artifacts/benchmarks/p7-v16-2d48f26-9bdab23f-cosine-v1-20261002/canonical-errata.json"], "scope": "V05 scoped production L2/L3 five rounds15/0/0; independent V16 PR47 proof limits corrected and nonunit/actual insertion variants retained; PR48 resource blocker explicitly open", "review": "V05 integration self-validation pending independent review; PR47 independent V16 audit preserved unchanged"}, {"target_sha": "a7efaae70cd0828b1a1b2d811e20176d855394b3", "artifact_paths": ["artifacts/benchmarks/p7-cachekeys-a7efaae-52cc1cbf-keys-v1-20261003/report.md", "artifacts/benchmarks/p7-cachekeys-a7efaae-52cc1cbf-keys-v1-20261003/cache-key-requirements.json", "artifacts/benchmarks/p7-cachekeys-a7efaae-52cc1cbf-keys-v1-20261003/formal-validation-map.json", "artifacts/benchmarks/p7-cachekeys-a7efaae-52cc1cbf-keys-v1-20261003/integration-receipt.json"], "scope": "production cache behavior five rounds15/0/0; bounded ArtifactCache original counterexample replay/resource2/0/0; all full gates pending independent consolidation", "review": "main integration self-validation; PR48/50 owner source/evidence unchanged; current independent reviewer pending"}, {"target_sha": "3dceedf3dcee851b3b2e4d4938bba11d76c63326", "artifact_paths": ["artifacts/checkpoints/cloud-p7-status-generation-fix-20261003/receipt.json", "artifacts/checkpoints/cloud-p7-status-generation-fix-20261003/deterministic-interleaves.json", "artifacts/checkpoints/cloud-p7-status-generation-fix-20261003/immutable-sources.json"], "scope": "bounded readiness race correction and unchanged independent V05/V11/P1 integration; full task not complete", "review": "integration self-replay; original independent reviewer must verify readiness fix SHA"}, {"target_sha": "d6a54a28eb17a1f71f9924a94e05877801c17037", "artifact_paths": ["docs/roadmap/code-index-v2/P7-REMAINING-GATES.json", "artifacts/checkpoints/cloud-p7-formal-gate-scope-20261003/receipt.json"], "scope": "37 authority rows; V05 combined raw/hydrate gap declared L2 closed; independently verified specific status race closed; full gates/task remain open", "review": "PR57 frozen120pass unchanged; combined current dualfeature10/0; newV05 main-owned block pending independent review"}]
实施备注：2026-10-02 父对话独立复核报告：带 languages 请求会忽略未覆盖文档，Rust eligible=1/published=0 仍报 Complete 已复现；scope guard 修复由独立任务负责。保持 todo，修复与当前 SHA 复验前不收口。 修复 PR #6 SHA46d6e5b 已非破坏性集成至 b738a6a；最终格式源码 c4dfa324 上四 crate lib975/0/1通过，正式 V05/V16 尚未全跑，保持 todo。 新独立矩阵PR14源24db8bb/58e20d0已原样纳入recovery；20轮semantic7/default2共180pass、PR12各4pass为既有精确基线证据，未升级成新SHA验收或整gate通过。原wiring许可释放竞态已最小等待两个计数归零，正式复验仍需绑定当前SHA。 2026-10-03 PR54冻结源码3dceedf：原样纳入PR52三提交及PR53；真实worker强制发布交错复现ready/root epoch1与实际2混拼，完整status读取纳入同一三次generation fence后五轮40/0，严格两次返回2；真实incremental churn严格三次retryable无generation/ready。原公共V11断言未改，public+independent五轮45/0，扩展六target27/0，default status5/0，strictclippy/fmt过。PR53独立P1七测试新树7/0，cache.rs逐字等于db9841e，仅关闭64MiB读前无界allocation具体P1；全V05/V11/V16/P7-015/V20及014仍未收口。原PR52失败、default cache夹具失败及错target命令保留文件hash，不冒充race或通过。CI144精确源码当次仍运行；原PR50/51精确CI由父核验success。 2026-10-03 精确剩余gate矩阵37row落P7-REMAINING-GATES.json，逐项固定source/证据、层级、断言缺口、输入规模与授权条件；V05八/V16六有declared范围证据，不冒充完整gate，V16尚余正式整合判定非强加100k/C8/16（归V20）。新增039d035真实worker/query编码/recall与六raw lane同域L2，7文件11case66lane receipts148候选21hits；跨文件Python graph邻居真实出现，scope排除，完整literal final sets/非空源码/kind-name断言，双feature各1/0，strictclippy/fmt过；semantic scope为真实port手工输入，不冒充新L3 DSL接线。PR57两提交原样纳入，冻结3d独立40commands120pass/原V11断言20轮/真实worker3次churn，specific statusrace关闭；新组合dualfeature两target10/0，260owner文件未改。旧P5 quality raw目录此checkout缺失，V19不能凭prose关闭；六项offline可推进，真实语义消融D1D2授权blocked。PR54精确c445 CI146及PR56精确4eac CI147直接回读success。task状态不翻done。

## 本批附录：公开 hit 的来源绑定 qname（2026-10-03）

- [ ] 来源绑定限定符号身份贯通：`awaiting_design_review`；基线
  `88f2cf099c8b81f3acef485fd5ac9b01c63ce790`；设计见
  [QNAME_SOURCE_IDENTITY_DESIGN.md](QNAME_SOURCE_IDENTITY_DESIGN.md)。
  现有持久化关系不足，拟议 schema 25 需原线程先审阅，当前保持 24/3。
- [ ] 产品实现、完整生命周期/代际拒绝测试及原 native evaluator 微型 gold
  验证：`blocked_on_design_review`，没有产品通过声明。
- [ ] 原线程独立代码审阅：`pending`。本附录不关闭 V19 或任何父项，
  不重跑公开 DEV，不修改评分、排名、预算或冻结 gold。

### 本批后续实施收据（保留上节 pending 状态的历史记录）

- [x] 原线程已审阅 `bc22dc9` 并批准 schema 25 / module model 3，沿
  PR 132 实施来源绑定身份；真实文件事务验证写后符号存活，cold/warm
  读取核对完整来源/文档/符号关系，公开 hit 新增 `metadata.qname`。
- [x] 有界验收：17 个本批产品测试、183 parser + 298 search 回归、
  5 schema + 10 epoch + 1 FIFO 回归、直接 Rust 1.95 locked fmt/clippy。
  未改 native evaluator 的独立微型 gold 实跑为 missing=0 / correct=1 /
  wrong-qname=0；真实 MCP JSON-RPC 同样 correct=1 / wrong=0。
  原 hit 字段、score trace、source/document proof 未变。
- [ ] 原线程独立最终代码审阅：`pending`。实施收据见
  `artifacts/checkpoints/qname-source-identity-implementation-20261003/README.md`。
  V19 和父项保持开放；公开 DEV、规模/发行认证及其他平台未验。


2026-10-03 新独立 Python AST 声明修复，基于 PR132/fd5146b；冻结源码 f4df9a83514e6ba901143547368ce4a3bee21940。decorated class 只有 canonical wrapper Class，按真实 AST 分派、遍历成员与 decorator 引用；函数提取器拒绝 class，boundary hint 需类别/名称兼容，保留合法 Method 和其它语言分类。原 PR134/fb8ca8a REJECT 证据逐字保留；原 parser 红例4/0，原公开 omission 断言仍按原样失败于正确 Class qname，非冒充全绿；新公开 exact source/SQL proof2/0、lifecycle7/0、diagnostic1/0。冻结树 bounded parser248/index14/public10/search298共570/0/0，locked Rust1.95 fmt/strictclippy通过；真实 engine/MCP Outer/Inner 均保留 Outer Class、Inner Class、pulse Method，普通5hit所有字段/score/source/document逐值相同；未改 native scorer/微型gold，correct1/wrong0。完整记录 docs/reviews/python-declaration-fix-20261003/README.md 与 source-manifest.json；生产只改 Python parser/references 和必要 boundary guard，schema_guard 由另 worker 负责，DB 独审未替代。原任务/status/acceptance不翻done，V19及父项仍开放，待原线程独立最终审阅；排除旧runtime/GC-WAL/kill/staging/EROFS/private-localdiag/42export，不运行包含旧worker用例的broad suites。 2026-10-03 独立 C/C++ boundary regression 修复：固定代码 aa271e52b9c2aa52e05696af96ac52116963e57e，base PR136 dbeefb4；PR134 final61390db 的 namespace/template leaf 红例根因为错误 Method hint 拒绝后把返回类型 T 当 name。仅 boundaries.rs 沿真实 declarator 提取 identifier/qualified/template/operator/destructor/pointer/reference 名称并校验 hint name，kind guard 不放宽，未知 conversion 保守 omit；旧 C++ namespace taxonomy 原有债不改，leaf Function/qname省略，不构造identity。原微型gold/scorer/fixture/budgets不变，真实 engine/MCP native recall10 0→1，Python普通5hit完整字段/score/source/document与Class/Class/Method proof保留；C/Go/JS/Rust/TS symbols/chunks/identities逐值不变。Rust1.95 locked bounded parser252/index30/public11/search298=591pass/0fail/1既有childhelper ignored；新四test精确base2pass2fail→fixed4pass0fail，strictparserclippy/fmt过。证据 docs/reviews/cpp-boundary-declarator-fix-20261003/README.md，原REJECT/旧失败不改；排除旧worker及包含suite、GC/WAL/kill/staging/EROFS/private/42export、publicDEV/规模、PR135CI；prioritypressure worker独占不碰。任务状态/父项/V19不翻done，等待独立最终审阅。

### [ ] P7-012｜融合与部分覆盖语义

状态：`todo`；批次：`P7-C`；优先级：`normal`。
范围：`crates/cc-search/src/fusion.rs`；`crates/cc-model/src/context.rs`
硬依赖：P7-011
步骤：独立dense rank RRF，partial/unavailable透出；不混cosine/BM25
交付物：融合与部分覆盖语义的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V19 验证证据（实施时生成）
验收：timeout与无命中可区分，declared full coverage有证据；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V19
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：[{"target_sha": "24db8bbe14e7b47bc5cc413bbf7c66a0be8f2710", "worktree_digest": "5721d0d639e0cfc66437c9ff48af15a016ee4bb7", "artifact_paths": ["artifacts/benchmarks/p7-acceptance-20261002/combined-v2/matrix.json", "artifacts/benchmarks/p7-acceptance-20261002/combined-v2/receipt.json"], "review": "independent offline matrix owner; integration review; declared subchecks only", "rollback_status": "revert new tests/evidence commits independently", "scope": "V05/V16 L1-L2 scope/exact, V11/V15 fault/cache/cancellation mechanisms; no unconditional full validation closure"}, {"target_sha": "05f4853e75be5ccddab3855d91ce164b2a017282", "artifact_paths": ["artifacts/benchmarks/p7-v19-offline-20261002/README.md", "artifacts/benchmarks/p7-v19-offline-20261002/plan.json"], "review": "independent V19 owner, retrospective offline scope", "rollback_status": "revert dedicated V19 test/evidence commits", "scope": "420 official-runner replay requests, three local ablations; V19 formal blocked: no unseen holdout/hard negatives/facet-span independent gold/full corpus/live dense quality"}]
实施备注： 新独立矩阵PR14源24db8bb/58e20d0已原样纳入recovery；20轮semantic7/default2共180pass、PR12各4pass为既有精确基线证据，未升级成新SHA验收或整gate通过。原wiring许可释放竞态已最小等待两个计数归零，正式复验仍需绑定当前SHA。

### [ ] P7-013｜查询总deadline和模型故障退化

状态：`todo`；批次：`P7-C`；优先级：`normal`。
范围：`crates/cc-search/src/execution.rs`；`crates/cc-server/src/handlers/context.rs`
硬依赖：P7-012
步骤：fake/HTTP慢请求测取消；auto回本地、explicit semantic明确不足
交付物：查询总deadline和模型故障退化的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V11, V15 验证证据（实施时生成）
验收：网络不占读写锁，故障结果不缓存成完整成功；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V11；V15
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：[{"target_sha": "c4dfa324143a8406ee7f54e1337c54c1e1700118", "worktree_digest": "dbf3939146b6ca532972e999de3e94f16438b608", "artifact_paths": ["artifacts/checkpoints/cloud-p7-014-20261002/integration-receipt.json"], "review": "integration self-review; prior independent fixes tracked by source SHA; full task review pending", "rollback_status": "revert individual integration/format commits; schemas/dependencies unchanged", "scope": "partial integration evidence only; full task acceptance remains pending"}, {"target_sha": "24db8bbe14e7b47bc5cc413bbf7c66a0be8f2710", "worktree_digest": "5721d0d639e0cfc66437c9ff48af15a016ee4bb7", "artifact_paths": ["artifacts/benchmarks/p7-acceptance-20261002/combined-v2/matrix.json", "artifacts/benchmarks/p7-acceptance-20261002/combined-v2/receipt.json"], "review": "independent offline matrix owner; integration review; declared subchecks only", "rollback_status": "revert new tests/evidence commits independently", "scope": "V05/V16 L1-L2 scope/exact, V11/V15 fault/cache/cancellation mechanisms; no unconditional full validation closure"}, {"target_sha": "2d48f2628ae7c745fcab1a21dd784eae4582a193", "artifact_paths": ["artifacts/benchmarks/p7-v16-2d48f26-9bdab23f-cosine-v1-20261002/report.md", "artifacts/benchmarks/p7-v16-2d48f26-9bdab23f-cosine-v1-20261002/integration-receipt.json", "artifacts/benchmarks/p7-v16-2d48f26-9bdab23f-cosine-v1-20261002/offline-gaps.md"], "scope": "V16 independent hand-vector production L2 subset five runs 5/0/0, 90 gold cases; independent integration targets20/0/0. Bounded-memory/full V05/V11/V16 pending; no holdout/live quality claim", "review": "integration hand oracle; PR43 six independent production cases and PR44 actual product stdio 155/0/0 preserved unchanged"}, {"target_sha": "715ab33e83ecb6c65228c18ed55e0fa6604ca0a6", "artifact_paths": ["artifacts/benchmarks/p7-v11-715ab33-ff968c63-generation-v1-20261002/report.md", "artifacts/benchmarks/p7-v11-715ab33-ff968c63-generation-v1-20261002/gate-assertions.json"], "scope": "V11 production mixed-generation L3 and finite-retry L2 subset; five fixed-source executions10/0/0, no full V05/V11 closure", "review": "integration owner; independent review pending; PR43/44 and V16 files untouched"}, {"target_sha": "32ce36ad424f5279fc0d2c37191e1f3dbb2eddb8", "artifact_paths": ["artifacts/benchmarks/p7-v05-32ce36a-a34ddca2-scope-v1-20261003/report.md", "artifacts/benchmarks/p7-v05-32ce36a-a34ddca2-scope-v1-20261003/gate-assertions-updated.json", "artifacts/benchmarks/p7-v16-independent-review-20261002/README.md", "artifacts/benchmarks/p7-v16-2d48f26-9bdab23f-cosine-v1-20261002/canonical-errata.json"], "scope": "V05 scoped production L2/L3 five rounds15/0/0; independent V16 PR47 proof limits corrected and nonunit/actual insertion variants retained; PR48 resource blocker explicitly open", "review": "V05 integration self-validation pending independent review; PR47 independent V16 audit preserved unchanged"}, {"target_sha": "a7efaae70cd0828b1a1b2d811e20176d855394b3", "artifact_paths": ["artifacts/benchmarks/p7-cachekeys-a7efaae-52cc1cbf-keys-v1-20261003/report.md", "artifacts/benchmarks/p7-cachekeys-a7efaae-52cc1cbf-keys-v1-20261003/cache-key-requirements.json", "artifacts/benchmarks/p7-cachekeys-a7efaae-52cc1cbf-keys-v1-20261003/formal-validation-map.json", "artifacts/benchmarks/p7-cachekeys-a7efaae-52cc1cbf-keys-v1-20261003/integration-receipt.json"], "scope": "production cache behavior five rounds15/0/0; bounded ArtifactCache original counterexample replay/resource2/0/0; all full gates pending independent consolidation", "review": "main integration self-validation; PR48/50 owner source/evidence unchanged; current independent reviewer pending"}, {"target_sha": "3dceedf3dcee851b3b2e4d4938bba11d76c63326", "artifact_paths": ["artifacts/checkpoints/cloud-p7-status-generation-fix-20261003/receipt.json", "artifacts/checkpoints/cloud-p7-status-generation-fix-20261003/deterministic-interleaves.json", "artifacts/checkpoints/cloud-p7-status-generation-fix-20261003/immutable-sources.json"], "scope": "bounded readiness race correction and unchanged independent V05/V11/P1 integration; full task not complete", "review": "integration self-replay; original independent reviewer must verify readiness fix SHA"}, {"target_sha": "d6a54a28eb17a1f71f9924a94e05877801c17037", "artifact_paths": ["docs/roadmap/code-index-v2/P7-REMAINING-GATES.json", "artifacts/checkpoints/cloud-p7-formal-gate-scope-20261003/receipt.json"], "scope": "37 authority rows; V05 combined raw/hydrate gap declared L2 closed; independently verified specific status race closed; full gates/task remain open", "review": "PR57 frozen120pass unchanged; combined current dualfeature10/0; newV05 main-owned block pending independent review"}]
实施备注：2026-10-02 父对话独立复核报告：真实执行器 20ms deadline/120ms 返回 probe 超时，wiring 同步扫描和 exact 候选循环缺 control；取消链交独立任务，云集成 owner 释放 semantic_wiring recall 段（约700～800行）。hydrator skip 顶层 partial 为尚未证实可达的测试缺口，不据此宣称生产 bug；保持 todo。 两提交057e283/4c3dfe1均已按序集成；最终格式源码c4dfa324上四crate lib975/0/1通过，单次同步IO仍不可强抢占；fake HTTP/故障缓存正式义务未齐，保持todo；单独PR发布被执行器拒绝，未代开同PR。 新独立矩阵PR14源24db8bb/58e20d0已原样纳入recovery；20轮semantic7/default2共180pass、PR12各4pass为既有精确基线证据，未升级成新SHA验收或整gate通过。原wiring许可释放竞态已最小等待两个计数归零，正式复验仍需绑定当前SHA。 2026-10-03 PR54冻结源码3dceedf：原样纳入PR52三提交及PR53；真实worker强制发布交错复现ready/root epoch1与实际2混拼，完整status读取纳入同一三次generation fence后五轮40/0，严格两次返回2；真实incremental churn严格三次retryable无generation/ready。原公共V11断言未改，public+independent五轮45/0，扩展六target27/0，default status5/0，strictclippy/fmt过。PR53独立P1七测试新树7/0，cache.rs逐字等于db9841e，仅关闭64MiB读前无界allocation具体P1；全V05/V11/V16/P7-015/V20及014仍未收口。原PR52失败、default cache夹具失败及错target命令保留文件hash，不冒充race或通过。CI144精确源码当次仍运行；原PR50/51精确CI由父核验success。 2026-10-03 精确剩余gate矩阵37row落P7-REMAINING-GATES.json，逐项固定source/证据、层级、断言缺口、输入规模与授权条件；V05八/V16六有declared范围证据，不冒充完整gate，V16尚余正式整合判定非强加100k/C8/16（归V20）。新增039d035真实worker/query编码/recall与六raw lane同域L2，7文件11case66lane receipts148候选21hits；跨文件Python graph邻居真实出现，scope排除，完整literal final sets/非空源码/kind-name断言，双feature各1/0，strictclippy/fmt过；semantic scope为真实port手工输入，不冒充新L3 DSL接线。PR57两提交原样纳入，冻结3d独立40commands120pass/原V11断言20轮/真实worker3次churn，specific statusrace关闭；新组合dualfeature两target10/0，260owner文件未改。旧P5 quality raw目录此checkout缺失，V19不能凭prose关闭；六项offline可推进，真实语义消融D1D2授权blocked。PR54精确c445 CI146及PR56精确4eac CI147直接回读success。task状态不翻done。

### [ ] P7-014｜配置/status/MCP全链贯通

状态：`in_progress`；批次：`P7-C`；优先级：`normal`。
范围：`crates/cc-server/src/tools.rs`；`crates/cc-server/src/capability_status.rs`；`docs/MCP_TOOLS.md`；`crates/cc-server/src/semantic_runtime.rs`；`crates/cc-server/src/engine.rs`；`crates/cc-server/src/handlers/core.rs`；`crates/cc-server/src/service_factory.rs`；`crates/cc-server/src/lib.rs`；`crates/cc-db/src/document_store.rs`
硬依赖：P7-013
步骤：新字段schema/sanitize/handler/doc/E2E一体；原mode语义不变
交付物：配置/status/MCP全链贯通的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V18 验证证据（实施时生成）
验收：未配置、关闭、回填、失败、就绪状态真实一致；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V18
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：[{"target_sha": "ddd4f7e38ffff2d1bdd0193d5ccf4054deef82fa", "worktree_digest": "9df8ebdd6db932509211c813797bcc843f6038c4", "artifact_paths": ["artifacts/checkpoints/cloud-p7-014-20261002/receipt.json"], "review": "self-reviewed; independent review pending; P7-014 not complete", "rollback_status": "revert recovery commit; no schema/credential/provider change", "scope": "cloud build recovery and capability projection only; full task remains in_progress"}, {"target_sha": "c4dfa324143a8406ee7f54e1337c54c1e1700118", "worktree_digest": "dbf3939146b6ca532972e999de3e94f16438b608", "artifact_paths": ["artifacts/checkpoints/cloud-p7-014-20261002/integration-receipt.json"], "review": "integration self-review; prior independent fixes tracked by source SHA; full task review pending", "rollback_status": "revert individual integration/format commits; schemas/dependencies unchanged", "scope": "partial integration evidence only; full task acceptance remains pending"}, {"target_sha": "2b8eb20c410ae9c3154bfba64e0bd82c98340fe0", "worktree_digest": "efee4e6fcda67dd6d03263e6839cbb43fba690eb", "artifact_paths": ["artifacts/checkpoints/cloud-p7-recovery-20261002/stdio-retry-receipt.json"], "review": "exact-binary synthetic loopback L3 lifecycle passed; independent review pending", "rollback_status": "revert dedicated recovery worker/retry commits; disable semantic remote config", "scope": "6/0 stdio lifecycle subchecks; no live model quality or unconditional full V18 closure"}, {"target_sha": "307a2f1d395436febe2a2e804e8c0c6baf86b1ae", "worktree_digest": "2efc5bcd1b6a11d2e70ba734542d2506f1c684e8", "artifact_paths": ["artifacts/checkpoints/cloud-p7-recovery-20261002/validation-receipt.json"], "review": "integration self-review; independent review pending", "rollback_status": "revert dedicated recovery blocks; configuration can disable semantic", "scope": "workspace HTTP 2342/0/62; exact combined default and semantic real stdio each3/0 under process seccomp; strict clippy failed, full gates remain open"}, {"target_sha": "1b8a3d54bc28581fd1d908ba5a9c9d155eff1e87", "worktree_digest": "c28346465f565d5be415720a8b61a81877968b2d", "artifact_paths": ["artifacts/checkpoints/cloud-p7-lifecycle-fences-20261002/receipt.json", "artifacts/checkpoints/cloud-p7-lifecycle-fences-20261002/stdio-receipt.json", "artifacts/checkpoints/cloud-p7-lifecycle-fences-20261002/semantic", "artifacts/checkpoints/cloud-p7-lifecycle-fences-20261002/http"], "review": "PR25 counterexamples unchanged; post-fix integration verification passes, independent re-review pending", "rollback_status": "revert dedicated lifecycle-fence commit; disable remote semantic configuration", "scope": "semantic/HTTP original3-failure regressions each3/0; HTTPworkspace2345/0/62; exact productstdio6/0; not unconditional full V18 closure"}, {"target_sha": "373448146fcd7114f99c0819f3fb02921f68a260", "worktree_digest": "981f4da294cc5463333ca13bc33d4ddf30f0a757", "artifact_paths": ["artifacts/checkpoints/cloud-p7-v18-integration-20261002/matrix-lint-receipt.json"], "review": "owner-authorized equivalent lexical-scope lint correction; independent overall acceptance pending", "rollback_status": "revert isolated test-scope commit; production untouched", "scope": "explicit HTTP full-workspace all-targets strictclippy passed; unchanged matrix7/0/0; no gate inference"}, {"target_sha": "be2b414d21968140d1cf7a9974094de868b2ec4f", "artifact_paths": ["artifacts/benchmarks/p7-014-fence-recheck-20261002/receipt.json"], "scope": "Independent original three counterexamples pass at frozen lifecycle source, both features; bounded issues resolved, full task pending", "review": "independent delivered source preserved; integration acceptance pending"}, {"target_sha": "6817bf148130070a2b40240a0a6c843faa6a7f2d", "artifact_paths": ["artifacts/checkpoints/cloud-p7-v18-fault-contract-20261002/receipt.json"], "scope": "Real stdio loopback fault/recovery; public cold query remains unavailable; no positive public dense claim", "review": "independent delivered source preserved; integration acceptance pending"}, {"target_sha": "a484f3dda4c372b9db65758c96307f24013058a1", "artifact_paths": ["artifacts/checkpoints/cloud-p7-v18-parameters-20261002/audit.json"], "scope": "Frozen existing-parameter matrix 105 passed; new query opt-in not covered by this source", "review": "independent delivered source preserved; integration acceptance pending"}, {"target_sha": "86c650794d29d2f4362aa5bf20adcf27c1088875", "artifact_paths": ["artifacts/benchmarks/p7-ci-architecture-20261002/receipt.json"], "scope": "Author local 17 CI stdio steps passed; full remote Actions at final query tree pending", "review": "independent delivered source preserved; integration acceptance pending"}, {"target_sha": "51d64871bc41b2596ea6261f8a21aa96edae1d1c", "artifact_paths": ["artifacts/checkpoints/cloud-p7-query-production-20261002/receipt.json"], "scope": "synthetic public query producer now wired; original independent positive assertions pass; not full acceptance"}, {"target_sha": "c8c20b5b7d416372ee06ed5e248663c48912064e", "artifact_paths": ["artifacts/checkpoints/cloud-p7-model-transition-20261002/receipt.json"], "scope": "explicit configured-space transition and corrected query health scheduling; partial production integration, full gates pending", "review": "integration self-validation; latest independent production review pending"}, {"target_sha": "2dec99c54cf48752239f7d8f4728fd40f8352e8a", "artifact_paths": ["artifacts/benchmarks/p7-query-encoding-independent-20261002/README.md"], "scope": "five independently written module cases per feature; not full production wiring review"}, {"target_sha": "c8c20b5b7d416372ee06ed5e248663c48912064e", "artifact_paths": ["artifacts/checkpoints/cloud-p7-model-transition-20261002/receipt.json"], "scope": "remote CI123 run37076300547 check/MSRV/security all success; distinct from local semantic-http and pending independent/formal acceptance", "review": "GitHub jobs terminal states read back by integration owner"}, {"target_sha": "2d48f2628ae7c745fcab1a21dd784eae4582a193", "artifact_paths": ["artifacts/benchmarks/p7-v16-2d48f26-9bdab23f-cosine-v1-20261002/report.md", "artifacts/benchmarks/p7-v16-2d48f26-9bdab23f-cosine-v1-20261002/integration-receipt.json", "artifacts/benchmarks/p7-v16-2d48f26-9bdab23f-cosine-v1-20261002/offline-gaps.md"], "scope": "V16 independent hand-vector production L2 subset five runs 5/0/0, 90 gold cases; independent integration targets20/0/0. Bounded-memory/full V05/V11/V16 pending; no holdout/live quality claim", "review": "integration hand oracle; PR43 six independent production cases and PR44 actual product stdio 155/0/0 preserved unchanged"}, {"target_sha": "715ab33e83ecb6c65228c18ed55e0fa6604ca0a6", "artifact_paths": ["artifacts/benchmarks/p7-v11-715ab33-ff968c63-generation-v1-20261002/report.md", "artifacts/benchmarks/p7-v11-715ab33-ff968c63-generation-v1-20261002/gate-assertions.json"], "scope": "V11 production mixed-generation L3 and finite-retry L2 subset; five fixed-source executions10/0/0, no full V05/V11 closure", "review": "integration owner; independent review pending; PR43/44 and V16 files untouched"}, {"target_sha": "32ce36ad424f5279fc0d2c37191e1f3dbb2eddb8", "artifact_paths": ["artifacts/benchmarks/p7-v05-32ce36a-a34ddca2-scope-v1-20261003/report.md", "artifacts/benchmarks/p7-v05-32ce36a-a34ddca2-scope-v1-20261003/gate-assertions-updated.json", "artifacts/benchmarks/p7-v16-independent-review-20261002/README.md", "artifacts/benchmarks/p7-v16-2d48f26-9bdab23f-cosine-v1-20261002/canonical-errata.json"], "scope": "V05 scoped production L2/L3 five rounds15/0/0; independent V16 PR47 proof limits corrected and nonunit/actual insertion variants retained; PR48 resource blocker explicitly open", "review": "V05 integration self-validation pending independent review; PR47 independent V16 audit preserved unchanged"}, {"target_sha": "a7efaae70cd0828b1a1b2d811e20176d855394b3", "artifact_paths": ["artifacts/benchmarks/p7-cachekeys-a7efaae-52cc1cbf-keys-v1-20261003/report.md", "artifacts/benchmarks/p7-cachekeys-a7efaae-52cc1cbf-keys-v1-20261003/cache-key-requirements.json", "artifacts/benchmarks/p7-cachekeys-a7efaae-52cc1cbf-keys-v1-20261003/formal-validation-map.json", "artifacts/benchmarks/p7-cachekeys-a7efaae-52cc1cbf-keys-v1-20261003/integration-receipt.json"], "scope": "production cache behavior five rounds15/0/0; bounded ArtifactCache original counterexample replay/resource2/0/0; all full gates pending independent consolidation", "review": "main integration self-validation; PR48/50 owner source/evidence unchanged; current independent reviewer pending"}, {"target_sha": "3dceedf3dcee851b3b2e4d4938bba11d76c63326", "artifact_paths": ["artifacts/checkpoints/cloud-p7-status-generation-fix-20261003/receipt.json", "artifacts/checkpoints/cloud-p7-status-generation-fix-20261003/deterministic-interleaves.json", "artifacts/checkpoints/cloud-p7-status-generation-fix-20261003/immutable-sources.json"], "scope": "bounded readiness race correction and unchanged independent V05/V11/P1 integration; full task not complete", "review": "integration self-replay; original independent reviewer must verify readiness fix SHA"}, {"target_sha": "8b6d9a1d9a7c05700d2de8611dbc20918647e527", "artifact_paths": ["artifacts/checkpoints/cloud-p7-cache-identity-integration-20261003/receipt.json", "artifacts/checkpoints/cloud-p7-cache-identity-integration-20261003/distinct-query-consumption.json", "artifacts/checkpoints/cloud-p7-cache-identity-integration-20261003/schema-reopen.json"], "scope": "bounded independent distinct consumer identity/schema integration; full C12/V11 not complete", "review": "PR55 original dual-Rust evidence unchanged; combined tree1.95 five tests5/0 plus spec12/0/readiness8/0"}, {"target_sha": "d6a54a28eb17a1f71f9924a94e05877801c17037", "artifact_paths": ["docs/roadmap/code-index-v2/P7-REMAINING-GATES.json", "artifacts/checkpoints/cloud-p7-formal-gate-scope-20261003/receipt.json"], "scope": "37 authority rows; V05 combined raw/hydrate gap declared L2 closed; independently verified specific status race closed; full gates/task remain open", "review": "PR57 frozen120pass unchanged; combined current dualfeature10/0; newV05 main-owned block pending independent review"}, {"target_sha": "744789649d1e0f2f3b41077ec3d1f5caacbff0cc", "artifact_paths": ["artifacts/checkpoints/cloud-p7-v11-cache-consumption-20261003/receipt.json", "docs/roadmap/code-index-v2/P7-REMAINING-GATES.json"], "scope": "V11 three remaining offline cache axes main-owned bounded L2; PR59/60 unchanged source-attributed evidence imported", "review": "ExactsourceHTTP8/0 +semantic3/0 strict scopedclippy/fmt. Independent current-cache review pending; futureupgrade runtime notrun; V19 externalreviewed0/600,8suite inputchecks not retrieval; full gates/task open."}, {"target_sha": "6f3cee1f2d23b43af5271bcaeaea67b931fd4fb7", "artifact_paths": ["artifacts/checkpoints/cloud-p7-v19-current-local-20261003/receipt.json", "artifacts/checkpoints/p7-gate-lifecycle-independent-20261003/gate-review-map.json", "docs/roadmap/code-index-v2/P7-REMAINING-GATES.json"], "scope": "New actual defaultstdio native dev56/164 plus8immutable replay; PR63 minimumV05/V16/V18 integrator decisions accepted; broader quality/crash/currentV11review remain open", "review": "181PR63files unchanged;388currentprod/Cargo fingerprints match; formalvalidatorcopiedtreeexit0;6f3CI153check/MSRV/securitysuccess. Run6gate0/2gate1 andR09invalid retained; compatibilitynotrun; no600holdout/liveclaim."}, {"target_sha": "5de088172afb5fbdbdeb23310835378818af2b01", "artifact_paths": ["artifacts/checkpoints/cloud-v11-consumption-review-20261003/receipt.json", "artifacts/checkpoints/cloud-p7-v19-current-local-20261003/receipt.json"], "scope": "PR66 independent boundedcurrentcache acceptance; actualcombinedoriginal3+newreview2HTTP5/0; fullV11/014 not claimed", "review": "20authorfilesunchanged/provenancepass; literalpath/bytes/score/pinnedreopen/budget actualconsumers. Initial outputdir setup failure hash retained; no production/assertion changes."}, {"target_sha": "78b0ce52cb2ff4c53c53893b9f7dd469269806b8", "artifact_paths": ["artifacts/checkpoints/cloud-p7-baseline-diagnosis-20261003/receipt.json", "artifacts/checkpoints/cloud-p7-baseline-diagnosis-20261003/diagnosis/summary.json", "artifacts/checkpoints/cloud-p7-baseline-diagnosis-20261003/current-V11-requirement-map.json"], "scope": "Actualfrozen164raw failureclassification,279physicalsourcechecks/164budgetchecks, finitecurrentV11run44/0; current-supportedV11minimumaccepted, futureupgradeconditionalnotrun", "review": "Existingindependentassertions unchanged;exact78b0CI159success. Nohardcorrectnessproductcounterexampleconfirmed,thusnoprod/scorer/goldpatch.63legalPartialqualityinconclusive/7validLocalrecallmisses/R09invalid/S11exactcompleteabsenceseparate. Parentholdoutcustodyblockedclean0; mainnewgoldunread."}, {"target_sha": "5385f5a7a2a875c6d5cbd049bdde039bf71bbf32", "artifact_paths": ["artifacts/checkpoints/cloud-p7-resource-prepared-20261003/public-dev-count-hash-intake.json"], "scope": "counts/hash-only four-repo fixed development-admission intake; no gold read/ranking or V19 quality acceptance", "review": "direct GitHub head/base/body matches parent final admission receipt hash; independent eval parent-owned"}, {"status": "accepted_fixed_e3_declared_local_100k_subgate_only", "artifacts": ["docs/checkpoints/2026-10-03-fixed-e3-integration/README.md", "docs/reviews/20261003-independent-e3", "artifacts/checkpoints/candidate-independent-100k-gate-20261003", "artifacts/checkpoints/candidate-independent-100k-recovery-20261003", "artifacts/checkpoints/baseline-cloud-20261003-build-blocked", "artifacts/checkpoints/baseline-cloud-20261003-resumed-once"], "limitations": "2026-10-03 新独立 fixed-e3 集成：唯一生产来源 e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207，显式 per-project>=2 local attempt width4，0/1 serial；HTTP4/2、claim16、fsync/cache layout 不变；PR127 facade 排除。PR125 b6b1639 独审限定通过。PR128 800d32d 只提取 driver/docs/tests；PR129 1be640e candidate 与 PR130 186ac53 baseline 各唯一正式100k，本地合成协议 count/FK/query/C4/normal EOF/reopen通过。candidate drain182.999904792s/cold22.086386410s，baseline257.929186553s/cold22.218803684s。独立云 cgroup关联/完整进程树未知，不宣称严格因果提速或统计显著。PR124/126失败、env-i DNS失败、旧EROFS及GC/WAL缺口保留。仅关闭本次固定协议子门，真实provider/heldout/质量/完整P7及P8-005均未验收。"}]
实施备注：2026-10-02 云端首块恢复默认/semantic 编译，状态与真实 stdio 兼容回归通过；完整工作区 2250 passed/4 failed/60 ignored，严格 lint/format 失败原件保留。P7-011～013 等待独立复核，P7-014 尚缺生产调度与 wired lifecycle E2E，不翻 done。 生命周期PR #7保留subsystem、初始化错误保原项目、close/reopen对称处理（默认253/semantic280通过）；纳入011/013后四crate lib975/0/1通过。生产worker调度与wired stdio仍未闭环，保持in_progress。 新云恢复分支codex/cloud-p7-recovery-integration以d6462c1为精确基线；逐文件diff证明PR10/11/12改动已组合，未重复cherry-pick。原样纳入PR13工厂9ea2b0b并注册，8/0/0局部测试通过；39文件格式问题已修正。生产worker/transport/wired stdio和独立正式验收仍待闭环，保持in_progress。 已恢复15326B旧worker补丁并核验hash；补齐有限job跨页消费、restart cache复用、输入完整provenance验证、close迟到响应fence与状态独立归属。纳入PR16生产transport7b335e5，feature默认关闭；装配/调用/销毁均在blocking job、无DB/CodeIndex网络锁，5runtime局部测试通过。stdio缺model echo的mock曾真实拒绝，修fixture而不放宽provider守卫，当前待精确binary复验；尚不收口。 检查点81b6188完整semantic-http workspace2334/0/61通过；真实stdio先通过五种状态/idle重开，但500重试被重开回填重置attempt_count，命令仍失败并保留hash摘要。新增短事务去重/版本检查保留pending/claimed/failed、已校验publication复用；6runtime回归含1100doc跨job和terminal failed通过。当前等待修复后精确SHA stdio复验，保持in_progress。 修复后2b8eb20固定HTTP产品真实stdio6/0：三种关闭门控零HTTP/缓存、装配失败、回填到ready、真实idle关闭重开零追加调用、三次500跨两次重开终态failed；所有进程exit0。独立审查及P7-013/完整V18未闭环，维持in_progress。 最新组合307a2f1 HTTP workspace2342/0/62(123 suites)通过；5c0c063两包重建default/semantic真实stdio各3/0/0、继承进程seccomp禁网且无缓存通过。fmt过，strict all-targets clippy仍有既有测试helper/表达式缺口；远端security/MSRV过，clippy失败下游skipped，未翻done。 独立PR25在993a64精确复现close/publish CAS窗口、取消耗未开始attempt、旧worker覆写新degraded三个真实阻塞（两feature各0/3），旧全仓绿不能代替。新分支codex/cloud-p7-lifecycle-fences原样纳入PR25及PR26；1b8a3d5以DB BEGIN后生命周期锁覆盖claim/publish提交，close不等外部writer；取消停止drain，pending hand-back保已开始计数、退未开始claim；投影原子校验当前recall端口实例。原target semantic/HTTP各3/0，HTTP全仓2345/0/62(124suites)，重建产品stdio6/0；HTTPlibstrictclippy过，alltargets仍受独立p7_acceptance_matrix awaitlock诊断阻塞。保持in_progress待修复后独立审查及完整V18/依赖。旧PR20/23/24共用993冻结。 3734481按父授权最小限定旧矩阵锁/读连接词法作用域，原断言不变：Rust1.95显式HTTP workspace alltargetsstrictclippy过、matrix7/0/0；旧748及993冻结。P7-014维持in_progress；新parameter/fault V18文件由独立owner编写，CIstdio receipt接线由原作者负责，未重复修改。 PR29/30/31/33/34真实包含关系核验后原样纳入query-optin新分支：三项生命周期反例独立复审双feature均3/0；原参数矩阵105/0；loopback502/断连/真实30sec超时与恢复通过但cold公开query仍unavailable；CI作者17步本地重放17/0，不等于最终Actions绿。f9d12c2新增独立allow_query_network默认false及四门控/活encoder状态，default和HTTP状态各1/0、配置7/0；执行接线未完成，保持in_progress。 51d6487原样纳入PR36模块/PR37断言并接线生产query：四门控默认关闭；前台2零排队/后台2保留，configured共享FIFO provider gate；同child绝对deadline、单次尝试、网络无DB/Index/CPU锁、物理退出前pin/capacity保留。修复不存在path_prefix仍编码缺口。module13/0、公开MCP1/0（非空语义search/context、cachehit零调用、held document期间非空Partial）、完整参数12/0、targetstrictclippy过。独立复核/完整矩阵/模型切换仍待闭环，保持in_progress。 bfaf45e补显式配置/provider安装切换权：worker BEGIN后生命锁覆盖原子注册/active切换/旧任务supersede及doc-spec revision审计，incarnation错或closed零切换；状态不拿旧空间覆盖冒充新模型ready。原P7-015 target两项不改断言且5s界2/0，DB3/0、状态6/0。全回归真实揭示async health callback阻塞回归，c8c20b5移回CPU且仅encoder存在时前探；另回填cache只读失败为命令漏临时cache根，修环境不扩权限。最终c8 HTTP workspace alltargets2383/0/62（123suites）、严格alltargetsclippy/fmt、semantic targetclippy通过。PR39模块双feature各5项独立审查原样保留，仅对应模块子项；最新生产独立复核、完整V18跨build/deadline、实际HTTP query超时及正式矩阵待闭环，不done。 远端CI123/run37076300547已直接回读：精确c8c20b5的check/MSRV/security三个job全部success；该项远端阻塞解除。与本地显式semantic-http全目标2383/0/62、独立公开/并发复核及正式矩阵分开计证，不翻done。 PR43 cb71d53六项生产公平/物理容量/DB与CPU进展及切换审查、PR44 027fc86实际产品stdio跨三build参数与HTTP deadline/cancel共155/0/0原样纳入；379生产/Cargo指纹与c8相同，解除对应有限子项独立审查缺口。CI124精确a9/run37077004045直接回读completed/success。2d48f26新增独立手算V16生产L2 oracle，三种文件创建轮换（非观测DB插入顺序，见PR47订正）/五次重放5/0/0、90 scope/cosine gold：独立literal范围判据、排除项余弦1高于合法项、tie、hydrate、delete、换space；合入独立targets20/0/0且strict clippy/fmt过。bounded-memory未测、完整V05/V11/V16/V18及依赖未闭环，保持in_progress；PR44取消后blockingHTTP可延续至deadline，不宣称立即abort或public成功晚响应隔离。 PR46/715ab33新增真实产品stdio混代反例：search/context先暖local旧源码731，hold cold query HTTP期间1连接读池真实rebuild/status提交新947；HTTP成功返回后明确-32603 retryable单次conflict且无envelope，稳定重发semantic/local只返947、同query vector复用零追加POST。L2直接生产fence真实rebuild每次改代精确3次耗尽、一次变更2次恢复、cancel/deadline/稳定损坏1次；固定源码五轮10/0/0、strict targetclippy/fmt过。权威V05/V11逐项断言/入口/余缺在gate-assertions.json；cache key完整行为矩阵及完整formal整合未完成，不将20/0/0或新局部重放冒充全门；P7-014维持in_progress。 PR49/32ce36a补V05实际产品stdio六DSL域×search/context×local/auto/semantic=每轮36，five轮180case；L2原始exact/path/lexical/grep四lane独立literal范围且正例各非空，Some(empty)files/languages、caller/DSL语言矛盾无candidate共25case；原unchanged p1a真实stdioBM25显式--ignored五轮5/0/0，总15/0/0。PR47/2edf200原样集成并复跑2/0/0，独立receipt audit15fixture90case，canonical订正旧单位vector不能排除dot mutation、文件创建轮换不等于实际DB插入，保原raw/report/test；新增nonunit/negative cosine和真实rowid顺序证明各自计证。PR48/671352d发现64MiB损坏cache payload/metadata读全文件导致~64MiB活跃分配/RSS，资源作者独占修复；V16资源blocked，不以正常矩阵抵消。PR45精确b973 CI131/run37079558785success已回读。完整V05/V11/V16及独立审查/正式consolidation仍开放，P7-014保持in_progress。 PR51/a7efaae真实cache行为矩阵：one共享querycache13命名case/轮，namespace/model/dim/query-only instruction/max_tokens/raw case/space/newline/Unicode触发factory+memorytransportPOST，repeat跳过；21 request/empty形式local+graph miss/hit与local新引擎重算一致、hard set序hit/soft hints序miss、graph六限制+预算miss、typed evidence仅graph失效、实际rebuild/source更新及public config权重/full DB incarnation观测。五轮15/0/0，65query+180local/graph命名case、55memoryPOST；8真实worker artifact哈希不变是query组件零文档副作用，不宣称真实换模型免reembed。完整C12维度要求/已测/静态不可选/not_run明确列账，非结构相等替代，完整V11未关。PR48原671352d及PR50 db9841e按真实链原样纳入，仅cache.rs生产变化；精确a7 server23/0/0、semantic227/0/0、cache17/0/0/exact4/0/0，strict targetclippy/fmt过。ignored资源显式original8cell+3case10k soak共2/0/0，原64MiB损坏allocation降9180/8391bytes；short RSS1sample局限保留，独立资源验收未done。首资源编译disk满无测试结果，清只可重建incremental且j1复编，不扩权限不删源码/证据。PR49精确1a6 CI137success已回读；最新树CI/独立consolidation待验，P7-014维持in_progress。 2026-10-03 PR54冻结源码3dceedf：原样纳入PR52三提交及PR53；真实worker强制发布交错复现ready/root epoch1与实际2混拼，完整status读取纳入同一三次generation fence后五轮40/0，严格两次返回2；真实incremental churn严格三次retryable无generation/ready。原公共V11断言未改，public+independent五轮45/0，扩展六target27/0，default status5/0，strictclippy/fmt过。PR53独立P1七测试新树7/0，cache.rs逐字等于db9841e，仅关闭64MiB读前无界allocation具体P1；全V05/V11/V16/P7-015/V20及014仍未收口。原PR52失败、default cache夹具失败及错target命令保留文件hash，不冒充race或通过。CI144精确源码当次仍运行；原PR50/51精确CI由父核验success。 2026-10-03 PR55两提交原样纳入新recovery cache-identity-review分支；8b6d9a1新树independent2+原cache矩阵3共5/0，11distinct消费值/9真实adapterPOST与schema真实reopen复核；spec12/0、status8/0、strictclippy/fmt通过。ADR0003完整digest及既有v1四字段序列化权威核验，三处dimension不进身份文字订正为完整tuple；去除整行注释后的Rust内容逐字不变，无公式/布局/版本兼容变更。作者1.99证据另计；原review脚本针对旧字节基线并写自身receipt，保留未改，不冒充新树检查。full C12/V11及014仍开放。 2026-10-03 精确剩余gate矩阵37row落P7-REMAINING-GATES.json，逐项固定source/证据、层级、断言缺口、输入规模与授权条件；V05八/V16六有declared范围证据，不冒充完整gate，V16尚余正式整合判定非强加100k/C8/16（归V20）。新增039d035真实worker/query编码/recall与六raw lane同域L2，7文件11case66lane receipts148候选21hits；跨文件Python graph邻居真实出现，scope排除，完整literal final sets/非空源码/kind-name断言，双feature各1/0，strictclippy/fmt过；semantic scope为真实port手工输入，不冒充新L3 DSL接线。PR57两提交原样纳入，冻结3d独立40commands120pass/原V11断言20轮/真实worker3次churn，specific statusrace关闭；新组合dualfeature两target10/0，260owner文件未改。旧P5 quality raw目录此checkout缺失，V19不能凭prose关闭；六项offline可推进，真实语义消融D1D2授权blocked。PR54精确c445 CI146及PR56精确4eac CI147直接回读success。task状态不翻done。 2026-10-03 PR61/7447896新增V11真实消费三项：50完整config字段独立不可变local/graph实例cold/warm/fresh及旧Arc复用，13score变化其余不冒充分支激活；5tier实际220files preselect120/120/120/150/200。graph top_k_only1/context30/topk2及hard集合换序Arc命中fresh相同。真实worker仅semantic_epoch发布，local/graph暖缓存保留而final hydrate报告新代；真实encoder/recall非空dense重复local/graph绕过缓存，public QueryHandle2miss与intent/topk重新selection/pack，无final envelope缓存。精确sourceHTTP三个targets8/0、semantic3/0、strictclippy/fmt过，主新断言待独立review/fullformal，非L3stdio/live。PR59 bf6c121三文件与PR60 bc8e22b九文件逐字原样纳入，verifier与7manifesthash过，future支持版本后继不存在，crossbuild0build/runtime保持not_run，不能人为改版本。V19早期102hash相符但两153不同source不能拼306，finalraw需原ownerarchive；56dev0holdout/externalreviewed0/600，八suite输入校验非检索。父六public作者/protocol独占准备，主不读holdout/gold不调参；新corpus不受旧raw阻塞。014维持in_progress。 2026-10-03 新V19/current-local default实际stdio基线精确6f3cee1，产品features空/SHA256 ef339ca364f837515321a16d3438e02fb565fee1d2534629a87cab3118d31247，engineBLAKE3另核非跨算法比较。八原dev/native56问164行、8validate/8run/8replay，所有raw/runhash重放不变；68success33nomatch63Partial，六gate0/twogate1原样保留。R09无当前identifier材料invalid三NoMatch与原score/denominator保留；S11新三空NoMatch不擦旧失败，未关闭泛化noanswer。compat无独立锁输入notrun；0holdout/0spangold/0chaingold/19symbolgold，非600公开质量。PR60 baselineprofile实际不支持错误留证，用现smoke观察不改prod/scorer/gold。PR63三提交181files原样纳入、388prod/Cargo指纹与审查83a6一致、临时副本formalrawvalidator过；660lane/1480candidate/210hit及39真实stdio lifecycle+6source回应按最小权威整合，V05/V16/V18 minimum正式接受，不无限追加allkind/100k/C8/16等不存在要求。精确6f3CI153check/MSRV/security已直接success，旧run不改名新验收。V11 cache独立review/条件性upgrade、V19quality/P7-016SIGKILL及014整体仍开放，状态不翻done。 2026-10-03 PR66 c9ae303两提交20files原样纳入，独立原3+新2双feature作者全过/provenance；主组合5de0881HTTP5/0、strictscopedclippy/fmt，literal路径源码/+2解析score/暖local+dense与reopenfinal/pinnedrerank/真实8KiBpack及2KiBfailclosed消费oracle支持三cache行closed_declared_subset。初次新review证据输出目录缺失导致write失败，mkdir后不改断言过，失败日志hash保留。fullV11/跨version/inactive评分branch未认证，014不翻done。精确PR61源码744CI152与最终6f3CI153已直接completed/success。 2026-10-03 PR67冻结164raw实际失败诊断：63合法Partial按21仅packing/9仅lane/33两者分列、维持qualityinconclusive不冒充success；S02/S04/S06/I04/I08/I12/I16七有效正例21completeNoMatch是真Local语义召回缺口、不改名invalid，I07/I15/R14在Partial下不做完整质量判定。R09独立invalid3行原gold/score/denominator保留，只出未执行的版本化建议。279hit真实源码正文/byte与line全部相符，164实际JSONcompact预算/used_bytes/tokenestimate同项目float_roundtrip全部相符；初步检查选错cachedserdefeature产生1byte假设差异已纠正非prod反例。S11三completeempty/noPartial、同域S01三positive及全3source无identifier证明单一负例，不擦历史failure不扩大generalnoanswer。当前78b0新run-idV11十integration31/0加queryencoding13/0，精确CI159success，当前支持产品字面V11minimum正式接受；未来实际支持版本迁移strictnot_run但条件性，不把不存在feature当永久当前产品阻塞。可选额外cc-searchunit编译diskfull未跑，日志hash保留，不算pass；进程退出后自动审批仅清理workspace可重建incremental，源/证据/产品和权限配置未动。无独立确认hardcorrectness实现反例，故不为分数造生产/参数/scorer/gold补丁。父protocol65d544cleanholdout0/custodyblocked及4repo dev-only条件记账，不读新gold/不改权限/credentials；V19真quality/P7-016等使014仍in_progress。 四repo开发准入仅metadata/count/hash接收PR86→90→91固定301native256compat100source476spans，280correlation非独立600；formal600/cleanholdout/20blocks/ranking0、两repoaccess/custody/live/统计与quality仍open，未读gold/不调参。 2026-10-03 PR95/5d3d9e固定78b0 DEV真实执行仅metadata准入：pilot81Partial、16suite全部尝试，783实际Partial/888missing/1671scheduled，Requests invaliddependencykey及Gin duplicatecallsite真实索引阻塞，非仅低质量分。1200raw/18replayhash为作者已验证而主未读gold/raw，新global统计invalid/notrun/quality未过。父独立最小fixture责任文件诊断中，主不重复；修后须冻结新sourceSHA重跑，不重标旧raw，formal600/cleanholdout0。 2026-10-03 ci-contract-integration 单块：从 PR110/5ffbadcf 固定树原样纳入 PR111/c69645d 的 ci.yml 与完整 semantic-http-ci-regression-20261003 checkpoint，生产 crates 字节保持 PR110；PR107 schema24 声明修正与显式 R09 PublicSurface::fingerprint DEV revision 已继承，旧基线完整保留，其它13题 query/answers 不改，新revision非质量提升。本批集成范围及组合树检查见 artifacts/checkpoints/ci-contract-integration-20261003/README.md；仅本批完成，不关闭014/015/016或全量gate。PR109/0c1e0b3a 独立Linux boundedpass仅链接，不复制harness。PR108/99973e7d 100k仍300s未ready29082，后续query/reopen未跑；原allfeatures2474pass/4fail/68ignore未修。全量CI只认新PR实际run/head；吞吐、GC/WAL fault、拒写runtime重试、真实provider/heldout及权限/凭据均不在本批。 组合树schema/Python9、DEV六suite/迁移negative controls、benchmark_lock10、semantic-http精确17及fmt通过；PR110固定5ffbadcf CI37120774371三个job success已独立回读，仅属PR110 checkpoint，不外推组合head。
2026-10-03 新独立 fixed-e3 集成：唯一生产来源 e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207，显式 per-project>=2 local attempt width4，0/1 serial；HTTP4/2、claim16、fsync/cache layout 不变；PR127 facade 排除。PR125 b6b1639 独审限定通过。PR128 800d32d 只提取 driver/docs/tests；PR129 1be640e candidate 与 PR130 186ac53 baseline 各唯一正式100k，本地合成协议 count/FK/query/C4/normal EOF/reopen通过。candidate drain182.999904792s/cold22.086386410s，baseline257.929186553s/cold22.218803684s。独立云 cgroup关联/完整进程树未知，不宣称严格因果提速或统计显著。PR124/126失败、env-i DNS失败、旧EROFS及GC/WAL缺口保留。仅关闭本次固定协议子门，真实provider/heldout/质量/完整P7及P8-005均未验收。

### [ ] P7-015｜后台回填与前台查询竞争测试

状态：`todo`；批次：`P7-C`；优先级：`normal`。
范围：`crates/cc-eval/tests/semantic_lifecycle.rs`；`crates/cc-eval/src/benchmark/sampler.rs`
硬依赖：P7-014
步骤：partial backfill时查询/写入/删除/切模型；测queue和CPU/DB占用
交付物：后台回填与前台查询竞争测试的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V14, V17, V20 验证证据（实施时生成）
验收：慢模型不会饿死local索引/查询，过期发布0；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V14；V17；V20
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：[{"target_sha": "9a2aa55a24fc0bcbb5009fea31e5b770060672eb", "artifact_paths": ["artifacts/checkpoints/cloud-p7-015-linux-sampler-20261002"], "review": "independent Linux sampler owner; bounded subitem only", "rollback_status": "revert dedicated sampler/evidence commits", "scope": "Linux /proc CPU/RSS unit/overflow/disappeared/unavailable; not full P7-015 acceptance"}, {"target_sha": "f255c60246a36f2980177814ceb770ea77c9c892", "artifact_paths": ["artifacts/checkpoints/cloud-p7-contention-validation-20261002/receipt.json"], "scope": "synthetic held-provider local query/write/delete/cancel subcase passes; unchanged model-switch assertion fails; complete V14/V17/V20 pending"}, {"target_sha": "c8c20b5b7d416372ee06ed5e248663c48912064e", "artifact_paths": ["artifacts/checkpoints/cloud-p7-model-transition-20261002/receipt.json"], "scope": "original unchanged contention model-switch negative now passes with original5s bound; bounded functional evidence only", "review": "integration self-validation; latest independent production review pending"}, {"target_sha": "3dceedf3dcee851b3b2e4d4938bba11d76c63326", "artifact_paths": ["artifacts/checkpoints/cloud-p7-status-generation-fix-20261003/receipt.json", "artifacts/checkpoints/cloud-p7-status-generation-fix-20261003/deterministic-interleaves.json", "artifacts/checkpoints/cloud-p7-status-generation-fix-20261003/immutable-sources.json"], "scope": "bounded readiness race correction and unchanged independent V05/V11/P1 integration; full task not complete", "review": "integration self-replay; original independent reviewer must verify readiness fix SHA"}, {"target_sha": "dee17f9a2965740297c154249b546ed90fe10992", "artifact_paths": ["artifacts/checkpoints/cloud-p7-resource-prepared-20261003/receipt.json"], "scope": "debug resource prepared24cells768query,1k/5k/10k C1/4/8/16;50k realcoldSQLlimitfailure/100knotrun/releasenotrun, harddep014 unmet", "review": "main exactbinary stdio; independent review pending", "rollback_status": "revert isolated runner/evidence; production/Cargo unchanged"}, {"target_sha": "854d69eb00f9507b355474d96303684cd8184d75", "artifact_paths": ["artifacts/checkpoints/cloud-p7-release-resource-small-20261003/receipt.json"], "scope": "1k only; 8 cells/256 queries synthetic loopback; prepared, not full V20", "review": "main exact release stdio; independent review pending"}, {"target_sha": "8b362e6b9d1334a51d5856ab21223d6e5ec28604", "artifact_paths": ["artifacts/checkpoints/cloud-p7-release-resource-50k-incomplete-20261003/receipt.json"], "scope": "actual release50k cold index300s client RPC timeout; cleanup original2x15s also timeout;0querycells/100knot_run", "review": "main retained failure; exact hot stage diagnosis pending"}, {"target_sha": "12428051e7342536432b9ae610552a6de4db885b", "artifact_paths": ["artifacts/checkpoints/cloud-p7-resource-review-intake-20261003/receipt.json"], "scope": "independent11files unchanged/main4testpass closes specific50k variableoverflow only; separate50k coldtimeout open"}, {"target_sha": "c70c68f2ff9b4ac40c635652858f56d2d0a06518", "artifact_paths": ["artifacts/checkpoints/cloud-p7-empty-test-population-20261003/handoff.json"], "scope": "stoprequested live50k release checkpoint:cold10.525s/8cells256zeroerrors; naturalbackfill/finalready notaccepted", "review": "main179functionalpass/1ignored +old3functionalpass; independentperf review/fulltask pending"}, {"status": "accepted_fixed_e3_declared_local_100k_subgate_only", "artifacts": ["docs/checkpoints/2026-10-03-fixed-e3-integration/README.md", "docs/reviews/20261003-independent-e3", "artifacts/checkpoints/candidate-independent-100k-gate-20261003", "artifacts/checkpoints/candidate-independent-100k-recovery-20261003", "artifacts/checkpoints/baseline-cloud-20261003-build-blocked", "artifacts/checkpoints/baseline-cloud-20261003-resumed-once"], "limitations": "2026-10-03 新独立 fixed-e3 集成：唯一生产来源 e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207，显式 per-project>=2 local attempt width4，0/1 serial；HTTP4/2、claim16、fsync/cache layout 不变；PR127 facade 排除。PR125 b6b1639 独审限定通过。PR128 800d32d 只提取 driver/docs/tests；PR129 1be640e candidate 与 PR130 186ac53 baseline 各唯一正式100k，本地合成协议 count/FK/query/C4/normal EOF/reopen通过。candidate drain182.999904792s/cold22.086386410s，baseline257.929186553s/cold22.218803684s。独立云 cgroup关联/完整进程树未知，不宣称严格因果提速或统计显著。PR124/126失败、env-i DNS失败、旧EROFS及GC/WAL缺口保留。仅关闭本次固定协议子门，真实provider/heldout/质量/完整P7及P8-005均未验收。"}]
实施备注： 独立Linux/proc采样子项PR19源9a2aa55/head640f668已原样纳入recovery：定向8测试20轮160pass、cc-eval lib39pass/5既有ignored、独立strictclippy/formatpass；不是完整P7-015，014/016依赖及资源完整矩阵未关闭，保持todo。 PR32只保留证据到主分支：真实public local query/write/delete在held provider期间通过，cancel旧worker零publication；完整新target1pass/1fail，model config切换缺显式注册/回填/激活触发（新provider0call），未改断言、未导入失败测试到主分支、未翻done。 c8c20b5恢复原f255测试源与optional默认关闭eval feature（未改原断言/5s界），精确源码重验2/0：3seed local query/write/delete/cancel及new model provider进展/旧空间零publication均通过，新路径保存正例，旧失败证据保留。只测合成in-process PID资源，未完成完整V14/V17/V20或014/016依赖，不done。 2026-10-03 PR54冻结源码3dceedf：原样纳入PR52三提交及PR53；真实worker强制发布交错复现ready/root epoch1与实际2混拼，完整status读取纳入同一三次generation fence后五轮40/0，严格两次返回2；真实incremental churn严格三次retryable无generation/ready。原公共V11断言未改，public+independent五轮45/0，扩展六target27/0，default status5/0，strictclippy/fmt过。PR53独立P1七测试新树7/0，cache.rs逐字等于db9841e，仅关闭64MiB读前无界allocation具体P1；全V05/V11/V16/P7-015/V20及014仍未收口。原PR52失败、default cache夹具失败及错target命令保留文件hash，不冒充race或通过。CI144精确源码当次仍运行；原PR50/51精确CI由父核验success。 2026-10-03 PR89/dee17f9独立debug资源prepared：1k/5k/10k×C1/4/8/16×distinct/repeated24cell768全样本，各真实inflight峰值达C。20ms root/runner/model RSS/CPU/thread/io计证，descendant枚举不可用tree及内部queue/service/锁等待null。50k真实cold index因resolver seed NOT IN50kbind超过SQLite变量上限失败，非OOM；100knotrun，最小生产修下一块。release/tmp构建需独立容量/内存评估，不拿debug过V20；015依赖014未满足保持todo。 2026-10-03 PR92/85692bb确认50k cold resolver seed SQL参数上限，改为单JSON bind保持literal排除语义；旧源真实1fail、新测试1pass及cc-db175pass/1ignored，strictclippy/fmt过。实际release source85692bb构建成功469.70s，binary2d45fac2d3e33b3557db0cdbee9c3b2e1e391de97ce6f6c613234683672fdccd；release矩阵尚not_run。PR89精确ff6 CI198失败为既有benchmark_fixture index warm p95 619.23ms>500ms，保留不改阈值。 2026-10-03 release source85692bb/driver854d69e实际1k×C1/4/8/16×distinct/repeated8cell256请求0error/empty，峰值达C、自然ready manifest1000/DBintegrityFK过/process0。原始release构建和binaryhash绑定，可只读重放；50k/100k该冻结块not_run。32/cell非正式tailCI，内部queue/service/锁等待和全树RSS unavailable，015保持todo/fullV20false。 2026-10-03 同实际release binary50k驱动8b362e6 cold index原300s RPC queue.Empty，0querycell/无ready，随后原close15s+terminate15s也超时；只核验PID/starttime/cmdline回收自建遗留进程，exe链接权限拒绝未扩权，原始DB/source/hash/失败日志保留。SQL变量错误未重现不等于SQL或50k全档通过，热点待定位；100knotrun，不加大阈值遮盖。PR89 ff6原CI198第一次index619.23>500ms保留，同精确SHA失败job已请求重跑用于稳定性诊断，待结果不判噪声。 2026-10-03 PR92精确ace2 CI200success。PR93精确8b36 CI201Test失败为空ready PID，确认我方fixture File::create先暴露文件再写内容竞态；独立新分支ea205fa仅我方p7_staging_gc_preparation durable helper改同目录temp写sync+rename+目录sync，原断言/15s不变，4轮各原15scenario/21kill通过，strictclippy/fmt过，prod/Cargo无变化。PR89 ff6 CI198同SHA第二次失败于独立crash_independent_review.rs229空PID，在benchmark前停止；原独立文件逐字保留交父协调原作者，原619.23ms性能稳定性/噪声仍未确定且500ms阈值未改。 2026-10-03 PR96/650311b原样纳入1242805，11owner文件逐字一致/verifier过，主新组合4/0/0真实API回归；具体SQLite变量overflow在已测literal/NULL/50kfiles100ksymbols/200003排除+4MiBstring范围独立确认关闭。1bind不保证无界内存耗时，原release50k300s timeout独立open/100knotrun/fullV20false。 2026-10-03 3a378fb隔离DEBUG既有timing诊断50k，binary616f6e3与正式2d45fac2分账且不进resourcecomparison；原300sRPC再次incomplete0cells，bounded自建child收尾，无provider请求。真实write/framework及synthesis0ms/Louvain20ms/analysis已完成，test_edges_apply无完成；只读DB50kfiles0test，代表现有LIKE候选query计划SCANfiles/0rows9.61ms，默认fullfalse50k新路径调用incrementaltestedges逐codefile空扫描。具体API/产品修复待回归，DEBUGmain已恢复，原正式失败不擦，015todo。 2026-10-03 用户要求开发/资源/集成拆session，主当前资源收敛停写：PR101/c70c68f仅cc-db incremental测试候选为空时同事务删除后一次存在检查省重复LIKEscan，原epoch/commit/非integerflag错误语义保留。定向4pass+原lib175pass/1ignored、strictclippy/fmt及旧9ac同3功能对照过。正式INFO releasec70实际binary8786768e，当前50k cold10.525s成功/8cell256request0error/empty/实峰C；只保存livecheckpoint，自然backfillready/完整性/process退出待确认，不作full50k/V20pass。不再新优化/更大规模，100knotrun，parser/resolution版本待真实语义fix新session集成，不造key。旧ledger仍主唯一至显式交权。 检查点最终回读订正：当前同一50k原执行已自然completed_prepared，ready50000manifest/files、integrityok/FK0/process0、drain179.243s，root采样peak2060541952B(非全树)，三自建PID均absent；只读raw/hash重放过，不补新测试/档，不扩fullV20，正式独立review/精确latestCI待新session。旧ledger此终点后停写至显式交权。 2026-10-03 正式移交唯一ledger给独立索引集成session；以PR101/574f759为新base，原样组合Requests103/da5b05e及Go102/99a2c04，固定算法升级source57bedba（schema23/manifest2）。真实PR101 old→new四组MCP14次index、旧pinned handle/manifest边界通过；Requests20同哈希文件及Gin109文件真实index过，workspace2340/0/65ignored、strictclippy/fmt过。Go独立reviewPR104/0647889远端一致；Requests新独立review BOUNDED_REJECT合法符号类型过滤回归，另新修复owner独占两函数，待其精确SHA及同reviewer复验，集成不得标accepted。coldreview101尚待父结论；本轮不扩性能/质量，015和live/heldout阻塞保持。证据artifacts/checkpoints/index-fix-integration-20261003，未读取protectedgold或造缓存key。 已读取可取回负证据7f650a5（43文件仅独立Requests review）明确BOUNDED_REJECT；cold块独立review70c2a64支持c70c68f原优化，9语义case/lib175与独立release50k9.816s通过，但不证明100k/fullV20。被拒review PR创建不重试。 2026-10-03 本轮集成闭环完成固定source9ebdb155：Requests修复671063b经同reviewer9e0c34a BOUNDED_PASS后组合。中间schema23/manifest2五份实际原库在未bump修复bf10b64上全no-op仍缺边，而同binary fresh各1边，按父明确决策升schema24/manifest3；原57bedba负例和快照保留。真实PR101/v22与中间v23→新版共10标识符路径恢复/保留targetUID+bucket、稳定no-op；旧v22/v23 pinned/query及manifest拒绝v3通过，Requests20与Gin109真实index过。default核心及9迁移测试、allfeaturesstrictclippy/fmt通过。完整allfeatures2474/4/68ignored，fixture阈值孤立重跑1/0，semantic lifecycle同5s失败在PR101复现，semantic_runtime明确cache put readonlyFS已停止对应写入不绕路，另一artifact等待超时未独立归因。仅算法/cache限定块通过，全部CI及015/V20/heldout/live不翻done。PR GraphQL Forbidden不重试。证据final-v24及same-v23-diagnostic。
2026-10-03 新独立 fixed-e3 集成：唯一生产来源 e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207，显式 per-project>=2 local attempt width4，0/1 serial；HTTP4/2、claim16、fsync/cache layout 不变；PR127 facade 排除。PR125 b6b1639 独审限定通过。PR128 800d32d 只提取 driver/docs/tests；PR129 1be640e candidate 与 PR130 186ac53 baseline 各唯一正式100k，本地合成协议 count/FK/query/C4/normal EOF/reopen通过。candidate drain182.999904792s/cold22.086386410s，baseline257.929186553s/cold22.218803684s。独立云 cgroup关联/完整进程树未知，不宣称严格因果提速或统计显著。PR124/126失败、env-i DNS失败、旧EROFS及GC/WAL缺口保留。仅关闭本次固定协议子门，真实provider/heldout/质量/完整P7及P8-005均未验收。

### [ ] P7-016｜fake全故障矩阵回归

状态：`todo`；批次：`P7-D`；优先级：`normal`。
范围：`crates/cc-eval/tests/semantic_lifecycle.rs`；`crates/cc-eval/tests/mcp_v2_contract.rs`
硬依赖：P6-020, P7-015
步骤：组合重试/close/rebuild/delete/lease/GC；每条保留可重放seed
交付物：fake全故障矩阵回归的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V14, V15, V17, V18 验证证据（实施时生成）
验收：依赖独立实验证明恢复，不把fake分数解释成语义效果；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V14；V15；V17；V18
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：[{"target_sha": "2d48f2628ae7c745fcab1a21dd784eae4582a193", "artifact_paths": ["artifacts/benchmarks/p7-v16-2d48f26-9bdab23f-cosine-v1-20261002/report.md", "artifacts/benchmarks/p7-v16-2d48f26-9bdab23f-cosine-v1-20261002/integration-receipt.json", "artifacts/benchmarks/p7-v16-2d48f26-9bdab23f-cosine-v1-20261002/offline-gaps.md"], "scope": "V16 independent hand-vector production L2 subset five runs 5/0/0, 90 gold cases; independent integration targets20/0/0. Bounded-memory/full V05/V11/V16 pending; no holdout/live quality claim", "review": "integration hand oracle; PR43 six independent production cases and PR44 actual product stdio 155/0/0 preserved unchanged"}, {"target_sha": "32ce36ad424f5279fc0d2c37191e1f3dbb2eddb8", "artifact_paths": ["artifacts/benchmarks/p7-v05-32ce36a-a34ddca2-scope-v1-20261003/report.md", "artifacts/benchmarks/p7-v05-32ce36a-a34ddca2-scope-v1-20261003/gate-assertions-updated.json", "artifacts/benchmarks/p7-v16-independent-review-20261002/README.md", "artifacts/benchmarks/p7-v16-2d48f26-9bdab23f-cosine-v1-20261002/canonical-errata.json"], "scope": "V05 scoped production L2/L3 five rounds15/0/0; independent V16 PR47 proof limits corrected and nonunit/actual insertion variants retained; PR48 resource blocker explicitly open", "review": "V05 integration self-validation pending independent review; PR47 independent V16 audit preserved unchanged"}, {"target_sha": "a7efaae70cd0828b1a1b2d811e20176d855394b3", "artifact_paths": ["artifacts/benchmarks/p7-cachekeys-a7efaae-52cc1cbf-keys-v1-20261003/report.md", "artifacts/benchmarks/p7-cachekeys-a7efaae-52cc1cbf-keys-v1-20261003/cache-key-requirements.json", "artifacts/benchmarks/p7-cachekeys-a7efaae-52cc1cbf-keys-v1-20261003/formal-validation-map.json", "artifacts/benchmarks/p7-cachekeys-a7efaae-52cc1cbf-keys-v1-20261003/integration-receipt.json"], "scope": "production cache behavior five rounds15/0/0; bounded ArtifactCache original counterexample replay/resource2/0/0; all full gates pending independent consolidation", "review": "main integration self-validation; PR48/50 owner source/evidence unchanged; current independent reviewer pending"}, {"target_sha": "3dceedf3dcee851b3b2e4d4938bba11d76c63326", "artifact_paths": ["artifacts/checkpoints/cloud-p7-status-generation-fix-20261003/receipt.json", "artifacts/checkpoints/cloud-p7-status-generation-fix-20261003/deterministic-interleaves.json", "artifacts/checkpoints/cloud-p7-status-generation-fix-20261003/immutable-sources.json"], "scope": "bounded readiness race correction and unchanged independent V05/V11/P1 integration; full task not complete", "review": "integration self-replay; original independent reviewer must verify readiness fix SHA"}, {"target_sha": "8b6d9a1d9a7c05700d2de8611dbc20918647e527", "artifact_paths": ["artifacts/checkpoints/cloud-p7-cache-identity-integration-20261003/receipt.json", "artifacts/checkpoints/cloud-p7-cache-identity-integration-20261003/distinct-query-consumption.json", "artifacts/checkpoints/cloud-p7-cache-identity-integration-20261003/schema-reopen.json"], "scope": "bounded independent distinct consumer identity/schema integration; full C12/V11 not complete", "review": "PR55 original dual-Rust evidence unchanged; combined tree1.95 five tests5/0 plus spec12/0/readiness8/0"}, {"target_sha": "d6a54a28eb17a1f71f9924a94e05877801c17037", "artifact_paths": ["docs/roadmap/code-index-v2/P7-REMAINING-GATES.json", "artifacts/checkpoints/cloud-p7-formal-gate-scope-20261003/receipt.json"], "scope": "37 authority rows; V05 combined raw/hydrate gap declared L2 closed; independently verified specific status race closed; full gates/task remain open", "review": "PR57 frozen120pass unchanged; combined current dualfeature10/0; newV05 main-owned block pending independent review"}, {"target_sha": "ee46fa564714ef75b2d839e5e37364cd35fd7406", "artifact_paths": ["artifacts/checkpoints/cloud-p7-crash-preparation-20261003/receipt.json"], "scope": "Independent prepared real SIGKILL L2 boundary matrix only; P7-016 todo, hard dependency P7-015 -> P7-014 unmet", "review": "main-owned new test:12 owned child SIGKILL, three different inputs,1 parent pass/0fail/1 ignored child entry; independent review pending", "rollback_status": "revert isolated preparation test/evidence; production/schema/Cargo unchanged"}, {"target_sha": "a83aa1bd4d0c7bbc6a9ce9d5d7c19a226c607bd3", "artifact_paths": ["artifacts/checkpoints/cloud-p7-staging-gc-prepared-20261003/receipt.json"], "scope": "prepared only:15 scenarios/21 owned real SIGKILL staging and GC collect-to-sweep schedules; internal rename/mark-unlink/pins not_run; task remains todo", "review": "main-owned test/11 unchanged regression passes; independent review pending", "rollback_status": "isolated new test/evidence revert; production/Cargo/oldtests unchanged"}, {"target_sha": "84613f7a3c6e319fb7c847a6ea64f0192a2cc43b", "artifact_paths": ["artifacts/checkpoints/crash-independent-review-20261003/receipt.json", "artifacts/checkpoints/cloud-p7-staging-gc-prepared-20261003/crash-cost-canonical.json"], "scope": "Independent completed-boundary review with response-before-cache duplicate-work negative case; prepared only", "review": "author originals74files unchanged; local integration replay tracked separately"}, {"target_sha": "c91d67250235266de376ea021ebafa2b0ca3b428", "artifact_paths": ["artifacts/checkpoints/staging-gc-independent-review-20261003/receipt.json"], "scope": "Independent completed staging/GC boundaries prepared;15scenes21kill/internalgapfalse/genericpinfalse", "review": "119 ownerfiles byte-identical; read-only verifier passed"}]
实施备注： 2026-10-03 PR54冻结源码3dceedf：原样纳入PR52三提交及PR53；真实worker强制发布交错复现ready/root epoch1与实际2混拼，完整status读取纳入同一三次generation fence后五轮40/0，严格两次返回2；真实incremental churn严格三次retryable无generation/ready。原公共V11断言未改，public+independent五轮45/0，扩展六target27/0，default status5/0，strictclippy/fmt过。PR53独立P1七测试新树7/0，cache.rs逐字等于db9841e，仅关闭64MiB读前无界allocation具体P1；全V05/V11/V16/P7-015/V20及014仍未收口。原PR52失败、default cache夹具失败及错target命令保留文件hash，不冒充race或通过。CI144精确源码当次仍运行；原PR50/51精确CI由父核验success。 2026-10-03 PR55两提交原样纳入新recovery cache-identity-review分支；8b6d9a1新树independent2+原cache矩阵3共5/0，11distinct消费值/9真实adapterPOST与schema真实reopen复核；spec12/0、status8/0、strictclippy/fmt通过。ADR0003完整digest及既有v1四字段序列化权威核验，三处dimension不进身份文字订正为完整tuple；去除整行注释后的Rust内容逐字不变，无公式/布局/版本兼容变更。作者1.99证据另计；原review脚本针对旧字节基线并写自身receipt，保留未改，不冒充新树检查。full C12/V11及014仍开放。 2026-10-03 精确剩余gate矩阵37row落P7-REMAINING-GATES.json，逐项固定source/证据、层级、断言缺口、输入规模与授权条件；V05八/V16六有declared范围证据，不冒充完整gate，V16尚余正式整合判定非强加100k/C8/16（归V20）。新增039d035真实worker/query编码/recall与六raw lane同域L2，7文件11case66lane receipts148候选21hits；跨文件Python graph邻居真实出现，scope排除，完整literal final sets/非空源码/kind-name断言，双feature各1/0，strictclippy/fmt过；semantic scope为真实port手工输入，不冒充新L3 DSL接线。PR57两提交原样纳入，冻结3d独立40commands120pass/原V11断言20轮/真实worker3次churn，specific statusrace关闭；新组合dualfeature两target10/0，260owner文件未改。旧P5 quality raw目录此checkout缺失，V19不能凭prose关闭；六项offline可推进，真实语义消融D1D2授权blocked。PR54精确c445 CI146及PR56精确4eac CI147直接回读success。task状态不翻done。 2026-10-03 独立prepared：PR81/ee46fa5真实子进程12 SIGKILL四完成持久化边界×3不同合成输入；自然lease过期、rollback/reclaim/artifact复用/commit ack/27no-op扫描。只自有child，零live/network；19相关原测试通过；claim3/2/1与provider每场景1分开。014未完成且015todo为硬依赖，016保持todo，不记正式开始/完整验收；内部put/CAS断点/staging rename/GC竞争/production stdio startup恢复待做。 2026-10-03 PR84/a83aa1b独立prepared：staging事务内/build完成/swap完成和GC collect→claim/publish→sweep跨进程15场景21kill，原published引用保全、swap后实际spaces0显式注册后1、cache reuse零新provider、GC删真实orphan；11原tests过。内部WAL删除→rename/mark→unlink无hooks，方案未实施，generic pin无API不假覆盖。016仍todo，014/015硬依赖未满足。 PR85/84613f7原74文件纳入并逐字核验；15kill/12task/36no-op独立review，仅boundary recovery，手工rollback不是productionCAS。response-before-cache3case各2fakecalls，原9任务1call域保留；crash原attempt billing unknown/retry可重复，现ledger明确in-memory，不把无receipt填0或声称任意crash exactly-once。 2026-10-03 父提供PR101静态边界，仅登记不扩当前cache迁移：gc collect/classify保存created_at/checksum，sweep旧mark后unlink与publish/cache原路径替换缺共享锁，可能旧mark删新产物，属于待实证并发推导；remove_quiet吞error而deleted无条件++为源码确定统计缺陷，未修；index_db_rebuild删旧WAL/SHM后rename、rename后释放write_conn锁再open新conn并重新拿锁替换的窗口，需核更高层串行约束，属于静态推导待实证。相关GC/WAL fault执行任务被平台拦截，本集成不换方式执行，不宣称已复现/已修或完整crash通过。

### [ ] P7-017｜离线默认包和未启用测试

状态：`todo`；批次：`P7-D`；优先级：`normal`。
范围：`crates/cc-server/Cargo.toml`；`crates/cc-eval/tests/benchmark_adapters.rs`
硬依赖：P7-016
步骤：禁网络环境启动所有旧工具；检查不创建语义cache文件
交付物：离线默认包和未启用测试的实现/配置或规格变更；artifacts/benchmarks/<run-id>/ 中对应 V18, V21 验证证据（实施时生成）
验收：零key、feature关、semantic.enabled=false都保持本地功能；相关旧功能回归通过；没有证据的项标not_run/blocked而非done。
验证：V18；V21
回滚：关闭远程语义与重试，切local；保留outbox/cache和费用收据供恢复。
证据：[{"target_sha": "5c0c0632791a2e0d2f27afe9659845f0aced786c", "worktree_digest": "bbeed33dc761ab6ba3556576c2fbcd037bab2239", "artifact_paths": ["artifacts/checkpoints/cloud-p7-recovery-20261002/validation-receipt.json"], "review": "integration self-review; independent review pending", "rollback_status": "revert dedicated recovery blocks; configuration can disable semantic", "scope": "workspace HTTP 2342/0/62; exact combined default and semantic real stdio each3/0 under process seccomp; strict clippy failed, full gates remain open"}, {"target_sha": "c8c20b5b7d416372ee06ed5e248663c48912064e", "artifact_paths": ["artifacts/checkpoints/cloud-p7-model-transition-20261002/receipt.json"], "scope": "HTTP alltargets workspace and strict clippy pass; six corpus locks pass; final remote CI and disabled product full gate pending", "review": "integration self-validation; latest independent production review pending"}, {"target_sha": "c8c20b5b7d416372ee06ed5e248663c48912064e", "artifact_paths": ["artifacts/checkpoints/cloud-p7-model-transition-20261002/receipt.json"], "scope": "remote CI123 run37076300547 check/MSRV/security all success; distinct from local semantic-http and pending independent/formal acceptance", "review": "GitHub jobs terminal states read back by integration owner"}]
实施备注： 原样纳入PR21 e20fee4/f9bcb19：default/semantic包各3/0真实stdio为独立855b4fc基线证据；进程seccomp禁网成功而非namespace隔离。组合307a2f1两包重验待执行，正式017不done。 最新组合307a2f1 HTTP workspace2342/0/62(123 suites)通过；5c0c063两包重建default/semantic真实stdio各3/0/0、继承进程seccomp禁网且无缓存通过。fmt过，strict all-targets clippy仍有既有测试helper/表达式缺口；远端security/MSRV过，clippy失败下游skipped，未翻done。 PR40源80a43e0仅修source digest，query/gold评分字节不变；最终树六套锁实测过。PR42源56318f0追溯R09历史helper不存在：当前源码评测单独标该题invalid/pending redesign，保留历史query/gold，不静默删除/算成功，不拿旧14题称新holdout；其余13只做实体有界审计未测检索表现。c8 HTTP全目标2383/0/62及strictalltargetsclippy/fmt过；远端CI123 MSRV/security成功、check进行中，不能称Actions全绿。 远端CI123/run37076300547已直接回读：精确c8c20b5的check/MSRV/security三个job全部success；该项远端阻塞解除。与本地显式semantic-http全目标2383/0/62、独立公开/并发复核及正式矩阵分开计证，不翻done。

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
实施备注：2026-10-02 用户拍板（D1）：本轮不授权真实付费 provider，本任务维持 conditional 不执行，live 证据栏显式标 blocked；详见 artifacts/checkpoints/p789-blocking-analysis-20261002/DECISIONS-RECORDED.json。

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
证据：[{"target_sha": "d6a54a28eb17a1f71f9924a94e05877801c17037", "artifact_paths": ["docs/roadmap/code-index-v2/P7-REMAINING-GATES.json", "artifacts/checkpoints/cloud-p7-formal-gate-scope-20261003/receipt.json"], "scope": "37 authority rows; V05 combined raw/hydrate gap declared L2 closed; independently verified specific status race closed; full gates/task remain open", "review": "PR57 frozen120pass unchanged; combined current dualfeature10/0; newV05 main-owned block pending independent review"}]
实施备注： 2026-10-03 精确剩余gate矩阵37row落P7-REMAINING-GATES.json，逐项固定source/证据、层级、断言缺口、输入规模与授权条件；V05八/V16六有declared范围证据，不冒充完整gate，V16尚余正式整合判定非强加100k/C8/16（归V20）。新增039d035真实worker/query编码/recall与六raw lane同域L2，7文件11case66lane receipts148候选21hits；跨文件Python graph邻居真实出现，scope排除，完整literal final sets/非空源码/kind-name断言，双feature各1/0，strictclippy/fmt过；semantic scope为真实port手工输入，不冒充新L3 DSL接线。PR57两提交原样纳入，冻结3d独立40commands120pass/原V11断言20轮/真实worker3次churn，specific statusrace关闭；新组合dualfeature两target10/0，260owner文件未改。旧P5 quality raw目录此checkout缺失，V19不能凭prose关闭；六项offline可推进，真实语义消融D1D2授权blocked。PR54精确c445 CI146及PR56精确4eac CI147直接回读success。task状态不翻done。

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
证据：[{"target_sha": "d6a54a28eb17a1f71f9924a94e05877801c17037", "artifact_paths": ["docs/roadmap/code-index-v2/P7-REMAINING-GATES.json", "artifacts/checkpoints/cloud-p7-formal-gate-scope-20261003/receipt.json"], "scope": "37 authority rows; V05 combined raw/hydrate gap declared L2 closed; independently verified specific status race closed; full gates/task remain open", "review": "PR57 frozen120pass unchanged; combined current dualfeature10/0; newV05 main-owned block pending independent review"}, {"target_sha": "79193f09ffd40c9e3d30ab6c1e0e2c8cc3a372db", "status": "candidate_limited_combination_verified_final_acceptance_blocked", "artifact_paths": ["artifacts/checkpoints/candidate-integration-20261003/README.md", "artifacts/checkpoints/candidate-integration-20261003/sources.json", "artifacts/checkpoints/candidate-integration-20261003/verified-final/receipts.json"], "scope": "PR117+121 canonical baseline; production-only three-way FIFO+parallel+retry; finite loopback actual doc/query gate+queue/runtime width0/2, 4/2 caps, shared batch16, ordinary retry/backoff, close/cancel/join; exact original regressions", "limitations": "仅候选限定集成；父最终验收未完成。PR120对旧5cce6eb的REJECT保留。父转达独立gate review完整验收REJECT：P1 ProjectSession::new吞gate Config后回退空index并缓存到被拒项目路径，后续指定路径仍Ok；本轮不扩改session/core startup，由父另立修复。另一独立复审仍待父收敛；CI/draft GitHub API权限阻塞保留，性能/100k not_run。"}, {"status": "accepted_fixed_e3_declared_local_100k_subgate_only", "artifacts": ["docs/checkpoints/2026-10-03-fixed-e3-integration/README.md", "docs/reviews/20261003-independent-e3", "artifacts/checkpoints/candidate-independent-100k-gate-20261003", "artifacts/checkpoints/candidate-independent-100k-recovery-20261003", "artifacts/checkpoints/baseline-cloud-20261003-build-blocked", "artifacts/checkpoints/baseline-cloud-20261003-resumed-once"], "limitations": "2026-10-03 新独立 fixed-e3 集成：唯一生产来源 e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207，显式 per-project>=2 local attempt width4，0/1 serial；HTTP4/2、claim16、fsync/cache layout 不变；PR127 facade 排除。PR125 b6b1639 独审限定通过。PR128 800d32d 只提取 driver/docs/tests；PR129 1be640e candidate 与 PR130 186ac53 baseline 各唯一正式100k，本地合成协议 count/FK/query/C4/normal EOF/reopen通过。candidate drain182.999904792s/cold22.086386410s，baseline257.929186553s/cold22.218803684s。独立云 cgroup关联/完整进程树未知，不宣称严格因果提速或统计显著。PR124/126失败、env-i DNS失败、旧EROFS及GC/WAL缺口保留。仅关闭本次固定协议子门，真实provider/heldout/质量/完整P7及P8-005均未验收。"}]
实施备注： 2026-10-03 精确剩余gate矩阵37row落P7-REMAINING-GATES.json，逐项固定source/证据、层级、断言缺口、输入规模与授权条件；V05八/V16六有declared范围证据，不冒充完整gate，V16尚余正式整合判定非强加100k/C8/16（归V20）。新增039d035真实worker/query编码/recall与六raw lane同域L2，7文件11case66lane receipts148候选21hits；跨文件Python graph邻居真实出现，scope排除，完整literal final sets/非空源码/kind-name断言，双feature各1/0，strictclippy/fmt过；semantic scope为真实port手工输入，不冒充新L3 DSL接线。PR57两提交原样纳入，冻结3d独立40commands120pass/原V11断言20轮/真实worker3次churn，specific statusrace关闭；新组合dualfeature两target10/0，260owner文件未改。旧P5 quality raw目录此checkout缺失，V19不能凭prose关闭；六项offline可推进，真实语义消融D1D2授权blocked。PR54精确c445 CI146及PR56精确4eac CI147直接回读success。task状态不翻done。 2026-10-03 独立候选最小集成：PR117/121为祖先，三方生产差异整合FIFO/parallel/retry，20文档真实loopback工厂组合width0/2、4/2、16claim、普通retry继续ready及close/cancel/join限定通过。完整验收仍blocked：父转达gate review P1 ProjectSession::new吞Config回退并缓存被拒路径，未关闭，不在此轮扩改；旧PR120拒绝结论保留，其他独立复审待父。性能/100k未运行，P7-020及P8-005状态不翻done。
#### 2026-10-03 startup P1 新独立组合限定验证

新分支 `candidate/integrate-startup-gate-parallel-retry-20261003`，固定 base `8ac869194534a154269a3ed424dd51976400e375`；仅三方应用 startup 生产 `6011116a1bb6f069c1d4b65cc20fe9f19b9b0003` 的 crates 差异（来源 head `2d4d033860551157b2500521b0f53ec3a6811f15`），最终生产/测试 source `11f5b76273a16b520e6b6e01ad32a0a6743173fb`。原 queue/runtime/wiring/admission/DirectWriter/SQL 及其他未适配 crate 文件逐字等于 base；必要调用点和新测试保留，新 stdio fixture 显式 own cache。

实际完成：default 全 workspace/all-target no-run + 严格 Clippy；semantic-http 全 workspace/all-target check + 严格 Clippy + 所有相关运行 targets no-run；startup 两个 isolated 叶子、default stdio/None 两个、semantic-http 三个共 7 叶子通过（实际 `run_mcp_server` Config 拒绝、无新增 task、cache/live 不登记、None/合法项目与 retry/reopen 正常）；原组合七隔离叶子 + retry/parallel/queue/FIFO/DirectWriter 共 51 叶子通过。fmt/diff check 通过。证据：`artifacts/checkpoints/startup-candidate-integration-20261003/{receipts.json,sources.json,selected-binary-identity.json}`。

这些是新组合限定验证，不是 startup 独立复审或父最终 accept。首次离线 reqwest 缺失、一次宽泛 semantic-http 全 workspace executable 链接磁盘耗尽及 stdio harness 可执行替换 Text file busy 均保留事实，后续精确 no-run/全 target check 与 atomic replace 完成。旧 100k 失败、旧组合完整验收 REJECT 和 parallel 性能未验收不改写；本轮 100k、真实 provider、heldout、GC/WAL crash/kill 均 not_run。P7-020/P8-005 及完整任务状态不翻 done；release 构建准备与远端交付回执另见同目录。

2026-10-03 新独立 fixed-e3 集成：唯一生产来源 e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207，显式 per-project>=2 local attempt width4，0/1 serial；HTTP4/2、claim16、fsync/cache layout 不变；PR127 facade 排除。PR125 b6b1639 独审限定通过。PR128 800d32d 只提取 driver/docs/tests；PR129 1be640e candidate 与 PR130 186ac53 baseline 各唯一正式100k，本地合成协议 count/FK/query/C4/normal EOF/reopen通过。candidate drain182.999904792s/cold22.086386410s，baseline257.929186553s/cold22.218803684s。独立云 cgroup关联/完整进程树未知，不宣称严格因果提速或统计显著。PR124/126失败、env-i DNS失败、旧EROFS及GC/WAL缺口保留。仅关闭本次固定协议子门，真实provider/heldout/质量/完整P7及P8-005均未验收。

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
实施备注：2026-10-02 用户拍板（D3）：真实多仓语料采用公开仓 + 固定 commit（延续 09-BENCHMARK 已锁定的 Flask/cc-switch 先例）；详见 artifacts/checkpoints/p789-blocking-analysis-20261002/DECISIONS-RECORDED.json。

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
证据：[{"status": "accepted_fixed_e3_declared_local_100k_subgate_only", "artifacts": ["docs/checkpoints/2026-10-03-fixed-e3-integration/README.md", "docs/reviews/20261003-independent-e3", "artifacts/checkpoints/candidate-independent-100k-gate-20261003", "artifacts/checkpoints/candidate-independent-100k-recovery-20261003", "artifacts/checkpoints/baseline-cloud-20261003-build-blocked", "artifacts/checkpoints/baseline-cloud-20261003-resumed-once"], "limitations": "2026-10-03 新独立 fixed-e3 集成：唯一生产来源 e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207，显式 per-project>=2 local attempt width4，0/1 serial；HTTP4/2、claim16、fsync/cache layout 不变；PR127 facade 排除。PR125 b6b1639 独审限定通过。PR128 800d32d 只提取 driver/docs/tests；PR129 1be640e candidate 与 PR130 186ac53 baseline 各唯一正式100k，本地合成协议 count/FK/query/C4/normal EOF/reopen通过。candidate drain182.999904792s/cold22.086386410s，baseline257.929186553s/cold22.218803684s。独立云 cgroup关联/完整进程树未知，不宣称严格因果提速或统计显著。PR124/126失败、env-i DNS失败、旧EROFS及GC/WAL缺口保留。仅关闭本次固定协议子门，真实provider/heldout/质量/完整P7及P8-005均未验收。"}]
实施备注：
2026-10-03 新独立 fixed-e3 集成：唯一生产来源 e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207，显式 per-project>=2 local attempt width4，0/1 serial；HTTP4/2、claim16、fsync/cache layout 不变；PR127 facade 排除。PR125 b6b1639 独审限定通过。PR128 800d32d 只提取 driver/docs/tests；PR129 1be640e candidate 与 PR130 186ac53 baseline 各唯一正式100k，本地合成协议 count/FK/query/C4/normal EOF/reopen通过。candidate drain182.999904792s/cold22.086386410s，baseline257.929186553s/cold22.218803684s。独立云 cgroup关联/完整进程树未知，不宣称严格因果提速或统计显著。PR124/126失败、env-i DNS失败、旧EROFS及GC/WAL缺口保留。仅关闭本次固定协议子门，真实provider/heldout/质量/完整P7及P8-005均未验收。

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
