# 独立固定源码 review — REJECT / HOLD

冻结 production source `5cce6eb3af90b79c786d30f349ca6a674dfcd4a1`，base PR114 `52a50730b58e2b59351449fc80f65771b24c26a8`。作者 PR119 最终 evidence `8c7c764` 与该 source 的 crates/Cargo 字节无差异。本次不评后续修复，不追 moving head。

## 新 blocker：普通 NeedsRetry 使 ready rows 无人续调

`queue.rs:496-497` 将 NeedsRetry 设置全局 stopped 并退出；runtime `semantic_runtime.rs:394` 只按 `backfilling || claimed == 16` 决定 more，schedule 在 more=false/requested=false 后归还运行所有权。

独立正常 fixture 使用真实 CodeIndex/build_index 生成 12 个有效 document records，真实 IndexDb、真实 runtime API、真实 AdmittedProvider，以及明确有效 ProviderGate 4/2。有限合成 provider 普通返回 ServerError；EmbedHandler 转成 NeedsRetry，未注入 DB/IO 故障。实际 schedule 完成时：12 rows，2 provider calls，10 pending 且 available_at<=当前带小数秒时间，10 attempt_count=0；running=false、pins=0、requested=false、gate.in_flight=0。单 round 返回 more=false、cursor=None、同样 ready=10。没有等待更久、修改源或更换终止规则来消除反例。

两个 counterexample 测试返回通过只说明成功观察并断言了缺陷；不是候选验收通过。日志见 final-run.log 与 retry-run.log。

0 模式同样退化：新 runtime 第一次 ServerError 后仅调用1次、more=false、仍ready=11；保留的公开串行 drain_worker_batch 对剩余11条会继续处理，claimed=11/retried=11。因此只能确认宽度0/1顺序执行，不能确认旧串行 retry 行为完整保持。

已知 shared gate first-wins global5/project3 > requested4/2 原反例仍 HOLD，未删除、未重跑作者测试、不以本次局部真实 gate 覆盖共享装配契约。

## 有限独立证据

11个自编测试通过（335个其他/作者测试过滤），仅有如下覆盖：

- 配置 width0/1 peak1；width2/4 peak2；真实 publish 单round 16 done、16不同token、attempt总16。width4使用合法8/4 gate。
- 多项目实际 decorator/gate：global4、a2/b2；global/per-project溢出均 Timeout且未进入inner；释放后permits0。
- close时两次物理 provider 调用仍持有running、pin1、permits2；物理退出后全部归零、claimed0、published0；同时真实 DB读/write API可以完成。
- 成功schedule处理40条并全部发布；忙时schedule=false但requested=true，完成后无剩余工作。该测试覆盖请求在held round期间到达，不穷尽最后drop/swap微小窗口。
- 普通切换active-space时原两次在flight调用完成，但旧空间semantic_manifest写入0，随后permits0。
- 普通 unhandled handler Err与NeedsRetry区分；Err在同伴物理调用释放/join后返回，permits0、claimed0。
- 未started取消 refund 后attempt总0、claimed0；handler观察token相互不同，wrong-token renew=false，正确token renew=true，返还后旧token renew=false。不能将handler观察数当全部成功claim token数；其他claim可能在prework被取消。
- 默认semantic.enabled=false/network_opt_in=false，disabled assemble不查环境/不装配provider。

Claim FIFO与completion order分别判断：新 admission mutex 线性化真实FIFO claim；worker completion无序是明文设计。没有以handler进入顺序或完成顺序冒充FIFO claim证明。成功token/done计数提供有限无重复证据。

## 只读检查及未覆盖范围

空claim分支在成功count增量前退出且不写stopped，因而源码未见空claim消耗预算/全局停同伴；本次没有新增独立确定性empty-claim交错用例。保留父/作者已有normal enqueue证据，但不计入本次独立信用。

Publisher/CAS及DB fence实现未被候选修改，incarnation/doc identity/token/active-space fence仍在原事务中；本次动态检查只涵盖成功发布、普通space切换和token renewal。未做incarnation reset、doc supersession交错、lease真实过期时长或factory销毁线程归属的独立证明，不声称全部取消与续调场景已验收。

错误与取消后的handback按task_id+token+claimed做CAS，unstarted扣回claim charge。长provider调用仍只在prework进行renew；本次有限hold低于lease，不推广成长调用续租保证。默认串行公开API仍存在且源码未改；runtime改用width1的新API并不保留全部retry语义。

## 身份、失败与 TODO

直接使用现成官方cargo/rustc1.95；未调用rustup、修改HOME/RUSTUP_HOME或重试EROFS目标。source hashes、binary hash、features、过滤数在identity.json。harness仅追加test module，生产正文一致。自有正常临时DB位于/tmp；cache/build都位于本checkpoint副本。未写生产、CI或中央账目。

初始独立harness误用not_before列，随后用整数秒判断available_at导致ready计数0；原失败日志完整保留，修正为实际available_at和带小数秒当前时间。生命周期初版误用publication表名及非法4/4配置，原失败保留，修正为semantic_manifest和合法8/4。没有修改生产来通过。

TODO：NeedsRetry最小修复由父指定唯一owner处理；共享gate修复另owner；对新冻结修复另立独立验收任务。本任务完成即停。未运行作者tests、额外AB/100k、GC/WAL kill/crash/fault、真实provider/heldout、merge/forcepush/deploy。

可复现：从仓库根运行本目录replay.py；其git archive固定source，只给副本追加独立测试模块，直接调用官方compiler并过滤independent_review_。
