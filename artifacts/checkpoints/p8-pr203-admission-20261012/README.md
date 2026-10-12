# PR #203 源码准入与当前 TODO 复核（2026-10-12）

## 固定范围

- 原 PR head：797e9e60516e9bb8cc3c35f14a6ec7dc0092aa4f。
- 本轮 main 基线：c2b726538cacfdf6ae9f01977a7b93da48d2e2b2。
- 已独立审查的实现 P：0692ed27c4f86a254484e76155fe07a55ab2e09c。
- P 保留上述两个父提交。相对 main 只包含原 #203 工件保存修正，以及原校验器实际输出生成的 PLAN-CHECK 回执。
- 本目录的独立源码审查仅批准固定源码和验证输入。后续 G 必须安装实际 P/R 及摘要，并通过原校验器与新 head CI；本记录不批准合并或发布。

## 本轮原始 TODO 计数

192 总项；176 done、3 in_progress、12 todo、1 blocked；剩余16。
本次执行新增关闭 ID 为 []，新增数量0。相对既有163 done基线，#197和#202历史上共新增13项；这次复核不再记一次完成。
保留原192项定义、依赖、状态和证据。PLAN-CHECK 旧163记录留在 progress/plan-check.before.original.json；新回执来源是未修改的 code_index_plan.py 实际无参数执行，exit0。

## 原始验收复核的边界

独立审查检查45个原始 ZIP、118个分片和85个原测量对象；哈希、CRC、原 inspect_raw 谓词和已发布矩阵逐项匹配。
#202其他组件与#197 P8-005完成不可变文档、范围与托管元数据复核，没有在此重放全部历史实验。
N=1描述性任务范围不等于尾延迟、统计速度提升、完整V20或G8发行认证。历史失败、100k增量成本高于full的观察均保留。

## #203 的工件保存变化

checkpoint19继续本地冻结，并确认真实upload18；停止远端upload19。final20不接收虚构前序身份，携带所有未确认chunks。
两个独立模拟字节控制均验证成功：正常ACK18时final包含19+20；upload18失败时ACK仍为17，final包含18+19+20。仅有raw仍被原verdict拒绝，未声明native EOF。
固定fresh attempt完整预定上传数为45×22+build1+aggregate1=992。额外工件和重跑attempt另算；此项未实现API分页。final成功前的暂存尾部更长，runner丢失时仍只承认最后真实远端确认边界。

## 源码审查链

independent-source-review.json 是非作者 /root/pr_audit 的原始审查，绑定1096产品输入、原BASE的80个产品差异及157验证输入。产品输入逐项保持已审3481da0b的Git mode/blob身份；验证域只改变一份workflow。
peer-custody/ 保存另一名 /root/acceptance_audit 的精确P审查、源码哈希及实际合成夹具。
原v1至v14历史registry、BASE、FROZEN、guard算法、CI selector和性能门限保持。independent-binding-protocol-review.json 说明当前v15已有的P→独立R→固定G流程；不得用当前checkout自动摘要替代独立批准。
validation/before-admission* 记录实际原校验器拒绝旧registry未收录workflow的失败；capture-tests* 是3项原有无native夹具测试的实际通过记录。

## 尚未完成的验收

#203原head的真实CI仍是18success/1failure/2skipped。原CI job114164870834中benchmark_fixture失败：context warm p95 1.72s >500ms；完整表格还有index4.23s、search659.53ms。日志按原样保留；源码审查不解释其性能根因，也不覆盖该失败。
当前G8原检查器实际exit1/not_accepted。local17项直接前置均已在台账完成；semantic仍缺P7-018和P8-015。progress中的G8 manifest明确为空记录、仅做元数据缺口准备；其缺口不表示全仓没有历史记录，556文件摘要也不是完整release输入证明。
P8-020须继续完成选定发布范围的真实V01/V18–V21及独立发行验收。P9-001..012仍受原依赖阻塞；P7-018/P8-014/P8-015仍需各自实际授权和预算。没有执行新native规模实验、在线provider或holdout，未选择发布范围。

本目录只记录已经发生的复核与准备。G的原源码校验和新head CI应另以实际执行证据补充。

## 最终绑定与实际原校验（2026-10-12 补充）

独立审查 R 为 `72723404303125d4dca201b27139cdc4089dfff9`，精确绑定 G 为 `f631350e07841164c5d27f23769483ef71aadea4`。G 只修改两份 v15 绑定文件；四个 guard 常量和 registry 的 product/review/摘要/已审 workflow leaf 被精确替换，算法、历史批准、CI 和门限未变。独立 AST、完整原字节替换、Git mode/blob 检查见 final/。

第一次在 G 上运行完整 guard 因浅层部分克隆缺少 v14 固定历史 blob 而 exit1；原日志留在 validation/after-admission.*。补齐原件并核实当前受保护文件后，2026-10-12 07:14:50–07:24:21 UTC 实际运行同一命令成功，exit0：

```sh
PYTHONDONTWRITEBYTECODE=1 python scripts/verify_reviewed_source_v15.py --source-version p8-completion-source-20261009-v15
```

实际 stdout 为572 bytes，SHA-256 `1a8eab0f8f0a1b0cb934b08612e02a65395a14e3672359f54bf9487ca2414586`；stderr为空。它明确报告1096 complete_inputs、executed_previous_proof=v14、runtime_and_quality_claims=require_separate_execution_evidence。完整命令、时间、退出码及原 stdout/stderr 见 validation/full-admission-final.*。没有改写旧失败，也没有修改验证器来适应本地环境。

本补充提交只追加审查/执行/缺口证据和本说明，不再改动源码、原始TODO或验证输入。此补充创建时，新 head 的 GitHub CI 尚待实际执行；普通 PR CI 通过前维持 Draft/HOLD，旧797e的性能失败继续保留。

### 下一原始任务的实质缺口

progress/g8-existing-evidence-gap-matrix.json 直接检查已存在的历史记录。公开质量原 gate 1、后续 coverage diagnostic gate 2、Boltons原候选 gate 1均保留；旧holdout已使用，不可当作新候选的clean数据。f97 raw45可保留为同产品的N=1描述性证据；044 cold150与当前产品之间有实际实现变化，不能重标为当前release测量。

原100k raw显示bounded resume反复扫描/prepare/commit的调查方向：body/API/config累计490/171/330 builds、49.0M/17.1M/33.0M扫描次数，分别只parse1/1/0；数字为累计工作量，不是独立文件数，也不是已证明的安全优化。benchmark-failure/单独分析原普通CI的三个超限工具和缺失的逐case诊断；没有宣称性能根因或已修复。

全部原始TODO计数不变：本执行每轮新增关闭ID []，新增0；历史累计新增13；176/192 done、剩余16。不能把本PR、审查、修复、执行通过或文档计为原始TODO。
