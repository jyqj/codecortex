# 十二项原47条验收判断
审查人：/root/todo_audit；实际完成判断时间：2026-10-10T07:25:08.759599+00:00。
**结论：十二项原自身范围具备按原硬依赖顺序闭合的证据。当前尚未写任务状态，apply_now=false；最终证据commit、非作者独审和Root实际应用指令到齐后才执行。**
当前账本164/192完成、28剩余，本会话此前005一项完成。不得把本报告签署等同已修改账本。
固定source f97c/run38026411200/a1：45格/85记录已逐格通过；07:18:55–07:19:32 UTC在新目录运行未改原aggregate，真实exit0，40setup/40mutation/5fanout，missing/errors空。独立1803检查通过。原Actions聚合因只下载42输入而失败的原matrix与完整日志不改标，完整45恢复未重跑测量。

## P8-006 增量规模与fanout曲线
原硬依赖：P7-020, P8-001, P8-005；原验证：V07, V20。
| 原字段及原文 | 实际证据与判断 | 保留范围 |
|---|---|---|
| steps[0]：测no-op/body/API/config/batch和超预算闭包 | **原任务范围通过**；[S45] 同一预登记source/run的45格及85记录已逐格原验收和独审；8种fresh profile×5规模及5fanout全部覆盖。完整45原aggregate恢复实际exit0，40setup/40mutation/5fanout、errors/missing均空。 | 原Actions42输入失败仍保留；独立恢复不是测量重跑，也不把Actions原失败改为成功。 |
| steps[1]：输出各phase计数 | **原任务范围通过**；[S45] 85条原记录与原raw派生phase/build计数均已保留；五cold点取no_op setup，8×5 mutation和5fanout单点按slot/role及环境分列。 | 各phase存在嵌套，不把嵌套耗时相加；N1不得声称稳定tail/统计显著或跨host因果提速。 |
| acceptance[0]：时间可归因且闭包完成后full parity，未完成有显式status | **原任务范围通过**；[S45] 原45输入全部由未改原aggregate真实接受，完整initial/final15表与所有原closure/status谓词通过；真实超预算首incomplete与后续resume/ready及原phase/count保留。 | 仅本原任务的完整描述性N1范围；语义provider关闭的vectors保持null，完整D4/V20/G8及旧研究状态不外推。独立原完整45恢复可补输入组装失败，不伪称原Actions成功。 |
| acceptance[1]：相关旧功能回归通过；没有证据的项标not_run/blocked而非done。 | **原任务范围通过**；[G2] 当前源码主CI/closeout/26工程checks/soak实际通过，原V07相关方法按同tree bridge与历史scope引用。 | 这不是45研究通过。旧真实repo质量/replay exit1不被当作G2增量phase；完整D4/V20/G8仍保持其实际状态。 |

自身范围验收通过；实际状态依上列原硬依赖顺序更新，不由准备/文档存在自动完成。

## P8-007 多并发与混合负载
原硬依赖：P8-006；原验证：V11, V20。
| 原字段及原文 | 实际证据与判断 | 保留范围 |
|---|---|---|
| steps[0]：C1/4/8/16 mixed read/build/backfill | **原任务范围通过**；[G275-MIX],[LOCK-G2] C1/4/8/16实际mixed峰1/4/7/12；独立backfill24cells/768requests，seeds7/19/43每seed4workers、最终pending/claimed/uncovered0；DB acquisition由另一次原G2四档各900提供。 | 同源同run的mixed与backfill是独立artifact，不宣同一进程；旧LOCK-G2不是当前fresh45 G2。 |
| steps[1]：记录offered load和排队 | **原任务范围通过**；[G275-MIX],[LOCK-G2] 每mixed档900 offered操作=300build+600read；排队/actual acquire和backfill分布保持全部原结果。 | writer_acquire_and_rollback包含BEGIN/ROLLBACK，不叫纯锁阻塞时间；不强制实际并发峰等配置峰。 |
| acceptance[0]：无死锁饥饿，吞吐不能隐藏timeout与尾部 | **原任务范围通过**；[G275-MIX],[LOCK-G2] 原collector/独审接受，无死锁/饥饿；offered/completed/timeout和原分布未用吞吐总数掩盖。 | fake provider不变成live费用或稳定p99/SLA认证。 |
| acceptance[1]：相关旧功能回归通过；没有证据的项标not_run/blocked而非done。 | **原任务范围通过**；[G2] 当前同树原主CI与26项工程终态，加本项上列原source的已接收组件回归。 | 不是用总体绿灯替代上列每条步骤；旧not_run不改名，新未完研究不计通过。 |

