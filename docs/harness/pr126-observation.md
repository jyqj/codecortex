# PR126 现有证据复核与可复用 harness 修复

证据固定在 `da8b618cbf8b67683c6b16880419089f7159c15e`。读取 baseline
`513a98c9a94b15ec77153df41af26fa3c8c0b5e8` 和 candidate
`e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207` 的原资源日志、环境、summary
及事件时间线。两提交的原 `scripts/p7_release_resource_preparation.py` 相同。
没有执行历史 driver、产品、100k、原件、旧 42files/localdiag 或旧被拒测试；
没有修改历史 artifacts、安全配置、生产源码、Cargo.lock。

## 观测结论

**两版原 ready gate 均失败；candidate 的 12GiB guard 失败保留。**

| 原始观测（B） | baseline | candidate | candidate − baseline |
| --- | ---: | ---: | ---: |
| startup 首个 cgroup 样本 | 4,306,055,168 | 8,217,739,264 | +3,911,684,096 |
| startup cgroup 采样最大值 | 4,307,361,792 | 8,219,230,208 | +3,911,868,416 |
| backfill-drain 首个 cgroup 样本 | 6,708,031,488 | 10,618,224,640 | +3,910,193,152 |
| product root RSS 采样峰值 | 2,052,325,376 | 1,995,472,896 | −56,852,480 |

startup 所给 4,307,361,792 / 8,219,230,208 是阶段采样最大值，不能称准确阶段起点。
drain 首样本分别晚于 summary 的 drain_start 11.655414ms / 17.113083ms。
新 JSON 逐阶段保留首样本、最大值、样本数、输入 SHA256；准确阶段起点组成填 null。

原 build 日志仅 `wall_seconds/pid/cgroup_memory_current/disk_free_bytes`；
runtime 仅 `time_ns/phase/product/product_tree/runner/model/tmp_free_bytes/cgroup_memory_current`；
原 environment 也没有 `memory.stat`。所以历史 anon/file/slab 均 **unknown**。
原日志没有可复核的 group path/inode/member 清单，完整 process tree/PSS/lifetime peak 也 unknown。
共享 cgroup 含 build/cache/harness，既不能把 3.91GB 的启动差额归因于候选产品，
也不能由较低的 root RSS 抹掉资源 gate 失败。baseline-first 顺序差是观测；缓存、
构建残留或其他因素各自造成多少变化，因缺少组成及独立配对而 unknown。

candidate 首次采样越过 12GiB 在 drain 181.409308344s，最后 HTTP 返回在
182.746808747s，未响应 status RPC 在 182.742759635s 发出，随后原 Queue.get(300)
以 Empty 结束，cleanup tail 到 488.502292s。原日志没有 guard 信号的准确时间，
这些时间只能界定观测，不能假称精准 183s 终止时刻。baseline cleanup tail 为
307.639484s。资源终止后的计数不能作为完整 300s 吞吐，cleanup 也不算通过 ready。

## 新模块与接入边界

`scripts/resource_harness/runtime.py` 不启动产品、不选择实验、不执行终止升级。
它给未来 driver 提供 `Observer`、`StdioRPC`、`guard_stop`，只使用 Python 标准库。
旧脚本及历史 artifacts 保持原样，因此历史结果不会被新模块改写。

接入时，新 driver 在创建 owned Popen 后，用一个 `StdioRPC` 替换旧 reader/queue，
不要同时保留两个 stdout reader。`rpc()` 方法签名保留 method/params/timeout=300，
返回 JSON-RPC result；初始化协议 `2024-11-05`、initialized 通知、tools 数量 14、
tool structuredContent 解包仍由 driver 按原协议执行，不由此模块改变。

```python
observer = Observer(resource_journal, measured_group,
                    scope="shared_build_cache_runner",
                    processes={"product_root": product.pid,
                               "harness_root": os.getpid(), "model_root": model.pid})
observer.begin_phase("startup")
transport = StdioRPC(product, rpc_journal, phase=lambda: observer.phase)
# 同步记录 generation/build/preflight/cold/drain/cleanup 各阶段起点；周期 sample 可继续 20ms。
# 保持原 memory.current > 12*1024**3 判断、guard 名称、原 terminate 行为和原 persist。
guard_stop(transport, "memory_above_12GiB", product.terminate,
           observed_bytes=observed_current, group_path=str(measured_group))
```

