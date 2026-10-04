# Bounded owner-context v3 精确集成

本分支 `integration/query-owner-context-v3-20261004` 从 PR140 固定 base
`3c8c204216cb54c41850c6da826a68fec3586430` 开始，完整 fast-forward 保留后续
18 个作者、拒绝、修正、独审提交。接受 delivery
`026b566f18525568ae16e505eca13ea5e258dff5`，tested product
`d32924f0b1b08f55956659df2cb253d0aba8146f`，独立接受
`9c28792137b89a69d6c0696fa112b7fe54f4c053`。
新增 registry/selector/metadata 的固定 integration source 是
`bd7fb168c1b9d70a0f21ae1bb92212d7801b3747`；当前 evidence delivery 单独交付。

接受范围仅 bounded contextual-owner / comparison lexical closing-qualifier
规则。全部 crate/Cargo/lock bytes 在 source、author delivery、独审及 integration
source 四个固定身份间逐项相同。生产变化限于 `cc-search/src/query_target.rs`、
`plan.rs`，相关原作者测试在 `engine_lane_tests.rs`；没有重新改写修正代码或测试。
`history.json` 保留完整 SHA/parent/path 映射，121 个新增作者/独审/control 文件由
registry 固定原 SHA/size，全部旧拒绝结果、invalid shared-target 输出和完整 archives
原样保留。没有 squash、重写作者身份或将历史失败改成通过。

## 显式 source registry

`scripts/current-source-registry-v3.json` 登记 `query-owner-context-20261004-v3`，
SHA256 `8525ded2edbcbf59f8f7965ffa26c684ef6cf838576d61d634efa8408299acec`。
guard 先调用原 v1/v2 重建逻辑，从原 accepted sources、完整 manifest、机械 lint
和 R1/capture transformations 独立推导 v2 union，核对固定 PR140 base 全路径/bytes；
然后仅替换三个 approved delta 的固定 source bytes，核对 before/after SHA、完整
764 个 crates/Cargo/lock inputs 的 membership/hash，以及三个接受身份的全部 bytes。
最后核对当前 Git tracked/disk inventory，拒绝 unknown add/remove/change/symlink。
所有固定 commit/version/registry digest 都是显式常量，无 HEAD/latest admission 或
blind refresh。Cargo.lock 原 SHA256 为
`ea6b177872de3749702e2cd967f4504efff5bcc67cd2a2459205d182243d1000`。

v1/v2 registry、approved manifests、旧 verifier 与 packing/e3/P0 historical anchors
逐字保留；固定 anchors 的 hash 见 `binding.json`。CI 在检查完整 workflow scope 后
只切换显式 selector 到：

```sh
python3 scripts/verify_current_source_v3.py --source-version query-owner-context-20261004-v3
```

另改一条说明注释；其余 workflow bytes 不变。原 v2 负控保留，仅将 CI selector
断言改为显式 v3。新 10 项 v3 负控实际拒绝 owner/Cargo/旧 capture mutations、
tracked/disk add/remove、symlink、HEAD/latest/旧 version、错误 delta/hash/history pin
和 full-manifest/history omission。原 v1 16 + v2 14 + v3 10 合计 40 passed。

## 本次实际检查

官方 Rust/Cargo 1.95.0、default features、原 lock、`--locked`，Cargo 编译/测试均
`-j 2`，初始 target 是本次全新专有 `target/query-owner-v3`。逐命令 argv/exit/日志与
初始失败均在 `checks.json`；`toolchain.log` 固定工具链身份。

| 检查 | 本次结果 |
| --- | --- |
| `cc-search query_target --lib` | 21 passed / 0 failed / 0 ignored；298 filtered 未执行 |
| 独立 direct production-module probes | 166 passed，含16 explicit bypass cases；逐值匹配固定 accepted outputs |
| Python source-integrity controls | 40 passed |
| resource neutral fixtures | 26 passed（补齐 read-only historical comparator 后 rerun） |
| historical corpus neutral controls | 14 passed；只 validate 固定历史输入，不执行 retrieval/DEV eval |
| strict Clippy workspace/all-targets | passed，`-D warnings` |
| workspace/all-targets test compile-only | rerun passed，`--no-run`，0 tests executed |
| fmt / source architecture / module architecture / plan | passed |
| current v3 / historical packing+e3 source guards | passed；historical guard 不认证当前质量 |

`replay_direct_probes.py` 只读取固定独审 archive 的 wrapper/166 inputs/accepted/previous
输出，编译 wrapper 直接引用当前 production dsl/query_target modules；没有启动产品进程。
fresh outputs 与独立接受 outputs 完全相同，并逐项检查 expectation、QueryTarget/permits
和 explicit bypass；previous outputs 明确是历史参考，没有冒称 fresh previous arm。
link receipt 记录 rustc argv、原 lock构建的 dependency rlib hashes、binary hash；实际结果、
inputs 和 wrapper 在本目录保留。复放使用专有 scoped target，先运行上述21项 scoped
Cargo test，再给脚本传新 owned-root、该 target-dir 和 Rust1.95 rustc。

独审的 3,840 paired API / 21,500 common-hit / 96 filter observations 是固定 source 接受
证据，未在本次重新运行两臂 API 矩阵；完整原报告/archive/binding 保留于
`artifacts/reviews/query-owner-closing-qualifiers-independent-20261004/`。

初次 resource fixture 因缺 da8b618 的 read-only comparator object 失败，按原 CI 固定 SHA
fetch 后通过；从未 checkout/执行其 historical driver。初次 historical controls 因默认
validator 路径不存在，rerun 显式指定当前 compile-only 已构建的 validator 后通过。
初次 all-target compile-only 因32GiB磁盘满出现 linker bus error/ENOSPC，没有执行测试；
保留 `compile-only.log`。进程结束后只移除本次 disposable incremental cache，原 argv
重跑通过。磁盘满使 sandbox 无法启动，自动审批允许限定 cache cleanup；无审批拒绝。
两次准备期断言还分别发现 records 中独立 neutral-matrix 路径、host/runtime 两个
serde_json rlib；完整保留 control，按原 float_roundtrip runtime artifact 选库后 replay
通过，未修改 source/expected/scorer。

metadata 只追加 `tasks.json` 的 P8-001 implementation_notes，通过既有
`code_index_plan.py --write` 生成 TODO，再运行无参数校验。192 项状态逐项等于固定 base：
150 done / 41 todo / 1 in_progress；`task-states.json` 保留原状态，没有 parent closure。

## 保留边界与交付

stronger lexical API inversion 仍 Top1=0 / MRR=.5；full NL/public quality、P7/V19 和
旧四仓质量失败/Gin边界均不升级。source integrity 不继承旧 quality/100k 结果。
formal DEV/100k、private42 exports、DBGC/WAL/kill/EROFS、production targets 和
excluded `semantic_runtime::tests::post_index_worker_crosses_pages_and_reopen_reuses_artifacts`
及可启用它的 execution suite 均 not_run。compile-only 不计为测试执行。

仅发布新 isolated integration branch，正常 origin push 并核对远端 SHA；没有创建 PR、
merge/deploy/force。parent 复审本 exact delivery 后负责创建 draft PR。
evidence manifest 保存本目录全部 deliverable 的 SHA256/size；最终交付 SHA 在
单独 `delivery.json` 和最终回复中给出。
