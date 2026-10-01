# 本轮交付

未完成P5-019；仅评测实现和正式实验准备，等待最终S11/Partial修复后的accepted source。

## 独占write set
- 修改 `crates/cc-eval/src/benchmark/ablation.rs`：MixedPlan / MixedQuery / run_mixed_load，shared真实rmcp单session，完整raw/queue/e2e、facet+source硬门、Partial保留；thread_count复用owner的sampler helper。
- 新 `crates/cc-eval/tests/p5e_ablation.rs`：3正常测试PASS、1显式release ignored；正例断言exit0，probe0必须线程null。
- 新 `artifacts/benchmarks/p5e-ablation-preparation/`：计划、artifact-onlydriver、8cell准备脚本、多facet和4并发档配置、验证日志与delivery哈希。
- 新 `artifacts/checkpoints/todolist-dataset-preparation/P8-PREPARATION.md`：轻量P8真实数据集/许可/holdout缺口，没有下载/付费/勾done。
- 未改 tasks、Cargo.toml、共享CLI、gold/scoring；sampler helper属于owner写集。

## 已验证
- SDK15.4 cargo check -p cc-eval --lib PASS。
- probe0下 cargo test -p cc-eval --test p5e_ablation -- --test-threads=1：3 passed、0 failed、1 ignored。
- artifact driver rustfmt --check PASS（Rust语法/格式，**不是typecheck/运行**）。
- artifact driver cargo metadata --no-deps --offline PASS（manifest/依赖闭包元数据，**不是build**）。
- prepare_factorial.py py_compile PASS，facet/mixed JSON schema版本及所有marker存在于locked原件。
- 文件SHA256见 DELIVERY-SHA256.json（不含其自身与后生成本文；hash不代表实验通过）。

## 等最终冻结后命令

```sh
SDKROOT=<verified-sdk> cargo build --release --manifest-path artifacts/benchmarks/p5e-ablation-preparation/driver/Cargo.toml --target-dir <isolated-target>
python3 artifacts/benchmarks/p5e-ablation-preparation/prepare_factorial.py --accepted-source <accepted-snapshot> --output <new-run> --sdkroot <verified-sdk> --runner <accepted-cc-eval> --build
<accepted-cc-eval> ablate --plan <new-run>/plan.json --output <new-run-quality>
<driver> facets <cell-binary> artifacts/benchmarks/p5e-ablation-preparation <new-cell-facet-output>
<driver> mixed artifacts/benchmarks/p5e-ablation-preparation/mixed-c1.json <new-c1-output>
# 每8cell重复facets；C1/4/8/16各运行相应mixed配置。
```

正式运行前由owner/auditor批准source hash、控制变量、资源隔离。8cell的关闭selector仅指intent-driven facet reservation（Locate替代），overlap/source authority保持。51开发题与新增3机制题不等600独立题/holdout；1224/720重复请求不充独立检索N。source-suite重新锁定须另栏，不能声称历史冻结源输入不变。完整函数/trait/测试函数marker必须位于independently valid返回正文；缺facet/Partial fail，不改标签/topk凑绿。

混合runner仅immutable-source full-rebuild争用，in-process公开dispatch；threads before/after不是峰值，probe0 unavailable不填0；不认证mutation/stdio隔离/100k/RSS/provider/p99。当前driver未typecheck、所有正式release matrix/facets/mixed均not_run。
