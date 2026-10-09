# 原 a23 soak 最小归档选择

这是原 artifact 11594089439 的小证据选择，不是完整原 sealed artifact。完整原 ZIP、70,117,809-byte raw.jsonl、322,493,166-byte product/rpc.jsonl、原 ELF 与原数据库仍在 GitHub 原 artifact 和 Mac central raw/11594089439 原件中；它们没有被改写、截短或用索引替代。官方定位及每个选择文件的 SHA256 / bytes 见同目录 archive-selection.json；官方 ZIP SHA256 为 d6debfa5e4a51bb59902cdb69a35ae7626a1e20b09bcaa5ab6cfda9ee3237086。

小文件完整保存 plan/report/parity/statistics/replay/execution receipts、build receipts/source maps/seals、原 observer scripts、原 full-control stdio、两个 process receipts、产品 stderr、原 GitHub 已解码日志以及两个独审脚本/结果。重复的 runtime/build-evidence 原文件通过逐一精确 SHA256 与 p8-build 原件对应，不额外复制；旧 mixed/lifecycle/backfill v2 审查引用既有文件，不覆盖或复制。

三个 gzip JSONL 索引覆盖原 raw 7,200 行、product stdio 57,603 行和 full-control stdio 19 行。首行是 schema 与原文件路径/bytes/SHA256，后续每行依次为原物理行号、byte offset、byte length、包含原换行的 SHA256、event/kind、原 request/operation id、原 clock 字段名及值。gzip 是无损索引封装，mtime=0；其压缩及解压摘要均在 raw-stdio-indices.json。索引只提供完整定位与覆盖证明，没有保留其中原载荷，不能单独重放 collector 或验收。full-control 的原 stdio 文件较小，也完整收录。

复核完整原验收时必须先取原官方 ZIP 并验证上述 digest，再解压到原 central raw/11594089439/extracted 结构，准备 exact-a23 source，才能运行 runtime-independent-review-v2.py 及 soak-stdio-binding-review.py。不能对本小选择目录运行原 seal CLI 后期待通过；也不能删除被引用的大文件后继续声称全量原验收可从此选择单独重放。

本轮只追加 Round 5 终态 evidence 和 soak 新审查结果，Round 4 pending JSON / Markdown 仍为原字节。新 P、PR #161 新 head 及任何其它 source 都不继承此 a23 执行信用。
