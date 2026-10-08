# 43 个历史 PR 的固定复核

本轮起点 `main@47d1939e43455ecd72ccc73e80825d971e3f33e7` 上，**43 个历史开放 PR 全部为 draft，新增可严格自动关闭数为 0**。这是 PR #148 建立前的历史队列快照，不能用作其后的实时开放数量。

[完整逐项记录](audit.json) 保存每个 head/base、祖先关系、完整差异路径分类及摘要、未匹配原提交和处置理由。43 个 head 均与既有审计索引相同；用本轮起点 main 重新计算后，43 项仍为 `retain_requires_scope_review`，路径分类计数与旧整合源一致。

## 判定方法

严格关闭需要满足至少一个完整条件：head 已为 main 祖先；整个 PR delta 的路径 mode/type/blob 与 main 一致且删除项仍不存在；或所有独有非 merge 提交的非空原始 raw delta 都在 main 历史中采用，且没有未计入的独有 merge。局部相似、旧 CI 成功或部分证据已采用都不足以自动判定整个 PR 已完成。

本附件重用原 `AuditGit` 的只读计算方法，明确将比较对象设为当前记录的 main。没有修改旧 139 项索引及其固定锚点验证器，也没有把新的比较冒充为旧固定验证器通过。Git 对象读取禁用 replace objects 与 lazy fetch；GitHub 元数据复用本会话先前实际读取，不重复抓取 43 项。

## 优先处理建议

| PR | 未采用范围 | 固定旧 head 的 CI | 建议 |
| --- | --- | --- | --- |
| [#127](https://github.com/jyqj/codecortex/pull/127) | bounded publish group API；1 演进、7 缺失 | 成功 | 按当前 CAS/锁/失败恢复边界单独迁移；仍未接生产 queue |
| [#133](https://github.com/jyqj/codecortex/pull/133) | kind taxonomy；24 缺失证据路径 | 成功 | 保留 owner 字段契约裁定，不改写 gold/质量结论 |
| [#137](https://github.com/jyqj/codecortex/pull/137) | native-zero/qname；18 缺失路径 | 默认回归失败，后续 skipped | 保留诊断与原失败，绑定当前源码后审查 |
| [#3](https://github.com/jyqj/codecortex/pull/3) | 旧索引与回放；32 演进、13 缺失 | security 失败；其他已列 gate 成功 | 按意图拆分移植，避免直接引入旧 schema/benchmark |
| [#4](https://github.com/jyqj/codecortex/pull/4) | 48 演进、21 缺失 | CI/quality 成功 | 先审独立差异与演进冲突，不能直接当作已采用 |

[优先项证据](priority-review.json) 给出全部固定 head/base、workflow run ID 和明确范围。#146/#127/#133/#137/#3/#4 的 submitted/inline review 在当时回读均为空；仓库内独审工件另行解释。

## 已合并 PR 与原 TODO 计数

下表按每个实际 merge 的第一父提交比较原台账，避免把叠加分支的旧 base 或“推进”计为新增完成。

| PR | 此次 merge 新增完全完成的原任务 | merge 后未完成 |
| --- | --- | ---: |
| #143 | 0 | 42 |
| #144 | P7-011、P7-012 | 40 |
| #145 | 0 | 40 |
| #147 | P7-013 | 39 |
| #146 | 0 | 39 |

起点台账为 **192 = 153 done + 21 in_progress + 17 todo + 1 blocked**，因此未完成为 **39**。#145 与 #146 的推进记录和证据有价值，但各自 merge 的新增 done 均为 0；本轮至少完成十项的目标须继续按原验收判断。

此审计子代理没有合并、关闭、评论或改写任何 GitHub PR，也没有修改任务台账。后续任何处置都应先回读 live state/head，再对当时 main 重新核对采用关系。
