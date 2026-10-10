# 原文档基线核对记录｜2026-10-10

原命令 `./scripts/update-doc-baselines.sh` 已在固定源码 `b9b089bb4eae072affe9326681d4980eae15fd84` 实际执行一次，退出码为 **0**，同时输出 **3条DRIFT**。本记录保留两项结果，补充此前缺少的原命令执行证据。

## 实际来源与执行

- [GitHub原运行38020782259](https://github.com/jyqj/codecortex/actions/runs/38020782259)，attempt1，job114121097102。
- 实际workflow来源为 `de14e6c7e80524efb0e43bec74bb48f518cfbc67`；产品与文档checkout固定为 `b9b089bb4eae072affe9326681d4980eae15fd84`，完整tree为 `6461938788665701cfa4fa095a064a3a404ec492`。
- 原脚本SHA256为 `04d63124fc8405610c91fb4a591e17e2dd06ce1f20cf995123263b12a4b0b59c`，执行的是原 `cargo test --workspace --all-targets`，没有追加feature或release选项。
- 原脚本于03:31:02.559350 UTC开始、03:38:46.711550 UTC结束，wall为464.152196577秒；退出0、无终止信号，外层stderr为空。
- Rust/Cargo1.95.0，Ubuntu hosted runner，明确的2个Cargo build jobs、debug0、incremental0设置；不将此记录称为默认构建配置的性能测量。

## 原脚本输出与历史对照

| 原脚本比较项 | 文档明确标注的历史值 | 本次原脚本输出 |
|---|---:|---:|
| cc-db tests | 142 | 206 |
| passed footer汇总 | 1258 | 2815 |
| ignored footer汇总 | 15 | 76 |

原脚本逐字输出：

```text
DRIFT: cc-db tests changed: doc=142 actual=206
DRIFT: passed count changed: doc=1258 actual=2815
DRIFT: ignored count changed: doc=15 actual=76
```

[固定版本TEST_PLAN](https://github.com/jyqj/codecortex/blob/b9b089bb4eae072affe9326681d4980eae15fd84/docs/TEST_PLAN.md) 把相关表明确标为历史快照。上述差异如实反映本次原脚本与该历史快照的比较；本记录保留历史表。成功退出并不表示没有DRIFT，DRIFT也不等于本次测试失败。

**2815、76及其和2891只按原脚本汇总口径解释。** 脚本对匹配的 `test result:` footer求和，内部Cargo全文没有被输出或归档，因此不能从该制品重建去重后的唯一测试方法总数。不同配置、嵌套执行及filtered-only的独立含义不能由汇总数补猜。

原cc-eval分项具有多行输出，原样保留如下：

```text
  cc-eval:    68
0 passed + 7
0 ignored
```

该制品还保留脚本报告的schema v25、MCP tools14、corpus cases94、framework resolvers16。这些字段来自脚本自身的来源检查；该脚本没有执行15表oracle统计核对，也不提供all-features、发行或规模性能通过结论。

## 文件状态与完整原件

实际before/after完整tracked index、HEAD及tree一致，tracked status、源码diff、lock diff为空；三份Cargo.lock逐字相同。检查没有覆盖所有untracked文件，因此不声称整个宿主或工作区完全不变。

原artifact11658346818含60成员，完整ZIP为1,329,132字节，SHA256为 `e705baaa44c1184d35b9c8f1a6a89a6aa7f96839ccf129d1df40ebfcf58db48e`。全部原ZIP字节已按顺序保全，重建说明和两个实际blob见 [完整原ZIP保全记录](../../original-zip-custody/baseline.json)。原接收只做了一次成员CRC/读取，原 `files.json` 的59条记录均对应实际成员。

- [独立原命令终态接收报告](independent-original-baseline-terminal-review.json)
- [完整选定文本和60成员身份清单](complete-selected-original-support.json)

这份记录补足原文档约定中的实际命令证据。P8-018的最终任务状态仍受其原硬依赖和整体验收约束；本记录本身不改变任务计数，也不增加“必须零DRIFT”或统一重跑要求。
