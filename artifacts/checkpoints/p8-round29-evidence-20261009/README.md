# 第29轮：审查更正与持续交接

观察截止：**2026-10-09 14:10:51 UTC**。本轮原 TODO 新增完成 **0**，累计新增完成 **0/至少10**。主干账本实际回读仍为 **192 项：163 done、16 in_progress、12 todo、1 blocked，剩余29**。任务定义、原验收、硬依赖和状态均没有为推进计数而修改。

本目录仅保存本轮证据和交接。其父归档为第28轮 `66dbfba14aac2de5f5dc5ab64fa85893f75f6411`；归档分支保留旧产品树，**不要将整个归档分支合入 main**。代码和 PR 操作应从实际当前分支出发，并保留其他协作者的内容。

## 当前可确认的进展

| 对象 | 结果与界限 |
| --- | --- |
| PR #187 | 已合并，merge `d22d36dddf8b38f1a864c933deefe0b6d3b5baf3`。只为 `docs/BENCHMARK.md` 追加已审使用说明；没有执行收费模型，没有关闭 P8-014。 |
| PR #184 | 当前 head `883230f2b44ebcf09495870db8c0e31d61270331`，仍 Draft。产品提交 `cfc77b68…` 已实际通过 fmt、workspace/all-targets strict Clippy、精确后代进程回归、完整 workspace 和 fixture/corpus；workspace 220 组、2731 passed、0 failed、73 原 ignored。完整原件及非作者审查在第28轮归档。最终 head 与产品提交的全部1092产品/139验证输入逐字相同；R/G仅审查记录与原绑定变化。 |
| #184 最终校验 | 已知原生 job `wc_job_I9QcXM3auHMwDf6g` 已启动，尚未收到终态；随后连接报告 tunnel-client 超过300秒未连接。没有取消或重派该任务。保留观察中的七项新 GitHub 工作流仍排队；未知本机结果不能改标成功。 |
| PR #180 | 当前 head `0e6b07c75183da79c5a6679d4c2b66c65f7db663`，仍 Draft。实际域1095产品/70原BASE差异/142验证；此前缺失的c92三文件公开错误前缀修复和#184三文件DB清理已全部存在，三项Mac cfg也已存在。无需重放旧11文件组合。新增锁观测/Cargo/server/runtime/workflow具有既有非作者源码链，实际执行的适用范围仍须逐项核收。 |
| #180 helper 合成 | 当前 `31b34057…` 已保留一次监督器readiness。已审 `969ba37…` 仅加两条结果断言，删除122字节后全文精确回到当前文件。它仍是未应用候选，没有本候选的新测试通过记录。后续整合#184时要保留双方意图并重新核当前叶，不能覆盖并行修改。 |

本轮已更新#180说明，明确原832f缺陷已被当前0e6修复；原历史记录原样保留。全部实际状态、完整ID、哈希、后续限制及十项目标的真实账本状态见 [PROGRESS.json](PROGRESS.json)。

## P8-007 的原要求与旧推断更正

[独立纠正](reviews/P8-007-original-requirement-correction.json)直接比对原规范、原任务字段与完整生产者源码。原 `09-BENCHMARK.md` §9 已要求在 C1/4/8/16 混合读取/构建下采集 DB lock wait。G275 的三次 `db_control` 可用性探针每seed仅执行一次，而且发生在held查询波次之前；writer的区间包含 `BEGIN IMMEDIATE; ROLLBACK;`。它们不能补齐实际mixed请求的DB获取观测。

因此保留旧 `c861a3ea…` 报告，只撤回其“三次探针补齐mixed DB采集、007只剩006依赖”的推断。旧混合负载、768次backfill、worker竞争/进度、三次DB可用性探针和原一小时soak仍保留各自真实范围。**没有新增所有锁的纯SQLite等待、逐RPC全链路、optional22架构、额外一小时或额外N150门槛。**

