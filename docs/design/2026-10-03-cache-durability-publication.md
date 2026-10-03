# Artifact cache durability / publication：独立静态设计

状态：供父级架构决定；未实现、未运行测试、未证明提速。本文与附录为本次自编内容。

固定源码：candidate `e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207`；用户指定 PR123 production `11f5b76273a16b520e6b6e01ad32a0a6743173fb`。两对象从 origin 正常 fetch。上述 production 标识来自任务，PR123 元数据未能独立验证（GraphQL Forbidden）。独立工作树从 candidate 创建。工作区 `.agents` 为空；仓库与本工作树未找到 AGENTS.md / SKILL.md。应用 agent-architecture 的边界、证据、恢复原则；未使用子 agent。

## 建议与可节省范围

先做**exact verified cache reuse，保持同步屏障与单项 DB publication**；其次单独审批 bounded DB publication group。跨不同对象的目录 fsync batching 不推荐：实际目录不是共享 shard，不能靠一次 root/namespace fsync 替代每个叶目录同步。冷对象仍需两个文件各自 write / sync_all / rename；不得删除文件 fsync、改变 layout 或缓存完整性校验。

在本次“不可删 fsync”的约束内，最小版本只省 verified identical hit 的 bin temp write/rename；meta 保持刷新，保留两个文件同步及原有目录同步尝试；不能宣称它解决冷对象逐条 fsync。进一步把同一对象的两次叶目录同步合为最终一次，是**需要父级单独决定的中间故障语义变更**，不属于当前建议直接实现的版本。DB grouping 能减少 publication BEGIN/COMMIT 数量，但不减少 cache 文件同步；实际吞吐收益未知。

当前四 attempt 只有 bounded correctness 证据；100k 因 cgroup 12 GiB guard 提前失败，不是 accepted。此结论按父级任务记录，本文没有重跑或把旧失败改写为通过。

## 实际调用链（路径与行号绑定固定 candidate）

| 层 | 调用与边界 |
| --- | --- |
| runtime | `cc-server/src/semantic_runtime.rs:154` close lifecycle；`:364` round 调 `semantic_wiring::drain_worker_batch_parallel`，resolve 闭包调用 `document_store::semantic_worker_input`，claim budget 16 |
| wiring | `cc-server/src/semantic_wiring.rs:583` parallel drain；`:617` queue drain；每项先检查 active space，再 `cache.get`，Corrupt → quarantine + requeue；`:660` 建 Publisher，绑定启动 incarnation / space / spec / lifecycle；serial API `:505` 保持独立 |
| queue | `cc-semantic/src/queue.rs:393` explicit project HTTP setting >=2 时 whole-attempt width 至多4，否则1，且 <= max_batch；admission mutex 下 reclaim/claim，释放后核对 task 的持久 space binding / active space / renew；`:649` EmbedHandler resolve → input.verify → provider → Publisher |
| source | `cc-db/src/document_store.rs:85` 读匹配 doc_key/version、non-null encoding；worker_record_input 核对 record reference / encoding，检查 source_range，`record.validate(original)`，再核对 input_hash；释放读连接后返回文本；`DocumentInput::from_bytes` 构造 digest，handler 再 verify |
| warm runtime | `cc-server/src/semantic_runtime.rs:402` FencedProvider 完整 `cache.get(space,input,spec)` Hit 时返回 hit.data，跳过底层 provider；close 前后检查保留。它丢弃 ArtifactRef / 验证来源，返回普通向量 |
| publisher | `cc-semantic/src/publish.rs:113` publish_embedding **总是** `cache.put` → `cache.get` Hit 且 exact returned ArtifactRef 相同 → DB lifecycle publication；故 warm hit 后仍重复 put |
| cache write | `cc-semantic/src/cache.rs:410` 验 space、维数、finite；生成 LE f32 payload / blake3 / metadata；`:445` create_dir_all；bin 与 meta 顺序分别 atomic_write；`:511` temp write_all → file.sync_all（错误传播）→ drop → rename（错误传播）→ open parent / dir.sync_all（均 best-effort）；meta 最后 |
| cache read | `cache.rs:301` 同时打开两文件固定 inode；bounded metadata/payload，format、tuple、dimension/model、checksum、exact length、finite 验证；缺一半 Miss，损坏 Corrupt；读成功不意味着持久化证明 |
| publication | `cc-db/src/semantic_publish.rs:391` write_conn mutex → BEGIN IMMEDIATE → lifecycle.enter → `publish_and_ack_on` → Q4 epoch effect → COMMIT；permit 保持至 commit 后；关闭时 rollback，返回 None |
| fences / ack | `semantic_publish.rs:209` live incarnation → task_id exact token + claimed → current doc_version / input_hash / non-null encoding → active space → manifest visible-set diff/upsert → `semantic_outbox::ack_done_on` 同事务；拒绝调用同 token retry；ack false 是事务错误 |
| queue DB | `cc-db/src/semantic_queue.rs:29` claim/renew/retry/reclaim 各自短 IMMEDIATE；`:88` claim lifecycle permit 穿过 commit；`semantic_outbox.rs:631` fenced UPDATE 仅 task_id/token/claimed，owner 非凭据；ack/retry 无硬时钟检查 |

