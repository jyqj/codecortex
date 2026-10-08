# P8 第 4 轮固定运行入口

此目录实现原 P8-002 / P8-003 / P8-004 的 `artifacts/benchmarks/<run-id>/` 证据入口。
完整运行清单见 [index.json](index.json)，逐项独立审阅、原退出码和来源身份见 [本轮检查点](../../checkpoints/p8-corpus-external-holdout-20261008/README.md)。

登记器实际执行 20 次 native CLI validate；外部 cc-switch / Flask 的 compat 与 native 分开执行，共 1,200 条 measured 和 400 次 warmup；fresh holdout 在两个新仓上共 64 条请求，仅运行一次。四套外部诊断原 gate 全部为 `invalid_measurement` / exit 2；留出集原两套 gate 全部为 `gate_failed` / exit 1。证据入口不把这些失败改成质量通过。

完整 canonical 题体、已使用的 holdout gold 和原 raw 均在随任务交付的两个封存包中；包名、字节数、SHA-256、成员数和 manifest SHA 在 index 中固定。仓库内发布可审查的无题体元数据，完整证据没有以摘要替代。原 F / D 二进制均包含在 external 包；原 scorer 在副本上的重放结果已经验证，封存原件未改。后续不能把已消耗的留出集重新称为未见数据。

本入口不授予完整 V19 / G8、真实 provider、1k–100k 全规模或发行认证。最终源码与回归门禁的具体终态见 [PR #156](https://github.com/jyqj/codecortex/pull/156)。
