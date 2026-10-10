# R48：十二项原任务结项决定

证据已在 `7d9acb9ccd31011ed106b41e8ac2e3c65338ee17` 封存，73个叶条目精确回读一致。本次按原23条步骤和24条验收要求，接受下列十二项的原任务范围并批准写入候选分支。当前主分支尚未应用；合并后才将会话计数从1提升到13、剩余从28降到16。

| 任务 | 原任务名称 | 决定 |
|---|---|---|
| P8-006 | 增量规模与fanout曲线 | 原范围验收通过，候选状态done |
| P8-007 | 多并发与混合负载 | 原范围验收通过，候选状态done |
| P8-008 | 冷建/重开/热查分层 | 原范围验收通过，候选状态done |
| P8-009 | 内存/磁盘/费用总账 | 原范围验收通过，候选状态done |
| P8-010 | 长时soak与连续修改 | 原范围验收通过，候选状态done |
| P8-011 | 端到端故障与恢复认证 | 原范围验收通过，候选状态done |
| P8-012 | MSRV与平台冷构建矩阵 | 原范围验收通过，候选状态done |
| P8-013 | 指标/门槛与失败退出最终认证 | 原范围验收通过，候选状态done |
| P8-016 | 数据库/配置/包回滚演练 | 原范围验收通过，候选状态done |
| P8-017 | 删除临时兼容和重复模块 | 原范围验收通过，候选状态done |
| P8-018 | 文档事实与安装契约同步 | 原范围验收通过，候选状态done |
| P8-019 | 发布工件与完整报告归档 | 原范围验收通过，候选状态done |

## 证据入口

- [47条原要求及逐条判断](https://github.com/jyqj/codecortex/blob/7d9acb9ccd31011ed106b41e8ac2e3c65338ee17/artifacts/checkpoints/p8-round47-complete-bfcc-20261010/formal-twelve/47-clause-acceptance.final.json)
- [简体中文完整验收报告](https://github.com/jyqj/codecortex/blob/7d9acb9ccd31011ed106b41e8ac2e3c65338ee17/artifacts/checkpoints/p8-round47-complete-bfcc-20261010/formal-twelve/TWELVE-TASK-ACCEPTANCE.final.md)
- [完整45格测量报告](https://github.com/jyqj/codecortex/blob/7d9acb9ccd31011ed106b41e8ac2e3c65338ee17/artifacts/checkpoints/p8-round47-complete-bfcc-20261010/final45/final45-descriptive-results.md)
- [所有原值、环境与出处](https://github.com/jyqj/codecortex/blob/7d9acb9ccd31011ed106b41e8ac2e3c65338ee17/artifacts/checkpoints/p8-round47-complete-bfcc-20261010/final45/final45-descriptive-results.json)
- [独立完整输入恢复报告](https://github.com/jyqj/codecortex/blob/7d9acb9ccd31011ed106b41e8ac2e3c65338ee17/artifacts/checkpoints/p8-round47-complete-bfcc-20261010/aggregate-recovery/complete45-recovery-execution-review.json)
- [45个原ZIP的完整已发表字节索引](complete-45-raw-custody.published.json)

## 聚合与原件

同一f97源码/run38026411200/a1的45格、85条记录齐全：40setup、40mutation、5fanout；5cold点是setup子集。原ZIP总68,960,381字节，118个原字节叶均有实际发表commit，顺序拼接可恢复Actions原ZIP及原SHA256。

原Actions聚合只接到42个输入，实际失败，原报告和日志完整保留。另一次在新目录用未修改的原程序、原build和完整45输入聚合，实际exit0、missing/errors为空，91次实际哈希子调用全部成功。没有重新测量或把Actions失败改称成功。

## 已暴露的性能问题

| 100k修改 | 增量引擎总秒数 | 全量对照秒数 | 增量构建数 |
|---|---:|---:|---:|
| body | 3065.083 | 685.832 | 490 |
| API | 1259.137 | 558.806 | 171 |
| config | 1915.250 | 599.643 | 330 |

这些是各自原单点的描述，不能推广为稳定尾部分位或跨机器因果结论。它们说明完成正确性闭包和测量验收之后仍有优化空间；本决定没有声称性能已经达标或G8发布通过。

## 落账和后续

只修改五份进度文件，十二项追加状态/证据/实施备注；全192项定义、顺序和依赖、其他180项完整记录保持。原生成器write/check各运行一次，再通过普通PR检查与合并规则。

下一项导航为P8-020，继续按local/semantic分别做发布评审。P7-018、P8-014、P8-015、P8-020与P9-001至P9-012共16项继续开放；可选LLM、live provider、holdout及发布所缺证据没有被推定为完成。

早期冻结模板的42/79、null字段是历史快照，新的正式状态以本目录完整45公开索引和ROOT-CLOSEOUT-DECISION.json为准。原失败、旧raw和旧声明均保留。
