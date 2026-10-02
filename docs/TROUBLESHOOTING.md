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

配置加载对未知键只告警不失败;已在历史版本移除的键会提示删除。

| 已移除的键 | 移除原因 / 替代 |
|------------|----------------|
| `indexing.parallelism` | 由 rayon 默认线程池 + `indexing.max_concurrent_parse`(或 `CODECORTEX_MAX_CONCURRENT_PARSE`)取代 |

行为契约的历史变更(客户端可能依赖的):

| 变更 | 迁移动作 |
|------|----------|
| 未知工具参数从"静默忽略"改为拒绝(工具级错误结果) | 按错误信息中的 serde 诊断改名或删除字段 |
| `relations` 的 `direction` 收敛为 `up`/`down`/`both` | 旧值 `ancestors`/`descendants` 仍作为别名接受 |

## 语义缓存与降级(P6,可选)

语义持久化是可选功能(`semantic` feature + 组合根接线),默认构建不加载
`cc-semantic`、不读配置、不创建任何文件;以下条目面向"语义功能已接线"
的部署。机制事实摘编自
[STORAGE.md](internals/STORAGE.md#语义持久化p6schema-v22)(crash 点恢复)
与 [CONFIGURATION.md](CONFIGURATION.md#语义缓存与降级p6可选)(降级语义);
组合根尚未接线的部分在条目内如实注明。

### dense 检索少了文档 / 怀疑语义缓存损坏

- **缓存缺失(Miss)是常态,不是故障**:冷缓存或未回填的文档在 dense lane
  缺席,lexical/graph 本地检索不受影响,绝不把缺向量当完整空结果;
  纯 Miss 永不触发降级状态。
- **缓存损坏(Corrupt)**:检索侧一律跳过候选、不报错——健康文档照常返回,
  坏对象不污染结果。worker 侧显式检测点会把坏对象双半隔离进
  `<root>/quarantine/`(保留 `.meta.json` 证据 + `.report.json` 诊断
  sidecar),补嵌成功后新产物覆盖原地址、发布 CAS 重新可见——**自愈**,
  不需要手工删库。
- **损坏的可见性**:判据为 `corrupt_events > 0 || re-embed 预算耗尽`,
  满足其一即 capability status 透出 `semantic_state: "degraded"` +
  `degraded_reason`。注意:透出槽与映射逻辑已交付
  (`capability_status.rs`),降级快照的转写由组合根完成——接线完成前该
  字段尚无生产写入点,事实依据以 outbox `last_error` 与 quarantine
  目录内容为准。

### `CODECORTEX_SEMANTIC_CACHE_ROOT` 不生效 / cache 目录没出现

1. 根目录解析三级序:环境变量(空白值视为**未设**)→ macOS
   `~/Library/Caches/codecortex/semantic` → Linux
   `$XDG_CACHE_HOME|~/.cache/codecortex/semantic`;
2. `open` 零文件系统副作用,首个 `put` 才惰性建目录——**目录不存在不代表
   功能故障**,只说明尚无嵌入写入;`put`/`get` 路径不读任何环境变量;
3. cache 首层按项目 namespace 隔离(`<root>/namespace-<ns>/…`):跨项目
   不共享、跨克隆(同项目身份)共享;**不绑 incarnation**——索引重建换库
   后同一项目解析出同一 namespace;
4. `.codecortex.json` 目前**没有**语义相关键;cache 机制已交付库层,
   根目录/项目身份的组合根接线归接线轮(见 CONFIGURATION.md 语义节)。

### outbox 出现 `failed` 死信 / 提示 "re-embed budget exhausted"

- **识别**:任务终态 `failed`、`last_error` 含 `semantic re-embed budget
  exhausted`——即"已付费产物损坏后的补嵌"触发了进程生命周期 re-embed
  预算上限。拒绝发生在调用付费 provider **之前**,attempt 预算耗尽而死
  信,**不静默无界重费**;死信只清点、绝不自动复活;manifest 行保留,
  dense 检索对受影响文档降级为空(不伪造结果)。
- **处置**:①查 `<root>/quarantine/` 的 `.report.json` 确认坏对象来源
  (磁盘/进程异常);②排除环境问题后重启进程——进程内预算随重启清零
  (outbox 行计数为持久审计轨);③重推 reconcile,任务重新入队后预算内
  补嵌 → put 覆盖 → 发布 CAS,可见集合自愈。首嵌不计入预算;预算与
  GC 宽限目前均为调用方参数,尚无配置文件键。

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
