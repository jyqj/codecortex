# 第 37 轮：原任务范围复核与具体收尾候选

本轮新增完成 **0** 个原 TODO；本会话 **1/10**。主线仍 `b9b089bb4eae072affe9326681d4980eae15fd84`，账本 **192 total / 164 done / 28 remaining**。任务定义、依赖与状态均未更改。

## P8-006 工作范围与发布研究

两位独立读者分别核对原 tasks、C16、V07/V20、benchmark 与 P8-SCALE。结论是：006 的描述性索引/fanout 工作验收与 G8/P8-020 的全量 release profile 应分开判定。原文没有直接写“N1即可done”；这是一项有条件的范围解释，当前没有给予最终验收。

必须先真实接受同一 source/build/run 的五个预登记 rep0，具备五尺度的九主阶段、原1k五fanout、phase计数、时间归因、闭包与完整十五表对账，并复核原V07/旧回归适用性，才可评审明确 **N=1、按环境分列**的描述性006结果。当前100k接收回执仍是null，只有四片，不能done。

当前研究的 **N30、150片/1500观测、全部原预算与aggregate规则保持原样**。即使未来006任务通过上述工作验收，完整研究、统计性能与G8仍须按各自原门判定。任何旧失败、后续反例或未运行项继续保留。

- [第一独审与追加论证](task-scope/first-review/task-versus-release-and-negative-results.addendum.json)
- [第二独立证伪审查](task-scope/second-review/second-independent-task-study-boundary-review.json)

## 已完成的具体准备

[四个首片描述报告](descriptive-four-rep0/descriptive-results.applyfalse.md)保留全部 **41个原观测**，原计时边界和99次主A/B build、28次fanout build的计数/闭包指针。相同hostname没有被当作相同环境：5k与其余三片CPU型号不同。没有混合分布、拟合因果、稳定尾部或提速声明；vectors继续disabled/null。

[新版九项待应用输入](nine-task-application/nine-task-dependent-update-input.applyfalse.v2.json)保持 **apply_now=false**。007–013和016的原更新对象及共享证据索引不变，没有增加统一重跑门。若全部真实验收，计数才会变为173 done / 19 remaining，本会话10项；当前没有应用。

## 原研究进度

完整cohort `38013753078/a1/G9cc6` 仍 **4/150片、41/1500观测**。03:10:13–14 UTC 官方读取中100k job仍在第7步运行，没有新measurement artifact。

独立诊断checkpoint06原件已通过一次冻结reader。00–06连续raw至 **14,674,260B**，新增唯一完整事件为body/full_control结束，wall 922.132074秒。其body parity/阶段结束尚未出现在前缀中；最终native结果、supervisor终态与observer-after仍未知，不声明EOF。

- [checkpoint06审查](diagnostic/checkpoint06/checkpoint06-prefix-review.json)
- [原ZIP来源与完整字节](original-zip-custody/diagnostic06.json)
- [03:10官方原记录](official/dual-study-0310-originals.json)

## 其他收尾边界

PR188继续原Draft/HOLD决定，既有负数据保留，未编造“已被取代”或作者撤回。P8-014本身要求可选LLM评审的明确授权和预算；原文不限定必须付费或联网，现有local结果不能替代实际模型执行。P8-018原文档脚本尚未执行，本地缺Rust工具链；正在核查隔离CI执行范围与候选，不添加九项统一新门。

[完整收尾记录与实际blob映射](formal-closeout.json)。本目录28个叶子均为新增，原始研究、任务定义与既有记录未重写。
