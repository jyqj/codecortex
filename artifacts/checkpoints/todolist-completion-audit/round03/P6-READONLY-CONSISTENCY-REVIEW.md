# P6 一致性要求与当前 seam 只读审计

本报告不修改生产、ADR、schema、tasks；不批准越过 G5 接线。当前 P6 二十项全部 todo，项目仍七 crate、schema21，`cc-semantic` 不存在。证据来自当前 checkout，不采用旧消息推断。

## 当前可复用事实与边界

| 当前来源 | 实际事实 | P6 必须补齐 |
|---|---|---|
| `crates/cc-model/src/generation.rs` / `cc-db/src/read_generation.rs` | 持久 incarnation16 bytes，strict读取拒坏ID/非法epoch；semantic_epoch可读但缺key时为None，当前没有publication clock写者 | 实际持久semantic epoch与启用/未配置语义一致；缓存不能把None当ready0 |
| `cc-db/src/index_db.rs::read_generation_on` | 旧IndexGeneration只读index/evidence且非法value落0；与strict ReadGeneration是两条不同用途API | claim/publish/fence不能用这个lossy旧epoch读取作权威；采用严格incarnation/version/space/lease核验 |
| `cc-db/src/epoch_rules.rs` | 仅Index/Evidence两clock；已有`semantic_edges`是静态结构语义边，仍属于Index | 新dense manifest/publish与静态semantic_edges分清；aux lease/heartbeat/retry不能冲刷index/evidence/完整查询cache |
| `cc-db/src/unit_of_work.rs` | BEGIN IMMEDIATE；commit无条件bump index一次；drop rollback；只提供当前合成边typed方法，没有WriteEffect | P6-004封闭typed effects与组合规则、commit/rollback精确epoch；不要散布手工bump或宽泛unscoped connection |
| `cc-db/src/document_store.rs` / `index_db_multi_insert.rs` / `index_db_write_batch.rs` | 当前documents由file写事务内部insert_on，与chunk/source/version镜像一致；incremental有同事务删除/写入/epoch/commit | 在同一个实际file事务中撤销旧visible manifest并enqueue desired outbox；rollback不能漏任务/失manifest |
| `cc-db/src/index_db_rebuild.rs` / `index_db.rs::finalize_rebuild_generation` | 固定staging路径、write mutex下rename、max(floor,live)+1 index/evidence，staging renew incarnation，再重开writer/readpool；bulk阶段synchronous OFF，WAL checkpoint有非致命处理 | paid cache不被结构rebuild清空；新incarnation fence旧worker；每crash点实际恢复；不能靠inprocess mutex假设覆盖另一进程 |
| `cc-model/src/semantic.rs` / `cc-search/src/lanes.rs` | 已有provider-neutral SemanticRecall查询port、generation/scope、最终DocRef/source守卫；没有cache/outbox/publisher/vector/provider生产实现 | 真实可选组合仍由cc-server唯一组装，cc-search不依赖HTTP/provider crate；fake先证工程，不冒真实效果 |
| `DESIGN.md` / `docs/adr/` | 章程明确全部state单index.sqlite3、7crate；正式ADR只有0001/0002，尚无可选派生cache/一致性决议；表/schema叙述已滞后 | P6-001正式修改why/physical边界，明确默认仍单DB且无network/cache/worker；发行文档事实债务归P8但不能先广告已实现 |

## 二十项实施验收矩阵