自身范围验收通过；实际状态依上列原硬依赖顺序更新，不由准备/文档存在自动完成。

## P8-008 冷建/重开/热查分层
原硬依赖：P8-007；原验证：V20。
| 原字段及原文 | 实际证据与判断 | 保留范围 |
|---|---|---|
| steps[0]：分离OS cache、process cold、result-cache hit与uncached warm | **原任务范围通过**；[GC8-LIFE] 明确分列process-cold、重开、uncached warm与result-cache hit，OS缓存状态如实标识。 | 没有清空OS page cache，不能写成drop-cache cold。 |
| steps[1]：采全部样本 | **原任务范围通过**；[GC8-LIFE] 完整30 cold+400 reopen+400 uncached+400 cache-hit=1230samples、431sessions，所有原样本保留。 | 没有best-of或挑最快样本。 |
| acceptance[0]：无best-of，sample N/分布/CI齐全 | **原任务范围通过**；[GC8-LIFE] 原N/分布/CI字段完整保留，原独审已接受该分层scope。 | 尾部可估性不足时CI为null并解释；不是把null当0或稳定尾部通过。 |
| acceptance[1]：相关旧功能回归通过；没有证据的项标not_run/blocked而非done。 | **原任务范围通过**；[G2] 当前同树原主CI与26项工程终态，加本项上列原source的已接收组件回归。 | 不是用总体绿灯替代上列每条步骤；旧not_run不改名，新未完研究不计通过。 |

自身范围验收通过；实际状态依上列原硬依赖顺序更新，不由准备/文档存在自动完成。

## P8-009 内存/磁盘/费用总账
原硬依赖：P8-008；原验证：V20。
| 原字段及原文 | 实际证据与判断 | 保留范围 |
|---|---|---|
| steps[0]：分别client/server/process-tree和artifact/FTS大小 | **原任务范围通过**；[GC8-LIFE],[GC8-SOAK] 原client/server/process-tree角色、物理artifact与FTS大小分别记录。 | 重叠角色不相加；whole-process-tree peak缺失不编造。 |
| steps[1]：成本reported/estimated分栏 | **原任务范围通过**；[GC8-LIFE],[GC8-SOAK] reported/estimated/unavailable分栏并保留单位和归属。 | 没有真实billing就保持unavailable/null。 |
| acceptance[0]：单位和归属正确，不把unavailable填0或把runner当server | **原任务范围通过**；[GC8-LIFE],[GC8-SOAK] 原独审核正确角色、大小、资源和费用边界；当前source相容桥保留。 | 不把runner当server，不将unavailable填0；不由资源快照声称完整峰值。 |
| acceptance[1]：相关旧功能回归通过；没有证据的项标not_run/blocked而非done。 | **原任务范围通过**；[G2] 当前同树原主CI与26项工程终态，加本项上列原source的已接收组件回归。 | 不是用总体绿灯替代上列每条步骤；旧not_run不改名，新未完研究不计通过。 |

自身范围验收通过；实际状态依上列原硬依赖顺序更新，不由准备/文档存在自动完成。

## P8-010 长时soak与连续修改
原硬依赖：P8-009；原验证：V07, V17, V20。
| 原字段及原文 | 实际证据与判断 | 保留范围 |
|---|---|---|
| steps[0]：持续编辑、删除、切分支、catalog压实、cache/worker复用 | **原任务范围通过**；[GC8-SOAK] 原3600.054553094s/3601operations=1201build+2400compoundread，实际编辑/删除/切分支/catalog压实/cache复用。 | 保原Gc8执行身份；当前G2hour-soak另作工程回归，不偷换同次measurement。 |
| steps[1]：终点完整对账 | **原任务范围通过**；[GC8-SOAK] 终点完整15表与原长期资源/队列控制已接受。 | 不是只用进程活着作为soak成功。 |
| acceptance[0]：内存/队列不无界增长，长期索引和全量一致 | **原任务范围通过**；[GC8-SOAK],[G2] 原增长/队列/完整full parity被原验收接收；当前G2原hour-soak终态也通过。 | 有限原时长/配置scope，不宣任意无限运行或全机峰值。 |
| acceptance[1]：相关旧功能回归通过；没有证据的项标not_run/blocked而非done。 | **原任务范围通过**；[G2] 当前同树原主CI与26项工程终态，加本项上列原source的已接收组件回归。 | 不是用总体绿灯替代上列每条步骤；旧not_run不改名，新未完研究不计通过。 |

