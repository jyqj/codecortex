# 自编 AST / 原始 byte 预期（实现前固定）

witness.py 为 UTF-8 LF 原文，282 bytes；雪占 3 bytes。以下半开范围不包含末行 newline。
装饰声明的 canonical span 从首个 @ 开始；name 来自 definition 的 name field。
block/if/else/decorator/parameters 不生成 lexical segment。

| occurrence | kind | declaration span | name span | 完整 ancestry |
|---|---|---|---|---|
| Tower | Class | 0..200 | 19..24 | Tower |
| Room | Class | 30..200 | 46..50 | Tower / Room |
| ring (async) | Function | 60..200 | 84..88 | Tower / Room / ring |
| local | Function | 108..175 | 112..117 | Tower / Room / ring / local |
| Hidden | Class | 137..175 | 143..149 | Tower / Room / ring / local / Hidden |
| repeat (if) | Function | 218..244 | 222..228 | repeat |
| repeat (else) | Function | 255..281 | 259..265 | repeat |

local/Hidden 必须让纯模型返回 LocalDeclaration；重复 repeat address 相同，binding 不同。
CRLF 自编 witness `# 雪\r\n@deco\r\nasync def ping():\r\n    pass\r\n`：
声明 7..41，name 24..28（原 bytes，不能 newline normalization）。
首次 scoped 验证纠正手算 end：pass 的最后 s 在 byte 40，半开 end 为 41。
畸形/缺失 name/body、keyword 当 name、错误参数/继承 field、任意 ERROR/MISSING：
拒绝整个文件，不从 recovery 节点猜 ancestry，也不保留看似正常 sibling 身份。

补充自编 wrapper witness：`@one\n@two(1)\ndef f():\n    pass\n`，31 bytes；
f Function canonical 0..30，name 17..18，只输出一次。局部 class 的 method
预期 outer(Function)/C(Class)/method(Function)，后两项都为 LocalDeclaration。