表内省略统一前缀 `crates/`。cache / publisher / DB publication / outbox / document_store / semantic_wiring 在两固定提交间 byte diff 为空；queue production 主要差异是 width 2→4 且 min(max_batch)，runtime 差异为 bounded 测试。SQLite 正常 IndexDb 使用 WAL / synchronous=NORMAL (`cc-db/src/index_db.rs:308`)，不能称每次 COMMIT 都有独立物理 fsync，不能改变 pragma 来换性能。

## 必须维持的不变量

1. 可发布请求只能来自完整 bounded cache 验证，exact namespace/space/input/spec/checksum ref 与所用向量一致；path 存在、metadata 存在、字符串 ref、旧的 Hit 均不能代替验证。f32 比较用 LE 字节 / to_bits，不能用浮点 `==`（+0/-0 区别）。provider 同 tuple 可能给不同内容，不能悄悄选旧向量。
2. cache 写/同步/读验成功在 DB mutation 之前。没有跨存储原子提交、没有持久 exactly-once；cache 为 discardable。当前 best-effort 叶目录同步、未逐层同步 create_dir_all 的祖先，均不足以宣称断电持久性；不能在设计中悄悄升级为 strict durability。
3. task_id + token + claimed 为 lease fence。超过 lease_expires_at、未 reclaim 的持有者可 renew/publish；reclaim 或 supersede 后旧 token 不能 ack/retry/renew，重新 claim 产生 fresh token。禁止加入 elapsed deadline 拒绝。
4. publication + fenced ack + rejection retry + 相应 epoch effect 在同一个 DB transaction 内。Q4 相同 visible set 不 bump；incarnation/source/version/input/space 校验不可搬到事务外代替 live 校验。保持原拒绝优先顺序。
5. close 与持有 lifecycle permit 的 commit 线性化：permit 已取得的 commit 在 close 返回前完成；close 返回后新 publication 不能进入。close 不取消已发 cache IO，不在 cache IO 上等待；取消后可留下 orphan。正常 fenced cancellation handback 是现有恢复路径，不是允许 publication。
6. 不扩 HTTP 4/2、claim16、默认 serial，不增加 batch 输入预取、不延长 DB/lifecycle 锁覆盖 provider/cache IO，不改 source、space、exacttoken 拒绝与 quarantine/retry attempt 记账。

## 最小版本：验证命中后避免重复覆盖

唯一 cache owner 在 `cache.rs` 新增私有/受限 `ensure_present` 路径；保持 public put 的旧行为与现有 layout。Publisher 可通过新路径得到 ref，但仍保留其最终 exact-ref readback 与既有单项 DB facade。**不要仅把 put 改成 get Hit 即 return**。

步骤：先执行与 put 相同的 space/dimension/finite 输入验证，计算目标 LE payload/checksum/ref；对该 tuple 执行现有 get 全链验证。Hit 必须 ref 与目标 ref 一致，并逐元素 to_bits 相同。非一致 Hit / Miss 走旧 put；Corrupt 在 runtime 已走 quarantine，竞态新发现 Corrupt 不静默认可，返回未验证交既有 degrade/retry 路径。

