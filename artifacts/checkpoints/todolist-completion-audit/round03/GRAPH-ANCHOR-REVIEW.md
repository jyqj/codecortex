# GraphLane 拆块映射独立审查（开发期，非验收）

## 确认的真实缺口

旧 GraphLane 按 `chunk.start_line <= symbol.start_line && chunk.end_line >= symbol.end_line` 寻找最小整符号容器。P4正确把长函数切为有界statement/block文档后，正常长符号就不再有一块完整覆盖，从而 `graph_source_unmapped`。这是正式P4→P5链路缺口，不只是top-k库存限制，更不是抽象自然语言miss，可/应修通用机制。

仅按start_line找最小块会错绑同一行兄弟函数、nested/container同名符号；必须保 UID/column/current source identity，不能为了消除Partial任意挑一个chunk。

## 当前方向与未结项条件

owner新增 `symbol_uid_anchor_chunk(uid, HardScope)`，在SQL直接按binary UID并在LIMIT前scope过滤，复用既有exact byte-position verifier读取symbols.start_line/start_col，首次line prefix从真实有界源码块重建绝对byte位置；候选span必须包含该位置、同snapshot、稳定排序。GraphLane改用真实UID锚点，不伪造whole-function source chunk。

这方向可以进入开发负例验证，但**尚不代表G5或所有source身份/正文义务完成**。以下必须由实际源码与回归证明：

1. UID而非同名替代，same-line sibling不同column、same-line不同container同名、nested parent/child、重复文件、UTF-8/CRLF/注释或decorator坐标。
2. 全HardScope交集和Some(empty)，在admission/limit前过滤，邻接seed/callee/witness关系无scope泄漏。
3. prefix source及candidate source snapshot一致；候选有当前DocKey/DocVersion/manifest，最终hydrate/disk/generation验证不省略。陈旧编辑/rename/delete/corrupt manifest不能把旧anchor当新事实。
4. 锚点是声明定位，不等长函数正文或整调用链证据。最低task110span和独立多facet/graph source witness义务照常审计，budget/omission状态真实。
5. GraphLane已有fusion-only/不annotate旧契约保持。新红测试最初要求`graph@` reason不合契约，应以实际graph LaneOutcome.candidates→fused byte-valid source核验来源，不为新测试改旧产品reason/score输出。
6. 原seed/neighbor/candidate限制仍以Partial记录，不能删除graph_source_unmapped或把未映射条件改Success。typed error与absence/cancelled区别保留。

## 成本/并发重点

旧实现批量加载symbol rows与file chunk spans；新每UID点查询+prefix decode可能退化N+1。已有20 seeds、每方向每seed10 neighbors，最坏约420 UID，且HashMap迭代顺序不固定。需batch UID-authoritative read model、跨UID共享file/line prefix、稳定处理/返回、deadline/取消检查和真实SQL/decode/内存上限证明；不能提前soft截断来伪称完整。

正式V20必须覆盖高fanout/长函数/长行，配对成本/尾延迟和所有失败，不拿三个小函数graph fixture替全范围证据。20%性能回退仍阻断适用profile，不用“映射更正确”偷换未解释的成本门。

实际开发红绿、最终batch/identity实现与hash还待owner提交。所有后续证据按实际时间记录；旧收据日期不改。
