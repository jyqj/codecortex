# P8 文档事实、安装与默认离线契约

## 当前事实的来源

本页推进 P8-018 的本地文档同步。`scripts/p8_facts.py` 从当前 Cargo、SQL、模块能力声明、
MCP 注册及少量明确的 Rust 默认值声明生成下表，同时检查 ARCHITECTURE、MCP_TOOLS 与
CONFIGURATION 的对应入口。它不修改生产代码，不联网，也不执行被索引项目。

本轮仓库内没有发现名为 `runtime.schema.json` 或 `defaults.md` 的生成物，因此不把它们
当作已交付契约。实际配置声明位于
[`config.rs`](../../../crates/cc-model/src/config.rs) 和
[`query.rs`](../../../crates/cc-model/src/query.rs)；数据库 schema 与
[`MODULE_CAPABILITIES.json`](../../internals/MODULE_CAPABILITIES.json) 交叉核验。
数据库版本、模块模型版本、MCP 工具输入 schema 和能力观测 spec 分别计数。

## 由声明生成的受管表

<!-- p8-facts:start -->
| 事实 | 当前声明值 | 直接来源 |
|---|---|---|
| Cargo workspace crate 数 | `8` | `Cargo.toml` |
| workspace 成员 | `cc-model, cc-db, cc-parsers, cc-index, cc-search, cc-server, cc-eval, cc-semantic` | `Cargo.toml` |
| 声明 MSRV | `1.95` | `Cargo.toml` |
| 数据库 schema | `25` | `index_migrate.rs / MODULE_CAPABILITIES.json` |
| 显式普通表数（不含 FTS/shadow） | `30` | `sql/index_v1.sql` |
| 显式 FTS5 虚表数 | `5` | `sql/index_v1.sql` |
| 数据库版本不匹配策略 | `rebuild_on_mismatch` | `MODULE_CAPABILITIES.json` |
| ProjectModel 版本 | `3` | `MODULE_CAPABILITIES.json` |
| 默认网络声明 | `false` | `MODULE_CAPABILITIES.json` |
| 执行被索引源码声明 | `false` | `MODULE_CAPABILITIES.json` |
| MCP 工具数 | `14` | `cc-server/src/mcp.rs` |
| MCP 工具名 | `status, index, search, context, node, explore, trace, relations, impact, architecture, files, graph_query, ingest_traces, adr` | `cc-server/src/mcp.rs` |
| 能力观测 spec | `retrieval-capabilities-v2` | `capability_status.rs` |
| 产品默认 Cargo features | `[]` | `cc-server/Cargo.toml` |
| query.strategy | `local` | `query.rs` |
| query.deadline_ms | `30000` | `query.rs` |
| query.lane_timeout_ms | `20000` | `query.rs` |
| query.semantic_timeout_ms | `5000` | `query.rs` |
| query.semantic_top_k | `24` | `query.rs` |
| auto_index.enabled | `true` | `config.rs` |
| auto_index.file_limit | `50000` | `config.rs` |
| auto_index.idle_timeout_secs | `60` | `config.rs` |
| semantic.enabled | `false` | `config.rs` |
| semantic.network_opt_in | `false` | `config.rs` |
| semantic.allow_query_network | `false` | `config.rs` |
| semantic.allow_http | `false` | `config.rs` |
| semantic.reembed_budget_max | `null` | `config.rs` |
| semantic.worker_lease_secs | `600` | `config.rs` |
| semantic.gc_min_retention_secs | `3600` | `config.rs` |
<!-- p8-facts:end -->

更新命令（Python 3.11+，只用标准库）：

```sh
python3 scripts/p8_facts.py --check
python3 scripts/p8_facts.py --write
```

`--check` 只读，文档漂移返回 1；声明不一致、读取或不支持的语法返回 2。
`--write` 只更新本页标记之间的表，不自动改写其它文档、能力 JSON 或源码；其它入口若仍
与事实不一致，依然返回 1。JSON 输出附每个实际输入文件的 SHA-256，便于本轮证据绑定。
变化需要先由实现者和审阅者解释，再更新受管表。

**自动检查范围明确有限**：Cargo/TOML 和能力 JSON 是结构化输入；SQL 仅识别当前显式
`CREATE TABLE ...` / `CREATE VIRTUAL TABLE ... USING fts5` 形状；工具表仅识别当前命名宏
形状。Rust 默认值只支持列出的类型中直接字面量和单个返回字面量的 default 函数。
遇到其它形状会拒绝，不猜值。它不是 Rust/SQL 语义分析器，也不是运行中 binary 的 schema
导出器；运行时行为、完整参数 schema 与实际 `tools/list` 仍需各自集成测试。

普通表与 FTS5 虚表分开列出，不计 SQLite 的内部表或 FTS shadow 表。本轮另在 Python 的
内存 SQLite 中执行了实际 `index_v1.sql`，独立核对普通表/FTS5 数量；这是 DDL 夹具检查，
不是生产库迁移或历史数据恢复认证。

## 安装、离线默认与故障定位

生产 CLI 的实际入口为 `mcp`（别名 `serve`）、`install`、`uninstall`，定义在
[`cli.rs`](../../../crates/cc-server/src/cli.rs)。本地默认产品的构建示例：

```sh
cargo build --release --locked -p cc-server --bin codecortex --no-default-features
/absolute/build/codecortex install
/absolute/build/codecortex mcp --project-path /absolute/project
```

