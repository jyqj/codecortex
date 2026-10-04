# 发布回执

正常 origin fetch 与 push 成功，远端分支：
`integration/bounded-groundwork-20261004`。

- 固定base：`37dd042eaa1209a86e0cafdcd92ae77e036e76f5`。
- 产品SHA：`e4a8df4cbc6dfae29af8cb0eac9ee83fbba4d696`。
- 已推送文档/验证SHA：`6b8f21a664c7b965ff99a12b3541fff98f7312eb`。
- 唯一draft attempt：exit1，`Post "https://api.github.com/graphql": Forbidden`。
- 没有PR URL，没有merge/deploy。拒绝后停止远端操作，未换身份、协议、路由或API重试。

正常push见 `receipts/push.log`；唯一draft命令、exit与精确拒绝见
`publication.json` / `receipts/draft-attempt.log`；请求正文保存在 `draft-body.md`。
本地最后一个文档receipt提交保存该拒绝证据，拒绝后未再次push；因此本地HEAD与
远端6b8f21a不同，但二者crates/Cargo源码完全一致。确切本地receipt SHA以交付响应为准。
