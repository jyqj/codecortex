原29项未完成TODO只读盘点（main `ee9a8e90237773502210f66bc284a1787d1ff203`）

原 tasks.json SHA256：`6e2ab2e90cbd820228e8ff397d70c770561ba20d3110e6eca31338f29a945207`。192项 = 163 done / 16 in_progress / 12 todo / 1 blocked，剩余29。状态权威为 tasks.json；05-TODO 是派生页。

只有 P8-005 与 P7-018 的硬依赖当前全部 done；P7-018 另受原 D1 未授权真实付费 provider 限制。其余27项全部直接或传递依赖 P8-005；P9 三个入口也均经 P8-020 依赖完整规模。没有可绕开原规模前置而直接关闭的其他非 live 任务。

下表只概括台账的完整验收尚待收口条件，不将本会话已有组件证据视为不存在，也不为任何后继源码移用旧样本。每项仍有原“相关旧回归通过、无证据不得 done”的共同要求。

| ID / 任务 | 原状态 | 原硬依赖 | 验收仍待收口关键点 |
|---|---|---|---|
| [P7-018](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2177) 受授权的真实provider小集认证 | blocked | P7-017 | 明确的真实provider授权与费用预算；固定公开输入的真实返回/用量/partial，live与fake分栏（当前原D1未授权）。 |
| [P8-005](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2330) 完整规模1k到100k | in_progress | P8-004 | 固定seed、release的1k/5k/10k/50k/100k完整规模；chunk/edge/vector和同工作量证据；原full-scale子门仍open。 |
| [P8-006](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2346) 增量规模与fanout曲线 | in_progress | P7-020, P8-001, P8-005 | no-op/body/API/config/batch与超预算fanout闭包；phase计数可归因、闭包后full parity、incomplete显式。 |
| [P8-007](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2361) 多并发与混合负载 | in_progress | P8-006 | C1/4/8/16 read/build/backfill混合；offered/排队/timeout/尾部完整，无死锁饥饿。 |
| [P8-008](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2375) 冷建/重开/热查分层 | in_progress | P8-007 | OS/process/result-cache/uncached warm分层，全部样本与N/分布/CI，无best-of。 |
| [P8-009](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2389) 内存/磁盘/费用总账 | in_progress | P8-008 | client/server/process-tree内存、artifact/FTS磁盘、reported/estimated费用分开；unavailable不填0。 |
| [P8-010](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2403) 长时soak与连续修改 | in_progress | P8-009 | 持续编辑/删除/切分支/压实及cache/worker复用；资源/队列有界与终点完整对账。 |
| [P8-011](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2417) 端到端故障与恢复认证 | in_progress | P7-020, P8-007, P8-010 | kill/断网/缓存损坏/DB忙/换库恢复；原故障工件保留，无假ready/删除复活并说明费用。 |
| [P8-012](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2429) MSRV与平台冷构建矩阵 | in_progress | P8-011 | Linux/macOS×MSRV/stable×default/semantic八格全新target；冷构建、SDK或明确发布平台范围。 |
| [P8-013](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2444) 指标/门槛与失败退出最终认证 | in_progress | P8-012 | quality/perf/lock故障注入；红线非零且保留raw，inconclusive不可自动passed。 |
| [P8-014](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2458) 可选LLM评审旁证流程 | in_progress | P8-013 | 明确授权/预算后的固定prompt/model、盲化LLM旁证；不得代替gold，未执行不阻塞确定性/local范围。 |
| [P8-015](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2471) 真实语义效果发布认证 | in_progress | P8-013, P7-018 | 已授权真实live的holdout、费用/错误分布、model revision；缺证据不能声称semantic认证。 |
| [P8-016](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2484) 数据库/配置/包回滚演练 | in_progress | P7-020, P8-012, P8-013 | 旧binary/新schema受控重建、cache版本隔离、disable语义回退实测；不误读向量或丢源码。 |
| [P8-017](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2498) 删除临时兼容和重复模块 | in_progress | P8-016 | 清临时旧branch/重复评分器/多份schema来源，保留必要wire兼容；删除有回归、唯一事实/算法所有者。 |
| [P8-018](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2512) 文档事实与安装契约同步 | in_progress | P8-017 | schema/capabilities可核查事实与安装/故障/默认离线契约同步；实现/设计分开且不漂移。 |
| [P8-019](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2526) 发布工件与完整报告归档 | in_progress | P8-018 | 精确binary/checksum/manifest/raw/gates完整归档，报告可重算；latest指向run、历史不可覆盖。 |
| [P8-020](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2540) P8发布评审与遗留关闭 | in_progress | P8-001～013、016～019；semantic 条件另需015 | 所选local/semantic范围G8独审与风险/回滚收口，范围内blocker为0；semantic另需015。 |
| [P9-001](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2557) ANN收益与选型决策 | todo | P8-020 | 基于exact实测延迟/内存/规模的ANN收益与选型；无压力不先引入依赖，可有证据地deferred。 |
| [P9-002](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2570) ANN适配与过滤更新闭环 | todo | P9-001 | 选型通过后的ANN适配、filter recall、insert/delete/tombstone；hard scope/版本正确性。 |
| [P9-003](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2583) ANN量化和规模认证 | todo | P9-002 | 独立量化、filter selectivity/cold加载，与exact完整比质量/资源/更新成本。 |
| [P9-004](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2596) ANN决策收口 | todo | P9-003 | 仅保留选定backend+exact并清试验依赖；收益不足回exact，未落地不算实现完成。 |
| [P9-005](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2609) LSP按需生命周期 | todo | P8-020 | LSP lazy/cooldown/idle/RSS/inflight生命周期；无LSP仍local可用，回收不打断健康请求。 |
| [P9-006](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2622) LSP源码快照和冲突表达 | todo | P9-005 | LSP绑定document version/workspace；冲突并列，不无条件覆盖持久化事实。 |
| [P9-007](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2635) LSP精度/内存收益认证 | todo | P9-006 | 固定server版本的精度/启动/idle/失败降级收益，缓存与外部进程内存完整计入。 |
| [P9-008](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2648) LSP工具面和安装边界收口 | todo | P9-007 | 有限工具面、precision选项、capability/许可/配置/缺失诊断；不自动执行未知server。 |
| [P9-009](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2661) 可选rerank端口与降级 | todo | P8-020 | 候选/预算边界和原证据保留，取消/fallback状态及确定性本地回退；不捏造文件/边。 |
| [P9-010](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2674) rerank盲测与费用消融 | todo | P9-009 | 固定候选/model/prompt的重复配对holdout；报告收益CI/费用/latency，注入文本只作数据。 |
| [P9-011](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2687) 按需查询分解收益决策 | todo | P9-010 | 复杂任务分解/facet召回收益决策，保留scope/总deadline；收益不足维持原路径。 |
| [P9-012](https://github.com/jyqj/codecortex/blob/ee9a8e90237773502210f66bc284a1787d1ff203/docs/roadmap/code-index-v2/05-TODO.md#L2700) P9可选增强逐项结项 | todo | P9-001～011 | ANN/LSP/rerank逐项启用或有证据deferred，清未选债务；未选功能不算实现done。 |

等待时可并行推进的现有编号工程工作：P8-017 的临时兼容/重复所有者清单；P8-018 的文档事实与安装契约漂移核查；P8-019 的工件/checksum/报告复算完整性；P8-020 的 local/semantic 证据缺口与回滚映射。这些可产出独立可评审结果，但不解除原硬依赖、不宣称任务 done。

P9 可选分支允许有证据地 deferred；这不是实现完成，也不能用来凑十项 done。P8-014 可选 LLM 旁证未执行不阻塞确定性/local发布。原十项完整关闭目标 P8-005～013 与 P8-016 的规模前置仍完整保留。
