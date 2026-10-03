# Cache put 正常路径分阶段诊断（2026-10-03）

固定 source `29b03a0fec6bfde92aac9c41dce996ebb2d08cc7`；PR112 remote head 核为 `199558c754aae7f8d1dba0ee73256a2ed11a4230`。生产工作树、cache 格式/version、CI 和中央 TODO 均未改。Linux x86_64 overlay filesystem，官方 Rust 1.95.0，release thin LTO / codegen-units=1 / strip=true。实际使用真实 cc-semantic ArtifactCache::put/get；不是 MCP worker，也未使用 provider、heldout、GC、WAL、kill/crash/fault、内存 filesystem 或跳过 sync。

3 轮各运行原版 baseline 和 source-copy instrumented binary，每个 binary 新 keys 1k 后同 keys 1k 重写，固定 128dims 和相同非零有限 vector、timestamp、spec。六个独立 root 都是任务新建；各 root 内的 rewrite 才复用同 keys。轮次顺序 B/I、I/B、B/I；每个 put phase 后全部 get 验证 ref、dimension、vector。合计 12,000 put、12,000 verified Hit，全部进程 exit 0；6,000 个最终 normalrun 对象在本地保留。未运行可选 5k 或正式 100k。

## 关键结果

Instrumented put 新 keys 平均 **1694.367ms/1k**，重写 **1132.660ms/1k**。两类 sync 合计分别 **93.31% / 91.82%**，编码/元数据不是本环境主因。所有阶段是三轮累计后除三；百分比以三轮外部 put wall time 总和为分母，含调度等待，不是 CPU time 或 syscall 内核耗时。

|阶段|新 keys ms/1k（占 put）|同 keys 重写 ms/1k（占 put）|
|---|---:|---:|
|validation / space digest|1.147 (0.07%)|1.089 (0.10%)|
|f32 编码 / checksum / meta 构造 / JSON|2.330 (0.14%)|2.089 (0.18%)|
|create_dir_all|35.856 (2.12%)|5.800 (0.51%)|
|两文件 temp create|21.679 (1.28%)|25.582 (2.26%)|
|两文件 write_all|9.133 (0.54%)|9.027 (0.80%)|
|两文件 sync_all|1170.065 (69.06%)|636.487 (56.19%)|
|两文件 rename|24.172 (1.43%)|32.080 (2.83%)|
|两次目录 open|5.772 (0.34%)|5.603 (0.49%)|
|两次目录 sync_all|410.905 (24.25%)|403.490 (35.62%)|
|path / reference / close|9.784 (0.58%)|8.372 (0.74%)|

未归因部分新写 0.208%、重写 0.269%，包括计时/计数、边界间指令、返回/refs 收集等。目录 open 两类每 phase 各 1000 次，目录 sync 两类各 1000 次；所有文件 sync/rename 同样各 1000 次。best-effort 目录 sync 返回值按原源码仍被忽略，计数代表尝试，不能据此声称其持久化保证已验证。

`bin_atomic_total` / `meta_atomic_total` 是嵌套聚合，分别含 path format、temp path、file create/write/sync/close、rename、dir open/sync/close 与 probe 开销；不能和这些叶阶段再相加。表只加不重叠叶阶段。validation 含 space.validate/digest 与 vector 检查；checksum/meta 不含 JSON；JSON 在 bin 写后、meta 写前单独计时，顺序与原版一致。dir open 仅打开对象最深 spec 目录，未覆盖祖先链 fsync。

## Probe 扰动与适用边界

Baseline 新 keys 三轮 1906.014 / 1734.367 / 1650.064ms；instrumented 1729.794 / 1697.225 / 1656.081ms。Baseline 重写 1142.452 / 1127.552 / 1202.920ms；instrumented 1129.806 / 1198.984 / 1069.190ms。Instrumented 均值较 baseline 新写低 3.92%、重写低 2.16%，是非同时不同 fresh roots 的环境/顺序波动，不能解释成负 probe 开销或性能改进；三轮不足以估计可信净开销。

Probe 用 Instant + thread-local RefCell 固定数组，27 个计时/记录点/put；不在 put 内输出日志、分配统计 map 或加全局锁。独立空闭包校准，同一 diag 模块，5×10,000 次：48.476 / 50.372 / 48.454 / 52.935 / 48.453ns/次，即 27 次约 1.31–1.43µs/put 的量级，**不是运行中开销的准确扣除值**。校准不调用 cache，也没有无 sync 对照。叶计时不含自身 record/TLS 更新；外层 aggregate 和 put 含这些成本。声明周期/timing locals 与显式 meta Vec 暂存也会扰动优化；保留 patch 和 binary 哈希以供核对。

因此，这组成本分布只能指导下一次在 PR112 同环境的正常路径诊断；不能把 overlay 的 1k 均值线性外推成其实际 MCP 5k worker17.12s/cache::put10.13s，也不能直接声称 PR112 内部各阶段已实测。

## 静态建议（仅建议，未实施）