自身范围验收通过；实际状态依上列原硬依赖顺序更新，不由准备/文档存在自动完成。

## P8-011 端到端故障与恢复认证
原硬依赖：P7-020, P8-007, P8-010；原验证：V14, V17, V18。
| 原字段及原文 | 实际证据与判断 | 保留范围 |
|---|---|---|
| steps[0]：kill进程/网络断开/缓存损坏/数据库忙/换库 | **原任务范围通过**；[G275-FAULT] kill/断网/缓存损坏/DB忙/换库分别映射：local3passed；独立同源SIGKILL12+15；active seeds223/227/229四case及7原Rust控制。 | 原local4not_run不改成passed；补充是不同实际case。不是物理断电测试或付费live故障。 |
| steps[1]：保持raw故障工件 | **原任务范围通过**；[G275-FAULT] 原完整499member recovery artifact、command/stdout/stderr/raw和source/observer绑定由既有原件保全。 | 本报告不重新开ZIP/CRC或跑故障。 |
| acceptance[0]：没有假ready或删除复活，恢复与费用影响清楚 | **原任务范围通过**；[G275-FAULT] 原故障中partial/nonready状态、恢复ready、deleted-source不复活、manifest/incarnation及provider请求增量均有原逐case证据。 | paid_cost=null，恢复费用只按已有真实字段表达；非release-tag物理演练范围不扩大。 |
| acceptance[1]：相关旧功能回归通过；没有证据的项标not_run/blocked而非done。 | **原任务范围通过**；[G2] 当前同树原主CI与26项工程终态，加本项上列原source的已接收组件回归。 | 不是用总体绿灯替代上列每条步骤；旧not_run不改名，新未完研究不计通过。 |

自身范围验收通过；实际状态依上列原硬依赖顺序更新，不由准备/文档存在自动完成。

## P8-012 MSRV与平台冷构建矩阵
原硬依赖：P8-011；原验证：V01, V21。
| 原字段及原文 | 实际证据与判断 | 保留范围 |
|---|---|---|
| steps[0]：Linux/macOS、当前声明MSRV与stable、默认/semantic features | **原任务范围通过**；[GC8-PLATFORM] 原Linux/macOS×1.95/stable×default/semantic八格及最终collector已接受，记录实际SDK/toolchain/binary。 | 原固定toolchain身份，不将历史stable字面升级成今天stable。 |
| steps[1]：全新target | **原任务范围通过**；[GC8-PLATFORM] 各格实际absent/fresh target收据保留。 | 普通warm-cache CI不替此冷构建证据。 |
| acceptance[0]：SDK blocker解决或发布平台范围明确，缓存测试不替冷构建 | **原任务范围通过**；[GC8-PLATFORM],[G2] 原八格完成、平台/SDK范围明确，当前ordinary工程另作source适用性。 | 不由八格旧源宣布新发行所有平台均认证；保留原发布范围。 |
| acceptance[1]：相关旧功能回归通过；没有证据的项标not_run/blocked而非done。 | **原任务范围通过**；[G2] 当前同树原主CI与26项工程终态，加本项上列原source的已接收组件回归。 | 不是用总体绿灯替代上列每条步骤；旧not_run不改名，新未完研究不计通过。 |

自身范围验收通过；实际状态依上列原硬依赖顺序更新，不由准备/文档存在自动完成。

## P8-013 指标/门槛与失败退出最终认证
原硬依赖：P8-012；原验证：V03, V04, V20。
| 原字段及原文 | 实际证据与判断 | 保留范围 |
|---|---|---|
| steps[0]：故意注入quality/perf/lock失败 | **原任务范围通过**；[GC8-GATES] 原六Rust failure contract和七compareCLI样本实际包含quality/perf/lock等故意失败。 | 成功的含义是正确拒绝负样本，不是负样本quality/perf变绿。 |
| steps[1]：验证退出和raw报告仍保留 | **原任务范围通过**；[GC8-GATES] 原七CLI退出[1,1,1,1,2,2,2]与raw/report保留，相关原独审接受。 | 原失败和invalid/inconclusive保留。 |
| acceptance[0]：红线失败必非零，inconclusive不被自动passed | **原任务范围通过**；[GC8-GATES],[G2] 红线错误非零、inconclusive/invalid非passed，当前回归验证保持同契约。 | 不因这些测试通过而宣未完规模研究或发布门通过。 |
| acceptance[1]：相关旧功能回归通过；没有证据的项标not_run/blocked而非done。 | **原任务范围通过**；[G2] 当前同树原主CI与26项工程终态，加本项上列原source的已接收组件回归。 | 不是用总体绿灯替代上列每条步骤；旧not_run不改名，新未完研究不计通过。 |

