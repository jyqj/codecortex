# 第15轮 multi-subagent 固定证据

本轮新增完全关闭 **0** 个原 TODO；**192 项中 163 done、16 in_progress、12 todo、1 blocked，剩余29项**。目标仍是 P8-005～P8-013、P8-016 十个原任务。测试、工程修补、PR 关闭、合并、接收器和归档均不代替原任务完整验收。

这是基于第14轮固定提交 `7520427ca691cf66232f8b41a3d09bb922a6e9d7` 的证据增补。manifest 收录23个正文的 Git blob、完整 UTF-8 字节数与 SHA256；另有本 README 和 manifest，合计25个纯新增文件。历史路径、mode、对象和任务事实保持原样。

## 已完成的验收

**G2 / PR169 原 CI。** 原 run [37877611353](https://github.com/jyqj/codecortex/actions/runs/37877611353) 的 check、MSRV、security 全部成功。完整原日志确认实际 checkout 是 merge `fa6c6c69ae3dd459a78b60c9d5ca369f9eb9824b`，其完整树与 G2 `0272a1fb152fd76a7cfb22386a580629d4038a64` 相同。181个原 source_integrity 方法在同一原命令、同一进程内全部通过；385个 P8 Python 方法全部通过。原 stable fmt、Clippy、全部 default test target 编译及既定回归范围成功，既有 ignored 项如实保留。v15 实际执行并准确绑定 P2/R2/1089输入，也执行旧v14和历史校验。此原CI验收独立于第14轮的 Rust53/Python50 辅助控制。

**G4 平台完整接收。** Root 实际完整接收外层137,027,856B ZIP，核全部203成员、九个原内层ZIP和153个内层成员，双向库存、CRC/SHA/字节数及源码/四个 cold observer 一致。外部接收器调用未修改的原 `p8_cold_build.py --collect-cells`，使用真实目录，8 passed、0 failed、0 not_run；完整输出矩阵与原矩阵只在 source_root 不同。原CLI耗时0.966287811秒，与外层接收器8.80703472秒分开记录。非作者二审核对了完整报告、checker和实际API关系，没有声称第二次下载全部ZIP。失败的容量准入与此前传输尝试保留。

**33条运输 fixture 控制。** 独立静审、实际对象桥和分支不存在检查后，首次发布 `281c8badb4e3d7ea6835289d9d47f106a697d648`。原 run [37882881654](https://github.com/jyqj/codecortex/actions/runs/37882881654) / attempt1 / job113666325306 实际33个唯一方法逐项通过，0失败、0错误、0跳过、0预期失败、0意外成功；五份固定输入前后全部一致。完整安全原job日志及原report、stdout/stderr镜像已由非作者和root分别读取。unittest耗时0.301秒；wrapper耗时0.415461119秒。原3093B输出ZIP仅核对API和上传digest，没有独立下载或CRC声明。这些是标准库ZIP与mock curl的合成控制，没有真实artifact网络传输或产品测量。

## 原运行成功、但本检查点尚未完成全包接收

| 原源码 | 接收器与原首次run | 实际已知 | 本检查点仍缺 |
| --- | --- | --- | --- |
| G4 `260f5965…` | `9aacfbea…` / 37879784342 | endpoint区分修补后的原离线复核exit0，完整typed回执和API已读取 | 外层233,905,529B ZIP及五个原内包全部字节、清单和输出接收 |
| G2 `0272a1fb…` | `08bfcb45…` / 37881939531 | 未改原八格collector实际8/8，原checkout/回执/外层artifact身份一致 | 外层137,100,343B ZIP及九个原内包全部字节、清单和输出接收 |

两组都保留各自源与原件，不互借完成结论。G4先前接收器 run37877082223 的真实失败继续保留；新控制只修正工作RPC与独立终点请求的区分，原2倍search/4倍RPC验收不变。新run的实际成功没有被写成已经完成全包接收。

本地执行服务断连，且本地空间/内存的原保留门不得降低。后续正在准备固定外部离线全包读取，采用有充足磁盘的标准ZIP流式核验，输出可完整读取的安全审计结果，不启动新产品测量或统计负载，也不无限套装新的大ZIP。

## PR治理和主线变化

- [PR168](https://github.com/jyqj/codecortex/pull/168) 已于04:16:02关闭为被 [PR169](https://github.com/jyqj/codecortex/pull/169) 完整包含的前序草稿。其G1 head、原E0282失败、Python50成功、标签和历史ref均保留。G2的P2→R2→G2链以G1为祖先，修补后原CI和独立控制已经实际通过。关闭PR只整理合并队列，不取消、重跑或替换原研究。
- PR169仍为独立冻结候选和Draft。原release及1k/10k工程jobs成功，100k仍在原执行；完整原 `validate_build`/`validate_shard` 接收后，才进入自身正式N150的登记和执行。辅助控制不计正式样本。
- 仓库方于04:16:47合并 [PR167](https://github.com/jyqj/codecortex/pull/167)，main成为 `b21cce4c8661589267ad5719f850accbec088d2f`，父为旧main4775和a23。Root实际读取完整根树：与a23相比仅artifacts不同，其他12个根条目的mode/type/OID完全一致。实际读取新main完整tasks.json，解析全文与此前台账相同，仍163已完成、29剩余。此合并不是root执行，也没有被计为任务完成。

## peer a23 的可复用证据和剩余工作

`peer167-4630-entry-audit.json` 对固定归档4630的222个文件及29份关键正文作入口与身份审查，明确引用先前具名独立审查的八个平台组合、恢复/真实历史回滚、失败CLI、四档混合负载、fake backfill和生命周期。该入口审查没有重新下载所有原ZIP，也不冒认之前的审查作者或原native执行。

原a23的新增一小时soak及原150片完整规模研究不在4630入口验收范围内。后续新4998归档正在分别复核；需要的补充限于其真实源和原数据。原150片研究保持原5个index0准入以及145个后续片、原N/预算/完整aggregate，不补换、筛选或混合其他source。a23可以凭自己完整满足原要求的证据关闭目标十项，不需要等待PR169的额外优化。

## 重放与保存边界

本检查点保存报告、审查、固定源码入口、真实原job日志和发布/PR证明。**它不是包含全部大型原始数据的独立重放包。** 大型原Actions artifacts与各固定源码仍需按报告中的原ID、大小、SHA256和路径获取。G4平台checker需要作为argv传入原artifact临时URL，并提供已有的精确只读G4 checkout。

只发布完整安全的合成测试job日志。其他原Actions日志可能含临时签名URL，因此只保存其完整原文hash、原API引用和明确允许的事件；含临时URL的本地下载脚本不进入归档。先前失败、未执行项、原study、所有source身份和任务验收门继续保留。

下一轮重点是完成上述原字节验收、a23 soak补充及仍在运行的原规模证据，再按原依赖更新事实台账和生成视图。当前不宣称目标十项已经完成。