一致 Hit 仍要同步已有 payload：固定并验证打开的 bin/meta handles，bin file.sync_all 后保留原目录同步尝试（open / sync_all best-effort）；meta 走原 atomic_write（temp file.sync_all、rename、目录同步尝试），最终重新 get 并核对目标 ref。bin 不发 temp writes/rename；meta 仍按原 atomic_write 刷新 created_at。I/O、校验、final-ref 失败不发布。同步 handles 必须属于验证的 inode；中间发生替换、删除或 quarantine，最终读验不一致则拒绝。并发不同向量在最后 readback 后仍可覆盖是原系统已有可丢缓存窗口；该方案不新增“验证成功后不可被删”的保证，也不允许据此去掉最终验证。

已核对 `cc-semantic/src/gc.rs:11–13,30–34,89–92`：created_at 明确由每次 put 刷新，保留窗口保护 artifact→CAS 空档。因此最小版本必须保留 meta atomic_write 刷新，仅避免 bin 重写，仍保留全部同步；保留旧 created_at 的更激进复用不推荐。缓存 live-task mark 不能在未证明与GC扫描竞争顺序前替代freshness。FencedProvider 当前丢弃 ref，最小版本从 Publisher 重验，避免改 provider trait、source gate 或 serial callback。该版本不省 file fsync；性能是否改善需后续正常 benchmark。

## 同一对象目录同步合并：条件方案，当前不落地

地址为 `<root>/ns-<namespace>/<space>/<input>/<spec>/<spec>.{bin,meta.json}`，两个文件共享同一末级目录。跨任务只有 exact 相同 namespace/space/input/spec 才共享此目录；不能用某祖先目录的 sync 代替后代目录 sync。

若父级以后允许 coalesce 目录 barrier，可将对象级过程改为：预序列化 payload/meta → prepare 两 unique temp、各自 file.sync_all → rename bin → rename meta → 最终 leaf directory sync attempt → final full readback → DB。两个 file.sync_all 必须保留；meta rename 保持最后。这只省冷对象一次 leaf-dir sync attempt，不是跨 N 输入省 N 次文件同步。

边界不同：原 bin rename 后已有一次目录同步尝试；新版本在 meta prepare/rename 失败、或两 rename 之间 crash 时尚未做目录同步，bin rename 可能丢失。旧 meta 仍在而 bin 更新时，读者可能见旧 meta / 新 bin → Corrupt；即使先同步两个 temp，pair replacement 仍非原子。并发写者 A/B 可以出现 A bin / B meta 的 mismatch，甚至 put returned ref 与 readback 不同；必须拒绝，不得任取 checksum。不得删除旧 meta、引入新 marker、换 layout 或声称 pair 原子。

失败恢复必须至少在任何成功 rename 后的 error exit 尝试同步同一 leaf-dir，并保留原主错误与 temp 清理。若要求原中间 crash 屏障也完全相同，则无法合并：必须保留 bin 后 barrier。保持 best-effort 与 error 传播原语义，不暗改为 mandatory dir fsync。create_dir_all 的祖先目录持久化、断电保证是另议的 correctness 设计，不混入性能优化。

## bounded DB publication group：可实现，但作为第二项架构决定

缓存准备与读验全部在事务外，准备项含 task/token/source tuple/ref/incarnation/lifecycle，不能裸 ref 构造。只聚合**同一 IndexDb incarnation、同一 lifecycle**的已完成项；serial 立即按单项路径发布。explicit parallel 可用至多4个 ready descriptors 的有限 coordinator，claim总数仍16；遇当前 ready frontier/end-of-drain 即 flush，不等齐4、不设依赖 lease 到期的 flush deadline、不用 barrier 阻塞所有 worker。保留物理 join 与 runtime pin，禁止向每项目再开常驻无限队列。