自身范围验收通过；实际状态依上列原硬依赖顺序更新，不由准备/文档存在自动完成。

## P8-016 数据库/配置/包回滚演练
原硬依赖：P7-020, P8-012, P8-013；原验证：V13, V17, V21。
| 原字段及原文 | 实际证据与判断 | 保留范围 |
|---|---|---|
| steps[0]：旧binary开新schema的受控重建、cache版本隔离、disable语义回退 | **原任务范围通过**；[GC8-PLATFORM] 原277f/schema24与Gc8/schema25实际受控25→24→25及backup恢复，cache版本/namespace隔离，default/semantic回退。 | 保持原真实binary/source；历史devbinary不是发行tag。 |
| acceptance[0]：回滚不误读新向量/丢用户源码，恢复步骤实测 | **原任务范围通过**；[GC8-PLATFORM],[G2] 原恢复实测保源码/config及cache隔离，当前schema/cache/offline方法实际通过；无新实现gap。 | 不将未授权live效果与费用当已恢复；必须先满足原012/013等依赖。 |
| acceptance[1]：相关旧功能回归通过；没有证据的项标not_run/blocked而非done。 | **原任务范围通过**；[G2] 当前同树原主CI与26项工程终态，加本项上列原source的已接收组件回归。 | 不是用总体绿灯替代上列每条步骤；旧not_run不改名，新未完研究不计通过。 |

自身范围验收通过；实际状态依上列原硬依赖顺序更新，不由准备/文档存在自动完成。

## P8-017 删除临时兼容和重复模块
原硬依赖：P8-016；原验证：V18, V21。
| 原字段及原文 | 实际证据与判断 | 保留范围 |
|---|---|---|
| steps[0]：清临时旧branch/重复评分器/多份schema来源 | **原任务范围通过**；[OWN] 统计、lane目录、DDL单一所有者已收口，历史18重复tests/helper删除且17其余保留；当前11owner路径OID与原独审相同。 | 必要compat/native契约与test-only独立参考并非必须删除的运行时重复。 |
| steps[1]：保留必要外部wire兼容 | **原任务范围通过**；[OWN],[G2] 原wire/mode/schema/参数流兼容控制保留，当前真实stdio/default-offline等对应控制实际通过。 | 四个prepare-only mechanism variants未编译，不计执行通过。 |
| acceptance[0]：运行路径只有一个事实与算法所有者，删除有回归证据 | **原任务范围通过**；[OWN],[G2] 原ownership/deletion证据加当前相容回归支持自身scope；PR198ownership说明已合并。 | 仍须016实际关闭及本项正式决定，不能仅因文档存在就done。 |
| acceptance[1]：相关旧功能回归通过；没有证据的项标not_run/blocked而非done。 | **原任务范围通过**；[G2] 当前同树原主CI与26项工程终态，加本项上列原source的已接收组件回归。 | 不是用总体绿灯替代上列每条步骤；旧not_run不改名，新未完研究不计通过。 |

自身范围验收通过；实际状态依上列原硬依赖顺序更新，不由准备/文档存在自动完成。

