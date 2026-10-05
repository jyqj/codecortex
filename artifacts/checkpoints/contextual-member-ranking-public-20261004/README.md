# Bounded contextual-member 排序修正

本分支的crate/Cargo/lock源码与已测试和独立接受的源码身份
`3a7633e9b4acaf343ca02bfe6542f600725d67ed`逐字相同。
发布提交仅表示源码等价，不声称该提交身份被直接执行。

## 修正范围

既有owner question grammar、完整bare member名字与direct indexed qname同时匹配时，
增加一次有上限的contextual-member软加分，并用独立score trace解释。有限否定/排除词
保守弃权，避免将without Commit中明确排除的member提升。API不推断成callable。
候选生成、硬过滤、rerank window和exact-target优先级保持不变。
实现与回归见`crates/cc-search/src/query_target.rs`、`plan.rs`和`engine_lane_tests.rs`。

## 已有验证结论

- Rust/Cargo1.95.0、原lock、default features。
- `cargo test --locked -p cc-search query_target --lib -j 2`：26 passed，0 failed/ignored；298 filtered未执行。
- scoped strict Clippy、fmt和source/module architecture checks通过。
- 独立冻结验证：780 paired observations、96否定控制、195direct probes/11,700 permits checks通过。
- 独立补充112项有限词表边界检查通过；cold/warm cache及0/.04/.18配置检查通过。
- 强lexical synthetic fixture及其重命名控制中，API目标Top1从0到1、MRR从.5到1。
  这不代表一般自然语言或公开语料的整体质量验收。

此前候选在without Commit负控中出现错误提权；修正后该负控恢复基线。
既有失败未被改写为通过。192项TODO状态不变，无父任务关闭。

## 证据与开放项

**本分支不包含完整原始证据archive（full raw evidence archives are not included in this branch）。**
这里只提供源码、相关文档/TODO和测试结论，不构成完整原始验证材料的交付。

source registry仍待独立集成；general NL/public quality、P7/V19和formal DEV/100k
尚未完成。没有全仓测试通过、release认证、merge或deploy声明。
