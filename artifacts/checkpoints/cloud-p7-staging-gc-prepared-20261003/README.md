# staging / GC 独立 prepared

PR84 固定执行源码 a83aa1bd4d0c7bbc6a9ce9d5d7c19a226c607bd3。15场景/21自有Unix child真实SIGKILL，3不同seed。一个parent测试1/0、child entry常规ignored但被显式执行/kill21次；原GC/reconcile测试11/0，strict scoped clippy/fmt通过。

staging-writing 是真实 build callback 写入事务内的物理停止点（is_autocommit=false），kill后未提交投影不入staging/主库；staging-built/swap-returned是完成边界，不能叫rename内部断点。swap后的semantic space实际0；显式重新注册唯一空间并reconcile复用原artifact零新增provider，不能叫自动ready/stdio启动恢复。原引用/verified payload保全，均integrity_check=ok。

GC collect后让另一publisher子进程claim或实际提交，然后release GC sweep：保护原引用和live-task目标/新引用，实际删除未引用对照。两child再kill，claim自然过期后recover复用、原已发布对象不丢，重启GC零追加删除。两种确定顺序是collect→claim/publish→snapshot→unlink，不是snapshot后新publish竞态证明。候选按caller clock/元数据确定性老化，不假称等了一小时；lease自然等待。

fixture投影/enqueue/space是直接SQL setup，不是生产索引写门面/配置入口；原enqueue primitive不刷epoch，一次真实publish +1。FakeProvider总次数仅准备时staging1或GC2，reuse时0；claim数不等于收费、无livecurrency结论。无通用cache pin API，仅manifest/live-task保护已测。

失败预跑保留本地日志hash：私有digest/field编译错误、缺target.body和错误把fixture enqueue计epoch均为harness错误，未造生产缺陷/修复。最终fixed-source run是独立新目录；源/Cargo/旧tests及PR81所有文件未改。

P7-016硬依赖P7-015→P7-014不满足，仍todo，本块prepared。WAL删除→rename、rename→reopen、GC mark→unlink、pair unlink内部、通用pin、power-loss/完整matrix/production stdio自动恢复not_run。internal-hook-proposal.json 给最小独立方案；没加生产hook、没扩权限，没假覆盖。

复放用新空目录：
```sh
P7_SG_EVIDENCE_DIR=/tmp/p7-sg-replay-new CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 cargo test --locked --offline -p cc-semantic --test p7_staging_gc_preparation -- --nocapture
python3 artifacts/checkpoints/cloud-p7-staging-gc-prepared-20261003/verify.py
```

results.json/children保留原始输出；receipt.json命令/source/binary/loghash；临时DB/cache原件留本地local-original-manifest.json逐文件hash。无网络/live/holdout读取/权限扩展，未merge。

## PR85 成本反例和口径订正

PR85 84613f7原74文件逐字纳入，原verify通过15kill/36no-op。provider返回而cache尚未落盘的新3场景各2FakeProvider调用；原PR81九任务各1call仅其完成边界域。首attempt在真实provider是否计费于重启后为unknown，不得填0，重试是可能重复计费的新attempt。ReceiptLedger/CostBudget明确process-lifetime，outbox attempt_count是跨重启claim记录而非精确billing。没有证明durable monetary ledger；不得称exactly-once任意crash。详见crash-cost-canonical.json；旧原始证据不改。