1. 同一对象的 bin 与 meta 都在同一个最深目录，各 rename 后各 open+best-effort sync。候选是保留两次 file sync、bin rename 在前、meta rename 在后，只在最后同步该对象目录。可被审查的成本项是 bin_dir_open + bin_dir_sync + bin_dir_close：新写约 210.77ms/1k（12.44%），重写约 207.16ms/1k（18.29%）；单 bin_dir_sync 为 206.59/203.34ms（12.19%/17.95%）。这是被测组件成本上限，最后一次 sync 的工作量/等待可能增加，实际收益未知。风险是改变 bin rename 到 meta 完成之间的持久化窗口、artifact-before-manifest 的时序依据；不能仅凭源码推导平台崩溃行为。原版 mkdir 新祖先链也没有逐层 fsync，现有 best-effort 机制从未证明严格跨平台崩溃保证。
2. 布局实际是 root/namespace/space/input/spec/spec.{bin,meta.json}，spec 同时出现在目录和文件名。静态候选是去掉冗余末级 spec 目录或用确定性分片共享目录，降低新目录/inode 和路径开销。create_dir_all 本次新写 35.86ms（2.12%）、重写 5.80ms（0.51%），可消除部分而非全部；创建 inode 的成本也可能在后续 sync 里，不能完全归到 mkdir。布局修改是 cache 寻址格式变化，需要明确迁移/旧缓存处理，不能作为本任务优化落地。共享目录还改变写竞争、目录增长、扫描/GC 隔离边界。
3. 批次同步共享目录可减少 open/sync 次数，但当前每对象独立最深目录，不能将任意 N 个对象简单改成一次目录 fsync；仅同步共同祖先不足以替代每个子目录条目持久化。批次需要对每个受影响目录规划同步，并定义所有 put 完成/manifest publication 前的屏障、部分失败归属与并发线程等待；这会改变单对象同步返回时序和可见性，不能无验证承诺收益。
4. file sync 是最大项（新写 1170.07ms/69.06%、重写636.49ms/56.19%）。本任务不通过删除 sync 证明速度，也不建议据此直接改持久化策略。当前 bin/meta 是分别原子替换，而非两文件同时原子提交；get 的 checksum 可拒绝不匹配组合。相同地址并发写正常收敛依赖 payload 相同；这次仅单线程正常 identical rewrite，没有证明不同 payload 并发或崩溃恢复。任何合并文件/批次文件方案须重新审查格式、完整标记和并发可见性。

## 证据与复现

- `cache.original.rs` 与固定 source 文件逐字相同；`instrumentation.patch` / `instrument.py` 展示只在 source-copy 添加的计时。
- `driver.rs`、`source-copy-*.toml`、`resolved.Cargo.lock`、`features.txt`、build logs、`source-receipt.json`、`binary-receipt.json` 提供实际来源/构建/features/哈希。cc-semantic 没有生产 feature；诊断 copy 只加 diag-probe；没有构建 cc-server 的 semantic feature 或产品 MCP binary。
- `results.json` / 各 stdout.jsonl / receipts 保留原始计时与命令；`summary.json` 可核所有聚合。`normalrun-manifest.json` 保留六个 root 的内容树哈希与文件数，`normalrun-samples/` 保留两对象真实 bin/meta，全部 normalrun/source/bin/target/cargo-home 本地保留，按该目录 .gitignore 不提交大型可再生文件。
- 初次原始 workspace `--locked --offline` 因无关 cc-eval reqwest 缺失失败，copy 仅收窄 members 为 model/db/semantic；第二次 offline 缺 chacha20。把现有 Cargo registry 复制到证据目录后正常补依赖，成功构建未升级固定锁内依赖；校验收窄后所有包 name/version/source 均来自原固定 lock。`build-baseline-online.log` 与 `build-instrumented.log` 为成功日志。rustup 默认 /home/agent/.rustup 曾返回只读，该具体动作未重试；使用预装 toolchain 目录直接 binary，不修改权限或凭据。
- 成功命令：证据内 cargo-home、RUSTC 指向官方预装 1.95.0，cargo build --manifest-path source/Cargo.toml --locked --release -p cc-semantic --example cache_put_costs --target-dir target；先保存 baseline，运行 instrument.py 后以 --features diag-probe --offline 构建并保存 instrumented；python3 run.py 顺序执行所有正常 phase。所有路径均相对本证据目录，完整执行 argv 在 receipts。run.py 拒绝覆盖已存在 normalrun root。复现须在新的授权诊断目录重新展开固定 git archive、复制 driver/manifests 后构建，不重跑现存 root。
- 生产 git diff 为空。全仓 CI、fault/GC/WAL 测试未运行；本任务验证为上面的正常真实 put/get 与文件完整性检查。

## GitHub 边界

只读 `git ls-remote` 成功核 main `ff458bc591b4e7e444af4464d6eef2513cdb335c`、PR112 head `199558c754aae7f8d1dba0ee73256a2ed11a4230`。`gh pr view 112 --json ...` 实际返回 GraphQL `Forbidden`；PR API 生命周期动作停止，不换 route/身份，draft PR 未创建。独立 Git evidence-only 提交/推送结果见 delivery-receipt。未 merge/forcepush/deploy。
