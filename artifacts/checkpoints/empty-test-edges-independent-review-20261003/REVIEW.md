# 空 testfile 重复扫描优化：独立验收通过（限定本块）

验收对象 PR101，固定远端与 review base 均为 `574f7598662334c63e020da136c87f4f7281554d`。
唯一生产修复 source 为 `c70c68f2ff9b4ac40c635652858f56d2d0a06518`；旧可读基线 PR93 为 `8b362e6b9d1334a51d5856ab21223d6e5ec28604`。
本 reviewer 只新增独占 tests/evidence，未改生产代码，未 merge/deploy/force-push，也未转做其他模块。

## 源码语义

生产 diff 只有 `index_db_edges.rs` 的 17 行。短路位于原 changed-edge 删除循环之后，仍在同一 write transaction 内。
非空 changed 且没有 testfile 时，一次存在性查询替代每个普通文件的候选 LIKE 全表扫描，仍一次 bump epoch 并 commit；错误传播使整个事务回滚。
空 changed 原有 no-op 不变。有 testfile 时原匹配逻辑原样执行。查询保守保留非零整数和非整数 flag 的原解码路径，因此不会把异常 flag 的原错误吞掉。
未发现需要向集成 owner 报告的真实反例。

## 独立测试与性能

`independent_review_empty_test_edges.rs` 的 9 项 correctness oracle 在新旧 SHA 都通过：双端 stale-edge 删除、无关边保留、重复 changed、重开数据库 commit、空 changed、空数据库非空 changed、删除最后一个 testfile、有 testfile 的 code/test/mixed changed、删除失败与 epoch 失败回滚、异常类型 flag、非标准整数 flag 的原分支差异。

| 验证 | 旧 PR93 | 新 PR101 |
| --- | --- | --- |
| 9 correctness + 1k profile，release 独占 target | 10 passed；1k 129.145 ms | 11 passed（还包含 50k）；1k 3.394 ms |
| 实际生产方法 50k 全 changed | 沿用原 300 秒产品失败，不重复覆盖 | 164.202 ms；文件数、epoch、integrity/FK 通过 |
| 独立 fresh 产品 1k 冷 index=false | 原负证据保留 | 179.227 ms，ready=1000 |
| 独立 fresh 产品 50k 冷 index=false | 原 300 秒 RPC 无响应，0 query cells | 9815.595 ms，ready/files/manifest=50000 |

独立产品两规模均 skipped=0、testfiles=0、test_edges=0、integrity=ok、FK=0、exit=0。
只调用生成的公开形状 synthetic fixture 和 `127.0.0.1` mock；不调用真实 provider，不外发私密源码，不读 public-v19 holdout。
该产品验证只有冷索引、backfill ready 和一次 local search，不是 V20 矩阵。其 ready drain 使用 300 秒上限；原冷 index RPC 的 300 秒等待未改。

`cc-db --lib` 175 passed / 0 failed / 1 ignored；独立 test target strict clippy `-D warnings`、全仓 fmt check 均通过。
新旧 target 最终隔离，避免跨 worktree 缓存污染。

## 精确 build 与归档负证据

本 session 从空独占 product target 构建官方 `--release --no-default-features --features semantic-http`，Rust 1.95.0、locked dependencies、默认 INFO logging。
build source HEAD 为 `574f7598662334c63e020da136c87f4f7281554d`，完整生产源码/Cargo 与 c70 相同。
实际 fresh 二进制 SHA256：`8786768e6301bfa699df32e801644b56abc0d3c5b86a8244f83100f653445f92`，逐字节哈希与作者封存二进制一致。

独立 `review_evidence.py` 核验了旧失败 21 个归档 hashes、新成功 33 个 hashes、release receipt/features、旧源码等价链、唯一生产 diff、冷 RPC、256 query responses、ready/完整性/FK。
旧实际 build source 是 `85692bb526ce5a9fdd7c92aa71d9392e498a7cf6`，其生产源码与 PR93 相同；旧 binary SHA256 为 `2d45fac2d3e33b3557db0cdbee9c3b2e1e391de97ce6f6c613234683672fdccd`。
作者归档 50k cold wire 10524.815 ms、8 cells/256 queries/0 error/0 empty 均回放通过；独立 live 结果单独保存，不覆盖原记录。

## 可复核产物

- `fresh-build-receipt.json` 和 fresh build JSONL：精确 artifact/profile/source 身份。
- `old-release-isolated.log`、`new-release-final.log`：有效新旧独立 release 验证。
- `archive-review.json`：独立归档 replay。
- `live-summary.json`、`live/`、`live-capture-manifest.json`：本 session 两规模完整 capture，非 live-prefix。
- `verify_review.py`：无需重新运行产品的本块交付验证。
- `run_cold_product.py` / `capture_live.py`：独立 cold/ready 验收及原始结果封存。

早期失败日志如实保留但不计入验收：首次 offline cache 缺 reqwest；初稿私有 API 调用；初稿 SQLite usize 解码；`old-release-shared-target-invalid.log` 因 Cargo 共享缓存没有旧源码重编译，明确作废。最终 oracle 已修正且单独重跑通过。
最初 rustup 启动误用默认 home 收到只读拒绝，已停止；随后仅采用 owner handoff 脚本明确提供的 `/workspace` 标准工具链配置，未改权限/凭据或访问被拒绝目录。

## 余项与边界

优化本块无剩余语义或性能阻塞。100k/full V20 未证，本 session 不扩展。原 500 ms benchmark 门槛问题没有在此宣布关闭，也未抬阈值。
GitHub PR 查询明确返回 `Forbidden`；已停止该 API/PR 动作，未换接口重试、未修改凭据。draft PR 创建仍阻塞，PR101 原分支保持不动。
