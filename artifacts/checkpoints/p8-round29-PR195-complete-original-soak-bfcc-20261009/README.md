# PR195：完整原始 soak 归档

本目录保留原 run `37994312877`、attempt `1`、artifact `11648857187` 的唯一已验收原始 ZIP；实际 source 为 `b8ab8ecdc5b32993fc02ce43877b7e172b8a4b3f`。

原 ZIP 为 51,047,184 B，SHA-256 `1c9c43b91016fde2c0857c4f800140ed520f59019495deec9a69d8fd0baf5000`。按 `ordered-original-zip-chunks.json` 的 index 顺序串接 65 个原始字节分块即可恢复同一 ZIP；分块仅服务于传输，未重跑工作负载、重压缩或替换原件。清单包含每块偏移、字节数、SHA-256 和实际 Git blob；创建响应见 `actual-github-chunk-upload-responses.json`。

`original-transport-review.json` 记录对唯一原 ZIP 的字节摘要、CRC 和 2,131 个成员的单次提取核对。独立语义复核、完整 soak 日志和准入决定已在前一提交 `4cc3f74850378bbded96ed2c3d23002bac411238` 的 `p8-round29-PR195-original-soak-and-admission-bfcc-20261009` 目录保留。

实际 soak 工作时长 3600.049141340 秒，3601 次 offered 全部成功；14401 请求/响应包含 9600 个 compound-read RPC 内容绑定，原资源与缓存报告按纯函数重算一致，15 表 parity 均为 0 差异。该运行保持原 b8ab 身份；PR195 的最终合并 `a406a496…` 与其只存在五份 P8-005 进度文档差异，不能宣称全树相同或已直接执行 a406。

本目录新增任务完成信用为 0。累计本次原始 TODO 完成 1 项，164/192 完成，仍剩 28 项。
