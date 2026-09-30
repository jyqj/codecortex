# 06｜验证登记与阶段门禁

> 测试名称/文件路径是拟实施位置；只有现有命令标为“当前可用”。Vxx 是任务验收引用，不代表已通过。详细 benchmark 方法见 09-BENCHMARK.md。

冻结616文件、6605476字节，摘要`471919b1d73828db2938d314e3319dd34827a7c1828388925c45b1db7372f473`。38条命令收据，源码/归档/日志/不可变二进制均核验；最终接受版本对当前源码重新运行全套验证；中间版本曾复用的旧收据只保留历史，不代替最终结果。证据目录`artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930`。两工具链各workspace 1767 passed/57 ignored、HTTP 255 passed/50 ignored、专项 71 passed/2 ignored、真实stdio 24 passed、取消协议1 passed、watcher17 passed。各组内部失败0，忽略项不计通过；测试组有重叠，不相加为唯一测试总数。固定51题306请求的逐题Top-1/nDCG无负差分，无效源码命中为0，raw回放一致。原Partial/S11继续失败，本次新增18次真实预算省略Partial，涉及6个问题；完整检索Gate保持not_passed，不能称G5/M2通过。gold与评分公式未改。一次默认并行HTTP测试的debug索引计时超过原500毫秒门限，原失败保留；源码不变的独立复验通过，后续计时相关测试组串行执行，内部并发测试不变。未放宽阈值，不把隔离通过当成共享负载性能认证。

## 1. 验证层级

L1 单元/属性测试：解析、hash、排序、状态机，快速确定性。L2 数据库/编排集成：真实 SQLite、FTS、构建、并发、故障。L3 产品黑盒：当前精确构建的 MCP stdio 子进程；OCE 对照仅公共 HTTP。L4 外部 oracle：固定编译器/LSP 或人工 gold，仅在声明支持范围可用。L5 规模/资源/成本：release runner 与原始样本。

不把 mock/fake 成功称作真实模型成功，不把进程内调用称 stdio，不把暖缓存执行称冷构建。基线已知失败用 issue/任务绑定的 known-failure 清单留痕；P0 能发现它们即完成测量目标，不要求 P0 偷做 P1/P2 修复。对应修复阶段结束时移除豁免并转成阻断测试。

## 2. 验证目录

| ID | 验证包 | 最低场景/断言 | 层级 |
|---|---|---|---|
| V01 | 可复现构建/旧测试 | SHA、dirty/binary digest、toolchain/SDK，现有测试/stdio、ignored 明列、全新 target 冷构建 | L1–L3 |
| V02 | benchmark schema/lock | 重复 ID、BOM、坏 JSON、空 gold、路径穿越、dirty、submodule、输入 hash、single/suite 均拒漂移 | L1/L3 |
| V03 | scorer goldens | primary/supporting、linear nDCG、重复、glob、空结果、must_contain 注记、native 范围/组/chain | L1 |
| V04 | 传输与报告 | MCP 握手/错误/取消、HTTP 失败、readiness、timeout、退出码、raw→metrics/report 可重算 | L2/L3 |
| V05 | 排序与范围 | BM25 单调、soft 预选外命中、Some(empty)、语言/path 交集、DSL、所有 lane 与最终 hydrate 不越界 | L1–L3 |
| V06 | 公共表面 | JS/TS/Rust/Python/Go API/body/visibility 变更、unknown≠empty、Canonical hash | L1/L2 |
| V07 | dirty oracle | 增删改/重导出链环/大 fanout/负向查找、新同名、warm/cold cache 多轮固定点与 full parity | L2/L3 |
| V08 | 项目模块规则 | TS config/extends/exports、Rust crate/module、Python src/relative、Go module/work、配置-only失效 | L1–L4 |
| V09 | parser/chunk/source | 一次 AST、长函数/类/gap/超长行、UTF-8/CRLF、span union、实际 bytes 和预算 | L1/L2 |
| V10 | 文档身份与投影 | 顶部插入/rename/重复函数、source vs embed text、模板变化、旧 chunk wire id 兼容 | L1/L2 |
| V11 | 查询执行/缓存 | deadline/取消、有界线程、1连接读池、锁外网络、混代读拒绝/有限重试、缓存 key 完整 | L2/L3 |
| V12 | 选择与预算 | 首块超大、metadata 膨胀、重叠、单文件 locate/多文件 facets、完整 JSON、source freshness | L1–L3 |
| V13 | schema/epoch | typed write effects、一次 commit 推进、aux不刷内容、rollback、换库 incarnation | L2 |
| V14 | outbox/publish | claim CAS、过期 lease、删除竞争、两进程、模型切换、artifact-before-manifest、重复 ack | L2 |
| V15 | provider 验证 | 数量/index/维度/NaN/Inf/zero、429/5xx/auth、timeout、并发/批次/费用上限、fake/live 区分 | L1–L3 |
| V16 | exact vector/filter | 手算 cosine/top-k、tie、scope 先过滤、删除不返回、bounded memory、不同空间拒混 | L1/L2 |
| V17 | 恢复与 GC | 每个 crash point、重启/lease、staging换库、cache损坏/删除、GC/publish 竞争、无自动无界收费 | L2/L3 |
| V18 | MCP/配置兼容 | 14工具不丢失，旧 mode不变，新参数贯穿schema/sanitize/dispatch/status，默认无网络无key可用 | L1–L3 |
| V19 | 检索质量/消融 | compat/native分报，真实公开 corpus，hard negatives、holdout、facet/span、macro/micro/CI | L3/L4 |
| V20 | 性能/资源/成本 | 1k–100k，冷建/no-op/body/API/config/batch，C1/4/8/16，RSS进程归属、no best-of、费用复用 | L3/L5 |
| V21 | 回滚/发行/文档 | 新旧 schema 降级重建，cache不误读，默认/semantic 两包，SDK/MSRV，文档事实漂移检测 | L2/L3/L5 |
| V22 | 可选增强准入 | ANN 对 exact filtered recall；LSP snapshot/生命周期；rerank heldout收益/成本和失败降级 | L1–L5 |

