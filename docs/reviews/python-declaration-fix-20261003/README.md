# Python 声明类别与 decorated class 修复

基于 PR132 `fd5146baf5a62f977ce5ed4682c16f485aef9da3`。独审来源
PR134 `fb8ca8ae3b2cebf7d08255d37032158daafa63f2`；其原证据目录
`../qname-parser-public-20261003/` 逐字保留。这里的新运行不覆盖旧失败证据。

实际声明节点决定 class/function 分派；函数提取器自身拒绝 class。
装饰 class 只产生一个 wrapper 坐标的 Class，真实 body、nested class、
method 及 decorator 表达式分别遍历一次。qname、UID 与最近 lexical parent
的 canonical symbol ID 一致；Python 未产生 scope catalog，保留空 scope_id。
Boundary 只有在 AST 声明类别与名称兼容时才接受 hint，拒绝后仍可尝试
inner-node hint，最后由 AST 决定类别。保留合法 Method 与其它语言的原分类。

本修复未改 scorer、gold、生产预算、Cargo.lock、schema 或 DB 写入逻辑。
仅处理声明与 source-bound 身份真实性；不关闭 V19 或 P7 整体任务。

原公开红例要求 class 的 qname 必须缺失。在正确的完整 class proof 出现后，
这一缺失断言会继续失败；原测试与运行失败保留，新公开测试额外要求正确
Class/Method、qname、exact source bytes 和 SQL 存续证明，不能以丢弃身份代替修复。
