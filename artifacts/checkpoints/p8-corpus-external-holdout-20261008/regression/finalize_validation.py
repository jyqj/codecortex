#!/usr/bin/env python3
"""Seal only this agent's fixed-P validation evidence; never rerun tests."""
import datetime, hashlib, importlib.util, json, os, pathlib, subprocess
OUT=pathlib.Path(__file__).parent
ROOT=OUT.parent/"codecortex-round4-validation"
NAMES=["rust-default-all-targets","rust-http-scoped","rust-semantic-ci-targets","python-all-scripts","rust-fmt","code-index-plan","p8-facts"]
def sha(p):return hashlib.sha256(p.read_bytes()).hexdigest()
def write(p,obj):
    if p.exists():raise RuntimeError("refuse overwrite "+str(p))
    p.write_text(json.dumps(obj,indent=2,ensure_ascii=False)+"\n")
spec=importlib.util.spec_from_file_location("fixed_validation_observer",OUT/"record_validation.py")
observer=importlib.util.module_from_spec(spec);spec.loader.exec_module(observer)
fresh=observer.inventory()
receipts={name:json.loads((OUT/(name+".json")).read_text()) for name in NAMES}
initial=json.loads((OUT/"rust-default-all-targets.before.json").read_text())
if fresh!=initial or fresh["status"]:raise RuntimeError("frozen source changed")
commands=[]
for name,rec in receipts.items():
    if rec["status"] not in ["passed","failed"] or not rec["source_unchanged"]:raise RuntimeError("incomplete or changed command "+name)
    for key in ["log","source_before","source_after"]:
        p=pathlib.Path(rec[key])
        if sha(p)!=rec[key+"_sha256"]:raise RuntimeError("receipt byte mismatch "+str(p))
    before=json.loads(pathlib.Path(rec["source_before"]).read_text());after=json.loads(pathlib.Path(rec["source_after"]).read_text())
    if before!=fresh or after!=fresh:raise RuntimeError("inventory mismatch "+name)
    item={"name":name,"argv":rec["argv"],"exit_code":rec["exit_code"],"status":rec["status"],"elapsed_seconds":rec["elapsed_seconds"],"source_unchanged":rec["source_unchanged"],"receipt":{"path":str(OUT/(name+".json")),"sha256":sha(OUT/(name+".json"))},"log":{"path":rec["log"],"sha256":rec["log_sha256"],"bytes":rec["log_bytes"]}}
    summary_path=OUT/(name+"-summary.json")
    if summary_path.exists():
        sub=json.loads(summary_path.read_text())
        item["summary"]={"path":str(summary_path),"sha256":sha(summary_path)}
        if name.startswith("rust-"):item.update({"target_count":sub["target_count"],"test_totals":sub["totals"],"failed_tests":sub["failures"]})
        else:item.update({"passed":sub["count"],"skipped":sub["skipped"],"module_count":len(set(x.split('.')[0] for x in sub["unique_method_ids"]))})
    commands.append(item)
