第28轮已完成可审的 main 保全候选，暂不移动本 PR head。

候选分支 [integrate/p8-main-preservation-28fe-20261009](https://github.com/jyqj/codecortex/tree/integrate/p8-main-preservation-28fe-20261009)，提交 [4652cad11dde4b41126544a38fddf25eb2fb7474](https://github.com/jyqj/codecortex/commit/4652cad11dde4b41126544a38fddf25eb2fb7474)，tree `6737ba0b700344853b6bce17cb3ee4bd792363d5`；父顺序为当前 G3 `3ffcefc3…` 与 main `fd9ca5db…`。两名非作者及主线程都已核实际 Git 对象。相对 G3 只有 7 个文档修改和 20 个历史工件新增，无删除或历史覆盖；1092 产品输入、139 验证输入及 G3 原 registry/guard pins 全部不变，#175/#179 已合代码均保留。

原 lifecycle `37908825715 /113748746643` 在09:56:59 UTC复读仍 queued。其 workflow 使用同 PR 的 `cancel-in-progress: true`，现在更新 #180 会自动取消这次原运行，因此候选先独立保存，待原运行终态后再根据实时 main/head 接续。scale workflow 只监听 workflow_dispatch 和 PR labeled，不监听 synchronize；没有为这个候选新开 PR、加标签、派发或重跑规模研究。

G3 原研究 `37910924354` 的 build 已 success，50k 原步骤09:49:56开始，其余容量格仍待原结果。这不等于完整研究通过。候选 M 自身没有构建或测量；将来引用 G3 证据时，保留它的原二进制、编译路径、源码 HEAD/tree、工具链、环境及收据身份，不能改标为 M。

上一轮完整证据在 [06964cd](https://github.com/jyqj/codecortex/tree/06964cdc931c3614e0487b69d195051155e9961a/artifacts/checkpoints/p8-round27-progress-28fe-20261009)。原任务账本仍 192 total /163 done /29 remaining；本轮新增完全完成0项，至少完成10项的目标继续推进。