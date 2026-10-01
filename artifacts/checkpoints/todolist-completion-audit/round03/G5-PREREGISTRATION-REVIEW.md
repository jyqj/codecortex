# G5 正式验证预注册合同独立审查

## 当前裁决

**不是通过收据；正式运行前还需冻结候选、对照、输入、阈值和环境，并由实施 owner 提交完整运行计划。** 当前 118 done / 1 in_progress / 73 todo。P5-019/020、G5/M2 未验收。原规范为 `06-VALIDATION.md` §3–4、`09-BENCHMARK.md` §3、§5、§10–11、tasks.json P5-019/020，以及 C09～C16。

源码、范围、版本、预算合法性与基础回归的硬门不得放宽。原规范没有要求离线自然语言召回 100%，也没有要求每道定位题穷举所有候选；不得事后凭空增加这种阈值后反过来写 fixture 词典。另一方面，不能把真实 Partial 改为 Success、删掉有意义的 lane、改 gold/预算或更换分母来换绿灯。

## 必须冻结的对象

- 候选 HEAD + dirty/source manifest 完整 hash closure，源码 archive、Cargo.lock、默认及测试 features、Rust/toolchain/SDK/build flags、cc-eval/driver/scorer 协议版本、不可变二进制 digest。
- 正式对照为已接受 P5-D 的源码闭包（不是路径名称或旧 HEAD 推断），以**同 release 配置/SDK/hardware**重新构建并锁二进制。不能用旧 debug 暖结果与新 release 比性能。
- 原四套历史 frozen suite/gold/scoring/预算/51 题输入保持；若另用当前源码 subset 重新锁 input，作为独立 current-source dataset，不宣称与旧输入相同。
- `task-obligations-v2.json`：51 题 / 110 最低核心 byte-span，由独立源码审阅固定，不从新 candidate 的结果反推；已公开开发数据，不称 holdout。
- 独立多 facet / graph / budget / concurrency 机制题、其源文件/标签/限制、全 8-cell 消融控制变量、运行次序种子、每档样本、测量/故障终止条件预注册。计时期间无其他编译/大负载。
- 正式运行发现红灯，不删失败、不覆盖旧 run、不缩 required facet、marker、top-k、scope 或样本数保分。改源则重新锁受影响证据闭包。

## 门禁矩阵

