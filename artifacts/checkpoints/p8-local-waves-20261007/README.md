# P8 多轮本地工程推进证据

固定起点：`6d02d77f018a5965a6f289b0b43558ed4b9f8322`。
原始 TODO 计数、正式验收与本地工程推进分开，见 `docs/roadmap/code-index-v2/P8-LOCAL-PROGRESS.md`。

- `pr-audit.json`：46 个开放 draft PR 的固定 head / 路径 / 采用检查；43 个仍保留。
- `pr-operations.json`：根代理再次回读后，关闭已采用的 #10、#11、#111；未删除分支。
- `round1/`：4 个原始 TODO 的本地交付及测试日志。native child 测试的失败与环境诊断原样保留。
- `round2/`：新增3项推进，完整增量对账中的真实路由修复及故障退出回归；规模驱动原始证据另见 `artifacts/checkpoints/p8-scale-20261007/`。
- `round3/`：最后3项推进，混合负载/连续修改观测与声明式文档事实检查；负载完整raw另见 `artifacts/checkpoints/p8-load-20261007/`。
- `integration/`：独立源码准入准备与最终组合检查，和各作者固定源的历史实测分开。
- 发布锁/归档工具的精确源码及 39 项测试收据另见 `artifacts/benchmarks/p8-release-local-20261007/`。

工程测试通过不代表原任务完整验收，不继承历史候选的质量、100k 或平台证据。

原作者与独立审阅历史保存在 `artifacts/checkpoints/p8-local-history-20261007/`：47个原始commit、6个固定ref，唯一前置为本批main基线；bundle不包含自身后续归档提交，也不用于CI源码批准。最终组合33项回归与scope Clippy见 `artifacts/checkpoints/p8-combined-validation-20261007/`；实际远端pins的直接验证见本目录 `integration/`。
