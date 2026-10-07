# 修正前定义的 lexical boundary contract

该规则只决定既有 `rather than` / `instead of` 比较短语是否应阻止 contextual-owner hint。不是自然语言意图模型，不修改 QueryTarget、DSL/filter、kind、ranking 或旧 single-word comparison helper。

1. 从原 free-text 的 Unicode 字母/数字/下划线连续区间提取 maximal word spans；不做 CamelCase、sigil、qualifier 或内部 punctuation 清理。匹配词仅 ASCII case-insensitive `rather`+`than` 或 `instead`+`of`。
2. 两个 span 必须相邻；中间必须是非空 Unicode whitespace，或紧跟第一词的一个 ASCII comma 再接非空 whitespace。其他 punctuation、wrapper 切换、代码符号或中间词不属于该 inter-word delimiter。
3. 记录匹配的 `()`/`[]`/`{}` grouping 及直/弯单、双引号/backtick 区间，每个区间有独立身份。两个 word 必须有相同的 context stack。wrapper 可以覆盖 pair，也可以覆盖其 target/article/完整子句；嵌套 grouping 按同一规则处理。分别包裹的 word、跨 wrapper 的 word 不共享 context。
4. 紧邻 identifier/sigil/closing-expression 的 opening group 是代码 context；紧邻 closing group 的 call/index、identifier 或 qualifier suffix 也使该 context 为代码。任何 code ancestor 都阻止 prose phrase。词紧邻 call/index opener、sigil、qualifier、escape 或非 prose boundary 也不是 prose word。一个 matched quote/backtick 中的纯 phrase 保持既有 positive；其 code-shaped body 不被清理成 phrase。
5. 普通 word boundary 只包括源开头/结尾、whitespace、comma/句末标点以及实际匹配的 opening/closing group 边界。相邻 code characters 不剥离。引号内 escape 保留其 code 边界；word 内 apostrophe 不作为 grouping。未闭合或错配的结构不提供 phrase evidence。
6. 只有所有 source contexts 完成验证后才判断 pair，避免在随后发现 call suffix/未闭合 context 之前过早认定 phrase。

测试覆盖交叉积：两个 phrase、case、Unicode whitespace、prose grouping 类型、nested grouping、closing placement（pair/完整 clause）；两个 word 的 independent quote/group/sigil/call/qualifier/identifier decorations、inter-word punctuation、malformed/code grouping。保留全部历史 query/expected/scorer，明确新增独立 grouped-clause contracts。不存在按个别查询添加的例外列表。

执行范围沿用任务限制：Rust1.95/default features/original Cargo.lock/至多2 build jobs；只聚焦 cc-search tests 与自写 synthetic engine/in-process MCP；不运行被禁止的 semantic runtime test 或可启用它的 suites、private exports、DB GC/WAL/kill/EROFS、production endpoint；无 registry/CI/TODO/PR/main merge/deploy/force。

准备阶段只删除旧 validation 的 Cargo target caches（非交付物）；历史 commits、archives、raw results、receipts、source files、driver binaries 保留。旧 receipts 中的 rlib 路径现在需要重建，hash 仍是原运行证据；不伪称 cache 仍存在。