| 任务 | 实际 owner/seam | 必须证明，不以规格/trait代完成 |
|---|---|---|
| P6-001 ADR | owner持章程/ADR/STORAGE | 单权威index+opt-in派生cache正式理由、费用/隐私/生命周期、默认不开第二库 |
| P6-002 可选crate | owner Cargo/lib/feature/server组装 | feature off/default无网络client初始化、无空cache/worker；现七crate老功能正常 |
| P6-003 编码规格 | owner spec/ports/identity | space隔provider/model/revision/dim/metric/options；document/query specs分离；真实input bytes hash；同维异model拒混 |
| P6-004 write effects | owner epoch/UoW | Index/Evidence/Semantic/Aux正确组合；heartbeat无query epoch；一次commit准确推进，rollback0推进，旧写默认不失效 |
| P6-005 schema | owner index/cache schema | schema mismatch明确安全rebuild，旧FTS不半升级；默认禁用兼容；staging/active format区别 |
| P6-006 outbox原子 | owner file write/docdelta/outbox | 全量/inc/删除/rename路径都在真实同事务更新desired+withdraw visible；故障注入rollback无半状态 |
| P6-007 lease | owner outbox/queue CAS | 两个独立进程/connection只一个claim；持久token/attempt/expiry；旧lease不能ack/retry/publish新lease |
| P6-008 cache | owner cache/validated artifact receipt | 内容地址、project namespace默认隔离、spec+checksum+dim/finite/norm校验；相同input复用，坏cache不伪命中 |
| P6-009 fake | 独占worker `providers/fake.rs`、`tests/p6_fake_provider.rs` | 确定性与故障控制、取消/超时/数量/乱序/维度/NaN/Inf/zero；不接网络，不把fake分数当语义质量 |
| P6-010 exact | 独占worker `vector/exact.rs`、`tests/p6_exact.rs` | 手算cosine/topk/tie、scope先filter、删除/不同space不返回、bounded batch与内存、不全仓每query复制 |
| P6-011 publish | owner publish/index CAS/cache receipt | artifact持久commit先于manifest；再在index短事务检查incarnation+lease+desired DocVersion/input/spec/active space；发布幂等、slow旧结果不能复活 |
| P6-012 coverage/epoch | owner status/epoch | eligible0有原因；published仅当前manifest+valid artifact；retry/aux不冲刷内容查询；可见集合变更才semantic epoch |
| P6-013 worker/admission | owner worker/admission | queue/cpu/io/inflight bounded，连续编辑合并、旧任务允许产物但不发布，local始终可用；不跨网络持DB/CodeIndex/read pool锁 |
| P6-014 rebuild | owner rebuild/reconcile | 换库新incarnation拒旧callback；cache保留并重用；当前desired扫描重挂manifest，不删已付费artifact |
| P6-015 recovery | owner reconcile + 独立生命周期fixtures | claim/preencode/postresponse/preartifact/postartifact/prepublish/postpublish/preack每点真实终止重启；优先cache，有界续做、不能伪exactlyonce |
| P6-016 GC | owner统一namespace协调 | mark/sweep与publish/active lease/query引用并发，无manifest引用刚删artifact；孤儿最终回收；两个进程也成立 |
| P6-017 space switch | owner spec/reconcile | active space原子受控切换，旧worker fence，不混cosine；旧cache可回滚；query-only spec不重嵌文档 |
| P6-018 degradation | owner cache/status | missing/corrupt不当完整空结果；local继续；不隐式无界重费；检测/隔离/恢复计数真实 |
| P6-019 docs | owner STORAGE/CONCURRENCY/config/troubleshooting | 明确恢复来源、at-least-once/费用不确定性、无跨库/远端exactlyonce承诺，实测步骤可复现 |
| P6-020 G6 | owner整合 + auditor独立 | 全V13/14/16/17/18无网络闭环、真实DB/进程/crash/GC/rebuild证据与回滚；所有19依赖当前源验证，不只静态清单PASS |

## 跨库与跨进程必须审计的窗口

1. source commit成功但outbox缺失：不允许，用同一index事务消除此窗口。
2. claim后崩溃未编码：expiry/reclaim新token；旧worker复活不能ack或发布。
3. response返回但artifact未durable：可能再请求；fake验证工程，真实费用未知attempt收据需后续P7保持。
4. artifact durable而manifest未publish：重启先cache，再CAS，不重新编码。
5. publish commit但ack前崩溃：读取当前manifest幂等完成，不双推进epoch。
6. edit/delete/rename/space切换与slow callback：比对当前desired/DocVersion/spec/lease/incarnation，旧结果只有可回收cache，不可见复活。
7. GC mark后publish抢引用、publish检查后GC删除：共享持久namespace协调不能只有内存mutex；query也校验artifact存在/版本。
8. rebuild替换index文件与另一进程持旧connection：必须验证**active path对应的新incarnation**并fence，不仅在旧connection读自己旧epoch。当前源码未见专门跨进程文件重建锁，不能靠注释证明安全；SQL/OS是否拒旧连接需真实two-process实验，不先声称漏洞/自动安全。
9. staging checkpoint/rename/reopen各crash点：不能把WAL非致命处理/正常drop当durability全证明；source/outbox/visible manifest和cache分别对账，当前旧rebuild不自动证明付费产物恢复。
10. cache手删/损坏：不自动无限外发代码；status degraded、有界修复/opt-in成本，不错宣100%coverage。

## 独占并行切片前置合同

owner先唯一冻结Cargo/lib/spec/ports与cache artifact/queue claim接口，明确所有权：Claim含project namespace/incarnation/DocKey/desiredVersion/inputHash/spec/space/leaseToken/attempt；ArtifactReceipt含checksum/dim/normalization/source encoding。这里只是原规范需要的数据，不新增第二状态机。

然后worker可独占fake provider及其测试；另一轮worker可独占filtered exact及其测试。不能改owner的schema/outbox/publish/cache/GC/reconcile/worker/组合根或“各自定义一套”job state。构建接线只由owner小patch集成，auditor核真实DB/two-process/crash与默认离线，不把worker单module测试当G6。

G5仍未通过，以上只读审查是后续准备，不是允许提前接生产。真实provider/100k/发行/P9仍保留原全目标范围。
