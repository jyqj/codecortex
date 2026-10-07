# 旧 head 2a75 的完整 CI 终态

本目录只归档 PR #146 的旧固定 head `2a75e65d01a3722155e7a1858d7e0e9c8a5558cf`。CI 实际 checkout 为 `c205775a16d97ee6effe951f3314599e5fa87348`，日志明确记录其为旧 head 合入当时 base `b951f27d3ed50b7755bc2456c6425355f753ec17` 的 merge commit。本结果不替代随后 main `d53a4972…` 合流后源码（包括 `8e12…`）的独立审查或 CI。

GitHub plugin 读取的 [CI run 37667560396](https://github.com/jyqj/codecortex/actions/runs/37667560396) 最终为 `completed/success`。`github-state.json` 原样保存结构化 run/job 返回值和查询参数。

| Job | 实际终态 | 全部步骤 |
| --- | --- | --- |
| [check 112950578294](https://github.com/jyqj/codecortex/actions/runs/37667560396/job/112950578294) | success | 35 项均 success，含最后的 semantic product disabled stdio contract |
| [msrv 112950578006](https://github.com/jyqj/codecortex/actions/runs/37667560396/job/112950578006) | success | 9 项均 success |
| [security 112950578390](https://github.com/jyqj/codecortex/actions/runs/37667560396/job/112950578390) | success | 5 项均 success |

## 正式 check 日志的实际数量

统计只来自 job `112950578294` 的完整正式 decoded 日志，不混入 MSRV/security 或独立 P7 engineering job 的数量：

- **392 组 Rust result：3115 passed、0 failed、126 ignored、0 measured、1853 filtered out。**每个 result 都是 `ok`，没有解析遗漏。
- **5 组 Python：46 + 197 + 104 + 9 + 14 = 370 tests，全部 OK。**
- 没有 Actions `##[error]` 行或以 `error:` 开始的行。

这些是实际执行组的累计，包含同一 suite 在不同命令中的重复运行，不是去重后的测试库存。151 组没有实际执行测试；389 组 quiet/unlabeled harness 没有输出 target 名称，因此解析记录保留 `target: null`，不猜测目标。1853 项 filtered 也没有被计为执行通过。

126 条 ignored 原名、原因、行号与所在命令组完整保留在 `parsed-results.json`。范围包括显式 release/scale/RSS 基准、需要明确产品路径的 MCP 测试，以及以下不能笼统归为 benchmark 的既有项：

- `index_db::tests::mismatch_rebuild_stress_loop`：日志未附原因。
- `special_config_child`：供 bounded FIFO parent tests 调用的子进程 harness。
- `slow_provider_beyond_lease_cannot_publish_and_old_tokens_cannot_ack`：日志保留原有 strict-clock policy 与现行 token-fencing contract 范围裁定，并指明旧失败已存 checkpoint。

部分显式产品测试在默认命令中 ignored，随后由有产品绑定的专门 CI 步骤执行；本计数保留各次运行的原结果，不删除 earlier ignored，也不把未执行项改写为通过。

## 原始日志与复查

`job-112950578294.log.gz` 无损压缩全部官方 decoded UTF-8 字节，包括开头 BOM 和最终换行。解压后为 **995,739 bytes**，SHA256：

`08cb5ee27640f487d977fa84910d3bb3efcc0a583ccb1ffda3fcd92a4fb76597`

`parsed-results.json` 保留全部 392 个 Rust summary、5 个 Python summary 和 126 条 ignored 原文及其原日志行号。`metadata.json` 记录固定 head、checkout/base、run/job URLs、数量与适用范围；`file-manifest.json` 绑定本目录载荷。归档时已验证 gzip 解压逐字节等于原日志，没有重新运行测试，没有修改任务状态或源码，也没有据此授予 TODO 完成或发布验收。