## P8-018 文档事实与安装契约同步
原硬依赖：P8-017；原验证：V18, V21。
| 原字段及原文 | 实际证据与判断 | 保留范围 |
|---|---|---|
| steps[0]：从schema/capabilities生成可核查事实 | **原任务范围通过**；[DOCS],[G2] 当前actualp8_facts--check：errors=[]、declared_facts passed；8crates/schema25/30base+5FTS/14tools和默认配置明确。 | runtime_certified=false保持；设计声明不升级为所有运行场景认证。 |
| steps[1]：更新安装/故障/默认离线说明 | **原任务范围通过**；[DOCS],[G2] 原installer56+CLI5、当前配置保留/错误拒绝和stdio离线方法通过；PR198三doc确切blob仍在main。 | PR198十五表清单修正不改变14MCPtools；没有live或未知安装包认证。 |
| acceptance[0]：文档表数/schema/工具数不再漂移，设计与已实现标识分开 | **原任务范围通过**；[DOCS],[BASELINE],[G2] 已证明的14→15表文字漂移已修；原贡献脚本Wde14固定b9真实exit0，3历史DRIFT与TEST_PLAN历史表定义核对。 | 不以exit0代表nodrift。206vs142、2815vs1258、76vs15是明确历史基线比较；后两为footer执行汇总，非unique/allfeatures。历史表/ENOSPC不覆盖，无当前falsefact需再修。 |
| acceptance[1]：相关旧功能回归通过；没有证据的项标not_run/blocked而非done。 | **原任务范围通过**；[G2] 当前同树原主CI与26项工程终态，加本项上列原source的已接收组件回归。 | 不是用总体绿灯替代上列每条步骤；旧not_run不改名，新未完研究不计通过。 |

自身范围验收通过；实际状态依上列原硬依赖顺序更新，不由准备/文档存在自动完成。

## P8-019 发布工件与完整报告归档
原硬依赖：P8-018；原验证：V01, V04, V21。
| 原字段及原文 | 实际证据与判断 | 保留范围 |
|---|---|---|
| steps[0]：归档精确binary/checksum/manifest/raw/gates | **原任务范围通过**；[ARCHIVE],[G2] 原精确binary/checksum/manifest/raw/gates保全/no-clobber控制齐；当前42archive命名控制实际ok，原实现OID一致。 | caller声明不等build证明，verify_archive0仅完整性，不是指标或release通过。 |
| steps[1]：latest只指向run | **原任务范围通过**；[ARCHIVE] latest只在原条件下指向run，失败、未完整、竞态/无关文件都受原控制。 | 不是全球发布channel或文件系统WORM承诺。 |
| acceptance[0]：报告可重算，历史原始run不被下一轮覆盖 | **原任务范围通过**；[ARCHIVE],[DOCS],[G2] 3原native replay命名控制在当前两配置实际通过；复制独立单run再replay写derived文件的说明已合入PR198，原run不被后轮覆盖。 | 重复default/HTTP不算6unique；不在原归档上写，suite父目录不是single--run；数值差异/失败不因verify0而抹掉。 |
| acceptance[1]：相关旧功能回归通过；没有证据的项标not_run/blocked而非done。 | **原任务范围通过**；[G2] 当前同树原主CI与26项工程终态，加本项上列原source的已接收组件回归。 | 不是用总体绿灯替代上列每条步骤；旧not_run不改名，新未完研究不计通过。 |

自身范围验收通过；实际状态依上列原硬依赖顺序更新，不由准备/文档存在自动完成。

## 结果范围与完整程序边界
新研究是预先独立登记的描述性N1：8变更profile×5规模、5fanout。五cold点是40setup的预选子集，不重复计入85；每环境分列，不声称稳定tail、N30统计或跨host因果提速。阶段计时含嵌套，不相加制造总时长；vectors disabled/countnull、过程资源snapshot与费用未测均保持原边界。
原006条款由全变更/超预算闭包、phase计数、显式status及闭包后的15表完整parity与相关回归逐条满足；09-D4完整程序的真实repo等其他范围仍按原任务记录。旧P8-002/003真实repoquality/replay exit1不作G2增量phase替身。旧150/1500与1350研究的失败/未完、完整V20/G8和语义认证没有变成通过。
007–016旧组件保留G275/Gc8/另次4d18等各自source/run，当前G2源码相容及工程回归不是把它们重跑或搬到本次N1研究。017–019保其原owner/wire/facts/安装/归档/replay证据，不能用未编译机制准备替代通过。
最终仅在独立fresh-main视图更新十二项status/evidence/implementation_notes和原允许导航，保全192定义/顺序/依赖及其他180整记录，原code_index_plan --write/check各一次生成同五文档。阶段A九项173/19/session10，阶段B三项176/16/session13；P8-020只作为下一导航，不是发布通过。
CONTRIBUTING适用性：仅任务进度及真实证据，不改测试/语料/schema基线或TEST_PLAN历史表。旧baseline0+3历史DRIFT与当前G2facts0分别按实际source保存，不冒称将来文档PR执行；实际diff如出现基线数字变更需单独处理。

