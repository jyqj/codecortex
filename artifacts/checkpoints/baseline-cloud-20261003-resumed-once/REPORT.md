# Baseline 独立云 consumer：唯一 100k 与完整后续验证通过

结果为 `passed_declared_100k_local_scope`，`formal_runs=1`，run-single 调用一次、exit0。baseline32 与 candidate 自有 cache32 smoke 均 passed_preflight_32；正式 baseline 在原300s内 ready，随后原 query/C4/EOF/reopen 检查全部完成，failures/not_run 均为空。该结果只适用此固定树与合成 loopback 本地 scope，不推导独立云消费者之间的严格 paired 因果提速。

固定 driver `800d32d50bfe158cf86366fb913aabef73f8d965`；baseline `513a98c9a94b15ec77153df41af26fa3c8c0b5e8`；candidate `e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207`，不含PR127 DB facade。原协议保持100000/128dims/parse4/HTTP4/2/claim16，ready300/RPC300/EOF15/overall900/hold120，12GiBguard/4GiBreserve。无改driver、cgroup/limits、OS cache、heldout/GC/WAL/kill/crash、merge/deploy/accept。

## 准备恢复与真实门禁

旧失败原日志/receipt/PR提交保留在相邻 build-blocked 目录。父级明确授权恢复同一尚未测量任务。只读核验平台已有 HTTP_PROXY/HTTPS_PROXY/NO_PROXY 与小写同名变量存在，ALL_PROXY及小写不存在，仅报告名字；build.py第21行的 dict(os.environ, ...) 保留代理，旧故障来自执行者外层 env-i，并非 frozen driver。新 launcher 保留平台现有网络、证书、PATH/HOME/locale变量，未带入 provider secrets；没有换镜像、自建代理、修改安全配置或读取代理值。

官方原lock fetch 在首次探测缺少rustc PATH后使用固定toolchain的RUSTC通过；baseline/candidate原Cargo.lock SHA256同为 ea6b177872de3749702e2cd967f4504efff5bcc67cd2a2459205d182243d1000。没有403/policy权限拒绝。随后新输出目录 prepare 一次，两版原lock、official direct Rust1.95.0、独立 cargo/tmp/target、jobs4、release semantic-http构建成功，exit0/guard_stop null，wall146.835s和148.348s。fixed driver load_bundle验证两份receipt/source/blob/binary；当前proc mount未观察到compiler，再启动run-single。driver worktree始终固定800d32d，未改源码。

baseline binary SHA256 `88bf3fa52b48d35d79a98264d32549e0a2185c298a23b1dced8c96b9729e3495`；candidate `d3d3c3830dd90a9652d56fff14f60468b875be85b56d903efc4abf8892c5d522`。精确HEAD/tree/源文件/Git blob/compiler/features/profile/命令及日志hash均在receipts与verified-gates中。

已有 /sys/fs/cgroup 由 self cgroup 0::/ 与 mountinfo cgroup2 root /..记录确认；只读，无迁移或新建。成员列表为空，关联与独立性unknown，不能证明无其他进程；角色真实membership在composition记录。初始及门禁memory.current/procs/stat/events/max/cpu.max读取成功，未触发guard。

## 正式结果

| 检查 | 真实结果 |
|---|---|
| 生成 | 100000 files，4088890 logical bytes，6.340095292s；路径/SHA256 manifest保留 |
| cold | scanned=parsed=added=100000、skipped0、parse_errors空，22.218803684s |
| held DB | files100000、integrity ok、FK0、co_change/test edges0 |
| ready | release/drain 257.929186553s；coherent v2 semantic/dense ready，deadline前接受且查终态 |
| final DB | files/symbols/chunks/document_manifest/semantic_manifest均100000，integrity ok，FK0，outbox done100000 |
| 查询 | resource_00000/00001/50000/99999四条local、top_k5参考，禁query network |
| C4 | repeated/distinct完整hits逐条等于参考，observed peak4；burst0.039750185/0.032278350s |
| normal EOF | exit0、forced=false，0.063830116s |
| reopen | ready、全部DB counts完全一致、四条完整hits完全一致 |
| reopen EOF | exit0、forced=false，0.007375380s |

summary wall301.116194226s（不等于ready耗时）；ready成功后边界observer停止，因此没有300s失败边界count。cleanup-tail为after cleanup、drain elapsed269.374156151s的另一次只读报告，明确不充当deadline验收；完整final/held/reopen/tail原报告和终态/RPC日志保留。两个32 smoke都实际held POST2/returned0/backfilling，再release与完整验证，未复用cache。

## CPU/RSS/IO/HTTP 与组成局限

正式cgroup峰值样本10258341888 bytes（约9.55GiB）是整个group使用量，不是product RSS。主产品root RSS峰值样本2076258304 bytes，VmHWM样本2076676096 bytes。13494个20ms资源样本均存在 /proc/<pid>/task/<tid>/children 不可见的tree coverage error；不能宣称完整进程树、PSS或精确lifetime peak。主产品root已观测CPU区间174.77s（user+system ticks），root I/O末样本read_bytes294912/write_bytes14624641024，rchar/wchar为逻辑系统调用累计量，不能当设备流量或完整树I/O；不是reopen完整生命周期CPU/IO结论。

mock原HTTP entered=returned=100000，body哈希/输入计数/时间保留；只为自生成Rust和loopback synthetic provider，不代表真实provider/network性能或完整backend排队分解。RPC保留offered/sent/received时序，backend queue/service未知。仅造无效dummy值，无真实provider secrets。

生成前后、两版build-before/build/build-complete、productstartup前后、cold、release前drain、serial/C4/normal EOF/reopen/cleanup前后均有同步composition；observations.json保留正式各phase边界的anon/file/slab/current。字段可能重叠，不相加；非原子、PID namespace/采样可见性限制保留。archives内同时保存role PID/start_time/state/membership，以及原root/tree RSS/CPU/I/O日志。

本目录只收录本次新精简manifest/报告/derived observations和三个raw tar.gz：官方fetch/setup、两版build receipt/log、两版smoke与唯一formal的session/config/source hash manifest/DB只读报告/RPC/HTTP/resources/composition/summary/logs-sha256。原日志逐项bytes/SHA256、archive SHA256可核验。排除任何repo Rust内容、cache、DB、binary、target、cargo cache和旧private/localdiag；原driver的logs-sha256保留未发布内容的hash而不带入内容。无原始cmdline或代理值/凭据。旧失败证据保持不变。
