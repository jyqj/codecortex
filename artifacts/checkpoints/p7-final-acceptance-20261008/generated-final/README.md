# P7 / G7 当前复核材料

当前未完成 **32 项**；状态分布：`{"done": 160, "blocked": 1, "in_progress": 19, "todo": 12}`。

工程 Gate：**未验收**。本命令只核对固定任务条件、证据字节和引用，不能凭状态或 hash 批准功能/发行。

live：**blocked**，沿用 D1+D2；未调用真实 provider、没有真实效果或金额证据。

| 原任务 | 当前状态 | 本轮交付类型 | 观察范围 |
|---|---|---|---|
| P7-014 | done | implementation | 原配置/schema/sanitize/handler/doc/MCP链和原mode兼容完成；实际13状态、107 RPC及配置关闭/重开/撤销/失败投影通过；显式/索引后有限后台任务串起分页对账、恢复与单页GC。原接线7/13按需条件已明确处置，未实现的历史持久计数和日志截断不宣称实现。 |
| P7-015 | done | current_validation | 原慢模型竞争工程要求完成：真实held后台工作下384请求、3seed×2phase×2concurrency×32，原2s查询/5s切换界限通过，过期发布0；CPU/RSS/DB/queue归属与12格原统计重算。1100真实manifest跨64文档分页及1024项有限job边界，重开不增加provider调用。 |
| P7-016 | done | implementation | 原fake故障矩阵及13状态工程链完成：3seed真实stdio组合重试/close/rebuild/delete/lease/GC与SIGKILL，原内部GC/WAL/发布/恢复回归通过；新增3个机会回收入口均为目标space内有界page，9新与29旧合同测试证据完整，失败历史保留。 |
| P7-017 | done | current_validation | 原default与semantic未启用工程合同完成：两真实私有构建包合计4配置×14旧工具和4次独立reopen断言（每包2配置，合计8个产品根进程），完整strace子进程树复核外部网络尝试0、semantic cache创建0；8个合法匿名IPC，原IPv4/6 SIGSYS及swallowed-child负控成立。 |
| P7-018 | blocked | blocked_disposition | Original D1+D2 live-provider authorization disposition remains blocked, with no provider calls, currency or live-quality claim. |
| P7-019 | done | current_validation | 原TASK-BRIEFS规定可先完成的fake机制工程腿完成：同输入同完整budget的none/local/dense-only/hybrid四真实构建及独立lane身份，36请求、24测量，原计分和2000次配对bootstrap重算一致，exact退化/heldout未评估/费用unknown分别报告；不将fake差异称为真实语义收益。 |
| P7-020 | done | scope_audit | 依赖014→015→016→017→019及先前001–013完成后，按原G7分别验收当前工程/fake并记录live blocked；13条原接线逐条对账，7/13显式条件处置。原dossier工具保持not_accepted，由单独非作者人工复核承载G7结论。完整V19/G8和live效果不认证。 |

## 尚需闭合

硬依赖：任务状态均已完成，仍需Gate独立验收。
尚未验收且未提交本轮证据：无。
来源与当前整合输入不同的记录：无；它们仍保持各自原源码的观察范围。

已验收、未重复提交本轮证据：P7-001, P7-002, P7-003, P7-004, P7-005, P7-006, P7-007, P7-008, P7-009, P7-010, P7-011, P7-012, P7-013。缺少本轮条目不会重开这些任务；原验收记录继续保留。

## 13 项接线对账

| 项 | 负责任务 | 当前判断 | 依据与剩余事项 |
|---:|---|---|---|
| 1 | P7-014 | implemented | 完整配置校验装配后替换旧实例；close/reopen对称关闭runtime/端口/状态槽；原13状态与旧mode通过。 |
| 2 | P7-014 | implemented | 索引事务后或显式有限调度串起64文档分页desired对账、fenced drain/cache reuse、目标space有界lease回收及成功job后一页GC。status/query/idle非调度触发。 |
| 3 | P7-014 | implemented | Corrupt经quarantine_detected/requeue_after_degrade处理并投影当前ledger；不把缓存损坏冒充本次付费失败。 |
| 4 | P7-010 | implemented | 保留P7-010已接受ownership；DegradationLedger投影绑定当前端口，retired worker不能覆写新投影，当前旧回归通过。 |
| 5 | P7-010 | implemented | 保留P7-010 ownership；规范项目namespace，disabled先于cache root lookup返回，空/关闭job零cache创建。 |
| 6 | P7-014 | implemented | gc_min_retention_secs使用正数且≤i64::MAX检查；disabled整节惰性，配置合法性与完整原字段链已回归。 |
| 7 | P7-014 | transferred | 按原“Auxiliary计数，按需”条件处置。当前GcCounters/分页结构化日志及真实GC删除计数已验证；历史持久累计表未实现且无当前产品消费者。root源码review及最终非作者人工接受此条件处置。 |
| 8 | P7-005 | implemented | 保留P7-005 ownership；project-FIFO/doc-round-robin闭枚举经配置到worker限制并供两drain消费，原公平性合同未放宽。 |
| 9 | P7-016 | implemented | 由P7-016关闭：串行drain、并行drain、显式revocation drain在目标space内每入口最多64且受max_batch裁剪，SQL space/state/expiry在LIMIT内；9新真实SQLite边界与29旧合同及本轮完整CI通过。旧全量显式facade不冒充生产有界路径。 |
| 10 | P7-014 | implemented | 公共status在短只读事务仅返回pending/failed/eligible/published标量，响应有界。未声称SQL扫描常数时间或所有内部list API分页。 |
| 11 | P7-015 | implemented | 由P7-015关闭：当前生产runtime使用keyset SQL LIMIT的64文档desired页；missing enqueue有限且检查当前版本。1100真实manifest旧测试本轮pass，跨64文档分页和1024项有限job边界，reopen provider调用不增。legacy全量facade无生产runtime调用。 |
| 12 | P7-012 | implemented | 保留P7-011/P7-012已接受hard scope/final hydration/查询内coverage合同及失败历史；当前原回归通过，不重计这些任务。 |
| 13 | P7-014 | transferred | 真实set_project/provider install触发configured-space事务切换，重复same-space不追加。原“日志上限（如需）”条件显式处置：当前没有高频自动切换/full-log状态消费者，历史log cap未实现，hot reload not_run。 |

每项原始证据的路径、SHA-256、固定源码、差异路径和限制见同目录 report.json。

生成成功不等于 G7 通过；本报告没有更新任务状态，也没有将 fake 观察转成真实语义收益。
