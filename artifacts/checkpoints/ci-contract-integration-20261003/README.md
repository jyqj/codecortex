# CI 契约单块集成 — 20261003

固定基线 PR110 `5ffbadcf48e26523b2eb46beda0d187a2e2e29cd`；仅纳入 PR111 `c69645d14ea5512c9556156664a126b26bdfba75` 的 `.github/workflows/ci.yml` 与完整 `artifacts/checkpoints/semantic-http-ci-regression-20261003/`。共同 base `350aaacf2f24f533321c495cd4a72ea38029f7df`。独立 `integration/ci-contract-20261003` 分支，draft PR 以 PR110 分支为 base；没有 merge 任何已有 PR/main 或 force push。

## 组合与路径审计

`evidence/static-audit.json`：crates 下全部 701 个跟踪文件逐字节等于 PR110（含 DEV manifest/queries），PR111 全部 34 个导入文件逐字节相等；删除精确四行增量后 CI 等于共同 base，原 CI 未弱化。已读 CONTRIBUTING.md，查找 AGENTS.md/.agents/skills：仓库无本地 AGENTS/SKILL，工作区指导目录为空；未委派子 agent。使用官方 Rust/Cargo 1.95.0、-D warnings、locked 依赖和已有 /workspace/.cargo；仅从官方 crates.io 补齐缺少依赖，不换源或修改权限/凭据。

DEV `verify_reconcile.py` 的 `HERE.parents[2]` 仍解析仓库根目录，其 historical-inputs/manifest.json、queries.jsonl、pre-removal-dirty.rs 均保留。PR111 check.py 直接调用当前 Cargo target，生成的证据在本目录 evidence/semantic-http/；原作者证据保留在 PR111 目录，不冒充当前执行。

继承 PR107 schema24/rebuild_on_mismatch 声明修正。R09 从已删除 compute_fingerprint_for_unit 显式迁移为 PublicSurface::fingerprint，是新 DEV revision；其它13题 query/answers不改，旧基线完整保留，不代表检索质量提高或同分母提升。没有运行检索。

## 当前组合树有限验证

- schema/module checker、source architecture checker 通过；Python guard 文件 9 tests 通过（含 schema 5 项）。
- 正式六 suite `cargo run -p cc-eval --bin cc-eval --locked -- validate ...` 全部 locks valid。
- DEV verify_reconcile.py 通过：14源annotation、授权R09、原13题及配置不变；正式validator对未声明source/query漂移各返回2。
- 正确 `cargo test -p cc-eval --test benchmark_lock --locked`：10 passed / 0 failed / 0 ignored。
- 原样 PR111 check.py：**17 passed / 0 failed / 0 ignored**（6 DB + 10 semantic-http status + 1 semantic-http stdio，真实search/context）。每组发现与执行的精确非空名称集合均校验；list不增加通过计数，stdio两份raw JSON保留。
- plan生成与校验通过：192 / 150 done / 41 todo / 1 in_progress；tasks.json仅追加P7-014实施备注，P7-014仍in_progress、015/016仍todo，生成05-TODO.md；HANDOVER追加本批状态与限制。
- cargo fmt --all -- --check、git diff --check通过，见最终检查日志。

初次离线运行因 reqwest index 缺失而失败，相关 commands.json/00–09.log 均保留；DEV verifier随后因尚未构建binary失败，不能计成功。补齐官方依赖后 rerun-commands.json 记录重跑。误用 `--lib benchmark_lock` 的 rerun-02.log 为0tests，明确不计通过；正确 integration target 的 benchmark-lock.log/receipt 才是10项通过凭据。各Cargo运行原始退出码保留；日志仅去除EOF多余空行以满足diff whitespace检查，诊断与失败内容完整保留。

## CI checkpoint 与未完成范围

[PR110 CI run 37120774371](https://github.com/jyqj/codecortex/actions/runs/37120774371) 的精确head是 `5ffbadcf48e26523b2eb46beda0d187a2e2e29cd`，check `111196279333` / msrv `111196279452` / security `111196279504` 均 success。父线程提供后本任务重新读取workflow runs/jobs，见 evidence/pr110-ci-checkpoint.json。**这是PR110，不外推新组合head全绿**；新PR实际run/head在最终交付回复中记录，全量远端CI交给PR运行。

[PR109](https://github.com/jyqj/codecortex/pull/109) 固定head `0c1e0b3a1b21620aa8f10abf4f48342b8f0458d8` 的独立Linux boundedpass仅链接，不复制自编harness、不计本次执行。[PR108](https://github.com/jyqj/codecortex/pull/108) 固定head `99973e7d6faf3809add4a12cd444f8e69f04d93c` 的100k仍300s未ready（29082），后续query/reopen未跑；本任务未重跑。原all-features **2474 passed / 4 failed / 68 ignored** 未修、未洗白。

本批完成仅指固定集成及上述有限检查，不关闭P7-014/015/016、完整V11/V20、性能或发行gate。没有吞吐修复、GC/WAL kill/fault、拒写runtime重试、真实provider/heldout或权限/凭据改动。完成交付即停。
