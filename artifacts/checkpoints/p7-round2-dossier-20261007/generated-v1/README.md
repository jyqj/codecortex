# P7 / G7 当前复核材料

当前未完成 **40 项**；状态分布：`{"done": 152, "in_progress": 2, "todo": 37, "blocked": 1}`。

工程 Gate：**未验收**。本命令只核对固定任务条件、证据字节和引用，不能凭状态或 hash 批准功能/发行。

live：**blocked**，沿用 D1+D2；未调用真实 provider、没有真实效果或金额证据。

| 原任务 | 当前状态 | 本轮交付类型 | 观察范围 |
|---|---|---|---|
| P7-011 | done | current_validation | 按原 V05/V16 工程范围复核：14 条原要求、13 个 HTTP 复验函数及相同 768 输入的 PR #143 CI 已核，任务已验收。 |
| P7-012 | done | implementation | 补全查询内 dense artifact coverage：过滤后统计缺失/损坏/拒绝，缺口返回不可缓存的 Partial；原回归 2 成功/2 失败到新版本 59 成功。任务已按原范围验收。 |
| P7-013 | in_progress | current_validation | 原 deadline/cache/public 矩阵 58 个函数实际复验：57 成功、1 个正常并发 recovery 失败；已接入新 CI 的原 58 函数。 |
| P7-014 | in_progress | implementation | 修复 GC retention u64→i64 溢出；冻结回归 3 成功/1 失败到 4 成功；多配置 63 次成功执行对应 46 个函数，另有 13 个实际生命周期检查点。 |
| P7-015 | todo | implementation | 新增真实 worker 竞争测量夹具：384 个固定 local 请求样本，实际 held provider 工作、读写进度及旧输入零发布；新旧 9 个行为函数通过。 |
| P7-016 | todo | implementation | 修复 GC unlink 的错误传播和实际删除计数；冻结 5 个失败控制修后通过，另 8 个原 GC 控制通过。 |
| P7-017 | todo | implementation | 实现固定 source/compiler/features 的私有构建收据；11 个工具控制通过，default/semantic 产品和原 runner 共 4 次构建成功，2 个原隔离门禁均保留失败。 |
| P7-018 | blocked | blocked_disposition | 对照原 D1+D2 决策记录，将需要 live 授权的任务明确列为 blocked。 |
| P7-019 | todo | implementation | 实现 ablate-strategies 公共策略消融；7 个协议函数、实际 default smoke 和 local/auto/semantic 三策略均通过，保留 27 个原返回值、18 个测量、12 次 loopback POST 和 2 个 family 配对比较。 |
| P7-020 | todo | implementation | 实现并实际生成 G7 证据总账：固定原条件、任务/证据哈希、13 项原接线责任、混合来源和分开的工程/live 结论；9 个最终合成控制通过。 |

## 尚需闭合

硬依赖：P7-013, P7-014, P7-015, P7-016, P7-017, P7-019。
尚未验收且未提交本轮证据：无。
来源与当前整合输入不同的记录：P7-011, P7-012, P7-013, P7-014, P7-015, P7-016, P7-017, P7-019；它们仍保持各自原源码的观察范围。

已验收、未重复提交本轮证据：P7-001, P7-002, P7-003, P7-004, P7-005, P7-006, P7-007, P7-008, P7-009, P7-010。缺少本轮条目不会重开这些任务；原验收记录继续保留。

## 13 项接线对账

| 项 | 负责任务 | 当前判断 | 依据与剩余事项 |
|---:|---|---|---|
| 1 | P7-014 | implemented | 原 P7-010/P7-014 组装入口已存在，当前配置及生命周期复验支持限定行为；不代表所有下列调度项完成。 |
| 2 | P7-014 | partial | post-index 与重开后的有界 worker 调度已接线；独立 recover_scan、reconcile_after_rebuild 和完整 GC 调度点仍有原责任缺口。 |
| 3 | P7-014 | implemented | 生产 drain 检查 artifact 并隔离/重排损坏对象，普通失败 attempt 不被挪用；完整故障注入认证仍归 P7-016。 |
| 4 | P7-010 | implemented | 原 P7-010 的 DegradationLedger→QueryServices 不可变投影归属保留；没有因缺少本轮重复条目撤销历史验收。 |
| 5 | P7-010 | implemented | 原 P7-010 的 namespace/cache root 接线及禁用先于 lookup 保留；本轮 retention 回归也核禁用无副作用。 |
| 6 | P7-014 | implemented | GC retention 用 checked conversion 校验正数有符号范围，真实 GC 边界 1/3600/i64MAX 及零/overflow 冻结回归已通过。 |
| 7 | P7-014 | partial | GC counters 已返回，status/persist 没有实现；原条件允许合理的 P7-016 evidence-only 处置，但当前未替所有者作该验收决定。 |
| 8 | P7-005 | implemented | 原 P7-005 共享 provider admission/fairness gate 保留；本轮原 HTTP 公平队列功能复验通过，完整013单独保留其恢复失败。 |
| 9 | P7-016 | partial | 机会式回收/故障恢复继续属于原 P7-016；本次实际 unlink 计数修复不代表 mark-unlink 竞争或完整回收矩阵完成。 |
| 10 | P7-014 | implemented | status 消费 SQL count(*) 标量，避免将 failed/dead-letter 全行组装到状态输出；不替泛型 list API 认证有界性。 |
| 11 | P7-015 | partial | runtime 使用 desired page 64 及有限轮/时限；独立 desired projection/reconcile 全部有界化、性能/资源门槛仍归 P7-015，384样本不关闭它们。 |
| 12 | P7-012 | implemented | 原 P7-011/P7-012 的 hard-scope/final hydration 与查询内 artifact coverage 已按原范围验收；来源与当前整合差异明确列出。 |
| 13 | P7-014 | partial | 显式配置/模型切换触发器已实现；switch log 仍 append-only，无条件上限。原按需 cap 的所有者处置未完成。 |

每项原始证据的路径、SHA-256、固定源码、差异路径和限制见同目录 report.json。

生成成功不等于 G7 通过；本报告没有更新任务状态，也没有将 fake 观察转成真实语义收益。
