# Candidate 独立云 consumer：唯一100k及后续检查通过

**最终 `passed_declared_100k_local_scope`，formal_runs=1，CLI exit0。** candidate 自身独立新cache的32 smoke通过后，固定driver仅执行一次正式100k；readiness drain **182.999904792s**（原300s内），cold index **22.086386410s**，测量含后续检查/cleanup **225.952954505s**。failures/not_run为空、guard_stop=null。

先前 prepare 因人为 env -i 去掉平台标准代理而DNS失败，原件完整保留于 ../candidate-independent-100k-gate-20261003。用户后续明确授权恢复尚未执行的测量任务。只读确认 HTTP_PROXY/HTTPS_PROXY/NO_PROXY 大小写变量存在（ALL_PROXY不存在），只记录名字/存在性。fixed build.py:20 本身继承os.environ，未修改driver。保留平台现有标准代理/网络证书设置，官方 index.crates.io 的原lock cargo fetch --locked exit0（4.11s）；随后只在新目录 prepare一次。没有换镜像、自建代理、改系统安全配置或获取新凭据；没有policy403/权限拒绝。整个任务prepare共2次，但run-single/正式测量均仅1次。

固定driver PR128 head `800d32d50bfe158cf86366fb913aabef73f8d965`；baseline `513a98c9a94b15ec77153df41af26fa3c8c0b5e8`，candidate `e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207`（不含PR127）。两版原Cargo.lock、official direct Rust1.95.0 release/semantic-http/jobs4，全部构建完毕后fixed load_bundle核验源HEAD/tree/逐项Git blob、receipt、binary与driver hashes，当前proc mount未观察到compiler，才启动产品。baseline build149.817549507s、candidate146.858531502s。精确source/tree/binary hash见 verified-builds.json 与原receipts。

32 smoke确认为 held POST2、returned0、backfilling，release后ready/完整DB/query/C4/EOF/reopen均通过；未使用baseline cache。正式cold scanned=parsed=added=100000、skipped0、parse_errors空；held DB files/document_manifest=100000、semantic_manifest=0，integrity ok/FK0，co_change/test edges0。ready final files/symbols/chunks/document_manifest/semantic_manifest全部100000，outbox done100000，semantic pending/failed0，coherent retrieval-v2 generation与active_space检查通过。

四条原serial查询后，C4 repeated/distinct各4请求、观测inflight4，逐条完整hits与serial严格相等；burst wall分别0.016348372s/0.036589583s。正常EOF exit0 forced=false（0.063682468s）。reopen ready、counts完全相等、四条hits完全相等，再次EOF exit0 forced=false（0.015859956s）。原300s boundary observer因ready早于deadline被取消，故没有deadline-300s-db；cleanup-tail明确标注after cleanup，只作尾部证据不替代deadline/ready验收。

已有cgroup实际读取为 /proc/self/cgroup 0::/、mount root /..、/sys/fs/cgroup inode148；direct members空，group relation/独立性unknown。无cgroup/namespace创建、迁移或limit修改，无dropcache。此为独立云consumer，不宣称同机严格paired因果提速。

生成前后、两版build前/中/完成、startup、cold、release前drain、query/C4、EOF、reopen和cleanup组成保存于原composition；memory.stat anon/file/slab字段重叠不相加。正式阶段sampled cgroup peak **10177134592 bytes (9.478GiB)**，包括build/cache/runner/file/slab，绝非product RSS。cold产品root sampled peak RSS **2023653376 bytes (1.885GiB)**，sampled VmHWM2024169472、CPU179.76s；reopen root另计。原proc IO累计计数与角色PID/start/membership均保留于raw和observation-analysis。每线程children文件在当前proc mount不可用，10084个样本有tree coverage error，完整树/PSS与精确lifetime peak unknown。20ms目标采样与同步phase读数非原子，有观测开销。

HTTP loopback raw含100000 entered/returned，每次input_count1（claim batch上限16并不保证实际HTTP批量16）。按mock entered/returned时间区间重建的peak active为4；这是mock日志区间，不能直接证明产品permit占用或实际effective per-project cap。配置固定global4/per-project2，smoke实际held2；status不暴露effective caps。HTTP/stdio不提供backend queue/service拆分，IO不是精确设备或网络归因。

新发布目录只有本任务报告/manifest/观测分析/verified identity及一个26.3MB raw archive（84个新证据文件，内含source-input hash manifest、原session/log hashes、build和RPC/HTTP/resource日志）。保留未改原字节，各entry用 raw-files-sha256.json核验，发布件用SHA256SUMS核验。archive排除repo/cache/cargo/target/tmp及所有corpus/DB/binary字节；无旧诊断、provider secret/代理值、非本任务进程原始cmdline。产品dummy key由原driver自造无效值，仅loopback；没有heldout/GCWAL/kill/crash或旧runtimeEROFS路线。

26项无害driver tests此前通过，fixeddriver仍clean；这是该单次declared local scope验收结果，不是性能acceptance。测量完成后停止，无merge/deploy/accept。