`/absolute/build/codecortex` 必须替换为刚构建并保存摘要的实际文件。
install 为检测到的客户端写 MCP 配置，`--force` 是显式选项；本轮只核对入口声明，
没有修改用户实际客户端配置，也没有执行安装/卸载。完整使用说明见仓库 README 与
[CONFIGURATION](../../CONFIGURATION.md)。MSRV 声明不是本轮已运行对应工具链的证明。

默认 `query.strategy=local`、语义总开关与两项网络授权均关闭。普通配置和本地索引仍正常读取，
本地索引可创建 `index.sqlite3`；不能把“语义关闭不创建 cache”写成“整个产品不读配置、不创建文件”。
`semantic` feature 接入本地语义持久化与编排；`semantic-http` 进一步接入 provider transport。
总开关、network opt-in 和 query 文本外发授权分别处理；仅开启 `enabled` 不等于授权 query 外发。

| 观察到的情况 | 正确解释与入口 |
|---|---|
| `no_project` / `closed` | 核对 `--project-path` 或 `index(path)`；没有索引时不能把端口挂接当成 ready |
| `semantic_disabled` / `network_opt_in_required` / `query_network_opt_in_required` | 是明确的默认/授权诊断，按所需能力核对配置；不要为本地查询自动开启网络 |
| backfilling / failed / ready / dense partial | 来自 active space 与发布/欠账状态；ready 只描述该观察时点，不证明模型收益或下一条查询完整 |
| schema mismatch | 当前数据库版本不相同则走重建路径；升级/回滚需隔离缓存及原始证据，不把旧 v21→v22 的历史例外套到当前 v25 |
| 配置未知键 | 普通字段日志告警；`query` 的 `deny_unknown_fields` 会使该次反序列化失败并回退默认配置，需检查日志而非假定生效 |

能力观测来自 [`capability_status.rs`](../../../crates/cc-server/src/capability_status.rs)；
组装与实际网络执行的边界分别在
[`semantic_wiring.rs`](../../../crates/cc-server/src/semantic_wiring.rs) 和
[`semantic_runtime.rs`](../../../crates/cc-server/src/semantic_runtime.rs)。
观测不发起 provider 探针，不等于真实服务认证。旧端口、替库与并发变化的实际保证应继续由
P7 的生产回归和独立审查验收，本页不将代码存在转成该父阶段已完成。

## 本轮 P8 工具与证据边界

| 已交付入口 | 本地行为 | 仍需单独证明 |
|---|---|---|
| `p8_release_evidence.py freeze/verify` | 同一候选固定源码/binary/config/model/scoring，以及显式 corpus-root 中 query/gold/source 的内容与增删 | binary 构建来源、完整传递输入准入、真实产品执行 |
| `p8_release_evidence.py archive` | 独占创建带摘要的 run，保存完整原始文件并离线校验；失败/取消不提升 latest | `passed_local` 的真实测量依据、G8/发行批准；metadata-only corpus 不可提升 latest |
| `cc-eval run/replay` 的 latency-strata/resource-ledger | 保留全部 attempt 分母、未知冷热状态及 unavailable 资源/费用 | 直接 lifecycle/cache 观测、受控 p95/p99、真实账单；没有观测不能填零 |
| `cc-eval compare` | 保留 failed/inconclusive/invalid 退出码及失败收据，已有输出拒绝覆盖 | 可比环境、足够样本、完整 release profile；普通 passed 不是发行认证 |
| `p8_facts.py --check/--write` | 生成并检查本页受管表与选定文档入口 | 完整运行时 schema、全部默认值、安装执行和跨平台验证 |

具体输入和 API 见 [候选/归档](P8-RELEASE-EVIDENCE.md)、[测量分层](P8-MEASUREMENTS.md)
与 [失败门](P8-GATES.md)。历史统计、旧源码证明和普通基线结果保留原来的适用范围。
P8-018 的 P8-017 依赖、完整 V18/V21 及发布文档验收仍待各自证据，本文和受管表通过不将任务自动标 done。

## 这次同步修复的可核对不一致

- ARCHITECTURE 的 workspace 数从旧 7 改为实际 8，schema 从旧 v22 改为实际 v25，
  SQL 普通表从旧 29 改为实际 30，补充 cc-semantic、三时钟和派生 cache 边界。
- CONFIGURATION 移除“dense 尚未实现、始终 disabled”的过期概括，补充现有状态投影；
  补齐已存在的 reembed/worker lease/GC 配置键与默认值，修正仍写“接线轮才实现”的旧表述。
- MCP_TOOLS 明确注册数量来自源码声明，数据库版本不当作 MCP schema 版本；
  BENCHMARK 加入本地 P8 工具入口，旧 benchmark 数字保持历史口径。
- STORAGE 顶部的“当前 schema 22”改为实际 25，保留 v22 表结构和旧迁移的历史说明；
  此项只修复已核对的版本矛盾，不代表全部 internals 文档通过审查。

验证收据位于 `artifacts/benchmarks/p8-facts-review-20261007/`。源码准入规则扩展的独立审查
单独保留在该目录：只接受不可变 base/source/review pin，对每个触及路径复核已接受 before bytes，
最终完整 PRODUCT 仍独立核对；该审查不替代历史 source verifier 全套重放。
