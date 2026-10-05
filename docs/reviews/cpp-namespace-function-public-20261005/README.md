# C++ namespace 自由函数与持久身份修复

命名 namespace 中已支持的未限定普通函数（含模板和嵌套 namespace）现在标为 Function，并从 AST 生成完整 namespace qname。kind、qname 和 symbol_uid 一起更新；源码坐标、symbol_id 和短 container 保持不变。class/struct 方法和既有限定名定义的行为不在本次改写范围。

调用者按精确源码范围匹配。涉及这些 namespace 函数的调用，只有已有源码证明时才保留目标；无法证明的目标保持 Unresolved，目标 ID/UID 为空。负向结果在普通解析、类型回填、增量依赖重解析和数据库重开后保持，避免短名相同就绑定到另一个 namespace。

数据库语义版本升至 **26**，对应能力声明同步为 26。旧索引需要从源码重建，即使文件内容未变，也不能继续使用旧 Method UID。

## 验证

- Rust 1.95：208 个 parser 测试和 6 个持久化/生命周期测试通过；parser 严格 lint 与格式检查通过。
- 独立复核：564 项 parser/edge 检查通过，单独验证跨文件拒绝、真实依赖重解析、重开，以及由旧解析器输出建立的 v25 索引在源文件未变时重建并移除旧 UID。该检查数不与作者测试数相加。
- 未宣称全工作区或整体检索质量验收。

## 保留限制

缺少跨文件 namespace owner 证明时保守不绑定，即使调用文本带有限定名；这会降低相关调用图召回。完整 qualified/type owner 解析、嵌套类遗漏、friend/匿名 namespace 等仍待单独处理。

当前 **v4 source registry 保持原固定来源**。本修复需另行显式注册到后续 source integration；本次不更新 registry、CI 或既有质量/规模认证。P7/V19 等父项保持开放。
