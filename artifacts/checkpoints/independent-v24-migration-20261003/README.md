# 独立 v24 迁移 review（2026-10-03，单轮完成即停）

结论：**整体 BOUNDED_REJECT / BLOCKED_INDEPENDENT_EXECUTION**。这拒绝的是本轮独立迁移验收签字，不是已发现生产迁移代码缺陷。代码版本失效路径与固定作者证据的只读核查为 **BOUNDED_PASS**；本轮产品构建、普通 reopen、regression、pinned 产品亲跑均为 **0**，不能用作者执行记录补成独立通过。

固定生产 SHA `9ebdb155c64e094b3d774be41dbe7ab8c4e222c1`，证据 final SHA `6db4d396e5d994388ada1e97c3d28947ffdb9c81`。PR101 的 v22 是 `574f7598662334c63e020da136c87f4f7281554d`，不是名为 `101574f759` 的提交。中间实际生产 v23 是 `57bedbaf193e28be34271a290d28c90dc662b613`；修复后但尚未 bump 的 same-version 构建是 `bf10b6477612f32d991bbfea3ed7791f468acc78`，不能用它冒充旧缺陷生产构建。

## 1. 权限边界与阻断

读取当前工作区及四个固定树，未找到 AGENTS.md/SKILL.md。已读 CONTRIBUTING.md；读取 agent-architecture skill 的证据与实际执行分离原则，无多 agent 工作。当前生产工作树保持原 `ff458bc591b4e7e444af4464d6eef2513cdb335c`，没有 checkout、生产代码编辑或 ledger 改动。临时源码快照、只读证据副本及本轮所有文件写入均在本目录；runtime 与作者副本不入提交。Git 对象与 review branch 元数据操作仅为用户要求的取证及提交交付。

默认 PATH 没有 cargo/rustc。读取现有 `/workspace/.cargo/bin/rustc -Vv` 和 cargo 版本时，工具链代理尝试创建 `/home/agent/.rustup`，两次返回 `Read-only file system (os error 30)`。见 [blockers.json](blockers.json)。按照用户“自己目标权限拒绝停对应动作”，停止工具链动作，未设置另一 RUSTUP_HOME/CARGO_HOME、未直接调用其他工具链路径、未 chmod、未换目录或凭据绕过。没有真正启动 cargo build，因此本轮没有 binary hash 或 feature 构建证明。源码导出目录不是构建结果。

无 GC/WAL kill/fault injection，无 semantic_runtime 写入动作，无 heldout/私源读取，无真实 provider（0），无 CI 阈值修改。没有重试作者被拦的 PR 动作；本轮新 review 的 PR 可用性只读查询返回 Forbidden，已停止对应 PR 操作，未发出 draft create 请求、未换入口。

## 2. 代码失效路径（只读）

`cc-db/src/index_migrate.rs:41` 的 schema24 对所有非零非24版本返回 Mismatch；旧 v21 additive 特例不再绕过 reset。`IndexDb::open_and_ensure_schema_inner`（index_db.rs:298）在 mismatch 后快照旧 epoch，执行既有 in-place schema reset，重新建表并由 read_generation::ensure 创建新的非零 incarnation，epoch floor 为旧值+1，返回 Initialized。普通旧库的文件/符号/manifest 派生行因此失效。失败后 unlink fallback 是现有分支，本轮没有故障注入，不能对该分支或 crash/WAL 安全作验收。

`CodeIndex::set_project`（engine.rs:249）设置 needs_initial_index；auto_index 开启时调用普通 build_auto_index(false)。作者迁移 fixture 关闭 auto-index，实际观察的是 reopen 后 files=0，随后 `index(full:false)` 解析并恢复边。**关闭 auto-index 后仅 reopen，不等于已恢复全部边**。本 review 把 automatic invalidation/reset 与普通 index 的重新解析分开。

`cc-model/src/resolution.rs:10/226` 将 manifest 常量设3且 validate 严格等于3；`resolution_dependency_store.rs:43` 的读路径先校验 digest，再 validate，再检查 SQL version 与 payload 一致。v3 reader 拒绝 v1/v2；v1/v2 reader 依各自常量拒绝 v3。旧 QueryHandle 持有 Arc<IndexDb>，search 的 generation fence 重新读 incarnation/epoch，变化会清缓存并拒绝混代结果；ReadGeneration 的 local_key 包含 incarnation+index_epoch，graph_key 加 evidence_epoch。代码支持普通 in-place 重建后的旧 handle 刷新，不能据此推导 pinned raw transaction、并发迁移、crash 分支全部正确。

`type_atoms` 最终过滤只排除 ASCII syntax-only 且保留 `_`/`$`/非 ASCII 字符；`...` 不作为名字，合法符号域能进入 type edge 与 name bucket。该文件 blob 与 repair `671063b11af8cb40a0d526098de82e684dd24aca` 一致，既有独立 `9e0c34`、Go102/`0647889` 的 bounded pass 范围不扩大。

## 3. 构建身份核查（作者证据，不是亲建）

