# 独立 Capability v2 review — bounded pass

审查固定生产 `11af963c33cfa68cc9497e355464c1d6d058adac`、交付 `29b03a0fec6bfde92aac9c41dce996ebb2d08cc7`、base `6db4d396e5d994388ada1e97c3d28947ffdb9c81`。接受父方已经批准的 `retrieval-capabilities-v2` point-in-time API 变更；不把“必须第二轮 latest”或“普通 churn 必须失败”的旧时序要求作为 v2 判据。本次单一独立审查完成即停，不追加生产修复。

结论：**bounded pass**。未发现本次范围内的生产正确性阻断；该结论仅覆盖下面实际执行的 Linux synthetic 正常操作。旧 root + 新 coverage 的实质禁止得到独立断言和实际 mutant 验证。作者 11 checks / 46 重叠 executions / 8 AB cases 没有获得任何本次独立通过数。

## 源码、构建与实际执行

初始本地 HEAD 为旧 checkpoint `ff458bc591b4e7e444af4464d6eef2513cdb335c`，缺少两个固定对象。GitHub connector 只读身份成功返回 `jyqj`；随后在本 review 目录隔离 clone 并 checkout 精确 final SHA。固定 tree 未发现 AGENTS.md/SKILL.md；已读 CONTRIBUTING、相关 ADR、全部本次生产模块及作者专属 77 个证据文件。适用的 agent-architecture skill 已读，执行与验证证据分开记录。

base → production 的生产差异仅 capability_read.rs、freshness_store.rs、cc-db/lib.rs、capability_status.rs；production → final 的 crates 差异为零。query_handle、read_generation、semantic_coverage、semantic_publish、index_db、rebuild 与真实 stdio query fence oracle 逐字节保持 base。完整 inventory 和 SHA256 见 `source-identity.json`。

自编 harness 复制相关 7 crates，排除 cc-eval，完整原生产文件字节保留在被编译文件的前缀，仅在 cc-db capability_read 与 cc-server capability_status 末尾追加自编 test module。没有 QueryServices 替身，没有旧 v1 拷贝冒充现代码。419 crate files 的原始/被编译字节摘要被逐一核验；恢复 mutants 后再次确认。这与作者省略 inline fixtures 的 prefix wrapper 分开；作者 wrapper 也额外验证了只允许 EOF whitespace 的等同性，没有忽略中间源码差异。

工具链使用存在的官方真实 binary `/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/{cargo,rustc}`，Rust 1.95.0 / host x86_64-unknown-linux-gnu。初次 offline 失败为 reqwest cache miss，按本任务授权通过 crates.io 补齐 `/workspace/.cargo` 可写 cache，之后独立检查 offline。没有 rustup 安装、RO HOME 写入或提权；cache miss 没有表述为服务器拒绝。`binary-identity.json` 给出工具链二进制、Cargo.lock、SQLite source、实际 test/stdio binary 的 SHA256、features 与 Cargo fingerprint。这里使用的 bundled SQLite source 版本为 3.53.2（libsqlite3-sys 0.38.1）；以本次 vendor bytes 为准。

| 本次检查 | 最终结果 | 独立信用 |
|---|---:|---|
| 自编 DB 事务 / FFI / pool1 | 3 passed | 3 unique |
| 自编 default server 边界 / 普通 epoch churn | 2 passed | 2 unique |
| 自编 semantic-http server | 4 passed | 2 新 unique、2 重叠 |
| 三个实际编译 mutant | 全部断言失败，exit101 | 3 killed |
| mutant 恢复后的 DB/HTTP | 3 + 4 passed | 恢复验证，不增加 unique |
| 固定原生产 DB inline | 6 passed | 补充，不计自编独立信用 |
| 原生产 stdio 严格 query fence | 1 passed，覆盖 search/context | 补充，不计自编独立信用 |

因此本次独立正向 matrix 为 **7 个 unique tests / 9 executions**，不是作者的 46 次。命令、cwd、退出码、时长见 independent-runs.json、restored-runs.json、mutants.json；实际原生产 query 执行命令为 `cargo test -p cc-server --features semantic-http --test p7_v11_ready_epoch_independent_review --locked`，工具链/cache/target/TMPDIR 路径与 run_checks.py 相同，cwd 为 source。query-fence.log 保存运行与两个真实 RPC generation conflict。

## 正确性证据

事务内部：先在同一 lease/transaction 读取 generation，再用另一个线程、真实普通 writer connection 提交 index/evidence/semantic epoch、files、symbols、frontier、active-space 与 pending/claimed/failed/eligible/published 的变化。返回值与完整旧 struct 相等；下一笔观察严格匹配新 tuple（index23/evidence29/semantic31，files2/symbols2，active review-b，pending2/failed1/eligible1/published1，freshness index23/basis8/incomplete）。事务结束回到 autocommit，pool1 的随后 identity checkout 正常完成；query_only=1、total_changes 未增加。生成与 freshness 的关系也被独立断言。

真实 normal publish：3 个合成 Rust 文件，实际 CodeIndex、SemanticRuntime、Gated FakeProvider、pool1。worker 在 provider 内等待，status DB snapshot 完成后释放 gate，真实 publish 提交在返回前完成。旧观察为 semantic epoch1/pending3/published0；crossed 的所有 DB fields 与完整 old 相等，root observer 仅一次；稳定新观察为 epoch4/pending0/published3、semantic/dense ready。完整 old/crossed/new 和错误 mixed tuple 写入 `normal-publish-observations.json`。独立允许完整 old 或 new，本次确定性 interleaving 实际命中 old；stable 后实际命中 new。另 6 次普通 index/evidence/semantic epoch 更新均只观察一次，保留完整旧 generation/freshness，不出现普通 epoch 重扫。