DB 唯一 owner `semantic_publish.rs` 增加 facade：write_conn → BEGIN IMMEDIATE → enter 一次共同 lifecycle → 按稳定顺序逐项原 `publish_and_ack_on` → **每个**成功且 changed 的项原 `bump_semantic_epoch_on` → 单 COMMIT，返回按输入顺序 outcomes。保持 changed k 次导致 epoch +k；不能未经决定只 bump 一次。拒绝是该项 outcome，fenced retry 一同 commit，其它合法项仍可成功。任何 SQL/ack/epoch/commit error 回滚整个 group，不返回任何已完成 outcome。无 partial success callback；report.completed 只在 commit 后累加。异常交现有 token-fenced recovery，不能先 plain ack，也不能同时再次计 rejected retry。

此 grouping 可保持每项 fence 与 atomic manifest+ack，但改变观察粒度：多个结果同时可见、group失败影响其它项、close可能等待至多4项DB操作。需父级明确接受，不能称“完全相同逐项行为”。重复 task/token 或同 doc 同组先拒绝输入，避免第二项受第一项状态影响而产生重复 accounting。缓存 orphan 可重复利用；重试必须重新验证 cache 并重新取当前 task token，不能用失败 group 的旧 prepared 请求直送。COMMIT结果不确定时不能推断没生效，查询当前 state/token/manifest 后走幂等恢复。

不在首版合并 claim/reclaim/renew/retry：claim只暴露已commit token，renew 的 false 是启动与 liveness gate，reclaim 与 publish 在 DB write 顺序上决定赢家；把它们并入 IO 后的 transaction 会改变 custody、fairness、attempt 计数及恢复窗口。group只替代已经存在的 publication facade事务，不拿cache batch包住索引source事务。

## 故障顺序与恢复判定

| 停止点 / 竞争顺序 | 不变量与结果 |
| --- | --- |
| payload temp write/sync失败 | 无DB publication；清temp；旧对象可能保留；task通过当前token retry或恢复 |
| bin rename成功、meta失败 | 新对象缺meta为Miss；覆盖旧对象可能Corrupt；无DB publication；不许把bin或旧meta作为verified；原/合并目录屏障差异见上 |
| meta rename成功、目录同步失败 | 现有目录best-effort，不把成功当强持久证明；正常readback仍需通过；若要失败即拒绝需另立契约 |
| cache完成、DB之前close | cache IO可完成并留orphan；BEGIN后lifecycle不许可则rollback，无publication；handoff记账由既有路径执行 |
| cache完成后source edit/delete、space switch、incarnation更换 | DB live fences拒绝；manifest不写，token-fenced retry/no-op；不能用准备快照通过 |
| lease时间已过，无reclaim | exact token仍claimed可成功；不增加clock拒绝 |
| reclaim先commit、旧任务后publication | claimed/token不符；LeaseLost，旧retry no-op；successor不受影响 |
| publication先取得write txn并commit、reclaim后执行 | ack done，reclaim不会夺走已完成任务；同一token再ack无效 |
| group中第2项SQL失败/ack异常/epoch失败 | 整组manifest+ack+retry+epoch rollback；第1项不是completed，cache对象仍可在后续经验证复用 |
| DB COMMIT前进程停止 / COMMIT后回包前停止 | 之前可能无publication；之后可能全组已完成；通过current state/token/Q4与cache验证恢复，不能承诺exactly-once |
| readback后GC/并发覆盖 | 仍是discardable cache既有窗口；后续读取Miss/Corrupt走degrade，不从静态分析承诺永久可读 |

## 文件唯一 owner、兼容与回退

| 未来变更文件 | 唯一责任 / owner |
| --- | --- |
| `crates/cc-semantic/src/cache.rs` | cache owner：ensure与固定handle验证/同步；不改目录、格式、put旧行为 |
| `crates/cc-semantic/src/publish.rs` | publication orchestration owner：artifact-ready与final-ref gate；不改DB fences |
| `crates/cc-db/src/semantic_publish.rs` | DB owner：group facade / atomic effect / lifecycle；禁止其他层自行BEGIN或ack |
| `crates/cc-semantic/src/queue.rs` | executor owner：只在group获批后负责bounded ready custody/report/joins；首版不动 |
| `crates/cc-server/src/semantic_wiring.rs` | integration owner：现有serial/parallel/degrade接线，获批后group opt-in |
| `crates/cc-server/src/semantic_runtime.rs` | runtime owner：本轮只读；不改FencedProvider、4/2 gate、claim16、close或provider factory |
| 对应cache / publish_cas / DB semantic_publish / queue / runtime tests | 各生产owner更新对应正常API oracle；禁止不同owner交叉改同一生产文件 |

