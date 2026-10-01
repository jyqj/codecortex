# S11通用identifier扩展设计（只读研究；未改生产）

实际链：SearchPlan::new augmented_query_text → expand_query_text → expanded_query → LexicalLane::run_detailed sanitize_fts_query → IndexDb fts_chunk_candidates_with_work。

旧expand把camel/snake分片flat拼接；sanitize把每词OR。未知compound标识符的generic片段（如in）即可独立召回for-loop正文，absence题因此误报。不是no-answer分支需要题号特判。

## 提案

新增 `compile_expanded_fts_query`（Rust文稿同目录）：每原token匹配 literal whole OR (ALL distinct identifier components AND)；natural separate词仍OR，不全局删in/this等词。对用户syntax逐atom双引号literal quoting，不允许OR/NEAR等元词直接变MATCH运算符。

- 全原tokens优先，12atom全局上限；只容纳完整derived conjunction，容量不足只保whole并计identifier_groups_omitted。超过12原tokens计source_tokens_omitted，不偷称完整。
- camel/snake/acronym/digit-uppercase边界，case折叠维持FTS默认；重复derived components只有一个distinct时不扩大whole为generic单词。
- Unicode按既有FTS_TOKEN_RE词界，不构造英文停用词猜测。空文本继续empty MATCH sentinel。
- quoted snake whole经unicode61 tokenizer可能是phrase；AND分支容许components非相邻或调序，仍只是lexical支持，非symbol identity。
- 旧sanitize_fts_query与旧expand_query_text保持供其他caller兼容；owner持有SearchPlan/lexical接线，**不再次sanitize编译结果**。owner同步policy/spec fingerprint/cache键与编译省略诊断。

## 正式红绿

独占新cc-db expansion integration测试先运行旧 sanitize(expand())实际SQLiteFTS，证明未知compound对只有in的原件产生错误命中；然后新增helper，切测试adapter到编译MATCH重跑。

独立正例：renderWidget匹配render/widget全components；get_user_name全components（允许非相邻）；自然单in仍返回in正文；literal compound自身返回；多自然词继续OR。负例：compound仅一fragment不返回、缺一个component不返回、duplicate/1part不变generic单词、budget不截AND前缀、不用syntax控制语句。边界：中文+camel、引号/OR/NEAR/括号/冒号/*、数字后缀、空文本、多group原词优先。

整个产品结论需owner接线后四suite、hard-scope、source proof与Partial回归，不能用helper通过代替S11真实stdio；gold/scorer不改。当前P5-D freeze期间仅artifact提案，无生产写入。
