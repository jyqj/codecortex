# 历史失败证据归档：PR #105、#108、#124

本次归档完整保留三个固定 PR 的证据差异，不改原失败结果、未运行阶段、源码身份、阈值或中央 TODO 状态。190 个原文件合计 **31,040,993 字节**，按原路径、原 Git blob 和 `100644` 模式加入；另外新增本目录的五份迁移与独审文件。

归档基线是 main 提交 `7354db236c9d9850a75f31672697ae9eab44565e`，tree 为 `e76c3828d585d7084a52e85c2c2e19a6f51c2b76`。归档提交只以该 main 为父，不引入旧证据分支的祖先历史。原 blob 直接引用，不重新压缩或改写原 JSON、日志、脚本和文档。

| 原 PR | 固定原 head | 原文件数 | 原字节数 | 原意及保留结论 |
|---|---|---:|---:|---|
| [#105](https://github.com/jyqj/codecortex/pull/105) | `7210cee93243621693418e6f26374557d9f30832` | 4 | 46,978 | 四失败静态诊断；2474 passed /4 failed /68 ignored 保持不变，后续诊断建议不视为已解决。 |
| [#108](https://github.com/jyqj/codecortex/pull/108) | `99973e7d6faf3809add4a12cd444f8e69f04d93c` | 58 | 9,383,961 | 单次100k观测未过300秒ready门槛；cleanup tail不计通过，post-ready阶段仍为not_run。 |
| [#124](https://github.com/jyqj/codecortex/pull/124) | `c32a08a591e4fb1e9d457eee91b2703727b3d619` | 128 | 21,610,054 | 单组100k配对两版均失败；保留后续解释纠正，不声称统计显著性、正确性或性能验收通过。 |

逐文件原 PR、固定 head、路径、模式、Git blob、SHA-256 和大小见 [manifest.json](manifest.json)；迁移范围和关闭前提见 [migration-index.json](migration-index.json)。本目录五个新增文件加上190个原文件构成唯一预期新增集合，共195项。

## 已做的独立范围检查

完整 Git 三点差异与原 API 全分页清单一致；190/190 原 Git blob 匹配，全部为 added-only、正常文件，位于三个独立 artifacts checkpoint。固定 main、集成M和当时发布head均无目标路径碰撞。原 SHA256SUMS 中 #108 的56/56项及 #124 的127/127项核对通过；其余原文件同样由固定 Git blob 和独立SHA-256绑定。没有运行归档中的脚本或产品、重跑规模矩阵、调用provider，或把字节核验当产品认证。

两份原独审报告逐字保留：[归档试点结论](reviews/archive-pilot-independent-review.json)、[完整固定差异审查](reviews/complete-fixed-delta-audit.json)。其中其他历史checkpoint、固定commit或本地排除材料的引用保持原样；本归档不制造原本未留存在Git的数据库、二进制或观测。

## 原 PR 关闭前提

1. 单独的归档 PR 合入 main；不能因这份准备清单而提前关闭原 PR。
2. 从实际合入后的 main 提交读回190个原路径，逐项核对原blob和100644模式，保存独立回执。
3. 再次刷新原PR head和完整差异；若不是本清单的固定head，应重新审查新增变化。
4. 关闭说明链接归档PR和合入commit，明确“证据已归档，原失败和not_run保持不变”。
5. 保留原分支、commit和CI历史；不删除分支、不改写旧证据，不增加原TODO完成数。

本次仅整理已保留的证据提案。归档中的checkpoint完成清单不等同于项目原TODO或产品gate完成；#105的诊断后续建议和#108/#124的失败状态仍保留。