| 门 | 通过所需证据 | 不足或失败时 |
|---|---|---|
| 本地工程 V11/V12/V18 | 当前双工具链严格 lint/build/workspace/HTTP、真实 stdio 14 工具契约/错误/取消、无锁网络 fake、deadline/queue/缓存代际、LRU/末租约回收、warm/cold/持续写负例 | 任一基础/协议/锁/混代错误阻断，不用质量均值抵消 |
| source/scope/version | 所有真实返回正文独立与原件字节/span/version 对齐；所有 lane/hydrate 不越 hard scope；reference/outline 不冒充正文；输出合法完整 JSON，包括首块/metadata/最终 proof 的预算 | 必须 0 invalid source/scope/version/budget object；budget honesty 与 source authority不移除 |
| no-answer | S11/I17/I18 与新增通用 identifier hard negatives：确实 NoMatch、零正文、可证明完整 absence；empty Partial/timeout/error不当正确拒答 | 真错误不豁免；自然 `in` 等检索语义保留，不用停用词/题目 alias 换 no-answer |
| 原固定质量 | 原 scorer/locks/raw/replay 原样保留，逐题 ranking/Recall/span/no-answer/status/gate 与对照配对，macro/micro/category/repo 分报 | 原 strict inventory gate 若仍失败必须显示失败，不能重命名或删除 Partial；不声称所有检索结果完整 |
| task/facet 诊断差分 | 110 核心 span 逐题/逐 repetition 审计；task evidence 与 lane inventory 分开；原成功正文的所需 facet 不因 budget/selector 优化丢失 | 新覆盖退化触发审阅且不能自动通过；NL 原 miss 保真记账，不凭路径 Top1 证明行为完整 |
| 完整多 facet / graph机制 | 独立完整实现/测试/interface 正文要求全部覆盖；source-valid span；graph调用方向/类型/端点与源事实一致，错边/错关系 hard negative 被拒，graph metadata不能代替正文 | full-on 产品配置必过；反事实 cells 可失败并量化成本/收益，不挑 cell 代表产品 |
| 消融 V19/V20 | path/exact/intent-facet-reservation 3 因子全 8 cells，one-factor edges；其他 source bytes/config/build flags 固定；scope/overlap/provenance不跟着关；原51+独立机制分报 | 没有真实控制变量、不同输入或只 on vs all-off 不构成完整单因结论 |
| 质量统计 | 按 repo/query_family 分层或 cluster bootstrap；翻译/改写同 family；repetition先在题内聚合；paired per-query/family delta、N和CI透明 | 不能把重复当独立题；不确定为 inconclusive；0.01 nDCG/1pp Top1是原建议审阅触发而不是错误豁免；不得用新宽松阈值掩盖退化 |
| tail / 资源 / 并发 | same-release baseline/candidate；C1/4/8/16同输入，read+build与真实修改负载分报；offered/queue/service/e2e、timeout/cancel/error/拒绝、成本/线程/CPU/资源归属，全部样本 | 现 statistics 明确 <200 samples insufficient_for_tail_claim，90 reads不足 G5 tail；建议每C档300 reads、全部状态计时，不只成功；原性能 >10%审阅、>20%阻断适用profile |
| 资源不可用 | RSS/thread sampling受有界准入、probe0为null、不能写0；before/after不叫峰值，runner不当server；队列/worker最终归零与合法lease最终释放 | 若资源采样不可用可保留独立机制证据，但不能声称完整内存/资源认证；缺G5必需证明仍未验收 |
| 独立复核 | current source/archive/log/binary/输入/原raw重算、适用验证覆盖、失败负例、回滚说明，由非实施owner出独立audit | 计划检查、作者自证、旧绿灯、只新增目录、只有trait不替真实完成 |

## grouped lexical 当前源审阅

`compile_expanded_fts_query` 先保最多12个原词，每 compound 为 whole OR(ALL distinct parts AND)，整组超预算只保 whole 并记录遗漏；原自然词仍OR，quoted literals阻止FTS操作符注入。snake whole明确保 unicode61 phrase、component branch允许重排。SearchPlan持有可信编译产物，LexicalLane不再重sanitize破坏AND边界，遗漏保持Partial及截断理由。QueryPolicy v2 与 retrieval/cache v17已更新。

通用13个SQLite/真实入口红绿覆盖了未知compound假召回、acronym/数字后缀、重复组件、完整组预算、源词预算、Unicode、字面OR/AND/NOT/NEAR/quote/punctuation、自然in和多自然词；仍须正式所有lane/hard scope/缓存/stdio全回归。不得把AND body命中升级为exact symbol身份。

## 已观察的发展诊断（不是正式结果）

`development-obligations.json` 对 P5-D 基线与 grouped lexical development 的306 rows进行独立原件byte/span核验：110核心span无负配对差分，3个no-answer当前全是真正NoMatch。

最低task证据仍有真实miss：R04/R08/R12/R14、S02/S04/S06、I04/I07/I08/I12/I16在当前输出core coverage=0，基线也是相同欠缺。它们不能因Top1文件命中而被称为行为完整。I07无词锚抽象自然查询不得通过fixture alias强行修绿；后续真实语义能力和公开heldout是P7/P8，不从本批fake/local数据推导。

## G5 声明边界与待确认项

工程和质量 profile 不承诺全部自然语言题命中；原严格 lane-inventory 门与实际任务证据/质量是不同证据，但原门失败不能被洗掉。只有 owner 在正式运行前锁定完整 profile、证明上述全部适用门、对真实 miss 与所有 Partial 逐项说明，并经独立审计后，才能裁决原 P5 本地增强版是否满足；本审查**不预先宣布 G5 passed**，也不把一份非退化小报告命名为全阶段成功。

P8公开6+仓/约600题/holdout/100k/跨平台与发行、P6/P7持久化语义/live、P9增强收益决策仍属于用户全部 todolist，不因为本批通过而缩小总目标。
