# FIFO 100k 单一预注册配对性能决策（2026-10-03）

**性能接受：False；生产集成：False。** Both fail the unchanged 300s readiness gate; more partial work is not a pass.

仅一个顺序配对：baseline → candidate，各唯一一次。没有统计显著性结论。正确性另独立 review；证据 replay 只核验身份与留存。

| 指标 | baseline | candidate |
| --- | ---: | ---: |
| 源码 SHA | 5ffbadcf48e26523b2eb46beda0d187a2e2e29cd | 807f4710495a38bc6631549da2ab9623c83684b8 |
| binary SHA256 | 48e9cb9c9b1b30d38ef8944aaf1e51e5ce801fb16f48629d0b044fd4de2f168c | c024019060ad3fe8eaeacb041b2b22b31f3fcfef436b63c558a7ff5799e7f549 |
| cold 秒 | 36.066914 | 29.679855 |
| 300s 内 ready 秒 | unknown | unknown |
| 结果 | failed | failed |
| 边界 semantic_manifest / 100000 | 25081 | 48809 |
| 边界快照开始 elapsed 秒 | 300.000158 | 300.000121 |
| cleanup 尾段 semantic_manifest | 25600 | 49152 |
| cleanup 尾段 elapsed 秒 | 311.135716 | 305.456626 |
| status 观察/错误 | 1335/0 | 1334/0 |
| status latency p50/p95 ms | 22.739/34.760 | 22.592/37.826 |
| root sampled RSS max bytes | 1941680128 | 1977581568 |
| root 最后可读 lifetime CPU user/system 秒 | 225.190/49.950 | 110.920/61.460 |
| root 最后可读 lifetime write_bytes | 5035180032 | 8189644800 |
| root 最后可读 lifetime rchar | 54498527323 | 48196252275 |
| drain sampled write_bytes delta | 3313831936 | 6487724032 |
| drain sampled rchar delta | 52507632890 | 46214391307 |

计数为实际记录时间的近边界只读快照，不冒称精确 300.000000s。尾段在期限之外，不提升 ready 判定。正常 EOF 清理退出不能替代 gated post-ready normal-exit 检查。

RSS 为 20ms 非原子采样。完整进程树在 children coverage 不可读时为 **unknown**；observable sum 不作为完整 tree RSS。CPU/IO 是最后可读累计计数或 phase 采样端点差，未知不填 0，不能当精确进程退出总量。read_bytes=0 也不表示没有读取工作。

baseline：not_run = `["ready_manifest_integrity_fk", "bounded_concurrency", "normal_exit", "reopen_stability"]`；failure cleanup exit = `{"exit_code": 0, "forced": false, "wall_seconds": 7.084515706000047}`。

baseline cleanup DB：`{"attempt_count": 25600, "chunks": 100000, "document_manifest": 100000, "edge_tables": {"call_edges": 0, "co_change_edges": 0, "data_flow_edges": 0, "http_call_edges": 0, "infra_edges": 0, "semantic_edges": 200000, "test_edges": 0}, "files": 100000, "outbox_states": {"done": 25600, "pending": 74400}, "semantic_manifest": 25600, "symbols": 100000}`；integrity=ok，FK errors=0。

baseline HTTP：`{"cleanup_tail": {"actual_batch_sizes": {"1": 519}, "entered_inputs": 519, "entered_requests": 519, "request_enter_gap_ms": {"count": 518, "max": 27.459366, "min": 10.93872, "p50": 13.223127, "p95": 17.06099}, "returned_inputs": 519, "returned_requests": 519, "server_duration_ms": {"count": 519, "max": 2.014866, "min": 0.233278, "p50": 0.293483, "p95": 0.503518}}, "pre_release": {"actual_batch_sizes": {"1": 1}, "entered_inputs": 1, "entered_requests": 1, "request_enter_gap_ms": {"count": 0, "max": null, "min": null, "p50": null, "p95": null}, "returned_inputs": 0, "returned_requests": 0, "server_duration_ms": {"count": 0, "max": null, "min": null, "p50": null, "p95": null}}, "within300s": {"actual_batch_sizes": {"1": 25080}, "entered_inputs": 25080, "entered_requests": 25080, "request_enter_gap_ms": {"count": 25079, "max": 210.765717, "min": 3.232595, "p50": 10.177346, "p95": 25.983396}, "returned_inputs": 25081, "returned_requests": 25081, "server_duration_ms": {"count": 25081, "max": 3729.540974, "min": 0.172479, "p50": 0.318219, "p95": 0.578697}}}`。

baseline poll gap ms：`{"count": 1334, "min": 200.425523, "p50": 200.664577, "p95": 201.767196, "max": 256.166566}`。

candidate：not_run = `["ready_manifest_integrity_fk", "bounded_concurrency", "normal_exit", "reopen_stability"]`；failure cleanup exit = `{"exit_code": 0, "forced": false, "wall_seconds": 1.5184336239999539}`。

