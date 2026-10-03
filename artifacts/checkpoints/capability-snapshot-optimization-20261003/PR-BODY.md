现有 capability status 强制跨发布丢弃第一轮、返回第二轮最新 epoch；独立 V18 oracle 同时要求持续 churn 三轮后保守错误。用一笔 SQLite read transaction 直接返回完整旧 as-of snapshot 可以避免混代，但不能默默替代这些 latest 判据。

按任务的设计阻塞备用路径，仅提交专属目录的契约分析、未应用 DB-only typed snapshot 草案与源码身份核验。父集成方需明确继续优化现有 latest 路径（保留 churn 有界错误），还是新增显式 as-of 消费路径并保留原契约。生产/测试/根 ledger/TODO 未修改。

验证：prepare_design.py 核验相关十个文件与指定 final 基线逐字节相同；git apply --check 通过。草案未编译、未运行；1k/5k AB 和100k均 not_run，没有性能或 ready 认证。未运行 kill/fault/GC/WAL，真实 provider=0、heldout未读。独立 review 另开任务。