## 精确证据索引

### G2

```json
{
  "description": "当前固定研究/代码与main同树；原主CI及全部26工程检查/soak",
  "source": "f97c5068d056969705e8387ba1f37d83847f5bee",
  "main": "0da9a2800d2b3112d584a8bad4f79785e01162a9",
  "tree": "39a053ae49b486764751a570d164e7cd41d7ff29",
  "main_CI_run": 38025451919,
  "main_CI_job": 114135224079,
  "main_log_git": "b42f2f685f4f2caaaa0497db6706399d9f89d998",
  "main_review_git": "a1c7594462f7d33ea7ebcb8998a46d0e3163086a",
  "soak_run": 38025451807,
  "soak_job": 114135223703,
  "terminal26_review_git": "52e0f0c71349f509244c72dc4428604d88aaf488",
  "terminal_publication_commit": "6fffd5c5bcbf80df47066660a48fb5964463db73"
}
```

### S45

```json
{
  "description": "45完整原样本及独立完整45原函数聚合恢复通过；原Actions42失败独立保留",
  "source": "f97c5068d056969705e8387ba1f37d83847f5bee",
  "run": 38026411200,
  "attempt": 1,
  "profile": "profile_task_descriptive_v1",
  "build_job": 114138076763,
  "build_review_git": "767527f7cccf372e6ca1dd866466d6bf66c86da3",
  "build_publication_commit": "3f5928a6acf8172fb04e0c7f5474846b45a0dd20",
  "api100k_intake_git": "99600080f8851fddde2c9c5b3d2c2f6b4f4f5cd9",
  "api100k_peer_git": "aa37eea73e8a77a9b1391fa88fe3e1e85fe8feba",
  "current_cells": 45,
  "current_records": 85,
  "final_aggregate": {
    "bytes": 19662526,
    "sha256": "307900563e166677db09721100179f8d885ea5e57acb643876a2abbbcac51fca",
    "actual_exit": 0,
    "passed": true,
    "publication_commit": null
  },
  "original_Actions_failure_review_git": "e6ab7248a27e731171f0d741499486c59ea11bb5",
  "full45_recovery_peer_git": "d9b9bb0b332e14793405c8848a2ab1eac1c60835"
}
```

### G275-MIX

```json
{
  "description": "原mixed+独立backfill明文步骤",
  "source": "275e8799d4947d297329073eaa3ca675d3fd0777",
  "run": 37890757030,
  "mixed_artifacts": [
    11598875346,
    11598690899,
    11598815621,
    11598543621
  ],
  "backfill_artifact": 11597519732,
  "review_git": "8fc2ad32460594dd060347e960d06660397bd30c",
  "publication_commit": "67574177439009bdbe75751a4e8fe0c2d1737399"
}
```

### LOCK-G2

```json
{
  "description": "另一次实际DB acquisition四并发组件，与当前研究G2不是同source",
  "source": "4d18dcdb34b5d7a277566b57801cc882d8b1eb61",
  "run": 37930392164,
  "review_member": "complete-four-C-cohort/complete-G2-four-C-independent-review.json",
  "publication_commit": "3548736f0dd42304e6b5f06f40007a756df6a361"
}
```

### GC8-LIFE

```json
{
  "description": "原cold/reopen/uncached/cache-hit分层与资源组件",
  "source": "c8be5afaac568ffd40ef86d3795423c3b73c9f39",
  "runs": [
    37901930818,
    37901930897
  ],
  "artifacts": [
    11604870469,
    11604162920
  ],
  "publication_commit": "da00e2802bd9304de698b263be9924e2dc96242f",
  "accepted_map_publication": "b66afc3a4967d7047b038fa8e6f9e20118018b5c"
}
```

### GC8-SOAK

```json
{
  "description": "原一小时修改/分支/catalog/cache和终点15表",
  "source": "c8be5afaac568ffd40ef86d3795423c3b73c9f39",
  "run": 37901930897,
  "artifact": 11609019055,
  "publication_commit": "b66afc3a4967d7047b038fa8e6f9e20118018b5c",
  "support_git": "20777261354d804b7811672cee454c0803949306"
}
```

### G275-FAULT

