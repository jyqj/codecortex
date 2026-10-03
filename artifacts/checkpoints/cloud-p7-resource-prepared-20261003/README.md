# P7-015/V20 最小独立资源 prepared 块

Draft PR89；执行runner源码 dee17f9a2965740297c154249b546ed90fe10992。产品为当前真实semantic-http debug构建，build source2b64c7c8f7f2d3736d621546706ccedc5b4406dc，两source间生产/Cargo diff为空。产品SHA2563ec4b720686e18742c86c51b0bd5cf1a13eea3fc2cff12edfc8034f003d8fc69，原件硬链接保存在/workspace/p7-resource-build-fixed/codecortex。不是release性能验收。

1k/5k/10k三个真实合成Rust规模 × C1/4/8/16 × distinct/repeated query =24完整cell、768请求；每cell32原始样本，无best-of，真实client在途峰值达到相应C。固定100ms burst schedule记录offered与sent/response，client排队明确，非闭环隐藏尾部；backendqueue/service/DB锁等待没有接口，null。distinct query IDs跨C不重用；repeated query是工作负载标签，不把内部LRU命中缺数据填成事实。响应保留hydration读缓存计数和SQL/grep/packing实际说明。

1k/5k/10k全部cell零错误/空hit，初次cold index、no-op、body、batch10/100/1000实际调用；查询与index分别在首个后台loopback provider被hold期间进展。前台查询/索引构建彼此未同时压测，不称完整混合负载；API/config-only、watcher/model switch/reopen等未做。模型是独立自有loopback子进程，仅合成源码；128维固定向量，使用usage字段仅fake单位，不是账单。模型总calls与outboxattempts记录；已知superseded中间结果可能浪费一次成功调用，不隐去。

设备：全部repo/DB/cache同/tmp设备（独立8.8GiB临时文件系统，内存影响计入cgroup），各档同binary和完全相同config/endpoint。4CPU配额/16GiB cgroup。20ms sampler计product root/runner/model各PID的RSS、CPU tick、threads与/proc/io；真实process children枚举不可用，所以完整tree峰值和short-lived外部子进程缺口null。VmHWM与瞬时RSS分列，不伪称OS cache cold。

| 文件规模 | product root sampled peak RSS MiB | peak threads | observed process CPU seconds |
|---|---:|---:|---:|
| 1000 | 139.91 | 33 | 13.14 |
| 5000 | 344.07 | 35 | 57.84 |
| 10000 | 579.57 | 35 | 150.24 |
| 50000 | 780.59 | 19 | 34.10 |

50k不是成功档：真实cold stdio index报-32603 too-many-SQL-variables，load_seed_rows_on 的NOT IN每排除文件1bind越界。N=50k即使低RSS仍失败，非OOM；100k因此未执行。原错误、raw/资源与源输入哈希保留；正确性修复属下一独立生产块，不能用1k/5k/10k抵消。

32样本的p95是描述值，不是规范至少200同profile正式尾延迟/CI；一次cold build不是30次构建profile统计。无release、真实SSD/page-cache-cold、六repo/live/quality/发行认证。release可评估/tmp独立target串行构建，需容量/内存停止线；不与大档并行编译。

采样初版把可选children错误丢弃整个snapshot，tree空和为0无效；原轮stdio请求成功但资源无效，只保留hash，不能算资源pass。修正runner后独立重跑才产生本块有根RSS/CPU原件，tree仍null。pilot metadata/mockdimensions/tablename错误均留本地hash，未改产品来迎合测试。

PR88 c91d672原119文件原样纳入、verifier过15scenes21kill，仍completed staging/GC边界、内部WALrename/markunlink/genericpin未测。PR85 response-before-cache两fakecalls/真实billingunknown口径不改。PR81 CI183与PR84 CI189精确head直接readbacksuccess历史按source保留。

PR86/90/91只GitHub metadata/body count/hash readback，未fetch题库/读gold/跑ranking。最新四repodevelopment-admitted301native256compat、100source476spans，280correlation不等600独立单位；formal600/cleanholdout/20blocks/ranking均0，剩余来源/custody/live open。

P7-015/P7-016保持todo/prepared，014未完成硬前置阻断，150done/41todo/1in_progress不变。

读取压缩raw并独立派生指标（只读）：
```sh
python3 artifacts/checkpoints/cloud-p7-resource-prepared-20261003/analyze.py
python3 artifacts/checkpoints/cloud-p7-resource-prepared-20261003/verify.py
```

新运行用全新空目录，receipt记录精确命令。fixed/内gzip是原parsed RPC payload/实际wire hash+bytes、20ms samples、loopbackcounter、源码输入锁和原summary；压缩不修改原件。measurements.json逐请求保留全样本。源/DB/cache仍留/tmp，下一块经授权可只回收自己的可重建中间产物并记录，raw/hash不删。
