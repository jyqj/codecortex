# V2 grammar：修复前冻结

生产基线 `c1aa6607aadbf27eb86ba63816982cf48ae277d0`。
先阅读独立 review `56fd54e26f3f9c388fdcf0a15543b43dd5f26590`，再冻结本版本。
旧 28 条矩阵、模型、源文件、结果及 hash 均原样保留在 query-target-20261004。
新矩阵明确保留 historical_v1_expected_model；qualified-dot/qualified-field 的
新期望是 fallback，旧的 named 期望是已知历史失败，不能称为 v1 控制继续通过。
45 条自编写控制及 11 条 reviewer delta 控制；reviewer fixture 来自其 supplement，
按 byte 引用，两条反例将通过实际 CodeIndex / MCP / normalizer 重新执行。

V2 模型：裸点分字符串全部 ambiguous，包含文件名、模块、member，各种 extension、
大小写、文件存在与否均不影响此分类。不读取 filesystem，不设 extension denylist。
混合 dot/:: 也 fallback。保留完整纯 :: 标识符链和严格 (*Receiver).member 单层
语法，保留 name: 优先、完整 kind/name、method NAME on CONTAINER、methods on。
没有覆盖的一般 prose 继续旧 token fallback；原始 public metrics 仍 FAIL。