```json
{
  "description": "五类故障步骤的精确原件映射",
  "source": "275e8799d4947d297329073eaa3ca675d3fd0777",
  "run": 37890757129,
  "artifact": 11597639035,
  "review_git": "bf56de9bbc5a3250aee70f946fbed3ff7860dee9",
  "support_git": "a9d5be4f0172af5b74e88bc433e8708eab9e3082",
  "publication_commit": "67574177439009bdbe75751a4e8fe0c2d1737399"
}
```

### GC8-PLATFORM

```json
{
  "description": "8个全新target平台/工具链/feature格与回滚原件",
  "source": "c8be5afaac568ffd40ef86d3795423c3b73c9f39",
  "run": 37901931021,
  "collector_artifact": 11605154697,
  "recovery_artifact": 11604900056,
  "publication_commits": [
    "56cf243df673a0745644ff17351353c8e416d349",
    "16ce2a5623b51b6dc4ae23df4912f939791fe7d8"
  ],
  "support_git": "7e347fd2391070c506fbbf39334a85357bdaf88d",
  "old_schema_binary_source": "277f2490fad3fa30f2812b5547bad033867c9ea5"
}
```

### GC8-GATES

```json
{
  "description": "6原Rust failure控制及7原compare CLI非零结果",
  "source": "c8be5afaac568ffd40ef86d3795423c3b73c9f39",
  "CI_run": 37901930820,
  "gate_run": 37901930843,
  "gate_artifact": 11603892263,
  "publication_commit": "56cf243df673a0745644ff17351353c8e416d349",
  "support_git": "dd75ae4b9faaddf4a66a1b6f78c83051c5c173eb"
}
```

### OWN

```json
{
  "description": "所有权/删除/必要wire兼容原审；当前11owner叶OID与原记录相同",
  "deletion_source": "b069c73a0d76736e1dfb3eb64e79cad6980b773f",
  "lane_source": "fa6562ad70c6a6f2a7a1e65594df23f807e62f18",
  "original_owner_review_git": "0e5fd5de6599825486e60cea567572c8010fd23a",
  "complete_claim_map_git": "e863b8b3fe42a5fe3797fac251f48f158362087d",
  "current_scope_report_git": "30bc665e25e0e4983c2a91e30b0a3bbb848e8606",
  "current_peer_git": "1664f89789179c5b690247e3542ef26721cbf408"
}
```

### DOCS

```json
{
  "description": "已合PR198三文档；当前facts和安装控制",
  "PR198_merge": "09d4454fa45455dc5a8bfdfec67e31bb7ed162c7",
  "benchmark_doc_git": "39513e0fa62eeab815e0c503594846d48b75c8e6",
  "ownership_doc_git": "487fef20517cac0b1ebab12a6ff10669ac476bba",
  "release_evidence_doc_git": "a42b65ef36c155cfefd7894a06faf9338db4fb63",
  "installer_original_source": "51138c236cf2ed47858e2306f9c7bfd3600aaef9",
  "installer_original_log_git": "907b9a33cb22d7a13b947e4fac6aaa121c594d55"
}
```

### BASELINE

```json
{
  "description": "原脚本真实执行与历史DRIFT解释",
  "source": "b9b089bb4eae072affe9326681d4980eae15fd84",
  "workflow": "de14e6c7e80524efb0e43bec74bb48f518cfbc67",
  "run": 38020782259,
  "attempt": 1,
  "job": 114121097102,
  "review_git": "565e1c18e24a382c43f18f8bf9ecef8873d4b4d0",
  "peer_git": "c1a9c241793a4c29e411249a5ea0af6dbca56462",
  "publication_commit": "5f6814a0c0ce5c11f692d8c09023694754d89ea3",
  "actual_exit": 0,
  "DRIFT": 3
}
```

### ARCHIVE

```json
{
  "description": "原archive/no-clobber/latest和native replay",
  "archive_source_blob": "79c8ab476d1c9fb17a190a1c280f82c614a5dc1a",
  "archive_tests_blob": "855d03942af118ebdbfef2506a437e9a2b313ec7",
  "native_report_blob": "8d633bd75772e2ef4cfead559688a6fea4ca6644",
  "native_replay_tests_blob": "e2ec2f623b9bbe44e0c0f7d10688d7301129127b",
  "original_scope_review_git": "383b475fed125e507bf1add78c1b7d287ceb029e",
  "current_scope_peer_git": "1664f89789179c5b690247e3542ef26721cbf408"
}
```