| 构建 | 固定源码 | schema/manifest | 作者 binary SHA256 | 本轮 hash 核源码文件数 |
|---|---|---|---|---:|
| PR101实际旧产品 | 574f7598662334c63e020da136c87f4f7281554d | 22/1 | 0154e6ee05dca4a6f545c5729fed02b8944de64a84982a34eb698470a7d42464 | 375 |
| 实际缺陷中间产品 | 57bedbaf193e28be34271a290d28c90dc662b613 | 23/2 | 3f9b5fd6cec0f231131961cb125576fb5f2f462a264c147c7306ab6a87103fc5 | 375 |
| 修复同版本对照 | bf10b6477612f32d991bbfea3ed7791f468acc78 | 23/2 | d9f8ca660def8d2391836ff5fbdca92e7dd8340b846f74aa06f9adc827821023 | 376 |
| 候选默认产品 | 9ebdb155c64e094b3d774be41dbe7ab8c4e222c1 | 24/3 | d378dbe5887331ff3f52d82a1540be0cf3cd5905c3f7d4d32e32cebc5e36e11a | 376 |

均为作者 default feature receipt。源码 hash 由本轮读取 Git archive 的 Rust/SQL/Cargo 字节复算，0 mismatch；未读 gold/heldout fixture 字节作 oracle。final-v24 与 same-v23 的 target 路径不同，compiler-artifact 记录标示相应独立 target。**没有拿到并复 hash 这些作者 binary，不能把声明的 hash 认成亲建身份**。本轮 runtime/v22、v23、v24/source 是不同固定 SHA 的源码导出，没有 target 被复用。

## 4. 十条迁移与不可变旧库核查

[audit.py](audit.py) 从旧 fixture 源码使用 Python ast 独立读出 class、消费函数和注解目标；TS `$` 用明确的最小 ASCII 标识符/声明语法核对。没有导入作者 validate_migrations.py 的 expected 字典，也没有用 after_types 倒推目标。这是小范围符号关系 oracle，不是完整 TS 解析器。自己的 model fixture 使用 `review_use`，并验证同版本缓存缺边的负对照、跨版本 reset→普通解析→稳定 no-op、manifest reader/payload 版本矩阵。

| 语法身份 | v22→v24 作者记录 | v23→v24 作者记录 | 本轮实际产品迁移 |
|---|---|---|---|
| Python `_` | 一致 | 一致 | 未跑 |
| Python `__` | 一致 | 一致 | 未跑 |
| TypeScript `$` | 一致 | 一致 | 未跑 |
| Python `℘` | 一致 | 一致 | 未跑 |
| Python `℮` | 一致 | 一致 | 未跑 |

五个保存的 v23 SQLite gzip 快照由本轮解压到本目录，验证 snapshot SHA256，然后用 `mode=ro&immutable=1` 读取：实际 user_version23，manifest2，type edges均0，generation 与 intermediate receipt 一致。fixture源码 hash 与 receipt 一致。最终 v23 before generation 又与旧 snapshot/intermediate full 一致，未混入修复同版本 fresh 库。v22路径是作者 old binary 执行 receipt，未保存可供本轮查询的原始 v22库；其真实性核查强度低于 v23 snapshot，不宣称亲跑生成。

10路径作者观察均为 reopen schema24/files0/new incarnation/epoch提高，普通 index parsed1；最终唯一 UsesType edge 的 consumer/target 与语法 oracle一致、target UID非空、对应合法 bucket存在；再次 reopen/index parsed0/skipped1且 receipt整个状态稳定。same-version五库作者对照均 parsed0、旧缺边仍0，而 fresh恢复1；其 before generation 与缺陷旧库一致。71个 final 文件的 SHA256全部与 artifact-manifest一致。

**源码/mtime未改的证据边界**：本轮读到保存的源码字节及旧记录 input_mtime_ns；没有访问作者原 runtime 目录，不能重观测其当前mtime。作者 final脚本在v23升级前断言 hash及mtime不变、升级后断言hash不变；v22通过copy2复制源码。这些属于作者脚本/记录证据，不是本轮 source+mtime 的端到端亲跑证明。

## 5. pinned reader 与验收边界

作者两个旧 handle记录在本轮只读复核：v22 reader源码PR101；v23 reader为bf10b64，打开57bedba生成的v23库，不将bf10b64误称旧生产构建。两个 reader重开后generation变化、manifest读取明确 unsupported version、local查询返回 corrected_marker。作者记录4次local search/0 provider。probe源码存在于final证据，binary hash为声明值，本轮未重新编译、持有或执行这些handle。温缓存“同一query升级前后”的更强对照纳入本轮 [普通验证设计](REOPEN-PLAN.md)，但未执行。

全features raw log本轮复hash、复计数：148 suites，**2474 passed / 4 failed / 68 ignored**，整体验收仍否。原有semantic_runtime read-only、runtime artifact timeout、lifecycle timeout及fixture最初失败不被本 review覆盖、抹除或换根重试。仅证据核查与自己的模型检查通过，不给全仓CI、100k、GC/WAL、V20、heldout或live质量通过结论。

## 6. 父逐段审入口

1. 权限阻断与整体 BOUNDED_REJECT：本文件第1段及 blockers.json。
2. 固定代码与自动失效行为：第2段，核 auto-index开关的措辞。
3. 四构建身份：第3段及 audit-results.json 的 author_build_identities。
4. 十路径与源/mtime界限：第4段及 immutable_v23_snapshot_reads、ten_migration_paths。
5. pinned/manifest、未跑边界：第5段及 REOPEN-PLAN.md。

本轮亲跑命令仅 `python3 audit.py`（成功）：语法/模型断言、Git证据hash和immutable旧快照SQL读取。产品执行为0。用户要求完成即停；本报告提交交付后不自动追加迁移重跑。
