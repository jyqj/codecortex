# 排障指南

按症状组织的常见问题与诊断步骤。运行时口径以
`status(aspect="capabilities")` 为准;错误码契约见
[MCP_TOOLS.md](MCP_TOOLS.md#通用契约)。

## 快速诊断开关

| 手段 | 用法 |
|------|------|
| 结构化日志 | 服务器日志走 stderr(stdio 传输下不会污染协议流)。`RUST_LOG=cc_index=debug,cc_db=debug,cc_server=debug` 打开子系统 debug 事件(阶段计时 `time_step`、watcher tick、缓存命中) |
| 索引健康 | `status(aspect="index")`:文件数 / 符号数 / epoch / 上次构建各阶段毫秒 |
| 构建决策 | `index()` 响应的 `build_explain`:postprocess / analysis 各签名门 run/skip 的原因与降级信号 |
| 图读解释 | 图工具响应的 `graph_explain`:遍历的边 kind、截断原因(`truncated_reason`)、被降级的 DB 读错误(`read_errors`) |

## 索引问题

### 服务器启动了,但工具都报 "project not set" / IndexUnavailable

MCP 客户端从别的工作目录拉起服务器时,自动项目发现(向上找 `.git` /
`.codecortex.json`)找不到项目。两种解法:调用一次 `index(path)`,或以
`codecortex mcp --project-path /abs/path` 启动。

空闲驱逐(默认 60 秒无活动)会关闭索引句柄,下一次调用透明重开——若
偶发 `no index database`,重试一次即可,不需要重启服务器。

### 索引结果陈旧 / watcher 没有跟上文件变更

1. 确认 `.codecortex.json` 没有 `auto_index.enabled = false`;
2. watcher 对事件风暴(分支切换、批量生成)做自适应去抖,大仓延迟
   数百毫秒到数秒属预期;
3. OS 事件丢失(编辑器原子替换、网络盘)时 watcher 回退全树扫描,
   自愈;若仍陈旧,手动 `index(path)` 一次;
4. `.gitignore` / `indexing.ignore` 的编辑只影响之后的构建,已被
   un-ignore 的旧文件在下一次全树构建(手动 `index`)时纳入。

### `index` 返回 BuildBusy

同项目已有一个结构性构建在跑(构建门串行化所有入口)。错误携带
`data.retryable: true`——等在跑的构建结束后重试同一调用即可,不要
并发重发。

### 构建后 schema 版本不匹配 / 索引像被清空重建了

设计即如此:索引是缓存,schema 版本不匹配时**直接重建**而非迁移
(`index.sqlite3` 可随时丢弃)。升级二进制后的第一次构建变慢属预期。

### 日志出现 "memory pressure: RSS exceeds budget"

解析阶段的自适应内存预算(默认物理内存 × 0.5)在收缩批大小,构建变慢
但不失败。若机器内存充裕想要更快:调高
`indexing.memory_budget_fraction` 或 `CODECORTEX_MEMORY_BUDGET_FRACTION`。
巨型仓库也可用 `CODECORTEX_SEED_CACHE_MAX_SYMBOLS=0` 换取更低常驻
(代价是每次构建重载 seed,见
[CONFIGURATION.md](CONFIGURATION.md#环境变量覆盖))。

## MCP 调用问题

### Codex CLI 安装或卸载失败

`codecortex install` 对 `~/.codex/config.toml` 做结构化 TOML 更新，匹配
`mcp_servers` 下名为 `codecortex` 的实际键。注释、其他服务的路径、
`codecortex.other` 这样的不同服务名都不会被当成该服务。带引号的键、
内联表、嵌套表和多行值按 TOML 结构处理；路径中的反斜杠、引号及其他
特殊字符由序列化器正确转义。

重复安装会更新该服务的 `command` 和 `args`，保留其 `env`、超时等其他
选项。已有 `enabled=false` 也会保留；安装后工具不可用时先核对这个设置。
卸载仅删除该服务及其子表，不删除其他服务。配置中没有该服务时，卸载
保持文件原始字节不变。

读取错误、无效 UTF-8、TOML 语法错误或非表类型的 `mcp_servers` /
`mcp_servers.codecortex` 都会使操作失败，不会把原配置当成空文件覆盖。
按错误提示检查对应配置；`--force` 只扩大自动检测的安装目标，不会跳过
配置校验。若现有 CodeCortex 条目使用 `url` 连接远端服务，安装器会保留
该条目并报告冲突；决定切换到本地 stdio 后，先明确移除旧条目再安装。

CLI 会逐个打印成功目标和错误；任一目标失败时，安装 / 卸载命令返回非零
退出码，其他已经成功的目标仍保留其结果。不要只据最后的成功目标数量
判断整次操作已成功。

### 工具调用报参数错误(-32602 或工具级错误结果)

参数没过校验,两条通道都自带诊断:

- **schema 反序列化失败**(未知字段/拼写错误、类型不匹配、缺必填字段)
  以工具级错误结果返回(`is_error: true`),文本给出 `expected one of`
  清单;
- **sanitize 校验失败**(非法枚举值等)返回 JSON-RPC `-32602`,列出全部
  合法值。

注意**未知参数名直接拒绝**——早期版本会静默忽略未知参数,依赖旧行为的
客户端按错误信息改名/删除字段即可(见
[MCP_TOOLS.md](MCP_TOOLS.md#参数校验sanitize))。

### 工具调用报 -32603 且 data.retryable = true

瞬态条件(并发构建 / 陈旧的 prepare 快照),原样重试一次即可。
`retryable` 缺失或为 false 的 -32603 是真实失败,看错误文本。

### 图查询结果比预期少

看响应里的 `graph_explain.truncated_reason`:`output_budget` /
`default_limit` / `max_depth` 等 token 指明第一个生效的裁剪。调大对应
参数或预算;`read_errors` 非空说明部分读被降级,通常是并发重建窗口,
重试即可。

## 配置迁移

普通配置对象的未知键会告警并忽略;已在历史版本移除的键会提示删除。
`query` 对象使用 `deny_unknown_fields`,其中未知键会使整份项目配置
反序列化失败,随后使用默认配置并应用环境变量覆盖。先按日志修正键名,
再确认所需配置已生效;读取或解析失败不能当作配置加载成功。
详见 [CONFIGURATION.md](CONFIGURATION.md) 与实际加载入口
[`load_project_config`](../crates/cc-model/src/config.rs)。

| 已移除的键 | 移除原因 / 替代 |
|------------|----------------|
| `indexing.parallelism` | 由 rayon 默认线程池 + `indexing.max_concurrent_parse`(或 `CODECORTEX_MAX_CONCURRENT_PARSE`)取代 |

行为契约的历史变更(客户端可能依赖的):

| 变更 | 迁移动作 |
|------|----------|
| 未知工具参数从"静默忽略"改为拒绝(工具级错误结果) | 按错误信息中的 serde 诊断改名或删除字段 |
| `relations` 的 `direction` 收敛为 `up`/`down`/`both` | 旧值 `ancestors`/`descendants` 仍作为别名接受 |

## 语义缓存与降级(P6,可选)

语义持久化是可选功能,默认构建不接入 `cc-semantic`。普通项目配置仍然
读取,本地索引和日志目录仍可创建,见
[`CodeIndex::set_project`](../crates/cc-server/src/engine.rs)。
启用 `semantic` feature 后,`semantic.enabled=false` 使语义子系统不装配,
不会为它创建 cache 目录;`semantic-http` 进一步提供受配置门控的 provider
transport。当前组合根已接入 cache、worker、预算、租约与 GC,以下按实际
部署是否启用对应能力排查。配置说明见
[CONFIGURATION.md](CONFIGURATION.md#语义缓存与降级p6可选),接线入口见
[`semantic_wiring.rs`](../crates/cc-server/src/semantic_wiring.rs)。

### dense 检索少了文档 / 怀疑语义缓存损坏

- **缓存缺失(Miss)是常态,不是故障**:冷缓存或未回填的文档在 dense lane
  缺席,lexical/graph 本地检索不受影响,绝不把缺向量当完整空结果;
  纯 Miss 不计为 corruption。实际范围内的 artifact 缺口仍会使原本
  Complete 的 dense receipt 变成 Partial,原因 `semantic_artifact_unavailable`。
- **缓存损坏(Corrupt)**:exact 扫描跳过已检测损坏的候选,保留合法候选,
  并报告该次扫描的 artifact 缺口。真实文件读取错误仍会传播为错误,
  不能把它们记成 Miss 或完整成功。边界见
  [`search_controlled_with_coverage`](../crates/cc-semantic/src/vector/exact.rs)。
  worker 侧显式检测点会把坏对象隔离进
  `<root>/quarantine/`(保留 `.meta.json` 证据 + `.report.json` 诊断
  sidecar),在预算内补嵌并通过发布 CAS 后,产物才可重新可见。
- **损坏的可见性**:判据为 `corrupt_events > 0 || re-embed 预算耗尽`,
  已挂接的语义子系统满足其一时,capability status 透出
  `semantic_state: "degraded"` 与 `degraded_reason`。worker drain 已将当前
  ledger 快照转写到对应服务,由
  [`apply_semantic_degradation`](../crates/cc-server/src/capability_status.rs)
  读取。ledger 为 degraded 时,后续 dense 查询可在扫描前返回
  Unavailable(`semantic_provider_degraded`),本地 lanes 继续工作。
  corruption 事件在该 ledger 生命周期内累积,补嵌成功本身不会清除它们;
  需同时核对当前能力状态、outbox `last_error` 与 quarantine 诊断,不能
  仅凭新产物存在就断言已经恢复 ready。计数规则见
  [`DegradationLedger::snapshot`](../crates/cc-semantic/src/degrade.rs)。

### `CODECORTEX_SEMANTIC_CACHE_ROOT` 不生效 / cache 目录没出现

1. 根目录解析三级序:环境变量(空白值视为**未设**)→ macOS
   `~/Library/Caches/codecortex/semantic` → Linux
   `$XDG_CACHE_HOME|~/.cache/codecortex/semantic`;
2. `open` 零文件系统副作用,首个 `put` 才惰性建目录——**目录不存在不代表
   功能故障**,只说明尚无嵌入写入;`put`/`get` 路径不读任何环境变量;
3. cache 首层按项目 namespace 隔离(`<root>/namespace-<ns>/…`):跨项目
   不共享、跨克隆(同项目身份)共享;**不绑 incarnation**——索引重建换库
   后同一项目解析出同一 namespace;
4. `.codecortex.json` 已有 `semantic` 配置节,由组合根读取;cache 根目录
   仍采用上面的环境变量/平台规则。核对 `semantic.enabled`、所构建的
   feature 以及启动日志,不能只凭目录不存在判断环境变量未生效。预算、
   租约与 GC 的配置键见 [CONFIGURATION.md](CONFIGURATION.md#语义缓存与降级p6可选)。

### outbox 出现 `failed` 死信 / 提示 "re-embed budget exhausted"

- **识别**:先核对 `last_error`,不能把所有 `failed` 都归为预算问题。
  包含 `semantic re-embed budget exhausted` 时,表示已检测损坏输入的
  补嵌被进程生命周期预算拒绝;拒绝发生在调用 provider **之前**。
  outbox 的 attempt 预算与这个补嵌预算独立,终态 `failed` 持久保留。
- **配置**:`semantic.reembed_budget_max` 默认 `null`(不另设数量上限),
  首次嵌入不计入这个补嵌预算;`semantic.worker_lease_secs` 默认 `600`,
  `semantic.gc_min_retention_secs` 默认 `3600`,启用语义时后两者均须
  为正值。配置值与允许范围见 [CONFIGURATION.md](CONFIGURATION.md)。

处置顺序:

1. 保存 `status(aspect="capabilities")` 输出、相关日志和
   `<root>/quarantine/` 中的 `.report.json`,排查存储、输入或 provider
   的实际失败原因。
2. 修复原因及错误配置后重新启动 MCP 服务器,再核对日志和能力状态。
   新的进程内 ledger 会重新开始计数,但 outbox 中的重试次数、退避、
   live lease 与终态不会因此重置。
3. 同一文档版本、输入和空间的任务仍为 `failed` 时,保留其诊断继续处理。
   当前 MCP/CLI 没有按 outbox `task_id` 复活单条死信的命令;普通重开
   和补齐缺失任务的流程也不会将它重新入队。后续恢复应依据完整诊断
   选择可验证的操作,不要手工修改数据库状态绕过重试预算。

这一持久边界由
[`enqueue_semantic_worker_missing`](../crates/cc-db/src/document_store.rs)
与实际运行时回归
[`reopen_preserves_live_retry_budget_and_terminal_failure`](../crates/cc-server/src/semantic_runtime.rs)
共同约束。

### 换库 / 重建索引后,语义要重新付费吗

不用(缓存完好时)。预期行为:

- 换库后语义三表从零开始、`semantic_epoch` 缺席,由 reconcile 协议
  恢复:fence 先行 → desired 重导重入队 → **cache 命中的任务直接经
  五重 fence CAS 重发布,零新 provider 调用**;只有 cache miss 才回
  worker 重新嵌入(重新付费面 = 真正丢失的产物)。
- crash 残态同样有界可恢复:

| crash 点 | 表现 | 恢复路径 |
|---|---|---|
| artifact 写入中 | cache 半文件 / `*.tmp-*` 残迹 | 读侧 Miss/Corrupt 交还 worker;GC 超期清扫 temp 与半文件 |
| 写入后、发布 CAS 前 | 产物已 durable、manifest 无行、任务 claimed | lease 过期回收 → 重放时 cache 命中直接发布,付费产物复用、零重复嵌入 |
| 发布 CAS 提交后 | 已原子(写与 ack 同事务) | 等值重复发布被吸收:零可见变化、零双 bump |
| 换库 rename 中/后 | 旧进程仍持旧 incarnation 连接 | freshness fence:publish/claim/recover 在任何事务前 `Fenced` 零写入,权威库零污染 |

- 明确口径:**不宣称跨两库/网络 exactly-once 或零重复收费**——重复做功
  由任务合并 supersede、fencing 丢弃慢结果、预算死信三道机制限制上界。

## 兼容性与稳定性口径

- **MCP 工具面**:14 个工具的名字、参数与响应形态以
  [MCP_TOOLS.md](MCP_TOOLS.md) 为契约文档;破坏性变更(参数语义、错误
  码映射)会在该文档的对应小节内注明旧行为与迁移方式(如上表)。
- **索引磁盘格式**:无稳定性承诺。`index.sqlite3` 是可再生缓存,
  schema 版本变更即重建,不提供迁移工具。
- **CLI**:`codecortex mcp` / `install` / `uninstall` 三个子命令是稳定
  面;其余一切经 MCP。
