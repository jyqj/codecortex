# 第 19 轮 PR 管理与规模原件读回记录

本检查点以 main b21cce4c8661589267ad5719f850accbec088d2f 为固定基线，只增加审计材料和原归档的对象引用。

## PR #160

独立审查允许按 superseded 关闭，原因是后继 A23 已由 #167 合入 main，而 G4 的源码历史和全部 69 份 checkpoint 文件已保全。该结论不是把 #160 标记 merged，也不授予 P8 TODO 验收。

G4 为 260f596582f2d82b8d7c707b61a6b8b6a43b069f。refs/heads/task/p8-round15-evidence-20261009 仍指向 aba11b861aee65e48b58b7fe8ac2c81564f91029；官方 compare 证明 G4 是其祖先，ahead 6 / behind 0。原 checkpoint tree 为 0fb28d8384302b9ce04dbd7c76a5de39f375c03b，69 文件共 67,054,093 字节。本提交还把完整原 tree 引用到 ../pr-evidence-archive-160/originals/ 新目录，保留每份文件的 mode/blob，无重压和覆盖。

main 同路径只有 6 份原件逐对象相同，其他 63 项不同或缺失，其中 62 项非空；这不被描述为原 69 份已全部存在于原 main 路径。完整差异、各固定 ref 和关闭条件见 pr160-preservation-review.json。关闭动作必须另取实际 API 回执；本检查点创建本身不代表动作已经执行。

## PR #168 与 #3 / #4

#168 已由外部操作关闭，并由 #169 的一行类型修补后继承接。原 E0282 编译失败仍保存。其 33 份原 Actions artifacts 中，2 个关键 ZIP 已有 Git 副本，其余 31 个仍依赖各自原保留期；详见 pr168-supersession-review.json，不能声称已全部永久复制。

#3 和 #4 继续保持 open/draft。现有路径和演进代码不足以证明全部旧范围已被语义替代，两份正文及现 main 已保留此决定。完整 48/70 个 PR 文件清单和当前路径对应关系见 pr3-pr4-management-review.json。

## A23 50k 原分片

从本地恢复的 independent-a23-50k-shard-review.json 是此前已实际完成的原 validate_shard 读回报告：固定原 ZIP 11593548201、build ZIP 11591482043；9 个测量，511 对 build_started/build_finished，原 report wall 4,214,047 ms。它没有执行新测量，也没有执行 validate_build checkout gate。完整 150 分片研究仍需各自真实终态；本文件不把单分片扩大为完整规模认证。

原任务清单保持 192 总项、163 done、29 未完成，本轮原 TODO 新关闭 0。PR 整理、修补、审计和归档均不计入至少十项原 TODO 的完成数。
