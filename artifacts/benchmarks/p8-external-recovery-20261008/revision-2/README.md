# P8-003 新候选入口（revision 2）

本入口保留首轮 run 37735653177 的原脚本、原输入锁与失败原件。首轮两个 compat 都是 invalid_measurement（cc-switch 索引超时、Flask 覆盖不足）；native 未执行，查询测量数为 0。本版脚本和封装文件本身不计为 TODO 完成。

新的产品与 evaluator 必须在同一份固定干净源码中真实构建。不要把旧 ffdc/9ebe 二进制当作新候选。SOURCE 是最终整合脚本、显式文本准入和逐路径 readiness 后的完整提交；下面变量必须由运行方填入实际固定值，不是宽泛分支名。

所有工作、构建与公共输出目录均位于 checkout 外，且第一次运行前不存在。设置 PYTHONDONTWRITEBYTECODE=1，使用同一固定 Rust 工具链。先运行原测试与新增回归，再构建。调用示例：

~~~sh
export PYTHONDONTWRITEBYTECODE=1
SOURCE=<完整40位候选提交>
ROOT=<该提交的干净checkout绝对路径>
WORK=<新建独占工作父目录>
cd "$ROOT"
python3 scripts/p7_stdio_build_receipt.py \
  --package-kind default --output-dir "$WORK/product-build" --release
python3 scripts/p8_candidate_execution.py build-runner \
  --output "$WORK/runner-build" --target-dir "$WORK/product-build/cargo-target" --release
python3 scripts/p8_external_candidate.py \
  --source-root "$ROOT" --expected-source "$SOURCE" \
  --product-build "$WORK/product-build" --runner-build "$WORK/runner-build" \
  --output "$WORK/candidate-package"
~~~

第二次构建明确复用同一 immutable source/toolchain 的 product 私有 Cargo target；回执如实记录是否最初不存在。这不是两个冷构建认证。产品 helper 的默认 dev 路径保持不变；此入口显式选择 release，验证真实 Cargo profile 的非零优化、关闭 debug assertions/test，不能仅凭 build_profile 文案通过。两个 receipt、原 Cargo JSONL、stderr、源码清单、源码/工具链与 helper hash 都进入新包。

pack helper 通过原 validate_build 核对原 Cargo 日志、source before/after 与当前源清单；严格要求 template 中的 upstream/query/source inventory/预算参数保持原字节。唯一旧 helper 变更是显式配置 p8_compat.py，新 profile 固定 auto_index=false 和 indexing.include_text_files/include_hidden_files=true。原 scorer/audit/policy 文件 hash 必须仍与原模板一致。候选 archive/manifest 的 SHA256 来自这次实际文件，不提前填写二进制摘要。

从 candidate-package/package-receipt.json 独立读取 manifest.sha256 后，把它作为必填 pin 传给下面命令。脚本也检查其自身 hash 与 manifest 一致：

~~~sh
MANIFEST_SHA=<刚生成并核对的manifest SHA256>
python3 artifacts/benchmarks/p8-external-recovery-20261008/revision-2/recover_external.py \
  --manifest "$WORK/candidate-package/manifest.json" --manifest-sha256 "$MANIFEST_SHA" \
  --expected-source "$SOURCE" --archive "$WORK/candidate-package/candidate-tools.zip" \
  --work "$WORK/private-external" --public-output "$WORK/public-external"
~~~

原 OCE commit、cc-switch/Flask commit、1030/229 common files 与逐文件全集不变。每套 compat/native 使用相同 source/config/budget，原 top-k 10、repetitions 3、warmup 0、timeout 30000 ms、seed 20261003，以及每命令 600 秒不改。导入、freeze/validate、四个新副本 query-drift 反例、真实 MCP、原 scorer/replay/gate 与即时原 raw 还原校验均仍执行。失败原件和非零退出保留；报告按实际 attempted/completed profiles 和真实 normalized 行数计数，未运行明确 not_run。

公开上传候选构建包、public-external 与测试/构建回执。不得上传 private-external，它包含暂时用于本地评测的外部题库与完整原 raw。public-external 的 external-reference-package 通过固定 query-ID 引用保存题文位置，必须同一次运行在私有目录重建并逐文件逐字节等于原 raw；它不是“原 ZIP”。不复制或重发整份 OCE 题库到 Git 或 PR。

native 仍为机械 expected-file 投影，并未获得独立人工 gold 认证；不得用它冒充 P8-002 原生公开金标或 P8-004 未见 holdout。release_certified/P8_003_done 保持 false，实际任务结项另按原条件和独立证据判断。