candidate cleanup DB：`{"attempt_count": 49152, "chunks": 100000, "document_manifest": 100000, "edge_tables": {"call_edges": 0, "co_change_edges": 0, "data_flow_edges": 0, "http_call_edges": 0, "infra_edges": 0, "semantic_edges": 200000, "test_edges": 0}, "files": 100000, "outbox_states": {"done": 49152, "pending": 50848}, "semantic_manifest": 49152, "symbols": 100000}`；integrity=ok，FK errors=0。

candidate HTTP：`{"cleanup_tail": {"actual_batch_sizes": {"1": 343}, "entered_inputs": 343, "entered_requests": 343, "request_enter_gap_ms": {"count": 342, "max": 23.255097, "min": 3.087758, "p50": 4.289639, "p95": 6.283095}, "returned_inputs": 343, "returned_requests": 343, "server_duration_ms": {"count": 343, "max": 3.439153, "min": 0.189816, "p50": 0.282607, "p95": 0.471155}}, "pre_release": {"actual_batch_sizes": {"1": 1}, "entered_inputs": 1, "entered_requests": 1, "request_enter_gap_ms": {"count": 0, "max": null, "min": null, "p50": null, "p95": null}, "returned_inputs": 0, "returned_requests": 0, "server_duration_ms": {"count": 0, "max": null, "min": null, "p50": null, "p95": null}}, "within300s": {"actual_batch_sizes": {"1": 48808}, "entered_inputs": 48808, "entered_requests": 48808, "request_enter_gap_ms": {"count": 48807, "max": 1143.838391, "min": 2.948158, "p50": 4.748613, "p95": 11.810249}, "returned_inputs": 48809, "returned_requests": 48809, "server_duration_ms": {"count": 48809, "max": 3868.302683, "min": 0.173262, "p50": 0.27825, "p95": 0.554316}}}`。

candidate poll gap ms：`{"count": 1333, "min": 200.408368, "p50": 200.674016, "p95": 202.00471, "max": 294.073653}`。

协议完整保留 PR108 `99973e7d` 最终 run.py：100000 个确定性 Rust 文件，source(i,value=0)、128dims、release semantic+semantic-http、真实 MCP stdio、loopback 假 HTTP、cold → held DB 校验 → HTTP release → 300s ready deadline，claim16/concurrency4/per-project2/parse4，200ms poll gap，原 index/search RPC300s、EOF15s、overall900s、memory12GiB/disk4GiB guard。模型第一批请求 gate 与 120s fixture gate deadline 均未改。

两个 binary 在测量前全部构建完成，无测量期并行 build；fresh root/DB/cache，各自空 synthetic Git repo。source-inputs manifest、配置（包括固定 endpoint 端口）、静态环境核验完全一致。两版唯一生产差异是 FIFO SQL、partial pending index DDL、matching-v24 physical maintenance 三个文件；无临时 production probes。OS/page cache 未清理，顺序效应保留，cgroup current 包含 harness/build/page cache，不作产品 RSS。

PR108 旧失败只作上下文，不作本次 baseline。PR114 原两组 5k ready 22.965/25.051 → 25.963/26.654s，物理写入约 +4.4%，此回归仍保留；claim 微基准或 100k partial count 增加都不单独构成通过。

PR114 新测试 lint 修复 head `52a50730b58e2b59351449fc80f65771b24c26a8` 的 382 个生产/构建输入文件与冻结 candidate 字节一致；`lint-fix-production-identity.json` 单列此核验。未追新 head 构建/重跑，也不外推其测试身份。

证据：`preregistration.json`、`pair-builds-ready.json`、`source-identity.json`、`production-delta.patch`；`pr108-run.py`/`pr108-protocol.json` 与逐版 derivation patches；各版 build receipt、compressed Cargo/resource/RPC/HTTP/input logs；独立 deadline DB / cleanup tail DB；`extended-analysis.json`、`paired-decision.json`、`verification.json`；`compare.py` 可重放，`SHA256SUMS` 为 curated 留存清单。

生成 corpus、DB/cache、release binaries 和编译目录保留在该 checkpoint 下本地 runtime/live，排除 Git；`retained-local-data.json` 记录占用。所有任务文件仅写入本目录；生产、中央 ledger/CI/DEV/heldout 均未修改。没有真实 provider、GC/WAL kill/crash/fault、RO 拒写重试、权限/credential 变更、merge/forcepush/deploy。

阶段结论先报告父任务，再仅 commit/push/draft PR，并核验 exact remote 和文件范围；交付完即停。

交付：Draft PR https://github.com/jyqj/codecortex/pull/116 ，首个证据 commit `bb35d5b1fc6ce9d471c8dc75eaad64df222e177e` 已由 GitHub App 与 origin 核验；全部 104 个远端差异文件仅位于本目录。随后仅追加交付回执，无新实验。
