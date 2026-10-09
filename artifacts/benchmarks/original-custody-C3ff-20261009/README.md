# C3ff 原始 Actions 工件保全

本目录保存已经验收的固定源码 `3ffcefc3b28ee1a4ed80caecebd7208a45c3e302` 原始 Actions ZIP。每个块都是原 ZIP 的连续原字节；没有重新打包、删去成员、重新压缩或重跑测试。每块有 SHA-256、Git blob OID、长度和偏移，重建后必须等于 GitHub 原始 ZIP 的 SHA-256。

## 当前完整范围

不可变的 `catalog-checkpoint-01.json` 仅确认生命周期工件 `11609403178`：49,230,392 字节、24 块；运行 `37908825715`。完整原件包含逐请求记录、数据库、日志和实际执行二进制。此工件在 Actions 上记录的到期时间是 2026-10-30T10:10:17Z。

初始计划共 25 件、467,676,992 字节；本 checkpoint 不把尚未保全的其他工件计入完整范围。后续目录清单会明确增加的工件。上传本身不产生新的 native 执行、通过样本或 TODO 完成数。当前原任务仍为 192 项、163 done、29 未完成。

## 恢复原 ZIP

在检出含本目录的固定提交后，从仓库根目录运行：

```sh
python artifacts/benchmarks/original-custody-C3ff-20261009/restore_original_zip.py artifacts/benchmarks/original-custody-C3ff-20261009/manifests/11609403178.json --output /tmp/11609403178.zip
```

命令校验全部块、整 ZIP 的长度与摘要；已有输出不会被覆盖。它不访问网络，不提取 ZIP，不执行内部二进制。恢复出的 ZIP 与原工件完全相同，可交给原有只读验收工具。

## 来源和验收边界

`manifests/<artifact-id>.json` 固定原 Actions run、artifact ID、源码提交、官方原 ZIP 大小和摘要，以及原始验收引用。里面的 review publication_status 是制作输入清单时的快照；后续 `artifacts/benchmarks/<run-id>/` 导航提供已发布评审的最新固定提交定位。原件保全不扩大相应报告的验收范围，不把历史来源混入当前规模研究，也不意味着完整 P8、G8 或发行认证。

大型原件位于独立分支 `evidence/p8-originals-c3ff-a217-20261009`；代码 PR 的 benchmark 导航引用固定提交，不重复嵌入整份原件。