write(OUT/"final-source-snapshot.json",fresh)
plan=json.loads((OUT/"code-index-plan.log").read_text());facts=json.loads((OUT/"p8-facts.log").read_text())
cleanup_names=["cache-cleanup-completed-default-part1.json","cache-cleanup-completed-default-part2.json","cache-cleanup-completed-http.json","cache-cleanup-completed-semantic.json"]
cleanup=[json.loads((OUT/name).read_text()) for name in cleanup_names]
fs=os.statvfs(OUT)
result={"schema_version":1,"reviewer":"/root/build_validation","created_at_utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"status":"authorized_validation_completed_with_original_failures_retained","source":{"root":str(ROOT),"head":fresh["head"],"tree":fresh["tree"],"clean":not bool(fresh["status"]),"inventory_sha256":fresh["manifest_sha256"],"counts":fresh["counts"],"bytes_by_group":{g:sum(x["bytes"] for x in fresh["files"] if x["group"]==g) for g in fresh["counts"]},"final_snapshot":{"path":str(OUT/"final-source-snapshot.json"),"sha256":sha(OUT/"final-source-snapshot.json")},"validation_inventory_scope":"all tracked scripts, workflows and source_integrity plus CONTRIBUTING; this runtime observer intentionally includes selected guard/registry/CI as well as the 842 Cargo/crates inputs"},"all_seven_commands_source_unchanged":True,"toolchain":receipts[NAMES[0]]["toolchain"],"environment_overrides":receipts[NAMES[0]]["environment_overrides"],"commands":commands,"interpretation":{"full_default":"The original complete default all-targets command finished every target: 398 passed, 2 failed, 57 ignored across 83 targets. Exit101 is retained.","http_scope":"Full lib plus benchmark_adapters and benchmark_coverage_diagnostics; 69 passed, 1 failed, 7 ignored across 3 targets. Five actual HTTP adapter tests and seven new diagnostics tests passed. This is not the full optional-feature CI matrix.","semantic_scope":"Full lib plus the existing CI p7_worker_contention, semantic_lifecycle and p7_strategy_ablation targets; 58 passed, 1 failed, 6 ignored across 4 targets. All 10 selected non-ignored integration tests and the 3 lib mechanism controls passed.","python_scope":"Unfiltered scripts/tests discovery: 268 unique completed test methods, zero skips; this includes both existing P7/P8 and the new native registry tests.","failure_scope":"The same unchanged namespace-sensitive sampler test failed in all three Rust configurations. The default first-run fixture index warm p95 of 547.91ms exceeded the unchanged500ms threshold. That performance test passed under each later feature configuration, which does not replace the failed default observation or prove a cause.","counts":"Keep counts per command/configuration. Repeated lib and integration tests are not distinct added tests or completed TODOs.","gate_status":"Local Rust gate is failed. Passing Python/fmt/plan/facts, new target controls, or the separately managed source-integrity gate cannot convert these local failures to pass. Standard-host CI is a separate pending/parent-managed evidence source."},"failure_analysis":{"path":str(OUT/"rust-default-failure-analysis.json"),"sha256":sha(OUT/"rust-default-failure-analysis.json"),"parent_reported_followup_issue":"jyqj/codecortex#155; not counted as an original TODO"},"not_run_here":["full workspace Rust/clippy/MSRV matrix","remaining eval-http integration targets and Cargo doctests","ignored actual product-stdio tests requiring build receipts and the four independently built P7 mechanism arms","ignored long scale/cost/soak/real-workspace benchmarks","final S v14 CLI and 174 source-integrity tests (assigned to root/corpus in its separate frozen worktree)","standard-host GitHub CI and any remote issue/PR mutations"],"fixed_P_task_ledger":{"status":plan["status"],"total":plan["task_count"],"states":plan["states"],"remaining":plan["task_count"]-plan["states"].get("done",0),"task_sha256":plan["task_sha256"],"scope":"immutable P task snapshot only; parent owns later closeout edits"},"facts_check":{"status":facts["status"],"runtime_certified":facts["runtime_certified"],"scope":facts["scope"]},"cache_cleanup":{"scope":"only this agent's already-completed, regenerable Cargo test/CLI executable cache in target-scale; every removed path and binary hash is retained in receipts","deleted_unique_bytes_across_operations":sum(x.get("deleted_unique_bytes",x.get("deleted_bytes",0)) for x in cleanup),"receipts":[{"path":str(OUT/name),"sha256":sha(OUT/name)} for name in cleanup_names],"available_bytes_at_seal":fs.f_bavail*fs.f_frsize,"all_cargo_processes_from_this_task_completed":True,"frozen_F_D_and_target_pr148_untouched":True},"additional_body_free_deliverable":{"path":str(OUT.parent/"round4-holdout/postseal-analysis/independent-summary-review.json"),"sha256":sha(OUT.parent/"round4-holdout/postseal-analysis/independent-summary-review.json"),"scope":"independent postseal raw/hash/score/statistics review; accepted_scoped; both original quality gates failed; no new retrieval or replay by this review"},"artifact_manifest":"validation-artifact-manifest.json"}
write(OUT/"final-validation-summary.json",result)
lines=["# 固定 P 的最终本地验证","", "本轮获准验证已完成；本地 Rust 门禁仍为失败。所有原始失败、忽略项和运行前后输入收据均保留，未修改产品源码或测试断言。","", "## 固定身份", "", "- PRODUCT: `"+fresh["head"]+"`", "- Git tree: `"+fresh["tree"]+"`", "- 输入：842 个 Cargo/crates 源文件 + 111 个验证/策略文件；七次命令前后均相同，工作树干净。", "- 完整输入清单 SHA-256: `"+fresh["manifest_sha256"]+"`", "- Rust 1.95.0 / Cargo 1.95.0 / Python 3.12.14，2 Cargo jobs，增量关闭，dev/test debug=0，`RUSTFLAGS=-D warnings`。", "", "## 实际结果", "", "|命令范围|目标/测试结果|退出码|耗时（秒）|", "|---|---|---:|---:|"]
for item in commands:
    if "test_totals" in item:
        t=item["test_totals"]; description=f'{item["target_count"]} targets; {t["passed"]} passed / {t["failed"]} failed / {t["ignored"]} ignored'
    elif item["name"]=="python-all-scripts":description="268 passed / 0 skipped (14 modules)"
    else:description=item["status"]
    lines.append(f'|{item["name"]}|{description}|{item["exit_code"]}|{item["elapsed_seconds"]:.6f}|')
lines += ["", "Python 是全部 `scripts/tests` 的原始 discover 入口。HTTP 的 5 项专属适配器控制和新增 7 项 coverage diagnostics 均通过；semantic worker 1 项、lifecycle 2 项、strategy 7 项及 lib 的 3 项 mechanism 控制均通过。原有忽略项没有执行，也没有按通过计数。每个配置中的重复测试应分别解释，不能相加当作不同测试或 TODO。", "", "## 两处原始失败", "", "1. `benchmark::sampler::process_probe_tests::linux::live_child_snapshot_is_attributed_monotonic_and_disappears`：三配置均失败，`sampler.rs:892` 报 live child snapshot unavailable。只读环境观测中本地 PID 5 对应 `/proc/self` 的 824327，存活子进程的本地 PID 6 对应的 `/proc/6` 属于无关进程；原采样实现直接读取局部数字 PID 路径且未绑定命名空间。", "2. `tests::benchmark_fixture`：默认首次执行 index warm p95=547.91ms，超过原 500ms。HTTP、semantic 配置下该测试通过，但不改变默认首次失败；未通过受控实验归因于磁盘或并行负载。", "", "两处相关源码在 BASE 9f6684ac、F fb772551 和 P 615662bd 完全同字节。根代理报告已建立独立跟踪 issue #155；该 issue 不计入原 TODO 完成数。故障分析的原始证据及逐文件哈希见 `rust-default-failure-analysis.json`。", "", "默认编译完成时出现磁盘耗尽，但日志没有明确 ENOSPC 堆栈，不能将两处失败直接归因于磁盘。完成各命令后仅清理 target-scale 中可再生成的测试/CLI 可执行缓存；共移除约 3.95 GB 的累积生成内容，每次删除路径、二进制哈希与释放量均有收据。F/D 固定二进制、target-pr148、源码及原始评测证据未动。", "", "## 可复现命令", "", "在固定工作树执行，并设置收据中记录的环境；不要直接使用指向 target-pr148 的旧 env 文件。", "", "```sh", "export CARGO_HOME=/workspace/scratch/031390cf22eb/cargo-cache", "export RUSTUP_HOME=/workspace/scratch/031390cf22eb/toolchains/rustup", "export RUSTUP_TOOLCHAIN=1.95.0", "export CARGO_TARGET_DIR=/workspace/scratch/031390cf22eb/target-scale", 'export PATH="$RUSTUP_HOME/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin:$CARGO_HOME/bin:$PATH"', "export CARGO_BUILD_JOBS=2 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 CARGO_PROFILE_TEST_DEBUG=0", "export CARGO_TERM_COLOR=never PYTHONDONTWRITEBYTECODE=1", "export RUSTFLAGS='-D warnings'", "cd /workspace/scratch/031390cf22eb/codecortex-round4-validation"]
for item in commands:
    argv=item["argv"].copy()
    if argv[0].endswith("/cargo"):argv[0]="cargo"
    lines.append(" ".join(argv))
lines += ["```", "", "以上是已执行命令，列出不表示需要重跑已失败的默认全套。初次 `--no-fail-fast` 完整执行了 83 个默认目标；后两配置按根代理在容量事件后明确收窄的范围执行。", "", "## 范围与交接", "", "完整标准主机 workspace/Clippy/MSRV、其余 HTTP 目标、doctests、原忽略的真实 stdio/4-arm/长时 benchmark 不在本地通过范围。最终 S 的 v14 CLI 与原 174 source-integrity 由另一代理在独立固定树执行；即使该门禁通过，也不覆盖这里的 Rust 失败。", "", "固定 P 原任务账本：192 总项、160 done、32 未完成（1 blocked、19 in_progress、12 todo）；后续验收状态由根代理统一维护。p8_facts 只检查声明的文档事实，runtime_certified=false。", "", "`final-validation-summary.json` 提供所有精确 argv、工具链、输入身份、日志及摘要哈希；`validation-artifact-manifest.json` 对本代理全部收据逐文件 SHA-256 封存。新回执可单独归档，旧 holdout 使用证据包未重打包。", ""]
report=OUT/"final-validation-report.md"
if report.exists():raise RuntimeError("refuse overwrite report")
report.write_text("\n".join(lines))
owned=[]
for name in NAMES:
    owned += [name+suffix for suffix in [".json",".log",".before.json",".after.json"]]
owned += [name+"-summary.json" for name in ["rust-default-all-targets","rust-http-scoped","rust-semantic-ci-targets","python-all-scripts"]]
owned += cleanup_names+["record_validation.py","summarize_cargo_tests.py","finalize_validation.py","procfs-environment-diagnostic.json","rust-default-failure-analysis.json","final-source-snapshot.json","final-validation-summary.json","final-validation-report.md"]
if len(owned)!=len(set(owned)):raise RuntimeError("duplicate artifact")
entries=[{"path":name,"bytes":(OUT/name).stat().st_size,"sha256":sha(OUT/name)} for name in sorted(owned)]
manifest={"schema_version":1,"source_commit":fresh["head"],"root":str(OUT),"scope":"all files written by /root/build_validation for this final fixed-P test task; excludes this manifest itself and independently parent-owned disk-cache-cleanup-root.json","files":entries,"file_count":len(entries),"total_bytes":sum(x["bytes"] for x in entries)}
write(OUT/"validation-artifact-manifest.json",manifest)
print(json.dumps({"summary_sha256":sha(OUT/"final-validation-summary.json"),"report_sha256":sha(report),"manifest_sha256":sha(OUT/"validation-artifact-manifest.json"),"artifact_files":len(entries),"artifact_bytes":manifest["total_bytes"],"source_unchanged":True,"local_rust_gate":"failed","remaining_in_fixed_P":result["fixed_P_task_ledger"]["remaining"]}))
