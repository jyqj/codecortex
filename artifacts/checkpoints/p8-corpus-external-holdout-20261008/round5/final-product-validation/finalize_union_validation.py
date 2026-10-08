#!/usr/bin/env python3
"""Seal exact-source targeted union validation without running more tests."""
import datetime
import hashlib
import importlib.util
import json
from pathlib import Path
import re

BASE = Path("/workspace/scratch/031390cf22eb")
OUT = BASE / "round5-validation"
ROOT = BASE / "codecortex-union-validation"
SOURCE = "1ed3c7df574db6d450f79ef29d5148f68556b3db"
TREE = "d16aa044e56c9347e6b4ff75d783cc4317f15087"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def write(path, data):
    assert not path.exists(), path
    path.write_text(json.dumps(data, ensure_ascii=False, sort_keys=True, indent=2) + "\n")


def reference(path):
    data = path.read_bytes()
    return {"path": str(path), "bytes": len(data), "sha256": sha(data)}


def main():
    spec = importlib.util.spec_from_file_location("union_observer", OUT / "record_validation.py")
    observer = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(observer)
    observer.EXPECTED_HEAD = SOURCE
    observer.EXPECTED_TREE = TREE
    final = observer.inventory()
    assert final["status"] == ""
    assert final["counts"] == {"source": 1082, "validation_and_policy": 114}
    commands = []
    identities = []
    for name in ("union-workspace-cache-clean", "union-product-build",
                 "union-coverage-diagnostics", "union-real-mcp-readiness",
                 "union-watcher-admission", "union-ci-clippy"):
        path = OUT / (name + ".json")
        receipt = json.loads(path.read_text())
        assert receipt["status"] == "passed" and receipt["exit_code"] == 0
        assert receipt["source_unchanged"] and receipt["source_identity"]["head"] == SOURCE
        log = Path(receipt["log"]).read_bytes()
        assert sha(log) == receipt["log_sha256"]
        for stage in ("before", "after"):
            snapshot_path = Path(receipt["source_" + stage])
            data = snapshot_path.read_bytes()
            assert sha(data) == receipt["source_" + stage + "_sha256"]
            assert json.loads(data) == final
        assert receipt["observer_sha256"] == sha((OUT / "record_validation.py").read_bytes())
        identities.append(receipt["source_identity"])
        row = {"name": name, "receipt": reference(path), "log": reference(Path(receipt["log"])),
               "argv": receipt["argv"], "environment": receipt["environment_overrides"],
               "exit_code": 0, "elapsed_seconds": receipt["elapsed_seconds"],
               "source_unchanged": True}
        methods = re.findall(r"^test (.+?) \.\.\. (ok|FAILED|ignored)(?:,.*)?$", log.decode(), re.M)
        if name in ("union-coverage-diagnostics", "union-real-mcp-readiness", "union-watcher-admission"):
            expected_count = {"union-coverage-diagnostics": 9, "union-real-mcp-readiness": 2,
                              "union-watcher-admission": 2}[name]
            assert len(methods) == expected_count and all(status == "ok" for _, status in methods)
            result = re.search(r"test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out", log.decode())
            assert result and tuple(map(int, result.groups()[:4])) == (expected_count, 0, 0, 0)
            row.update(passed=expected_count, failed=0, ignored=0,
                       filtered_out=int(result.group(5)), methods=[name for name, _ in methods])
            if name != "union-watcher-admission":
                target = "benchmark_coverage_diagnostics" if expected_count == 9 else "p8_mcp_readiness"
                text = (ROOT / f"crates/cc-eval/tests/{target}.rs").read_text()
                names = re.findall(r"#\[(?:tokio::)?test\](?:\s*#\[[^\n]*\])*\s*(?:async\s+)?fn\s+(\w+)\(", text)
                assert set(names) == set(row["methods"])
            else:
                text = (ROOT / "crates/cc-server/src/watcher.rs").read_text().split("mod explicit_text_admission_tests {", 1)[1]
                names = re.findall(r"#\[test\]\s*fn\s+(\w+)\(", text)
                assert {"watcher::explicit_text_admission_tests::" + n for n in names} == set(row["methods"])
        if name == "union-real-mcp-readiness":
            assert receipt["external_inputs_before"] == receipt["external_inputs_after"]
            assert len(receipt["external_inputs_before"]) == 1
            row["explicit_binary"] = receipt["external_inputs_before"][0]
            assert row["explicit_binary"]["sha256"] == sha(Path(row["explicit_binary"]["path"]).read_bytes())
            assert receipt["argv"][-2:] == ["--", "--ignored"]
        commands.append(row)
    assert all(identity == identities[0] for identity in identities)
    assert sum(row.get("passed", 0) for row in commands) == 13

    product_path = OUT / "product/build-receipt.json"
    product = json.loads(product_path.read_text())
    assert product["status"] == "passed" and product["build_exit_code"] == 0
    assert product["source_before"] == product["source_after"]
    assert product["source_before"]["source_commit"] == SOURCE
    assert product["source_unchanged"] and product["toolchain_unchanged"] and product["observer_unchanged"]
    assert product["observer_sha256"] == sha((OUT / "build_union_product.py").read_bytes())
    assert product["cargo_artifact"]["profile"] == product["actual_cargo_profile"]
    assert product["cargo_artifact"]["features"] == [] and product["cargo_artifact"]["fresh"] is False
    assert product["all_workspace_artifacts_fresh_false"]
    assert len(product["workspace_cargo_artifacts"]) == 7
    assert all(artifact["fresh"] is False for artifact in product["workspace_cargo_artifacts"])
    binary = Path(product["binary_path"])
    assert sha(binary.read_bytes()) == product["binary_sha256"]
    assert sha((OUT / "product/cargo-build.jsonl").read_bytes()) == product["cargo_stdout_sha256"]
    assert sha((OUT / "product/cargo-build.stderr.log").read_bytes()) == product["cargo_stderr_sha256"]
    cargo = [json.loads(line) for line in (OUT / "product/cargo-build.jsonl").read_text().splitlines()]
    actual = [row for row in cargo if row.get("reason") == "compiler-artifact" and row.get("target", {}).get("name") == "codecortex" and row.get("target", {}).get("kind") == ["bin"]]
    assert actual == [product["cargo_artifact"]]
    assert [row.get("success") for row in cargo if row.get("reason") == "build-finished"] == [True]

    prior = BASE / "round4-final-validation/final-validation-summary.json"
    assert sha(prior.read_bytes()) == "40c96f5767b47bd40b011e32d1909ef52ae2bfbbe200ccc70b389a79630cd194"
    write(OUT / "final-source-snapshot.json", final)
    summary = {
        "schema_version": 1,
        "created_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
        "status": "completed_scoped_validation",
        "source": SOURCE, "tree": TREE, "worktree": str(ROOT),
        "source_identity": identities[0],
        "all_1196_source_and_policy_inputs_unchanged": True,
        "final_source_snapshot": reference(OUT / "final-source-snapshot.json"),
        "commands": commands,
        "targeted_tests": {"passed": 13, "failed": 0, "ignored": 0},
        "ci_workspace_all_targets_clippy": {"exit_code": 0, "warnings_as_errors": True, "feature_scope": "workspace defaults"},
        "product": {"receipt": reference(product_path), "binary": reference(binary),
                    "build_profile": product["build_profile"], "actual_cargo_profile": product["actual_cargo_profile"],
                    "actual_cargo_elapsed_seconds": product["elapsed_seconds"],
                    "workspace_packages": product["workspace_package_count"],
                    "workspace_artifacts": product["workspace_artifact_count"],
                    "all_workspace_artifacts_fresh_false": True, "toolchain": product["toolchain"]},
        "verified_behavior": [
            "Coverage diagnostics preserve the original seven controls and pass both new hidden-admission explanations using the real scanner and read-only SQLite observation.",
            "The two explicitly selected normally-ignored MCP tests run the exact copied product through actual child-process stdio: 137 files across at least three pages including quoted paths, and rejection of the same file count with the wrong actual path.",
            "The two watcher controls call the real notify event classification function for create/change/remove and protected/user-ignore handling; they are not an OS watcher subscription/end-to-end timing certification.",
            "All eight workspace packages and all default-feature targets are checked by Clippy with warnings denied; Clippy does not execute those tests.",
        ],
        "preserved_prior_failed_validation": reference(prior),
        "limits": [
            "This exact-source product is a default dev binary with opt_level=0. It is not an optimized release or performance measurement witness.",
            "The old P615 default full-suite sampler and 500 ms fixture failures remain recorded under issue #155. These targeted successes do not overwrite them or stand in for a new complete standard-host test run.",
            "The 19 scanner/config controls on local source 2d575c60 and 12 profile/binding Python controls retain their original distinct source receipts; they are not relabeled as runs on this remote P.",
            "Final selected source-integrity guard, all 174 original methods, other CI feature configurations and original TODO acceptance are owned by the parent/final CI and are not claimed by this receipt.",
            "No benchmark retrieval, evaluation question, holdout body, gold, original task ledger, remote branch or PR was modified or newly executed by this validation task.",
        ],
    }
    write(OUT / "final-validation-summary.json", summary)
    report = f"""# 固定整合 P 的本地定向验证

源码提交为 `{SOURCE}`，tree 为 `{TREE}`。独占工作树保持干净；六次记录命令的全部 1,196 个源码与验证/策略输入在执行前后完全一致，其中 crates/Cargo 1,082 项、验证与策略 114 项。

## 结果

| 验证 | 实际结果 | 命令耗时 |
|---|---:|---:|
| 新产品构建 | Cargo exit 0，源码/工具链一致 | {product['elapsed_seconds']:.3f} 秒 |
| coverage diagnostics 完整 target | 9 通过、0 忽略 | {commands[2]['elapsed_seconds']:.3f} 秒 |
| 显式选入的真实 MCP readiness | 2 通过、0 忽略 | {commands[3]['elapsed_seconds']:.3f} 秒 |
| watcher notify 事件分类 | 2 通过、0 忽略 | {commands[4]['elapsed_seconds']:.3f} 秒 |
| CI 原 workspace/all-targets Clippy | exit 0，警告作为错误 | {commands[5]['elapsed_seconds']:.3f} 秒 |

测试共 **13 通过、0 失败、0 忽略**。watcher 只运行指定的两个方法，另 256 个方法被过滤；这里不把过滤项算为执行。完整方法 ID、argv、环境、前后源码清单和原日志摘要保存在 `final-validation-summary.json` 及每条命令的收据中。

## 实际产品与控制

产品保存为 `product/codecortex`，SHA-256 为 `{product['binary_sha256']}`，大小 {product['binary_bytes']:,} 字节。原始 Cargo JSONL 与 stderr 完整保留。构建前仅清除了 task-owned target-scale 中八个工作区包的可再生编译缓存；第三方依赖缓存继续使用。六个实际参与默认产品构建的工作区包产生七份 Cargo 产物，全部明确 `fresh=false`。复制后的产品字节在两项真实 stdio 测试前后保持不变。

真实 profile 为 dev，`opt_level=0`、`debuginfo=0`、`debug_assertions=true`、`test=false`，它用于功能回归验证。此构建不宣称优化 release 或性能结果。

coverage target 保留原七个控制，并检验默认隐藏排除与启用后实际准入但尚未索引时的未知原因。MCP target 使用明确指定的新产品子进程，显式 `--ignored` 运行原本默认忽略的两项：137 个文件跨至少三页、引号路径精确保留，以及相同数量但错误实际路径被拒绝。watcher 控制调用真实 notify 事件分类函数，覆盖新增/修改/删除和受保护路径、用户忽略规则；没有把它写成操作系统订阅时序的完整认证。

Clippy 命令为 `cargo clippy --workspace --all-targets --locked -- -D warnings`，沿用 CI 的全工作区与全部 target 范围。它检查默认 feature 配置下的编译/lint，不执行所有测试。

## 保留范围

此前 P615 完整本地测试中的 sampler 与 500 ms fixture 两项失败、原日志及 issue #155 继续保留。这次定向成功不覆盖那些结果。local 2d 的 19 项 scanner/config 控制和 12 项 profile/binding Python 控制保留各自原始源码身份。

最终四 pins 的 source-integrity guard、原完整 174 个方法、其他 CI feature 配置及原 TODO 验收由主代理和最终 CI 单独记录。此任务没有读取/执行任何新的评测或 holdout 题体，也没有更改 gold、任务账本、远端分支或 PR。
"""
    report_path = OUT / "final-validation-report.md"
    assert not report_path.exists()
    report_path.write_text(report)
    manifest_path = OUT / "validation-artifact-manifest.json"
    files = []
    for path in sorted(OUT.rglob("*")):
        if path.is_file():
            assert not path.is_symlink()
            files.append({"path": path.relative_to(OUT).as_posix(), **{k: v for k, v in reference(path).items() if k != "path"}})
    write(manifest_path, {"schema_version": 1, "source": SOURCE, "root": str(OUT),
                         "scope": "all files in this fixed union validation directory except this manifest itself",
                         "file_count": len(files), "total_bytes": sum(row["bytes"] for row in files), "files": files})
    for row in files:
        assert sha((OUT / row["path"]).read_bytes()) == row["sha256"]
    print(json.dumps({"summary": reference(OUT / "final-validation-summary.json"),
                      "report": reference(report_path), "manifest": reference(manifest_path),
                      "files": len(files), "bytes": sum(row["bytes"] for row in files),
                      "tests_passed": 13, "source_unchanged": True}))


if __name__ == "__main__":
    main()
