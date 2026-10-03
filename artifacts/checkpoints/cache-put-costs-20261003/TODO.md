# Cache put 分阶段诊断

- [x] 固定 source、读约束、检查 remotehead；无 AGENTS/.agents skills。
- [x] 创建仅证据目录内 source-copy。
- [x] 构建原版 baseline 与 instrumented binary，记录哈希/features。
- [x] 正常合成 1k × new/rewrite，get 全量验证、保留 normalrun。
- [x] 先回传关键计时，记录扰动与嵌套。
- [x] 静态建议/风险与 normalrun 证据，核生产 diff 为空。
- [x] evidence-only commit/push；首个提交 bcb8260694f5485b4c4a3908d30dbc4931b54a9a remotehead 一致。最终 receipt 另提交并核 head。
- [!] draft PR：blocked，gh PR API 返回 Forbidden，停止对应 action。

完成即停：无后续 5k/100k、生产优化或 fault 动作。
