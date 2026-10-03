# docs: measure cache put normal-path stage costs

PR112 的实际 MCP 5k cache::put 占 worker 59.2%，但同步内部成本尚未细分。本提交仅新增 checkpoint 证据：固定 source 29b03a0，官方 Rust 1.95.0 release，真实 cc-semantic ArtifactCache put/get，3轮×1k×原版/探针×新写/同 keys 重写，128dims，12,000次验证 Hit。

Linux overlay 的探针新写均值1.694s/1k，file sync69.06%、dir sync24.25%；同 keys重写1.133s/1k，分别56.19%、35.62%。目录建立、编码/meta、rename和dir open已独立计时；binary/source/features、原始日志、探针校准、嵌套说明、normalrun样本/manifest均保留。

只给出减少重复目录sync、布局与批次的静态候选及持久化/并发可见性风险，未落生产改动。结果不能直接归因或线性外推PR112环境。未运行正式100k、provider/heldout或fault/GC/WAL；生产/cache格式/version/CI/中央TODO不改。范围验证和真实put/get校验通过。

预期草稿PR：head diag/cache-put-costs-20261003，base main。PR API查询实际Forbidden后停止API生命周期，本文案已准备但未提交创建请求。
