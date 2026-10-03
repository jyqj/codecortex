回填期间普通 semantic publish 会推进 generation；v1 capability status 因此反复执行八表 stats 和嵌套 coverage retry，常在三轮后报错。此次将诊断明确升级为 retrieval-capabilities-v2：必要计数、freshness、active space 和完整观察 generation 来自同一短只读事务；普通发布不重扫，ready只说明该快照的覆盖。

SQLite实际打开文件的只读HAS_MOVED及typed incarnation控制检查保守拒绝可检测旧lease/替库；unsupported或不确定身份不假定未移动。worker failed/degraded优先级保留，query严格generation fence和rebuild/GC/WAL协议未改。当前CI入口迁到当前v2源码一致性oracle，旧反例保留，冻结v1候选和重复测试入口移出当前验证；ADR/消费者说明与公共spec断言同步迁移。

验证：46次定向test executions全部通过（profile有重叠），fmt及cc-db/cc-server -D warnings clippy通过。有限release本地FakeProvider AB每规模两组，负载/poll/期限相同：1k status错误3/6→0/7、5k14/28→0/28，poll mean成本约降84.5%/91.0%，steady成本约降20.8%/25.6%。ready观察没有一致加速：1k首组候选慢约198ms，多一轮poll；5k均值约快5.56%。完整原始数据及不利结果保留，不能外推100k或宣称MCP RPC加速。Linux身份实测；其他平台未测。真实provider0，未执行heldout/kill/GC/WAL故障。

本单块完成即停，独立review与100k另开。本body供后续审阅；此前PRcreate Forbidden动作未重试。
