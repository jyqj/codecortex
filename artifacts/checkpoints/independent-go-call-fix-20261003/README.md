# Go 调用身份修复独立验收

结论：**PASS（fresh parse / clean DB / full index）**。本报告不宣称旧 parsecache 或既有索引升级已经得到修复。

- 被验收 PR102：`99a2c0426054ddbed7edf42de43593a780a76a49`
- 对照 PR92：`ace2bc7983be2955831c9384e44d1bdd0749c909`
- 旧诊断 PR99：`b25723458a77edd2207ed1a52dceb0ea1009cb93`
- 三个远端 pull head 均核验一致，见 `remote-shas.txt`。PR92 是 PR102 的祖先。
- PR92 / PR99 的 `go.rs` blob 均为 `226104105f513f6ba49e54a529445bc1b9748e31`；旧行为复现运行在精确 PR92 快照，未将其称为 PR99 全树运行。

## 独立验证

独立测试 `independent_go_call_fix.rs` 未使用 PR102 自带断言或 probe。`prepare.py` 从固定 Git 对象提取 Cargo 文件与 crate 源码到 `/tmp/independent-go-call-fix-20261003/{base,fixed}`，未提取 benchmark/corpus/holdout。没有修改生产工作树；其 HEAD 为 `ff458bc591b4e7e444af4464d6eef2513cdb335c`，不是验收目标。仓库、父目录和 `.agents` 未发现 AGENTS.md/本地 skills；遵循 CONTRIBUTING 的 Rust 1.95 和中文文档要求。

| 样例 | calls / call refs（前后相同） | PR92 重复 call ID 组 | PR102 重复组 |
|---|---:|---:|---:|
| 128 层同起始位置、重复 Step selector | 128 / 128 | 1 | 0 |
| 参数内调用、method、indexed receiver、generic receiver/call | 13 / 13 | 4 | 0 |
| Unicode 跨行 selector、direct/selector 同名 | 5 / 5 | 1 | 0 |
| Gin context.go | 256 / 256 | 13 | 0 |

PR92 独立测试 **4 passed / 0 failed**，每个碰撞样例均复现真实 Indexer 的重复 site 拒绝；PR102 独立测试 **4 passed / 0 failed**，每个样例均成功 full index，SQL 核对调用数量、call refs 数量以及 call→ref 链接无悬空；再次 full build 数量保持。调用和引用 ID 分别唯一。所有调用的 byte span 精确截取 callee token，调用与对应 ref 的行/列/span 一致，Unicode 和跨行断言通过。新建 parser 重解析的完整序列化输出一致。

每个样例的完整 ParseOutcome 与 PR92 基线逐字段对照，仅去除调用的 identity/span 字段及调用 ref 的 identity/span 字段；其余语义字段、记录数量和顺序均保全，direct call 的完整记录另行逐字段比对。没有通过删除内层/外层 call 消除错误。手写样例使用独立预期 callee 多重集；Gin 使用 PR92 的 256 项多重集，另加固定总数断言，不冒充新 AST oracle。

现有 Go 单测：**15 passed / 0 failed**（`go-unit-tests.log`）。独立测试文件 rustfmt 检查通过。未运行全工作区测试或真实业务/provider。

## 来源和复现

Gin 公开 MIT 源码来自修复 PR 已锁定的副本：`gin-gonic/gin@43fe48e8a0f44af783116cdb010725e6bb50255f/context.go`，48336 bytes，SHA256 `b2336e5768f9175f2855c7df91bb48de0bbb533d74f6a6e7c3af5576491524ce`。LICENSE SHA256 `b104efb2c7700691650f27034e8541c5ae0ed9af54d884287f19a7467ca2fe7f`。两者均与 PR 锁一致。只静态 parse/index，从未执行 Gin 业务。没有读取 public-v19 holdout 内容。

复现：在本执行环境仓库根目录先运行 `git fetch --no-tags origin refs/pull/102/head refs/pull/92/head refs/pull/99/head`，确保指定 Git 对象存在，再运行 `bash artifacts/checkpoints/independent-go-call-fix-20261003/run.sh`。Cargo 全程 `--locked`；首次离线构建缺少 reqwest 缓存，随后仅通过 Cargo 获取锁定依赖，最终证据运行均 `--offline`。没有更改 Cargo/凭据/权限；初次独立测试的 SQL COUNT Rust 类型编译错误已在测试内修正，最终日志对应当前测试。

## 集成必需条件和范围

旧缓存/旧索引中以 receiver 起点生成的 ID/span 不会因为此函数修复自动变成新数据。集成方必须使修复前的 Go parsecache 失效，确保固定 parser 重新解析，并重建受影响的 call/ref 持久化记录；需要另行提供旧缓存升级和增量路径的证据。此 PR 不含 Cargo/version/cache migration 变更，本任务不修改这些部分。当前通过结论限于 fresh parse、干净 DB 和 full rebuild。

仅新增本目录的独立测试、日志、公开锁定输入和报告。未改生产 go.rs、Cargo、version、ledger、corpus；验收运行阶段没有 commit/push/draft PR/merge/force-push/deploy；后续按授权仅封存 review 分支并提交、推送此目录，发布 SHA 以 Git refs 为准。详细机器结果见 `receipt.json`；文件摘要见 `SHA256SUMS`。
