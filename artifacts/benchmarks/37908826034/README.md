# C 原运行 37908826034：指标、故意失败与非零退出门

本目录导航至 [原 Actions 运行 37908826034](https://github.com/jyqj/codecortex/actions/runs/37908826034) 的已接受原件与独立报告。实际执行源为 `3ffcefc3b28ee1a4ed80caecebd7208a45c3e302`，tree `0e655b4258afd9ca040dd7c1ace868bf1c322a30`；报告不改标为后续 main、M4 或锁等待诊断来源。

**任务账仍为 163/192 完成、29 剩余，本次新增关闭 0 项。** 本目录不修改原定义、硬依赖或状态。

## 已接受范围

- 六个原故意失败 case 及保留的原 CLI 产物已独立核对；原 73 条验收断言保留。
- 门禁覆盖不可测延迟、零测量、质量/延迟失败、inconclusive、锁失败及无效 policy 的机器可读失败与原 raw 保留。
- 原 CLI compare 重放按固定源和原工件校验，不重新执行故障负载。

## 已发布的独立报告

完整 SHA256、字节数、Git blob、容器 SHA256 与成员路径均列在相邻 `manifest.json`。下列链接使用固定提交；容器里的报告保留原字节。

| 报告 | 固定提交 | 容器内成员 | 原报告 SHA256 |
| --- | --- | --- | --- |
| review.json | [1f7e6816f165](https://github.com/jyqj/codecortex/blob/1f7e6816f1656b330cafc88f1a34a40d0d4c6086/artifacts/checkpoints/p8-round28-a217-20261009/round28-public-evidence.tar.gz) | `platform-gates/platform-gates/gates-review/review.json` | `0e3367c58365731ae5f6d6a9ca13b3052e1e79284cad01920203527ba361969e` |

## 原始 ZIP 与长期保留

**原始 ZIP 的完整长期保留已固定并核验于提交 `1d56747eafcef6c22ceaec949fabe30a3362862c`；所有原字节块已发布。** 保管分支为 `evidence/p8-originals-c3ff-a217-20261009`，路径为 `artifacts/benchmarks/original-custody-C3ff-20261009`。独立报告已经发布，与此原始 ZIP 运输状态分开。

每份 ZIP 的原 Actions ID、完整摘要、大小、实际到期时间及分块 manifest 摘要见下表和 `manifest.json`。保留方案不排除任何 ZIP 成员，也不重新打包 ZIP。

| Artifact ID | 原范围 | ZIP 字节数 | 原 Actions 到期时间（UTC） | ZIP SHA256 |
| --- | --- | ---: | --- | --- |
| 11608597487 | failure-gates-six-cases | 33027724 | 2027-01-07T09:03:33Z | `67b189e6890a20825e899e7b501a5358c153701af8290ffc8616d5340fb45f87` |

完整原始 ZIP 已固定并验证于 `1d56747eafcef6c22ceaec949fabe30a3362862c`；在该提交的 checkout 根目录执行以下恢复示例；输出文件必须不存在，目标父目录须已存在。恢复程序逐块核 size/SHA256/Git blob，最后核完整原 ZIP 的 size/SHA256。这个命令只恢复原文件，不启动 benchmark。

```sh
python3 artifacts/benchmarks/original-custody-C3ff-20261009/restore_original_zip.py \
  artifacts/benchmarks/original-custody-C3ff-20261009/manifests/11608597487.json \
  --output /tmp/11608597487.zip
```

可把示例 ID 替换为本目录表中的另一 ID。原始 custody manifest 内的旧报告发布状态标签是保留的元数据快照；本目录 `acceptance_reports` 提供后续 R28–R31 固定报告定位，不改写原文件。

## 范围限制

- expected nonzero、invalid 与 inconclusive 是这些负控应保留的结果，不能改写为性能测量通过。
- 本目录证明门禁的失败行为；不提供完整规模、稳定尾延迟、跨环境性能或发行批准。

本导航仅读取已有小报告、公开归档清单和原件 manifest；没有重新计算测量统计、重新读取大 ZIP、执行 native 负载或触发新的 CI。

完整保管绑定已核：主25 ZIP 467,676,992B，加后到官方collector原ZIP 1,494B，总26 ZIP 467,678,486B。该发布只完成原件持久交付，不把原任务状态或尚未完成的规模/观察工作改为通过。
