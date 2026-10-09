# C 原运行 37908825722：八格平台冷构建、恢复与版本回滚

本目录导航至 [原 Actions 运行 37908825722](https://github.com/jyqj/codecortex/actions/runs/37908825722) 的已接受原件与独立报告。实际执行源为 `3ffcefc3b28ee1a4ed80caecebd7208a45c3e302`，tree `0e655b4258afd9ca040dd7c1ace868bf1c322a30`；报告不改标为后续 main、M4 或锁等待诊断来源。

**任务账仍为 163/192 完成、29 剩余，本次新增关闭 0 项。** 本目录不修改原定义、硬依赖或状态。

## 已接受范围

- Linux/macOS × Rust 1.95/stable × default/semantic 八个原 fresh release/stdio 单元已逐原件接受。
- 每格 portable v2 与对应 full raw 的 lineage 字节一致；原 collector 已独立实际执行并产生 passed 8 / failed 0 / not_run 0。
- 恢复工件原 84+55 条检查已接受；保留当前 schema 25、旧 277f2490 二进制 schema 24、当前 25 重建及原 backup 25 恢复的真实版本顺序。
- 原源/config 完整性、原缓存格式拒读与既有 fault-test 对应证据、回滚后 disabled local 可用性均以原报告的精确范围为准。

## 已发布的独立报告

完整 SHA256、字节数、Git blob、容器 SHA256 与成员路径均列在相邻 `manifest.json`。下列链接使用固定提交；容器里的报告保留原字节。

| 报告 | 固定提交 | 容器内成员 | 原报告 SHA256 |
| --- | --- | --- | --- |
| platform-C3ff-cell-review.json | [8a8cc951d6c1](https://github.com/jyqj/codecortex/blob/8a8cc951d6c198d0ea28c01eaaaaf12df903ab77/artifacts/checkpoints/p8-round31-a217-20261009/round31-public-evidence.tar.gz) | `platform/complete-eight/review/platform-C3ff-cell-review.json` | `d8dba40033b5ebc5e0a47f6ca0e277646e38eb0ceedeb72a018d7ed7c3aa5e8b` |
| recovery-C3ff-review.json | [95c7144abe8d](https://github.com/jyqj/codecortex/blob/95c7144abe8d8d0c4fe94e126727863319ffd579/artifacts/checkpoints/p8-round29-a217-20261009/round29-public-evidence.tar.gz) | `runtime/C-recovery/reviews/recovery-C3ff-review.json` | `cead9d468065c80158b4c30abe98ec542b43f45d8b2d204397c99d26b9ca0eb6` |
| recovery-C3ff-011-review.json | [95c7144abe8d](https://github.com/jyqj/codecortex/blob/95c7144abe8d8d0c4fe94e126727863319ffd579/artifacts/checkpoints/p8-round29-a217-20261009/round29-public-evidence.tar.gz) | `runtime/C-recovery/reviews/recovery-C3ff-011-review.json` | `bdf214a68fe4c82af9fef3a6e84aea68e8520acb3379ced171b177f5622d909a` |
| independent-collected-matrix.json | [8a8cc951d6c1](https://github.com/jyqj/codecortex/blob/8a8cc951d6c198d0ea28c01eaaaaf12df903ab77/artifacts/checkpoints/p8-round31-a217-20261009/round31-public-evidence.tar.gz) | `platform/complete-eight/review/independent-collected-matrix.json` | `f093fe520784c76d5cf81c3c3dce1acff1d213dbd7361bc98119c668562fb98d` |

## 原始 ZIP 与长期保留

**原始 ZIP 的完整长期保留已固定并核验于提交 `1d56747eafcef6c22ceaec949fabe30a3362862c`；所有原字节块已发布。** 保管分支为 `evidence/p8-originals-c3ff-a217-20261009`，路径为 `artifacts/benchmarks/original-custody-C3ff-20261009`。独立报告已经发布，与此原始 ZIP 运输状态分开。

每份 ZIP 的原 Actions ID、完整摘要、大小、实际到期时间及分块 manifest 摘要见下表和 `manifest.json`。保留方案不排除任何 ZIP 成员，也不重新打包 ZIP。

| Artifact ID | 原范围 | ZIP 字节数 | 原 Actions 到期时间（UTC） | ZIP SHA256 |
| --- | --- | ---: | --- | --- |
| 11609383875 | platform-portable-cell | 8037941 | 2027-01-07T09:03:32Z | `91edc8357bd2f58fd4292c1e637a587170cb9dc7ba697f795dd5c51812873116` |
| 11609723220 | platform-full-raw | 8126202 | 2027-01-07T09:03:32Z | `afa2445e4186b31bef9f54f29fe47f154f7b77e1b9229704b917c095c6255cf6` |
| 11609808293 | platform-portable-cell | 8361627 | 2027-01-07T09:03:32Z | `cea726d7c61e7c81280b8221102d0f96494234620b3dbd87800d9193365e885e` |
| 11609853328 | platform-full-raw | 8449381 | 2027-01-07T09:03:32Z | `c6960de65dbb04c5bb956214bd96cb196053b1bea8e6dd964203d9a144af2986` |
| 11610185903 | platform-full-raw | 8700816 | 2027-01-07T09:03:32Z | `7039ac68aeee4d48cd40ffa79dbd9145e7a4ac85ef42aee07ca8ec6f16efdd6f` |
| 11610850143 | platform-portable-cell | 8613033 | 2027-01-07T09:03:32Z | `dbb9bbd09280511c4aecc4832307acf0b58a3485ae52e6eeba7caf150b6052e4` |
| 11610971397 | platform-portable-cell | 8278183 | 2027-01-07T09:03:32Z | `a18336861a68eee003d8eea84bb39f5e197948c1a28f10186b2a16e2b10a192c` |
| 11611246104 | platform-full-raw | 8366467 | 2027-01-07T09:03:32Z | `5d85e73919d060ab80df0528f080a602110325de821488bcd087066e57f5cdbb` |
| 11611995300 | recovery-and-version-pair | 121801440 | 2027-01-07T09:03:32Z | `cedbfc80ae8df8ce29c4c8478535e2af855cda143a5dfa6454aeebeb4ea808e4` |
| 11612174906 | platform-portable-cell | 9031835 | 2027-01-07T09:03:32Z | `60b57be8cf0470c3357ab6ea9ff8738877394abf2fd7c6b22a5a84dfcc4945d7` |
| 11612296390 | platform-portable-cell | 8976904 | 2027-01-07T09:03:32Z | `7823901a81d7d9fac4be71a12834a93d6110aec6c2f156f44ef0c0c450d11d54` |
| 11612451345 | platform-full-raw | 9064425 | 2027-01-07T09:03:32Z | `3ebe09aafb695ec346418ef7a531f170fc024c67265c392bbac9cdea0092a02b` |
| 11612759454 | platform-full-raw | 8865644 | 2027-01-07T09:03:32Z | `c5754f9d3ec69a6dd15c41577263f7de6ddb6e095d19eaa08283778f1ab2af67` |
| 11612849803 | platform-full-raw | 8815264 | 2027-01-07T09:03:32Z | `546ade1636b0c99a856cbc91259b5be6fa86502452f20c409250129e3ac25e50` |
| 11612935410 | platform-full-raw | 9119846 | 2027-01-07T09:03:32Z | `71a96a793f7dc056a7bfd4c920a031d46e77c231e5a9d1d4f30f48fdbcf7c6df` |
| 11612952575 | platform-portable-cell | 8777661 | 2027-01-07T09:03:32Z | `f34db922ae00c97b0a0d259289b68f7e5390bed29ab8c771f30733b1764c035a` |
| 11614026675 | platform-portable-cell | 8727770 | 2027-01-07T09:03:32Z | `8b872926a757b66629ee43d3d43dee9bb8e12b4ea0c3d3ecc23ae9183ced5a24` |

完整原始 ZIP 已固定并验证于 `1d56747eafcef6c22ceaec949fabe30a3362862c`；在该提交的 checkout 根目录执行以下恢复示例；输出文件必须不存在，目标父目录须已存在。恢复程序逐块核 size/SHA256/Git blob，最后核完整原 ZIP 的 size/SHA256。这个命令只恢复原文件，不启动 benchmark。

```sh
python3 artifacts/benchmarks/original-custody-C3ff-20261009/restore_original_zip.py \
  artifacts/benchmarks/original-custody-C3ff-20261009/manifests/11609383875.json \
  --output /tmp/11609383875.zip
```

可把示例 ID 替换为本目录表中的另一 ID。原始 custody manifest 内的旧报告发布状态标签是保留的元数据快照；本目录 `acceptance_reports` 提供后续 R28–R31 固定报告定位，不改写原文件。

## 范围限制

- 八格仅认证选定包的 fresh release build 与 stdio smoke，不能据此声称八格完整 workspace 测试通过。
- macOS 实际 runner 为 26.6.2/25G83、image 20260907.0351.1；没有记录具体 OS SDK 版本，不能外推任意 SDK 环境。
- 原 Mac fresh-helper 250ms 的历史 101 与 cold 起因未知保持；prepared-helper 的已接受前置修正不重标旧失败。
- 恢复是原受控场景与真实版本对的有限演练，保留原 not_run、loopback、disabled/null 和 no-release 边界，不作无限跨平台故障认证。
- 官方 collector 后续汇总与本地原 collector 验收分开记录；官方排队或执行中的状态不是新增任务验收门。
- 官方 collector 已于 12:58:05 UTC 成功完成，原 run 共 10 个 job 全 success。后续原汇总 ZIP `11617153706`（1494 字节，SHA256 `9f8f8d1b853009998f1e151d3a8c6d8ca5eb64ea279a30ba756499cdaf5b1dd0`）与独立原 collector 的共同 payload 只重定位 `source.source_root` 后完全相同。官方 CLI 另带四个原 envelope 字段（exit_code、expected_commit、input_directory、runner_sha256），已各自按原源码/命令/日志核实；不声称两个完整 JSON 仅有一个字段差异。该后到小 ZIP 是第 26 个 metadata 汇总原件，独立列明，未混入前述主 25 ZIP 清单。

本导航仅读取已有小报告、公开归档清单和原件 manifest；没有重新计算测量统计、重新读取大 ZIP、执行 native 负载或触发新的 CI。

完整保管绑定已核：主25 ZIP 467,676,992B，加后到官方collector原ZIP 1,494B，总26 ZIP 467,678,486B。该发布只完成原件持久交付，不把原任务状态或尚未完成的规模/观察工作改为通过。
