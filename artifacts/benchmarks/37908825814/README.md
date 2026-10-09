# C 原运行 37908825814：四档混合负载、受控 backfill 与一小时 soak

本目录导航至 [原 Actions 运行 37908825814](https://github.com/jyqj/codecortex/actions/runs/37908825814) 的已接受原件与独立报告。实际执行源为 `3ffcefc3b28ee1a4ed80caecebd7208a45c3e302`，tree `0e655b4258afd9ca040dd7c1ace868bf1c322a30`；报告不改标为后续 main、M4 或锁等待诊断来源。

**任务账仍为 163/192 完成、29 剩余，本次新增关闭 0 项。** 本目录不修改原定义、硬依赖或状态。

## 已接受范围

- 四档原 mixed 各保留 900 个 offered/terminal 结果，配置并发 1/4/8/16；实际峰值见各独立报告，不将配置值当实测重叠。
- 另有原 768 请求受控 backfill，以及 3601 个终态、3600.044958014 秒的原 soak。
- soak 的 2400 个 compound read 保留 9600 次 RPC；四个时间区间各 600 read，原 cache 命中/未命中/失效分别为 1400/1000/999。
- 原 Rust statistics、纳秒 wire 等值、15 表 oracle、完整 seal、owned cleanup 的独立复验已通过。

## 已发布的独立报告

完整 SHA256、字节数、Git blob、容器 SHA256 与成员路径均列在相邻 `manifest.json`。下列链接使用固定提交；容器里的报告保留原字节。

| 报告 | 固定提交 | 容器内成员 | 原报告 SHA256 |
| --- | --- | --- | --- |
| independent-review.json | [95c7144abe8d](https://github.com/jyqj/codecortex/blob/95c7144abe8d8d0c4fe94e126727863319ffd579/artifacts/checkpoints/p8-round29-a217-20261009/round29-public-evidence.tar.gz) | `runtime/C-backfill/reviews/independent-review.json` | `9e0cd7c8d7d71e283bb2b3d719be1c95c4d9c00cbbf88b4512c457685f12258f` |
| independent-review.json | [0be18dd59785](https://github.com/jyqj/codecortex/blob/0be18dd597850880ad488e9e6978cfc92c6107ff/artifacts/checkpoints/p8-round30-a217-20261009/round30-public-evidence.tar.gz) | `runtime/mixed-c16/replay/independent-review.json` | `b64895f461d7d49ddc01858c70167bd1f28259558b8cc73a2e39af7630f02d98` |
| independent-review.json | [0be18dd59785](https://github.com/jyqj/codecortex/blob/0be18dd597850880ad488e9e6978cfc92c6107ff/artifacts/checkpoints/p8-round30-a217-20261009/round30-public-evidence.tar.gz) | `runtime/mixed-c1/replay/independent-review.json` | `4796d4a34fab446ccacb15b6beefa26f8de7c4327953bf3c4b247e05616c99f9` |
| independent-review.json | [0be18dd59785](https://github.com/jyqj/codecortex/blob/0be18dd597850880ad488e9e6978cfc92c6107ff/artifacts/checkpoints/p8-round30-a217-20261009/round30-public-evidence.tar.gz) | `runtime/mixed-c8/replay/independent-review.json` | `733d508a6b3aa21340f8d9559198b4b5ba401fc98104412372b983317815eb19` |
| independent-review.json | [0be18dd59785](https://github.com/jyqj/codecortex/blob/0be18dd597850880ad488e9e6978cfc92c6107ff/artifacts/checkpoints/p8-round30-a217-20261009/round30-public-evidence.tar.gz) | `runtime/mixed-c4/replay/independent-review.json` | `2b3414f730b0f5b918b4121a00a76418dbc0e49e307d496ac24b1ef8ff0f7c5e` |
| independent-review.json | [8a8cc951d6c1](https://github.com/jyqj/codecortex/blob/8a8cc951d6c198d0ea28c01eaaaaf12df903ab77/artifacts/checkpoints/p8-round31-a217-20261009/round31-public-evidence.tar.gz) | `runtime/C-soak-11613497404/accepted/artifact-11613497404-3ffcefc3-independent-final/independent-review.json` | `6a6cc801c38a2a0ca6655b516f72a1ebffcb27b6b6bc6b0a83f18402a9565e39` |

## 原始 ZIP 与长期保留

**原始 ZIP 的完整长期保留已固定并核验于提交 `1d56747eafcef6c22ceaec949fabe30a3362862c`；所有原字节块已发布。** 保管分支为 `evidence/p8-originals-c3ff-a217-20261009`，路径为 `artifacts/benchmarks/original-custody-C3ff-20261009`。独立报告已经发布，与此原始 ZIP 运输状态分开。

每份 ZIP 的原 Actions ID、完整摘要、大小、实际到期时间及分块 manifest 摘要见下表和 `manifest.json`。保留方案不排除任何 ZIP 成员，也不重新打包 ZIP。

| Artifact ID | 原范围 | ZIP 字节数 | 原 Actions 到期时间（UTC） | ZIP SHA256 |
| --- | --- | ---: | --- | --- |
| 11610619483 | controlled-backfill | 7835686 | 2027-01-07T09:03:32Z | `eb07f5f3719018799dbe6bfd305e391be26b716872ed59cbb3bd0463fa1489b0` |
| 11611524778 | mixed-c16 | 16365126 | 2027-01-07T09:03:32Z | `1c62e498c2c15b5f67090f8e2ca4ee77a01c7c9c57ea3c8a00077fd113b5a2d6` |
| 11611743908 | mixed-c1 | 16564970 | 2027-01-07T09:03:32Z | `6b8718eaba251bb71e77cd16fabb49a7e82cc5b68b79cbc0d4c811d5c9a4135b` |
| 11612256751 | mixed-c8 | 16600500 | 2027-01-07T09:03:32Z | `ae80133510856df3cf08cfd20c3e335caf53dff543aa2fb1ed65421b12eb2138` |
| 11612539681 | mixed-c4 | 16943374 | 2027-01-07T09:03:32Z | `6d7e8f61bef70baea0d746e7e2fc87e0d026db107d289e8d9cbf7a8dfd4d57ea` |
| 11613497404 | soak | 50994781 | 2027-01-07T09:03:32Z | `18a7e50472313bfcd1290810894e8ef3e9e0f18c476bd927e6a840ce8ecb8a1f` |

完整原始 ZIP 已固定并验证于 `1d56747eafcef6c22ceaec949fabe30a3362862c`；在该提交的 checkout 根目录执行以下恢复示例；输出文件必须不存在，目标父目录须已存在。恢复程序逐块核 size/SHA256/Git blob，最后核完整原 ZIP 的 size/SHA256。这个命令只恢复原文件，不启动 benchmark。

```sh
python3 artifacts/benchmarks/original-custody-C3ff-20261009/restore_original_zip.py \
  artifacts/benchmarks/original-custody-C3ff-20261009/manifests/11610619483.json \
  --output /tmp/11610619483.zip
```

可把示例 ID 替换为本目录表中的另一 ID。原始 custody manifest 内的旧报告发布状态标签是保留的元数据快照；本目录 `acceptance_reports` 提供后续 R28–R31 固定报告定位，不改写原文件。

## 范围限制

- 所有记录只属于 C 的实际原二进制、feature、环境与协议；不与旧 173、E 或独立 LG/G2 诊断样本合并。
- soak 配置并发 4，实际串行峰值 1；证据不代表持续四路饱和，也不提供稳定 p99 或性能提升结论。
- 内存结论保持原采样与增长规则的范围，不声称连续全进程树峰值或统计上的无泄漏证明。
- backfill 使用受控本地 provider；不代表 live-provider、付费账单或 G8 批准。
- P8-007 的实际 DB acquisition 等待补充由单独的默认关闭诊断来源处理；本目录不替该待验收项声明完成。

本导航仅读取已有小报告、公开归档清单和原件 manifest；没有重新计算测量统计、重新读取大 ZIP、执行 native 负载或触发新的 CI。

完整保管绑定已核：主25 ZIP 467,676,992B，加后到官方collector原ZIP 1,494B，总26 ZIP 467,678,486B。该发布只完成原件持久交付，不把原任务状态或尚未完成的规模/观察工作改为通过。
