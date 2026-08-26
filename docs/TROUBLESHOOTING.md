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

## 兼容性与稳定性口径

- **MCP 工具面**:14 个工具的名字、参数与响应形态以
  [MCP_TOOLS.md](MCP_TOOLS.md) 为契约文档;破坏性变更(参数语义、错误
  码映射)会在该文档的对应小节内注明旧行为与迁移方式(如上表)。
- **索引磁盘格式**:无稳定性承诺。`index.sqlite3` 是可再生缓存,
  schema 版本变更即重建,不提供迁移工具。
- **CLI**:`codecortex mcp` / `install` / `uninstall` 三个子命令是稳定
  面;其余一切经 MCP。