现有 observer run [37930392164](https://github.com/jyqj/codecortex/actions/runs/37930392164)，固定 source `4d18dcdb34b5d7a277566b57801cc882d8b1eb61`、attempt1，提供了继续接收原件的入口。root于14:07:43 UTC读取的完整5-job/3-artifact页中，C4与C8成功，C1与C16已在执行。C4 artifact11620977433、C8 artifact11621010611；本轮仅定位元数据，**没有完整接收或独立验收这两包，也没有认证四档完成**。这属于明确feature诊断身份，不替换默认构建的延迟/吞吐样本。只接收现有运行，不因此重起研究。

## 规模研究与性能判断

原275e run37896198208、c8 run37902429727均已失败：100k触及原五小时native期限，aggregate失败、后续measure跳过，原失败和完整已有原件保留。c8整包已在第28轮归档，Git blob `ecabdc3c5503dfbdfb4e041812c6588d01052d6a`，5,212,266字节，SHA256 `f758ca519a6b5ed90bfe1040944ad32b1daad8248e44b8ea4c7d4840d4a43236`。

现有3ff run [37910924354](https://github.com/jyqj/codecortex/actions/runs/37910924354)，source `3ffcefc3b28ee1a4ed80caecebd7208a45c3e302`、attempt1，在13:54的保留观察中仍执行100k job113760872142，step7自10:23:50 UTC开始；总6job。它的完整原规模验收尚未完成。

保留原5容量×30重复、150shards/1500复合观测、seed12648430、15表、18,000,000ms native、512MiB输出、dirty200、resume1024、fanout8/128；不跨源合并、换标、替换失败或降低预算/样本。不要为了规避等待而取消、重跑或另起同一研究。

c8完整成本派生尚未成功接收。第一次只读解析返回被工具截断；之后捕获文件的请求遭遇连接超时，是否实际生成文件未知。恢复连接后先检查既有 `formalc8-100k-failure-reception/readonly-cost-extract.json`，不要盲目重派。GitHub通用fetch只支持UTF-8，未取回二进制base64；没有编写新解压器或据截断片段拼出总量。

[剩余路径静读](performance/current-residual-hotpaths.json)定位了仍存在的500k seed-cache容量拒绝、resume扫描和增长dependency前缀，但没有证据将所有resolve/commit残差归于这些路径。本轮没有据此编写推测性能补丁或宣称100k能在原期限内完成。

## CI 整理决定

[完整审查](ci/old-and-new-run-review.json)核查14条run和50个job。旧b356七条运行均已有实际执行：14个job完成、2个正在执行、9个排队；包括仍在执行的soak与C16。顶层queued不能证明整条run未启动。安全取消候选为0，本轮没有取消任何run、正式研究或新建替代研究。

## 下一轮具体步骤

1. 读取当前main、#184/#180真实head及当前CI。接收#184同head原校验；若现有GitHub必要校验真实通过，可按既有输入桥接与非作者结论评估正常合并，不需把未知本机结果改成成功。每次ref/merge前重新核head/main/保护规则。
2. 完成既有observer四档工件接收与独立核验，保持feature/default身份，不丢错误、未结束和不可用观测。不以源码审查、绿色job或三次旧探针替代原件。
3. 继续接收现有3ff原规模研究；完整原验收和硬依赖满足后再更新原TODO。未来闭合计划 `ed85c0e0efcc33811a0fda35bba72c35b5045170` 需要同时遵守本轮P8-007更正。保留全部18定义字段和完整原acceptance_subgates，仅更新真实进度/证据/implementation_notes，执行原计划与事实检查并保存实际输出。
4. 将本检查点交给唯一既有的小时续办任务，保持原计划、权限和完成后暂停的约定。该任务不依赖重新启动本机执行；已有GitHub运行与原件继续按真实状态处理。私人任务标识不写入本目录。

## 原件与目录边界

清单中的Git对象均为固定文本或已保存的原记录。API文件是连接器返回的完整已解码JSON对象按明确格式序列化，保留字段，不声称HTTP传输字节原封不动。本轮没有新Rust/性能执行，没有改动源文件、任务状态或研究规约。后续结果保留后续观察时间，不能倒填本轮截止状态。
