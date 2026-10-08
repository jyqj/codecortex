# 历史证据归档：第二批9个PR

本交付完整保留 #106、#109、#112、#115、#116、#118、#120、#126、#133 的固定证据差异，共 **596 个原文件、55,294,523 字节**。所有原文件按原路径、`100644` 模式和原 Git blob 加入，另增本目录7份归档说明、索引、清单及审查记录；完整新增集合为603项。

唯一父提交固定为实际 main `78e194574c25fc636e96c82b34abd89ab31452cd`，tree `d351dc5197b4c206300935583ae7e1836f9819b9`。不带入旧分支的产品祖先历史，不重写原JSON/日志/压缩包/法律文本，不将原失败、拒绝、未运行或规范未裁定改成通过。#137包含crate测试，未纳入本批。

| 原PR | 固定head | 文件数 | 字节数 | 原范围及必须保留的结论 |
|---|---|---:|---:|---|
| [#106](https://github.com/jyqj/codecortex/pull/106) | `b53c85e2822a42acf802e3df25266fca9b492253` | 8 | 965,865 | 完整保全v24迁移传输包；有限10迁移/20no-op/2旧reader等原通过范围保留，2474/4/68全特性旧结果与GC/WAL/live/heldout/100k未覆盖边界不变。 |
| [#109](https://github.com/jyqj/codecortex/pull/109) | `0c1e0b3a1b21620aa8f10abf4f48342b8f0458d8` | 38 | 389,611 | 固定v2 point-in-time API的bounded pass；跨平台/一般性能/100k等未覆盖。原upload-manifest的verdict摘要矛盾原样保全，不能视为原清单全通过。 |
| [#112](https://github.com/jyqj/codecortex/pull/112) | `199558c754aae7f8d1dba0ee73256a2ed11a4230` | 84 | 6,414,739 | 固定v2的有限backfill阶段/合成SQL诊断；PR108原100k失败不变，不能把1k/5k份额或VMsteps改进外推为真实100k性能验收。 |
| [#115](https://github.com/jyqj/codecortex/pull/115) | `c2b731840d0051c18d1dca316574da063e7ef909` | 61 | 287,955 | FIFO有限正确性审查；旧版与候选共同direct-writer new.rowid失败保留，不是全路径通过、性能接受或生产集成批准。 |
| [#116](https://github.com/jyqj/codecortex/pull/116) | `9a4f97877f1b18af77013f7bed7cd047994e26f3` | 111 | 19,887,275 | 单次配对baseline/candidate均未过300s ready；post-ready仍not_run，deadline计数与cleanup尾段分开，无统计显著性声明。 |
| [#118](https://github.com/jyqj/codecortex/pull/118) | `5ee2a75a9a41281aa4250735cb8fa44f9b752558` | 69 | 398,262 | 固定DirectWriter boundedpass及6个case groups；旧base失败与三个限定mutant失败保留，不外推一般swap/crash/GC/WAL/100k保证。 |
| [#120](https://github.com/jyqj/codecortex/pull/120) | `c6e0d13811ad2d315dfc3c797f170ab61c68567f` | 13 | 52,393 | 冻结parallel drain为REJECT/HOLD；普通NeedsRetry留下ready未开始行、width0续调回归及共享gate blocker保留；反例测试通过不等于候选通过。 |
| [#126](https://github.com/jyqj/codecortex/pull/126) | `da8b618cbf8b67683c6b16880419089f7159c15e` | 188 | 26,428,588 | 单次配对两版硬gate失败；baseline超时、candidate原12GiB资源guard提前终止，非完整300s吞吐对照，post-ready仍not_run。 |
| [#133](https://github.com/jyqj/codecortex/pull/133) | `6c1416109003bcff0c1911307a4af5bd48870517` | 24 | 469,835 | 公开DEV kind的规范决策仍未裁定；未建立原gold或parser错误，无relabel/新admission/accuracy/holdout声明，9份法律文本逐字保留。 |

## 完整性核对及历史矛盾

完整Git三点差异与父线程提供的API全分页清单逐项一致：各PR `changed_files`、路径、added状态和Git blob全部一致。596项均为artifacts新增普通文件，与固定main及本批其他PR无路径冲突。原SHA256SUMS共438/438项匹配：#112为82、#115为60、#116为109、#126为187。#133的artifact清单23/23及法律文本清单9/9匹配，CRLF法律文本保留原字节。

**#109存在原清单矛盾，归档保留而不修正。** `upload-manifest.json` 的36项中35项匹配；`verdict.json` 声明2227字节、SHA256 `a4a175f596af72443604e758835b81a860295c8cc0d6dee5e9c65986df02a9f2`，固定head实际2333字节、SHA256 `3555405d33665574c7d48eda1f373d1d1d1bb3d9b73a44208cb7f43bdec18c02`。这两份原文件均直接引用固定head的blob；不能说原上传清单全部通过，也不据此改写原bounded pass或新增接受结论。新的归档manifest独立绑定实际保留的原blob。

#106保留原8文件传输布局。独立只读重组五个原ZIP分片，核对每片/整包摘要、ZIP唯一安全路径/CRC/长度，以及内95文件的路径、2,942,466总字节、SHA256和Git blob；未执行materialize或任何包内代码、未向工作树释放包内文件。此核对不声称公开传输树与原本地Git tree相同。

逐项实际身份见[manifest.json](manifest.json)；完整原README/PR body解释、摘要结果和三点差异证据见[完整差异审查](reviews/complete-fixed-delta-audit.json)；[归档范围独审](reviews/archive-batch9-independent-review.json)记录本次解释边界。没有执行历史脚本、产品、矩阵或provider。本次作者负责准备payload，另需父线程独立复核完整树后发布。

## 上一批实际处置

#162已合入main `78e194574c25fc636e96c82b34abd89ab31452cd`。原#105/#108/#124在实际main的190原件逐项核回、固定head刷新后，于原记录时间关闭（均未合并旧分支）。原[merged-main独审](previous-162/merged-main-independent-readback.json)与[关闭结果](previous-162/closure-outcome.json)逐字附存，包含评论ID和观察时间。原8项CI在当时仍queued的边界保持，不据此新增CI通过声明；原分支、commit和CI保留。

## 本批旧PR的关闭前提

本准备文件不请求自动关闭9个原PR。待独立归档PR实际合入后，先从实际main读回596个原path/blob/mode；再刷新每个原head及完整差异，确认没有新的未审内容；最后手动说明“证据已归档、原失败/not_run/REJECT及原清单矛盾均保留”，链接归档PR和合入commit。分支、commit、CI历史均保留。

历史checkpoint内的局部TODO不是项目原任务。**本次原TODO新增完成数0；中央192项中163完成、29剩余。** 归档也不代表被评产品修复、性能门槛或发布验收已完成。
