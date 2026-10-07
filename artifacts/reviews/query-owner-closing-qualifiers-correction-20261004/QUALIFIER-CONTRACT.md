# Closing qualifier 修正契约

本轮只修正比较短语所在词法组关闭后的连续访问运算符。`::`、`->` 及 `.member` 表示代码附件；`?.` 仅在随后有词字符、`(` 或 `[` 时表示附件。同一个附件判定也用于比较关键词的词边界。连续字符不做空白归一化；单冒号、终止句点、无后继的 `?.` 和分开的运算符字符保持原有 prose 判定。内层或祖先组的代码标记屏蔽该上下文中的比较短语；仅比较目标被限定的外层 prose 仍是比较。

保持原有 maximal Unicode 字词、ASCII case-insensitive 关键词、同一平衡上下文、空白/逗号间隙和显式过滤优先级。QueryTarget、旧单词比较守卫、计分逻辑、普通不支持语法回退不变。这不是完整自然语言质量结论；更强的 lexical API inversion 仍未解决。

2032 条 direct matrix 含独立 reviewer 全部 182 条与 12 条精确失败；558 条完整 driver 含 reviewer 原有 292 条（含先前 245 契约），加选择的 qualifier 负控制。前置红测试为 19 pass / 2 fail；修正后 21 pass / 0 fail（298 filtered）。两个 fresh synthetic arms 为 delivery 1520407 与本轮冻结 source；每个 arm 独立 Cargo target / fixture / results，无私有数据。
