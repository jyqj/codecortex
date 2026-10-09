# Wave002 前瞻候选交付

**完整候选已冻结，尚未发布、调度或执行 native 测量。** 原 wave001 失败、D0原study、原29个任务、shared Git HEAD/index与所有活动数据库均未修改。不同代理正在审查该完整controller；本文件属于作者交付说明，不自批。

## 具体对象

- 候选目录：`candidate/`，28个UTF-8文件，共1,198,652字节。
- 冻结清单：`candidate-v1-receipt.json`，SHA `dbf189c4f65e87841f126f40150b8dc5ffc92fa735aa78cf4c771a53fc7a15ec`。
- 新controller：`bcfc1e3882b262ffbaf64ee58553dc1069573b76847896984539cfa71b4ca649`。
- 新registration：`b1a33c916da5d5f9b61bc9227411db462db644edf517a30bbf953e1c230d517f`。
- 新workflow：`d1b3b3bc3557ebc5177cdd3e7ea33147964fd04fc08f91b0fa6f6032bc14d23f`。
- 拟议独立branch：`task/p8-d0-recovery-wave002-28fe-20261009`；父提交仍为固定G4 `260f596582f2d82b8d7c707b61a6b8b6a43b069f`，新package前缀为 `artifacts/checkpoints/p8-d0-recovery-wave002-20261009-28fe/`。

## 改动和保留

wave001原官方job的容量step失败、execute skipped，原ZIP无execution/started/shard。原capacity receipt在checkout身份守卫处exit2；空间88,661,061,632 B实际够用，原门30,509,367,296 B不变。容量命令原工作目录已经是measured，但继承的GITHUB_WORKSPACE为上层。新workflow只为这一条Python命令设置 `GITHUB_WORKSPACE="$GITHUB_WORKSPACE/measured"`，其他step环境不变，原helper/删除白名单/hosted限制逐字不改。

新schema wave-v2登记唯一100k/rep8/ordinal2。policy `20d94bc…` 精确绑定原001官方job canonical SHA、完整日志、唯一ZIP digest/size、三份原receipt、D0 helper及先前完整receiver eccffe7a…。原001仍invalid，两个原known_errors均保留；新解释仅证明这次固定容量调用错误发生在进入native之前，不扩展原143/unknown类别。原0..2和全300上限不变。

每次准入、execute之前、最终recheck均重读原证据及两个branch完整run集合。重复002 push、旧001 rerun、缺任何初始身份、原running回queued、旧artifact丢失或漂移、原rep8晚到raw/额外失败、原001日志/step/archive变化、任何追加native错误均阻断。所有旧状态和原错误仍进入151条prior账本，当前实际job另列为第152条；完整覆盖分母保持150/1500。原main / driver / 原验证器调用的AST逐项相等。

## 验证结果与实际局限

冻结候选的26项控制全部通过，原日志 `controls-second.log` SHA `c8e21ee5bb100250c9f0929e451dc61353dc400ece056eb29ef156535fbbfba5`。控制使用真实原001 job/log/ZIP/receiver，以及明确标注synthetic的未发布wave002 API身份。包含完整150/151/152正例、两页原jobs、当前包全字节/registration/workflow闭环、同一job不可换run/source、重复波次、旧rerun、原有效/活跃/known failure不可重测、0..2配额、late原raw/log和容量证据改变等负控。没有执行任何原生测量。

本地另有6项路径/进程环境控制通过，见 `capacity-path-followup.log` 与 `capacity-path-followup/receipt.json`：实际symlink祖先拒绝、原SDK白名单与supervised argv只读检查、hosted上下文限制、实际候选shell语法、含空格路径的真实单进程环境赋值。后者使用只打印环境的Python替身，证明child workspace恰等于measured cwd且parent变量不变；没有调用capacity准备或native。

原capacity helper本地曾以明确的host-path adapter执行真实prepare body及其Git/head/blob/disk检查。错误workspace在原checkout守卫退出；正确workspace通过源码身份守卫，但本地只有48,517,120 B，因此仍按原30.5GB门返回exit2 / not_run_capacity_unavailable。**这不是容量准备通过或hosted VM端到端成功。** 原CLI在真实非hosted本地上下文也先行拒绝。首次预期capacity_available断言失败、两个原receipt及原脚本都保留，没有清理源码/数据库或降阈值。

第一轮controller控制的API适配器分页标记有作者夹具错误：19项固定证据控制通过，但当前API正例失败；五个API负控的初次结果不作为有效验证依据。原25项日志和原夹具/登记/workflow留在 `controls-first.log` / `history-control-first/`；修正夹具并为负控明确匹配实际拒绝理由后，26项完整重放通过。产品controller未为该夹具错误改变。首次候选准备使用共享本地Git读取D0 helper失败，也单独保留；随后用固定D0 commit的官方Contents原字节核得同一helper SHA `2e37954d…`，没有伪造本地Git树。

## root发布前的具体核对

不同作者对上述28个冻结文件完成独审后，root可在新branch创建单父G4的新commit；需核实际Git tree每个文件等于冻结清单、parent和registration固定时间均正确，并再次只读原rep8/原001官方job/log/artifact，确认没有已知late evidence。新branch正常push仅会触发这一个受注册波次。该决定与receiver v6新分类实现的独审是两个审查对象。

原capture001仍待固定独立调和；本controller的coverage永远false。最终原150格/1500样本、全部attempt与原validate_shard/combine及硬依赖验收仍开放，原任务本次关闭0项。三项逐字差异见 controller / workflow / registration diff，未来receipt契约见 `RECEIPT-DIFFERENCES.md`。
