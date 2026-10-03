# PR114 独立 FIFO 正确性 review — 20261003

结论：在下述有限、独立验证范围内，未发现 FIFO 候选引入的正确性回归。**不代表性能接受、全路径通过或生产集成批准。** Direct-writer rebuild 有旧版与候选共同的真实失败，必须保留为失败路径。

冻结候选 final `807f4710495a38bc6631549da2ab9623c83684b8`，生产提交 `c73128b500c2a19daa8a293caa52afb711b03516`，PR110 base `5ffbadcf48e26523b2eb46beda0d187a2e2e29cd`。所有实验使用固定 archive，未追随移动分支。父后续告知 PR114 CI 在新测试的七元 tuple 上触发 `clippy::type_complexity`；本任务没有重跑作者 CI/tests。

交付前独立读取 PR114 远端 head `52a50730b58e2b59351449fc80f65771b24c26a8`，核对它与冻结 final/production 的三份生产文件 SHA256 完全相同（production-byte-identity.json）。新增改动为测试 type alias 和修复记录，实验结果仍只归属于冻结 final。Review 交付分支可基于该 lint 修复 head，仅添加本目录，避免在 draft diff 中反向撤销作者的测试修复。

## 独立证据

- 自编 Rust oracle 每版 27,000 个三行组合：pending/claimed/done/failed/superseded × active/other space × `<now`/`=now`/`>now`。按模型过滤后取最小 task_id，与生产 `claim_next_on` 对照；其余行 state/attempt 保持。候选与 PR110 各通过，不能将两个版本合计为更多候选覆盖。
- 自编 retry/backoff/reclaim trace：旧任务尚在 backoff 时领取下一项；到边界后旧任务恢复最优先。每次产生不同 token，attempt 增加、lease 正确；旧 token 的 renew/ack/retry 均拒绝，reclaim 后再次拒绝过期 token。
- 真实两个 `IndexDb::open` 连接同时调用正常 `claim_semantic`（生产 `BEGIN IMMEDIATE` 门面），只有 active-space 一项被领取一次，另一个返回 None；较老 other-space 项保持 pending，无 active 配置时不 claim，epochs 不移动。每版一组正常竞争，无 kill/crash/fault。
- 真实 `cc_semantic::queue::drain_pending_with_lifecycle`：首项在 handler 中关闭生命周期并报告未开始取消，生产交还恢复 pending/attempt=0/空 token；余项没有被 claim。已关闭 fence 的后续 drain 为零 claim，epochs 保持。另测预先关闭的正常 DB claim 门面。没有模拟完整异步 server runtime 或真实 provider。
- DocRoundRobin 真实生产调用保留同 doc 的 terminal-row MAX(updated_at)，排除其他 space 的 MAX，选择 task2；旧版与候选均通过。原 format SQL（含 SET/RETURNING）的源码字节完全相同，见 source-audit.json。
- 真正的 PR110 源码编译出的自有旧驱动，调用旧 `IndexDb::open` + 旧固定 DDL 创建磁盘 v24；不使用候选 DDL 再 DROP 索引伪造。种入 files/chunks/FTS/document_manifest/semantic_manifest/queue/spaces、非零三个 epochs 和旧 incarnation，随后候选两次真实 open。比较 **60 张表的全部逻辑 rows（含 FTS shadow 与 sqlite_sequence）**、全部 metadata、user_version、原 sqlite_schema 对象。只有新增非唯一 partial FIFO index；逻辑 rows、unchanged content_hash/text、manifest、incarnation、epochs、version 全相同，integrity/FK 通过。old-v24-before/after/comparison.json 可复核。
- fresh DDL、正常 mismatch reset、普通 temp rebuild 后验证两个 INDEXED BY 索引存在，并实际准备/运行生产 FIFO。v21/22/23 fixture 为 **旧 PR110 二进制生成的 v24 DDL 再设置 synthetic user_version**，并非历史 v21/22/23 二进制/DDL；用于检验既有 Mismatch/reset 契约。旧文件/manifest 被清空，version 回到24，index/evidence epochs 17/23→18/24；没有 additive semantic migration。见 legacy-reset-summary.json。
- 附加 Python SQLite 3.53.1 SQL 敏感性检查：从候选 Rust 提取完整真实 UPDATE，324 个两行案例与自编 oracle 一致；六个 synthetic mutants（倒序、严格 readiness、available_at 排序、漏 space、纳入 terminal、反 guard）均被杀死。Python SQLite 只算附加负控，不替代 Rust production SQLite 3.53.2 的上述真实调用。