负例有效性：在正常数据库提交后实际执行“旧 generation + 新 semantic snapshot”的 split 算法，独立完整 tuple oracle 拒绝；进一步真正编译三个 harness-only source mutants：generation 移出统计事务（击中 pinned old 完整 struct 相等断言）、unsupported HAS_MOVED fail-open（击中真实 in-memory connection 的 fail-closed 断言）、取消 degradation priority（击中 semantic_state=degraded 断言）。均有运行 binary/hash、mutation before/after、exit101 和 FAILED assertion；编译错误不算 mutant killed。恢复原字节后正向重测通过。

service scope：实际 QueryServices 与 SemanticWorkerStatus。无 active、零 eligible、配置期望 space 不匹配分别保持 unverified/partial/backfilling。assembly failure 和 round failure 覆盖 DB ready/backfilling；degraded 再覆盖 failed，并保留 worker reason。DB fields 不变而 callback 后 process failure 可以变化，符合 `process_observed_separately`。真实发布 stable ready 后再设置 worker failure，semantic_state=failed、dense_state=ready 表达数据库覆盖与运行态的两个 scope；随后 degraded 优先。no-project、closed、unwired、pool1 三次 incarnation churn 耗尽均不产生旧 ready/counts/generation。

identity：FFI 的 borrowed Connection/lease 保持 sqlite3 handle 生命周期并排他使用；`c"main"` 是静态 NUL terminated CStr，输出变量为真实 std::ffi::c_int，缓冲存活覆盖同步调用，没有指针逃逸。snapshot 前/后与后续独立 lease 都检查 opened main file；identity validation 只消费 incarnation，不将其 ordinary epochs/数值投影进响应。实际 standalone DELETE-journal SQLite 正常 rename + 新 pathname 库，旧连接仍读73、新连接读97；HAS_MOVED 拒绝旧打开文件，未操作 WAL。in-place metadata incarnation 更新后旧期望被拒绝；ordinary epochs 不被误拒。真实 memory connection 不支持 identity 的结果保守报错，错误/非定义 bool 控制结果同样不假设 unmoved。

[SQLite 官方 file-control 文档](https://www.sqlite.org/c3ref/c_fcntl_begin_atomic_write.html)将 HAS_MOVED 的参数定义为 integer pointer，用于 opened file 被 rename/move/delete 的判断。本次 vendored Unix implementation 与 SQLITE_NOTFOUND 历史内部 fallback 已直接读；生产 wrapper 没有采用该 fallback。**只实测 Linux bundled Unix VFS；macOS、Windows、自定义 VFS 未测。** 不把这个诊断的 fail-closed 外推成普通 query 的可用性结论。

严格 query fence：源码逐字节未改；在实际 semantic-http stdio binary 中分别将 search/context 的成功 synthetic loopback query IO 挂起，正常增量 index 和 ready 观察推进后再释放。两者都返回 `-32603`、retryable=true、`index changed during retrieval after 1 attempts; retry the query`；provider IO 不内部重跑。随后稳定查询通过当前 source/generation 断言。不是仅静态比对或模拟错误。

## CI 与审查边界

现 CI YAML 的默认 workspace tests 实际编译 current cc-db/capability_read 和 server/capability_status；semantic stdio build 也编译当前 production。旧 candidate/checks 已从 current tree 删除，legacy.rs 仅是明确 red counterfactual；迁移 entry 读取 current-status-under-test，source guard 确认 EOF-only prefix 同一性。不能把默认 CI 中 `cfg(feature="semantic")` entry 的零 tests 当 v2 oracle 通过。CI 中未找到 explicit semantic-http test command；本次实际编译/执行了它，但此自动回归覆盖缺口应单独补齐，不是本次生产数据一致性 reject。

本结论不覆盖所有调度、跨平台 VFS、serialization/delivery 后替库、一般 worker throughput 或 100k 性能。没有重跑作者 AB、100k、GC、WAL kill/fault、曾在其他任务 RO 拒绝的具体动作、heldout/eval 或真实 provider。生产/版本/ledger 只读，所有工作文件位于本 review 目录（除用户明确授权的可写 Cargo cache）。原 source checkout 最终 clean。

保留自编 fixture 首次失败：pending/claimed 使用同一 live doc key 违反唯一约束；helper 使用 &IndexDb 而 API 要求 Arc；误以为 side-effect-free assembly 自动插入 active-space row。均已用正常合成 fixture 修正，失败日志和 receipts 在目录中，未改生产或放宽成功断言。空参数 process 探测曾被自动审批因目标不受约束拒绝，没有重试该空动作；明确目标的后续 review 操作成功，本次无因此未完成的检查。

## 复现

在本目录隔离 clone 指定仓库，`git -C source checkout --detach 29b03a0fec6bfde92aac9c41dce996ebb2d08cc7`；调整 run_checks.py 的真实工具链/cache 路径后运行 `python3 prepare.py`、`python3 run_checks.py`、`python3 run_mutants.py`。只创建合成库，TMPDIR 指向 review 目录。mutant restoration 必须完成，最后 `python3 collect_identity.py`、`python3 audit_evidence.py`、`python3 finalize.py` 核验。在有许可的环境另执行上述 original production strict query 命令并保存 query-fence.log。输出不会保证 bitwise 重建或时延一致。
