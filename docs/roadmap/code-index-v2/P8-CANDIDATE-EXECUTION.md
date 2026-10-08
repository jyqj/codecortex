# P8-001 候选冻结的真实执行见证

`scripts/p8_candidate_execution.py` 将原有 `p8_release_evidence.py` 的
freeze / verify / archive 与现有 `cc-eval` 的 validate / MCP stdio run / replay
组合起来。输入锁本身仍不批准发行；本入口补上实际构建来源、实际产品执行和评分复算之间的关联。
P7-020 的原硬依赖、P8-002 语料认证和后续各项发布验收均保持原条件。

## 固定范围

- 产品必须是 Cargo 真实输出的默认 `codecortex` binary，runner 必须是默认 `cc-eval`
  binary；不能以 test executable、semantic feature 构建或另一个包替代。
- 两者的构建收据、完整 crate/Cargo 输入清单、原始 Cargo JSONL 和当前工具链必须绑定同一完整
  Git commit。构建退出码与 raw `build-finished` 均须成功，收据 artifact 必须逐字段匹配 raw。
- 产品构建的 `dev` / `release` 身份原样保留。输入锁中的 `debug` 是原工具对 Cargo dev profile
  的名称映射，不是 release 构建。本项不冒充 P8-012 的优化发行或平台冷构建认证。
- 源码快照范围固定为完整 `crates`、Cargo.toml、Cargo.lock、scripts、CI workflows。
  显式环境变量、OS/kernel、架构、CPU、RAM/cgroup 上限、Python 和 Cargo/rustc 身份一并绑定；
  不枚举全部环境，不读取模型凭据。无法读取的硬件项保留 null。
- 复用未经改动的 `p0-rust-api.json`：两份自有 Rust 源文件、一题原 DEV query/gold、原 seed、
  timeout、repetitions、config 和 `codecortex-native-v1` scorer。query/gold 位于同一 JSONL，
  其完整字节共同固定。既有 BLAKE3 locks 由原 Rust validator 校验，新增 SHA-256 清单另列，
  不混淆两种摘要。

这是验证冻结和执行关系的最小实际测量，不是新质量语料，也不声称一题能证明检索质量、
100k、长时运行或真实模型收益。默认产品不能执行远程语义，model 保持 disabled。

## 在固定 CI checkout 构建 runner

产品构建复用原 `p7_stdio_build_receipt.py --package-kind default` 的实际产物和完整目录。
随后在同一个已停止改源码的 checkout 执行：

```sh
python3 -B scripts/p8_candidate_execution.py build-runner \
  --output "$RUNNER_TEMP/p8-runner-build" \
  --target-dir "$RUNNER_TEMP/p7-default-build/cargo-target"
```

`--target-dir` 可选；显式复用的 target 必须属于同一固定源码。收据如实记录它是否一开始为空，
不会把复用 target 称为冷构建。省略时创建本次私有 target。`--release` / `--offline` 仅影响
真实执行的 Cargo 命令，不能通过编辑 profile 标签代替执行。该命令自身保留 stdout、stderr、
PID、退出码、完整 Cargo artifact、before/after 源码和编译器身份。

## 冻结、执行、复算和归档

```sh
python3 -B scripts/p8_candidate_execution.py run \
  --product "$RUNNER_TEMP/p7-default-build/codecortex" \
  --product-build-receipt "$RUNNER_TEMP/p7-default-build/build-receipt.json" \
  --runner "$RUNNER_TEMP/p8-runner-build/cc-eval" \
  --runner-build-receipt "$RUNNER_TEMP/p8-runner-build/build-receipt.json" \
  --expected-source-commit "$FIXED_FULL_COMMIT_SHA" \
  --output "$RUNNER_TEMP/p8-candidate-execution"
```

工作目录必须是接收原构建的固定源码 checkout；不要在运行期间编辑源码、输入或 binary。
输出目录须全新。流程先拷贝并验证原 fixture，冻结完整输入，然后使用候选中的实际 binary
和 corpus 快照运行 `cc-eval --backend mcp-stdio`。测量必须产生预定数量的 normalized/raw rows，
原 gate 须成功、基础设施失败为空、runner 实测 HEAD 与固定源码一致且没有 dirty 状态。

原 measurement 的 raw / normalized / metrics / gate / report 保持不变。replay 在新副本内调用
原 scorer，metrics、gate、report 必须逐字节相同。七项漂移控制只修改新建的一次性候选副本，
验证 binary、config、scoring、model、source、query/gold、corpus-source 的改写会使旧外部 pin
验证失败；原 binary、源码、corpus 和候选绝不被阴性控制修改。

每个命令有自己的执行回执及原始 stdout/stderr。`evidence/execution-witness.json` 关联：
candidate SHA、产品/runner SHA、两份构建收据、实际 profile、工具链/环境、实际测量与 replay、
原始 gate、完整测量文件清单和漂移控制。包装层 `evidence/gate.json` 从本次真实 gate 派生，
额外绑定 candidate 与 witness；原 `measurement/gate.json` 不重写。

最终用原归档工具创建全新 archive 和 checksums。`result.json` 返回独立外部 pin，后续可用：

```sh
python3 -B scripts/p8_release_evidence.py verify \
  --archive /absolute/path/from-result \
  --expected-sha256 "$RECORDED_ARCHIVE_SHA256"
```

不更新 latest，不修改 tasks.json，不自动授予 P7/G8 或发行批准。失败保留已经产生的真实输出，
不将打印的 passed 字样、收据字段齐全或脚本准备就绪当作产品执行成功。

## 独立反例控制

```sh
python3 -B -m unittest discover -s scripts/tests -p test_p8_candidate_execution.py -v
python3 -B -m unittest discover -s scripts/tests -p test_p8_release_evidence.py -v
```

新增测试只证明错误 build/source/raw/profile 绑定会被拒绝、真实非零进程退出不会被 stdout
中的 passed 覆盖，以及漂移控制不会改原始候选。测试夹具 binary 从未被当作产品执行。
正式完成证据应引用上述实际 CI 运行、真实 binary 与源绑定独立评审。