## 保留的失败和边界

**既有生产失败：DirectWriter 正常 rebuild。** 两个固定版本的实际 `rebuild_with_direct_writer` 都返回 `schema tables: no such column: new.rowid`，退出2（old-direct.log / candidate-direct.log / direct-rebuild-failure-comparison.json）。`direct_writer.rs` 字节不变；其 `split_sql_statements` 按分号拆 trigger body，`extract_table_statements` 把 trigger 中的 `INSERT … new.rowid` 当作独立 INSERT。失败发生在延迟索引创建前。候选没有引入它；因此本 review **无法把 direct-writer 这条路径的索引可用性记为成功**。不在本任务修改源码。

普通 temp rebuild 先完整执行 DDL，随后由同一 DDL drop/recreate 索引，实际候选调用通过。matching-v24 只安装性能索引并 ensure 原 incarnation；fresh/reset 执行完整 DDL。正常 write reopen 经过 migrate，read pool 为 query_only 且不承接 claim；任意未经 migrate 的 raw connection 不在正常 open 契约内。未通过临时删索引、权限更改或重试 RO 拒写证明恢复。

首次运行因 direct rebuild 失败而中止，原 receipt 保存在 first-run/；后续拆分失败与通过路径并继续。review-run-v2.log 保留 supplementary mutant 的五参数/四槽位 harness 错误，mutant-correction.log 记录修正及成功；Rust生产验证未因此重跑。初始 PATH/rustup home 与离线依赖解析/下载失败日志也保留。静态 RR 初次无函数范围 regex 错匹配，改为固定 `claim_next_fair_on` 范围后真实字节比对通过（audit.py）。这些均非生产测试通过记录。

未运行：完整 codecortex/server 可执行文件、semantic-http/HTTP/provider、heldout、历史 v21/22/23 原生 DDL/二进制、作者47 tests/全仓 CI/lint、正式100k、性能 AB、GC/WAL kill/crash/fault、真实用户库、权限/credential/root变更、先前 RO 拒写重跑、merge/forcepush/deploy。父/作者的模型、tests 和快照不记入独立信用。父已报告的5k更慢与IO+4.4%不由此 correctness review 代为接受。

## 来源与复现

provenance.json 明示 Rust1.95、source SHAs、最终自有 debug driver binaries 的 SHA256 和 features：cc-db/cc-semantic default，rusqlite bundled/vtab/functions，SQLite3.53.2。这是链接固定生产库的独立验证二进制，**不是官方 release CLI 二进制**；没有修改或 instrumentation 生产文件。源码副本、targets、测试 DB 全位于本目录 ignored local/；生产源码、DEV、CI、中央TODO仅只读。

未发现 checkout/固定提交/workspace ancestors 中的 AGENTS.md 或 .agents/skills/SKILL.md；读取了 CONTRIBUTING.md。所有证据、README 和任务TODO限定本目录，没有使用 subagents。

在含固定 git objects 的新 checkout（local/ 尚未生成）中，执行：

```sh
python3 artifacts/checkpoints/independent-fifo-claim-review-20261003/prepare.py
python3 artifacts/checkpoints/independent-fifo-claim-review-20261003/review.py
python3 artifacts/checkpoints/independent-fifo-claim-review-20261003/audit.py
```

prepare.py 使用现有 `/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin` 和 `/workspace/.cargo`，复制保留的 driver manifests/locks，以 `--locked` 构建。若依赖 cache 不齐需普通 cargo registry 下载；无身份或权限切换。review.py 记录生产 checks；已知 direct 错误作为失败 observation 收集。audit.py 核固定生产字节、RR literal、reset 与二进制身份。首次失败与后续检查的执行记录按本文对应，不能单凭脚本最终 exit0 宣称 direct 路径通过。

## 本任务 TODO

- [x] 固定 source/base/production 身份、工作区指引及只读范围。
- [x] 自编有限模型 + 真实生产原语/门面/取消调用。
- [x] 真正旧 PR110 生成 v24，候选 open/reopen 全逻辑快照。
- [x] fresh、synthetic legacy mismatch/reset、普通 temp rebuild。
- [x] direct rebuild 真实失败及旧版对照，失败不抹除。
- [x] Supplementary synthetic mutants、失败修正记录与 not_run。
- [x] PR114 lint 修复 head 三 production 字节身份；不重做冻结实验。
- [ ] 仅本目录 commit/push、自有 draft PR、核 remote head；完成即停。

Review 交付状态由 delivery.json 和最终响应给出；发布不是集成授权。
