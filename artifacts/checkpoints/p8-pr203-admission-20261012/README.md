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
