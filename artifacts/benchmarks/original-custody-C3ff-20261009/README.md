# C3ff 原始 Actions 工件保全

本目录完整保存固定源码 `3ffcefc3b28ee1a4ed80caecebd7208a45c3e302` 已验收运行的 **25 份主要原始 Actions ZIP，共 467,676,992 字节**，以及后来取得的官方平台汇总 ZIP 1,494 字节。合计 26 份、467,678,486 字节。`catalog.json` 是完整索引。

主要 ZIP 使用 236 个连续原字节块保存。没有重新打包、重新压缩、删除成员或重跑测试；逐请求记录、原数据库、日志、夹具和实际执行二进制都保留。每块记录长度、原偏移、SHA-256 与 Git blob OID。25 份原 ZIP 的完整摘要均与官方值核准；每个远端上传对象均匹配计算出的 Git OID。

## 范围

| 原运行 | 主要原件 | 对应已验收范围 |
| --- | ---: | --- |
| 37908825814 | 6 | C1/C4/C8/C16 mixed、controlled backfill、1 小时 soak |
| 37908825715 | 1 | 生命周期与缓存分层 |
| 37908825722 | 17 | 8 格平台的 16 原 ZIP，以及 recovery/version-pair |
| 37908826034 | 1 | 故障 gate 与失败退出 |

后到的 `11617153706` 是运行 37908825722 的官方平台汇总，只增加 metadata 原件，不增加平台样本或 native 执行。独立对照验证了共同 collector payload；原 CLI 额外四个字段单独保留，没有把两份完整 JSON 说成逐字相同。

## 恢复一个原 ZIP

在检出包含本目录的固定提交后，从仓库根目录运行：

```sh
python artifacts/benchmarks/original-custody-C3ff-20261009/restore_original_zip.py artifacts/benchmarks/original-custody-C3ff-20261009/manifests/11609403178.json --output /tmp/11609403178.zip
```

更换 manifest 即可恢复另一主要原件。命令逐块校验 SHA-256、Git OID 和长度，再校验整 ZIP；已有输出不会被覆盖。它不访问网络、不提取 ZIP、不运行内部程序。官方汇总小 ZIP 已直接保存在 `supplemental/official-platform-collector/`。

恢复工具已独立实际验证：以原 backfill ZIP 的四块恢复 7,835,686 字节，与原 ZIP 逐字一致；已有输出拒绝且不变；一位损坏的测试块被拒绝且没有残留输出。完整命令、输出和仅自有测试文件的清理记录见 `transport-review/`。

## 固定来源和边界

`manifests/<artifact-id>.json` 固定官方 run、artifact ID、原源码、ZIP 大小与摘要及验收引用。`source-receipts/` 保留制作输入和脚本；`upload-receipts.json` 记录实际对象绑定。`catalog-checkpoint-01.json` 与其发布收据记录首件保全的历史状态，`catalog-planned.json` 是原始计划，均保留不改。

custody manifest 里的 review publication_status 是原制作输入清单时的快照；相应 `artifacts/benchmarks/<run-id>/` 导航给出后续已发布报告的固定提交。报告解释各自的源码、样本和平台范围；原件保存不扩大原验收，不把不同源码的规模样本合并。

大型原件只放在独立分支 `evidence/p8-originals-c3ff-a217-20261009`，代码 PR 的 benchmark 导航引用固定提交。本次保全不产生新的 native 执行、统计样本或 TODO 完成数。当前原任务仍 192 项、163 done、29 未完成；完整规模研究与其他尚缺原件的验收继续进行。
