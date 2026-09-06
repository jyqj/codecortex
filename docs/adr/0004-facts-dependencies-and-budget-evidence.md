# ADR-0004：代码事实、解析依赖和预算证据

状态：提议；继承 ADR-0003，不引入 embedding 或新的 Rust 依赖。

## 问题

一份正确的源码命中可以附带错误的调用边。旧 JS/TS/Python regex 会把函数声明、
注释和字符串当成调用，再把同名匹配标为 parser_exact。旧 chunk→symbol 的
`name == name || contains(span)` 也允许外层容器抢先匹配。检索质量全绿不能代替
事实正确性；增量/全量相等也可能是两边犯同一个错。

## 决策

1. JS/TS/Python 的调用点和标识符引用由 AST 节点产生；提取事实的存在性与目标
   解析分离。局部遮蔽、歧义和动态下标是终止的语法未解析状态，resolver 与
   dirty reload 不把它们升级成无关全局目标。保留真实递归；不采用“删除所有自环”。
   框架路由/事件提取仍独立，不代表框架关系已经获得编译器级语义证明。
2. 具名源码片段先按名字+相交区间匹配；无名字时用最小完整容器。相同区间的不同
   身份保持歧义。一个符号的所有返回片段获得图分，邻接读取和附加节点单独去重。
   每个 JS declarator 有独立 id/区间，不能共享整条 const 声明的 id。
3. ES import 按实际 default/named/namespace binding 记录；显式导入失败不得退回
   不相关的全局同名。重导出追踪有 cycle guard 和 128 次预算；冲突/不完整路径
   保留未解析。相同 import-distance 的候选不再按插入顺序任选一个。
4. `lookup_dependencies` 与调用图分开，写入和删除与文件事实处于同一事务。
   保存调用/引用的未解析、全局和启发式名字桶，以及模块查找。候选增删触发反向
   失效；文件拓扑改变保守重访所有模块查找。导出指纹为空不证明 barrel 没变。
   所有依赖仍共享 dirty closure 的既有预算，不静默无限扩张。
5. commit 阶段写 `resolution_freshness_v1`。超预算/解析失败留下 incomplete，
   no-op 不清除；成功 full build 才恢复已经 incomplete 的状态。complete 仅指
   本索引契约内的更新完成，不证明静态语义完备；查询前源码又变或 watcher 丢事件
   仍需上层扫描/刷新。暂不承诺自动 drain 队列或通用 snapshot isolation。
6. Grep 与 lexical/graph 一样只服从调用者 hard scope。预选只做排序先验。
   返回每 lane 扫描量、候选限制、预算耗尽与读取错误；空结果也带诊断。
   工作预算耗尽/读错误的结果不缓存。查询截断采用有界可解释的 token 优先级，
   而不是简单保留句首。保留人工排序默认值，不在回归夹具上伪装概率校准。
7. graph_features=false 是完整图消融（预选、lane、重排、富化）。零 lane 权重
   关闭候选贡献，不让零分候选在后续人工 boost 中复活。
8. 事实、定位、源码证据、增量一致性分别验收。源码证据按冻结源码逐行核验再做
   区间并集；真实 token 计数的 handler 成本与离线 normalized-prefix adapter
   分开。已返回名字/行号不代表正文已可见。

## 迁移和代价

Schema 8 触发一次全量重建，以清除旧伪调用并初始化查找依赖。新增存储/保守
传播/无预选剪裁 grep 可能增加成本。trace_candidates 默认关闭：完整候选位置
需要额外读取，最多记录 512 个 union 候选，截断标志明确；不得将开启 trace 的
延迟与关闭 trace 的延迟混为同一观测。

## 尚未覆盖的证明义务

不是类型检查器：Python global/nonlocal/comprehension、复杂 JS 求值时序、
运行时反射、动态 import、复杂多继承/泛型、其他语言解析器和框架推断仍需独立
扩展金标。lookup_dependencies 当前不覆盖所有派生语义边或任意配置决策。
错误降级可见不等于不存在证明。小型闭世界夹具没有跨仓库置信区间。
真实仓库 held-out、编译器交叉核验、ranker 校准和 agent 成功率是另外的实验，
不能用本轮单元测试/回归报告冒充。
