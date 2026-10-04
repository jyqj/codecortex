# R1 正向 byte witnesses v1（修复前固定）

原独立审查 c2b2a22e0536e163d1f9f7353f10ddf400dfd854 的 characterization
及失败报告/日志保持原样，不把旧 UnsupportedAst assertion 当作正确契约。
本新增版本只断言应成功的 exact absolute offsets；parser 不 trim/normalize/reparse。
所有范围半开，declaration 末端在最后一个 pass 后，不含 CR/LF。

| 原始 bytes | Function span | name span | module start（独立观察） |
|---|---|---|---|
| `\n\ndef f(): pass\n` | 2..15 | 6..7 | 2 |
| `  # lead\r\n\r\ndef f(): pass\r\n  ` | 12..25 | 16..17 | 2 |
| `EF BB BF` + `def f(): pass\r\n` | 3..16 | 7..8 | 3 |

empty、blank/whitespace-only、comment-only、BOM-only：Inputs([])。
original source digest 覆盖包括所有 leading/trailing trivia 和 BOM 的完整 bytes。

nested 原文（86 bytes）：`EF BB BF` +
`\r\n  # lead\r\n\r\n@one\r\nclass C:\r\n    @two\r\n    async def f(self):\r\n        pass\r\n  \t\r\n`。
C Class wrapper 17..79，name 29..30；f Function wrapper 37..79，name 57..58；
完整 ancestry 为 C / f。不含前缀 BOM/comment，也不把装饰器生成独立 declaration。