## 3. 阶段 Gate

| Gate | 阶段 | 必须提交的证据 |
|---|---|---|
| G0 | P0 | V01–V04 基础实现、首份可复算 baseline、已知失败清单、scoring/spec locks；平台 blocker 清楚，不伪绿 |
| G1 | P1 | V05 全部过；V03/V04/V18 兼容；V19 排序前后逐题 diff；基准不晚于修复 |
| G2 | P2 | V06/V07；缓存历史不改变事实；partial frontier 可恢复；每支持语言已知缺陷闭合 |
| G3 | P3 | V08 与配置-only V07；支持范围声明；导入解析成本无重复全仓扫描 |
| G4 | P4 | V09/V10；源码定位/版本正确；切块消融与数量/资源成本报告 |
| G5 | P5 | V11/V12/V18；网络等待锁验证用 fake；本地检索质量/预算完整性与 tail latency |
| G6 | P6 | V13/V14/V16/V17 的无网络闭环；正式单库修订 ADR；显式重建/GC/fencing证据 |
| G7 | P7 | V15–V19；fake全过，live受授权单独记；默认离线零网络；语义覆盖和费用可解释 |
| G8 | P8 | V01/V18–V21 全量 release profile，holdout及100k、原始报告和回滚演练；blocker=0 |
| G9 | P9 | 分项 V22 决策报告；收益不够则 deferred，不阻塞 G8 |

阶段先后见 04-PHASES；同一 gate 的所有任务必须有当前 run-id 证据。对没有真实 provider 的环境，P7 工程集成可验证，live 认证标 blocked/deferred，并在 M3/M4 发布能力中清楚限定；不能以 fake 完成全部真实语义效果声明。

## 4. 硬失败与统计门

硬正确性：scope泄漏、错误源码/版本、删除复活、错误空间、非法预算对象、编译/基础测试失败、缓存历史改变 deterministic 结果、输入锁失效、评分器 goldens 失败均阻断。不能用吞吐/nDCG 提升抵消。

质量/性能 gate 不凭本轮设计虚构绝对 SLA。P0 在固定设备/配置上冻结基线、margin、重复数与置信区间方法；参考初始 guardrails 在 09-BENCHMARK 第10节。正式报告包含 sample N、比较适用性和 inconclusive，数据不足不能自动 passed。

## 5. 当前可用命令与未来命令

当前实际执行的命令、工具链和收据：`artifacts/benchmarks/p5c-20260929-completion/final-v5-20260930/validation.json`。下方示例不是新测试结果。

```sh
cargo test --workspace --locked --offline --lib --tests
cargo test --workspace --locked --offline
cargo build --locked --offline -p cc-server --bin codecortex
```

依赖缓存不足的 offline 失败和源码编译失败分开。冷构建使用独立临时 CARGO_TARGET_DIR/隔离 runner，不删除开发者现有 target。不擅自变更全局 SDK 或 toolchain；固定运行环境/容器或显式局部环境设置，记录实际命令和原因。

冻结验证：`artifacts/benchmarks/p5c-20260929-completion/run-final-complete.py`；独立审计：`audit-final-20260930-v2.py`。 固定51题306请求的逐题Top-1/nDCG无负差分，无效源码命中为0，raw回放一致。原Partial/S11继续失败，本次新增18次真实预算省略Partial，涉及6个问题；完整检索Gate保持not_passed，不能称G5/M2通过。gold与评分公式未改。schema21不变，public adapter7、retrieval policy16、packing spec2。预算针对code_index_context结果对象，不包含JSON-RPC帧；其他旧工具形状保留原预算。没有真实provider/vector、持久化semantic epoch、原子文件系统快照、公开holdout、跨平台或发行认证。

## 6. 每批证据模板

```json
{
  "batch_id": "P0-A",
  "engine_sha": "<actual>",
  "worktree_digest": "<actual>",
  "tasks": ["P0-001"],
  "commands": [{"argv": ["<actual>"], "exit": null, "log": "<path>"}],
  "validations": [{"id": "V01", "status": "not_run", "run_id": null}],
  "known_failures": [],
  "quality_delta": null,
  "performance_delta": null,
  "review": "pending",
  "rollback_checked": false
}
```

模板字段不是运行结果。收工时必须填写真实值，任务 status 与 evidence 配套，未来工单引用此模板不能自动变成 done。
