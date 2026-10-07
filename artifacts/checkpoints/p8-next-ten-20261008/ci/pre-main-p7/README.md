# 合入新 main 前的 P7 engineering 成功记录

这是 PR #146 固定 head `2a75e65d01a3722155e7a1858d7e0e9c8a5558cf`
在当时 main `b951f27d3ed50b7755bc2456c6425355f753ec17` 上的实际结果：
[workflow 37667560377](https://github.com/jyqj/codecortex/actions/runs/37667560377)，
job `112950585779`，14 个步骤全部成功。

24 组 Rust 测试为 **95 passed / 0 failed / 1 ignored**；另外 15 项 Python
input-lock 控制通过。651 个 filtered 项来自两个明确的 lib selector（337 + 314）。
唯一 ignored 是原 strategy 测试中要求显式产品路径及摘要的真实 stdio 场景，未改状态。

官方 artifact `11502953837` 的压缩包摘要及逐 seed 检查保存在
`artifact-check.json`。实际 worker 记录为 **384 个请求**：seed 7/19/43，
每 seed quiet/held 两阶段、C1/C4 两并发格、每格 32 请求；每条均包含稳定
`stable.rs` 命中，held 旧输入发布数为 0。查询/进展 watchdog 仍为原 2s/5s。

执行前后全部 785 项源码输入一致，与固定产品 `65dd3293…` 相同。
原始 job 日志按原字节 gzip 保存，解压后 SHA-256 为
`e3b7d95c856d7b9fcd07c25bd625c198f24283c097c6929cee7600e4a33f28a7`。
这份结果只证明该固定版本的工程回归；它不覆盖随后合入的 `d53a4972…`，
也不将 P7-015 或 G7/G8 改为完成。
