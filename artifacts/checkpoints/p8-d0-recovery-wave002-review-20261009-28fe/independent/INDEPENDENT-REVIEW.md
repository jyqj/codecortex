# Wave002 controller 非作者限定发布审查

结论：冻结的 28 文件候选可按原定程序发布为独立 wave002，且只允许一个 100000 files / repetition 8 / supplemental ordinal2。未发现阻止本次固定发布的源码或接口缺口。这是 controller 启动控制审查，不是实际运行、150/1500 coverage、partial 调和、原 study 或 TODO 验收。

## 精确冻结身份

- candidate receipt：`dbf189c4f65e87841f126f40150b8dc5ffc92fa735aa78cf4c771a53fc7a15ec`
- controller：`bcfc1e3882b262ffbaf64ee58553dc1069573b76847896984539cfa71b4ca649`
- registration：`b1a33c916da5d5f9b61bc9227411db462db644edf517a30bbf953e1c230d517f`
- workflow：`d1b3b3bc3557ebc5177cdd3e7ea33147964fd04fc08f91b0fa6f6032bc14d23f`
- 28 文件 / 1,198,652 B，在测试前后逐项核 size/SHA，目录集合完全一致，无候选文件被 reviewer 改写。

## 准入与历史保全

完整 150 初始身份与原 failed/incomplete D0 study 保留。当前固定前驱还包括真实 wave001 / run37865643378 / job113611597284 / artifact11588924279，旧 `state=invalid`、两条已见宽错误、原 log/ZIP/3receipt、native_outcome:null 均保留。只有新登记 policy20d94bc… 精确固定的 pre_native_capacity_observer_checkout_mismatch 可以成为 ordinal2 的前驱；它不是通用 exit2、无raw、infra、source失败或budget失败豁免。

controller 每次 admit、execute 前、recheck 都读取当前两条 branch 的完整 workflow run 列表、原 D0 150 jobs/artifact 集合、原 rep8 日志、001 job/artifact/log 和原 build 当前 metadata。原 rep8 late log/raw、001 任一指纹/step/原件变化、额外run/job、任何已知额外错误均阻止准入；日志指纹变化不依赖宽 regex是否识别新的错误。

check_ledger 的已有 ordinal1 链仍要求原 ordinal0 是无已知错误的明确143/shutdown。只有 proposed 末尾增加固定 ordinal2 分类，要求原0仍143且无late_revoked、原1经过本次 fixed proof。固定 wave/ordinal2、每格2supp、总300、150初始和1500完整样本门没有改变。全局串行 concurrency group 保留，额外1个supplement与原20上限分开声明为最多21；未冒称仍20。

已真实固定 receiver `eccffe7a…` 的151尝试及 capture001 `993496ad…` 未调和原历史都在登记包中。控制器明确 `partial_reconciliation_performed=false`，无法通过启动这格解除覆盖阻门。原 all_initial_attempts=150、all_prior_attempts=151、含当前attempt=152，current_wave_attempt 不从step或分配时间推断native已开始。

## 原执行与容量修复

以 AST 独立逐项比较，旧 main、API、NoRedirect、interruption、validate_original_run、original_job_map、validate_artifacts、initial_map、strict_json 均保持一致。旧 main 仍验证固定 D0 源 `d0cb69c…` 的完整1087 native inputs、原两个observer/helper、原build receipt/binary，调用原 validate_build、validate_shard 和同一原driver。既有15表native parity/oracle逻辑未改。

固定计划仍 release / seed12648430 / 30 repetitions / shard8 of30 / files100000 / dirty_budget200 / max_resume1024 / deadline18000000ms / max_output536870912B / batches1,10,100,1000。workflow 350分钟及上传always/include-hidden-files/error-on-none保留；不隐藏原driver非零或post-recheck失败，不重试、替换或改样本分母。

唯一容量命令修复在一次进程环境中将 GITHUB_WORKSPACE 指向 nested measured checkout。workflow仍checkout确切D0、下载原11582571291且merge-multiple:true，容量helper原SHA2e37954…和其固定30,509,367,296B要求、hosted/path/SDK白名单门均未改。独立bash -n通过，并以环境打印stand-in验证含空格workspace仅child加/measured、parent值不变；没有执行SDK删除或capacity/native测量。

作者首轮capacity探针的正确workspace只通过真实source守卫，随后因48,517,120B空间不足且preparation disabled返回exit2。原失败日志和两份receipt继续保留；后续6/6 path/env控制没有被冒称为capacity_available。

## 独立验证与明确限制

- 原26项作者控制在独占临时目录独立复跑26/26，exit0；日志SHA00482b72f6aca358ae807ee4de45118573aad4926e210d866d9aa2e529297317。
- 新23检查全部通过：额外/错误run、attempt、原job/artifact、原build身份/可用性、晚到非零native日志；以及9个原关键函数AST等价。
- 其中独立接口控制直接采用当前完整controller.observe的输出，送入已非作者审过的v6 validate_supplement_envelope，150/151/152结构、原invalid/error保留和current job身份实际匹配。合成execution元数据随后改为driver_exit2时仍拒绝。未来成功phase/当前run都是明确synthetic，没有制造真实002 raw或成功结论。
- 最初独立probe未完成：/dev/shm已满导致临时phase JSON ENOSPC，另2个负控已正确拒绝但 reviewer预期错误消息substring过窄；首源/输出保留。修正 reviewer 预期并把3份明确synthetic phase JSON/目录接口改为内存fixture后23/23；固定源码读取、candidate、原API/ZIP未改。没有把首轮当通过。
- 较早10/10固定helper独立反例及其原件报告保留，是探索审辅助；最终结论绑定本28文件。

发布仍按既定最后步骤执行：实际commit/tree必须与冻结28路径字节匹配并先于该run执行；正常push仅触发这一注册branch的一格。reviewer未调用发布/重试/取消/网络API，也未改变权限或任一原source/task。最新官方变化必须由原定发布读回/运行前admit重新核验。实际运行完成后仍需官方原件与完整外部receiver收件，capture001调和另由root独审。

原TODO192/163/29，本审查完成0项。