guard 分支先持久化原 gate 失败，再 `guard_stop` 通知所有 unresolved RPC，最后才
执行原受控终止动作。guard 事件记录准确观测时间、reason 与传入的实际 group/bytes；
不声称这是内核信号完成时间。新请求立即失败，pending 请求立即收到 `RpcFailure`
及结构化原因，不再等待 Empty300s。ready 判断前还必须调用
`transport.pending.raise_if_terminal()` 并保留 driver 的原 guard_stop/failed gate；
已收到 ready 响应不清除 guard 失败。ready 的原 deadline 前后检查仍保留，
迟到响应不接受。

EOF、root exit、reader/write 错误、RPC timeout 独立记录；每请求第一份已投递结果
获胜，迟到响应继续写日志。root exit 通知不依赖 stdout EOF；退出 watcher 不阻塞在
可能被继承的 pipe 上。调用者仍执行原 EOF15s、原受控 terminate 与其15s等待，
没有新增 kill；进程退出及 pipe 关闭后 `transport.join()`，再关闭日志。
若后代保持 pipe，调用者必须按既有 owned-process 清理政策处理；该模块不会擅自杀后代。

memory.stat 原计数完整保留，只有 anon/file/slab 子集标记 bytes（其余有计数单位）；
slab 子计数重叠，不相加。每次 read_start/read_end 表明非原子读取。
group path/inode/cgroup.procs 与每个 process role/pid/start_time/state/cgroup membership
分别标记；root RSS 从不冒称完整树。不可读或缺少字段填 null 并保留错误，不填 0。
Observer 不写 cgroup/proc 文件，也不改变 cache、memory limit 或安全策略。

## 验证与复核

```sh
PYTHONDONTWRITEBYTECODE=1 python3 -m unittest discover -s tests/resource_harness -v
PYTHONDONTWRITEBYTECODE=1 python3 scripts/resource_harness/analyze_existing.py \
  artifacts/checkpoints/localwidth4-100k-paired-20261003
```

fixture 全为本次自写 Python 子进程、临时普通文件或内存队列：验证 guard 先通知、
多个 pending 唤醒、response/guard/EOF/exit 顺序、并发竞争、迟到响应、timeout、
坏 JSON、EOF0退出、并发日志完整、缺失数据 unknown 和只读组成/阶段标签。
短 timeout 仅显式传给无害 fixture；生产默认 RPC300s/ready300s/EOF15s/12GiB 未变。
本次不运行全仓 Rust 测试或产品 GC/WAL/kill/crash 测试。

最终结果：14 项 unittest 全部通过（0.108s），`git diff --check` 通过。
两源提交 driver 的 SHA256 均为
`a76865a466252f5863c8e0ad278c4c6814093d4a4a629a9409acf2e6b916901b`。

## 下一对独立资源测量建议（未执行）

先完成两版 locked release build 并记录组成，随后给每次正式 run 分配独立、确已
授权的 measured cgroup/环境，验证 group path/inode/member，明确 runner/model 是否
同组；如不允许隔离则明确记录 shared 并把因果结论保持 unknown。
在生成前、生成后、产品启动前、cold 起点、HTTP release/drain 起点、guard 和 cleanup
边界同步记录 memory.current/stat/events 与 root RSS。日志写到各次独立目录，
关联相同设备/配置/源 SHA/二进制摘要；build 阶段与测量阶段分开。
不清系统缓存、不改 limit 或安全配置；观察 file/slab 的实际基线。
如果另获授权做多个配对，用预注册 AB/BA 顺序检查顺序效应，不能用本次单对推断。
仍保留原 12GiB、300s、EOF 与所有协议数值和 gate 判定。这里仅提出下一次设计，
本任务没有重跑任何产品测量。