owner是未来实施职责，本文不授权实施、不分派并行编辑。最小版维持所有public API、默认serial、semantic-disabled零工作、query cache、非HTTP注入provider、FIFO/rotation、revoke路径。无需schema/config/dependency/lock改动。回退仅撤销ensure调用恢复put；group facade使用单项fallback，已commit数据无需迁移。错误不得自动弱化fsync或verification来回退；回退旧完整路径不能绕过实际权限拒绝。目录合并单独提交、独立开关/调用路径，未批准不出现生产接线。

## 附录：专项 TODO（全部待批准、待实施/验证）

- [ ] 已由静态GC契约排除created_at保持方案；父级批准最小版保留meta重写。冻结“不删除同步屏障”的实现边界。
- [ ] cache owner拟定ensure结果类型，防止调用者自行伪造ArtifactReady；仍支持普通publish_embedding。固定handle同步与final-ref重新验证，不能复用旧Hit作为durability receipt。
- [ ] 正例：exact tuple/ref/LE位相同warm hit的bin不temp-write/rename、meta正常刷新，仍保持bin/meta各sync和目录尝试；冷Miss用旧put；+0/-0不同须旧put；metadata保持/刷新行为按父级决定验证。
- [ ] 负例：wrong namespace/space/input/spec、dimension/model、format、checksum、oversized metadata/payload、NaN/Inf、缺一半、同tuple不同vector均不走reuse；Corrupt保留quarantine/requeue、不消耗既有degrade预算。
- [ ] 竞态例：open后replace、同步后delete、finalreadback不同ref，必须拒绝publication。用自有fixture正常API或受控test seam，不做真实权限/EROFS/kill/provider故障实验；本轮不执行。
- [ ] 复用现有`publish_cas.rs`、`cc-db/tests/semantic_publish.rs`、`attempt_lease_contract.rs`的正常API正负oracle：全部live fences、duplicate Q4、expired unreclaimed可成功、reclaimed/reclaimed新token旧请求拒绝。
- [ ] 若group获批，DB owner先实现facade独立fixture：mix accepted/rejected/duplicate；changed k epoch+k；第2项error全rollback；lifecycle关闭0publication、持permit commit在close返回前完成；不扩大error复用或retry预算。
- [ ] 若group获批，queue owner再设计ready frontier / end flush，<=4 descriptor与claim16；测试width1无需等group，部分ready不等慢provider；失败/取消有custody且物理join、pins不早释放。
- [ ] integration兼容：serial FnMut、explicit width4 + HTTP4/2、semantic-http与default-disabled、注入provider、FIFO/rotation、source edit/delete/switch-away-and-back、revoke、degrade；不新增batch输入prefetch。
- [ ] 如果以后另批目录barrier coalesce，先决定中途crash/失败前缀差异是否可接受；保留file.sync_all与error-exit dir尝试，列出旧meta覆盖/多writer混合；要求原中间屏障等价则取消该优化。
- [ ] 后续只在授权自有worktree/fixture以原Cargo.lock做适当验证；正常官方registry可用；权限拒绝停止对应动作不换路，不改任何安全设置。动态故障、真实provider、已拒EROFS不在本设计验证计划。
- [ ] 性能验证另立任务：记录fixed source/candidate、模式、warm/cold、sync/write/rename/transaction counts、wall time、峰值内存与guard，正常资源完整跑；12GiB guard失败不得标accepted，不以静态syscall计数称提速。

本轮仅git静态diff/调用链审阅与文档检查；没有编译、测试、故障注入、真实provider或生产修改。等待父级决定后另发实现。
