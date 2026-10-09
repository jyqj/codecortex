#!/usr/bin/env python3
"""Read saved original CI/API records and immutable Git objects; execute no product/tests."""
import ast
from collections import Counter
from datetime import datetime, timezone
import hashlib
import json
from pathlib import Path
import re
import subprocess

HERE = Path(__file__).resolve().parent
ROOT = HERE.parent.parent
REPO = ROOT / "codecortex"
G = "50d9b3bc7d1ecca1b6b1c3822a36c12b546b0529"
P = "b4fef72211e5967f4fba729d25d0ca2958094fd5"
R = "98fe910f22eb92f8d9f9c8a8043492ba45dc997c"
CHECKOUT = "0805905f48b963b60864f40e74fef6aed7c4f934"
TREE = "87e746f47d4ea8c1a6b06cb29e180fc7bbd78137"
SAMPLER = "benchmark::sampler::process_probe_tests::linux::live_child_snapshot_is_attributed_monotonic_and_disappears"
EXPECTED_LOG_SHA = "1ae3d8991dc6839be84460e0f324c08069050bacf6cff09ebfc5d5dacf3102eb"


def sha(data):
    return hashlib.sha256(data).hexdigest()


def file_record(path):
    data = path.read_bytes()
    return {"path": str(path.relative_to(ROOT)), "bytes": len(data), "sha256": sha(data)}


def git(*args):
    return subprocess.check_output(["git", *args], cwd=REPO)


def inventory(ref):
    out = {}
    for row in git("ls-tree", "-r", "-z", ref).split(b"\0"):
        if not row:
            continue
        meta, path = row.decode().split("\t", 1)
        mode, kind, oid = meta.split()
        out[path] = {"mode": mode, "kind": kind, "oid": oid}
    return out


def subset_digest(paths, items):
    payload = [{"path": p, **items[p]} for p in sorted(paths)]
    return sha((json.dumps(payload, sort_keys=True, separators=(",", ":")) + "\n").encode())


def main():
    run = json.loads((HERE / "api-run.json").read_text())
    jobs = json.loads((HERE / "api-jobs.json").read_text())
    api_g = json.loads((HERE / "api-head_commit.json").read_text())
    api_checkout = json.loads((HERE / "api-checkout_commit.json").read_text())
    assert run["id"] == 37903250638 and run["run_attempt"] == 1
    assert run["head_sha"] == G and run["status"] == "completed" and run["conclusion"] == "success"
    assert jobs["total_count"] == len(jobs["jobs"]) == 3
    assert all(j["status"] == "completed" and j["conclusion"] == "success" for j in jobs["jobs"])
    assert all(s["status"] == "completed" and s["conclusion"] == "success" for j in jobs["jobs"] for s in j["steps"])
    check = next(j for j in jobs["jobs"] if j["id"] == 113730503903)
    steps = {s["name"]: s for s in check["steps"]}
    raw = (HERE / "original-check.log").read_bytes()
    assert len(raw) == 1066114 and sha(raw) == EXPECTED_LOG_SHA
    lines = [re.sub(r"\x1b\[[0-9;]*m", "", re.sub(r"^\ufeff?\d{4}-\d\d-\d\dT\S+Z ", "", x))
             for x in raw.decode().splitlines()]
    assert len(lines) == 9603
    at = next(i for i, line in enumerate(lines) if "git log -1 --format=%H" in line)
    assert lines[at + 1] == CHECKOUT
    assert api_g["sha"] == G and api_checkout["sha"] == CHECKOUT
    assert api_g["tree"]["sha"] == api_checkout["tree"]["sha"] == TREE
    assert git("rev-parse", G + "^{tree}").decode().strip() == TREE
    assert [p["sha"] for p in api_checkout["parents"]] == ["bd4693351149a443bd0054a37508c53f74ed7b52", G]

    def position(text):
        return next(i for i, line in enumerate(lines) if line == text)

    def block(start_text, end_text):
        return position(start_text), position(end_text)

    fmt = block("##[group]Run cargo fmt --all -- --check", "##[group]Run cargo clippy --workspace --all-targets -- -D warnings")
    clippy = block("##[group]Run cargo clippy --workspace --all-targets -- -D warnings", "##[group]Run cargo test --workspace --all-targets --locked --no-run")
    compile_only = block("##[group]Run cargo test --workspace --all-targets --locked --no-run", "##[group]Run cargo test --workspace --all-targets --exclude cc-semantic --locked")
    default = block("##[group]Run cargo test --workspace --all-targets --exclude cc-semantic --locked", "##[group]Run # Read-only AST comparator object; never check out or run its driver.")
    source = block("##[group]Run # Read-only AST comparator object; never check out or run its driver.", '##[group]Run python3 artifacts/checkpoints/semantic-http-ci-regression-20261003/check.py --output-dir "$RUNNER_TEMP/semantic-http-ci-regression"')
    rust = []
    python = []
    active_methods = []
    methods = []
    for i, line in enumerate(lines):
        m = re.search(r"test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;", line)
        if m:
            item = {"line": i + 1, "status": m[1], **dict(zip(["passed", "failed", "ignored", "measured", "filtered_out"], map(int, m.groups()[1:])))}
            assert item["status"] == "ok" and item["failed"] == 0
            rust.append(item)
        m = re.fullmatch(r"(test_\S+) \(([^)]+)\) \.\.\. (.*)", line)
        if m:
            assert m[3] == "ok", (i + 1, line)
            item = {"line": i + 1, "id": m[2], "result": m[3]}
            active_methods.append(item)
            methods.append(item)
        m = re.fullmatch(r"Ran (\d+) tests in ([\d.]+)s", line)
        if m:
            assert lines[i + 2] == "OK"
            if active_methods:
                assert len(active_methods) == int(m[1])
            else:
                assert (i + 1, int(m[1])) == (8997, 9)
            python.append({"summary_line": i + 1, "count": int(m[1]), "seconds": float(m[2]), "terminal": "OK",
                           "first_method_line": active_methods[0]["line"] if active_methods else None, "last_method_line": active_methods[-1]["line"] if active_methods else None,
                           "visible_method_lines": len(active_methods), "unique_full_ids": len({x["id"] for x in active_methods}),
                           "granularity": "verbose method lines plus terminal summary" if active_methods else "original nonverbose terminal summary only"})
            active_methods = []
    assert not active_methods
    assert [x["count"] for x in python] == [46, 409, 181, 9, 14]
    for item, name in zip(python, ["tests/resource_harness", "scripts/tests/test_p8_*", "tests/source_integrity", "source_architecture", "tests/historical_corpus"]):
        item["suite"] = name
    assert len(rust) == 419

    def totals(selected):
        return {"summary_count": len(selected), **{k: sum(x[k] for x in selected) for k in ["passed", "failed", "ignored", "measured", "filtered_out"]}}

    proofs = []
    for i, line in enumerate(lines):
        if not line.startswith("{"):
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if value.get("source_version") == "p8-completion-source-20261009-v15":
            proofs.append({"line": i + 1, "value": value})
    assert len(proofs) == 1
    proof = proofs[0]["value"]
    assert proof["status"] == "passed" and proof["product_source"] == P and proof["review_source"] == R
    assert proof["complete_inputs"] == 1091
    sampler = []
    for i, line in enumerate(lines):
        if line == f"test {SAMPLER} ... ok":
            summary = next(x for x in rust if x["line"] > i + 1)
            sampler.append({"line": i + 1, "result": "ok", "command_scope": "default authorized workspace" if default[0] <= i < default[1] else "cc-eval eval-http", "enclosing_suite": summary})
    assert len(sampler) == 2
    assert [x["enclosing_suite"]["passed"] for x in sampler] == [60, 60]
    assert [x["enclosing_suite"]["ignored"] for x in sampler] == [5, 5]

    tree_g, tree_p = inventory(G), inventory(P)
    guard_path = "scripts/verify_reviewed_source_v15.py"
    guard_bytes = git("show", G + ":" + guard_path)
    constants = {}
    for node in ast.parse(guard_bytes).body:
        if isinstance(node, ast.Assign):
            for target in node.targets:
                if isinstance(target, ast.Name):
                    try:
                        constants[target.id] = ast.literal_eval(node.value)
                    except (ValueError, TypeError):
                        pass
    registry_path = "scripts/reviewed-source-registry-v15.json"
    registry_bytes = git("show", G + ":" + registry_path)
    registry = json.loads(registry_bytes)
    assert sha(registry_bytes) == constants["REGISTRY_SHA256"]
    assert constants["PRODUCT"] == registry["product_source"] == P
    assert constants["REVIEW"] == registry["review_source"] == R
    review_path = constants["REVIEW_PATH"]
    review_bytes = git("show", R + ":" + review_path)
    assert sha(review_bytes) == registry["review_sha256"]
    assert git("rev-parse", G + ":" + review_path) == git("rev-parse", R + ":" + review_path)
    domains = {}
    for field, count in [("complete_inputs", 1091), ("validation_inputs", 139)]:
        paths = sorted(registry[field])
        assert len(paths) == count and all(p in tree_g for p in paths)
        domains[field] = {"count": count, "path_mode_type_blob_manifest_sha256": subset_digest(paths, tree_g),
                          "checkout_and_G_equal": True, "equality_basis": "The complete root Git tree object is identical; this includes all artifact evidence and every current guard/registry file."}
    assert all(tree_g[p] == tree_p[p] for p in registry["complete_inputs"])
    domains["complete_inputs"]["all_P2_to_G_path_mode_type_blob_equal"] = True
    local_path = ROOT / "validation/final-product-checks-corrected/receipt.json"
    local = json.loads(local_path.read_text())
    assert local["source_before"] == local["source_after"]
    assert local["source_before"]["source_commit"] == P
    assert local["source_before"]["inputs"] == registry["complete_inputs"]
    local_workspace = next(x for x in local["commands"] if x["name"] == "workspace-tests")
    assert local_workspace["exit_code"] == 101
    local_log_path = local_path.parent / local_workspace["log"]
    assert sha(local_log_path.read_bytes()) == local_workspace["log_sha256"]
    sampler_path = "crates/cc-eval/src/benchmark/sampler.rs"
    sampler_sources = {ref: {"git_blob": git("rev-parse", ref + ":" + sampler_path).decode().strip(),
                             "sha256": sha(git("show", ref + ":" + sampler_path))}
                       for ref in ["bd4693351149a443bd0054a37508c53f74ed7b52", P, G]}
    assert len({x["sha256"] for x in sampler_sources.values()}) == 1
    workflow_path = ".github/workflows/ci.yml"
    api_sources = [
        {"file": "api-run.json", "url": "https://api.github.com/repos/jyqj/codecortex/actions/runs/37903250638"},
        {"file": "api-jobs.json", "url": "https://api.github.com/repos/jyqj/codecortex/actions/runs/37903250638/attempts/1/jobs", "query_parameters": {"per_page": 100}},
        {"file": "api-head_commit.json", "url": f"https://api.github.com/repos/jyqj/codecortex/git/commits/{G}"},
        {"file": "api-checkout_commit.json", "url": f"https://api.github.com/repos/jyqj/codecortex/git/commits/{CHECKOUT}"},
    ]
    for item in api_sources:
        item.update(file_record(HERE / item["file"]))
    out = {
        "schema_version": 1, "reviewer": "/root/pr_audit", "reviewed_at_utc": datetime.now(timezone.utc).isoformat(),
        "status": "accepted_original_G181_CI_scope_with_local_failure_preserved", "repository": "jyqj/codecortex", "pr": 181,
        "run": {k: run[k] for k in ["id", "run_attempt", "event", "head_sha", "head_branch", "status", "conclusion", "created_at", "updated_at", "run_started_at", "html_url"]},
        "actual_execution_checkout": {"sha": CHECKOUT, "tree": TREE, "log_line": at + 2, "parents": [x["sha"] for x in api_checkout["parents"]]},
        "source_input_bridge": {"head": G, "head_tree": TREE, "checkout_tree": TREE, "complete_root_tree_identical": True,
            "includes_all_artifact_files_and_source_guard_consumers": True, "whole_tree_path_count": len(tree_g),
            "artifact_path_count": sum(p.startswith("artifacts/") for p in tree_g), "domains": domains,
            "registry": {"path": registry_path, "git_blob": tree_g[registry_path]["oid"], "sha256": sha(registry_bytes)},
            "current_guard": {"path": guard_path, "git_blob": tree_g[guard_path]["oid"], "sha256": sha(guard_bytes)},
            "fixed_review": {"source": R, "path": review_path, "git_blob": tree_g[review_path]["oid"], "sha256": sha(review_bytes), "same_blob_in_G": True},
            "basis": "Official Git commit objects for both immutable identities reference the same complete tree; the locally held G tree matches that official tree. All declared P2 product paths were also compared by mode/type/blob. No historical G275 conclusion is reused."},
        "original_check_log": {**file_record(HERE / "original-check.log"), "lines": len(lines), "encoding": "UTF-8, original leading BOM and trailing newline preserved",
            "retrieval": "GitHub plugin fetch_workflow_job_logs returned the complete decoded log; no truncation or curl fallback was needed.",
            "official_api_url": "https://api.github.com/repos/jyqj/codecortex/actions/jobs/113730503903/logs",
            "official_job_url": "https://github.com/jyqj/codecortex/actions/runs/37903250638/job/113730503903",
            "line_number_basis": "One-based physical lines in this saved decoded original; timestamps and ANSI escapes were removed only in memory for parsing."},
        "original_check": {
            "job_id": check["id"], "started_at": check["started_at"], "completed_at": check["completed_at"],
            "environment": {"os": "Ubuntu 24.04.5 LTS", "runner_image": "ubuntu-24.04 20261004.327.1", "rustc": "1.99.0 (b940084d7 2026-09-28)", "host": "x86_64-unknown-linux-gnu", "RUSTFLAGS": "-D warnings", "CARGO_INCREMENTAL": "0", "evidence_lines": [12, 13, 17, 18, 377, 379, 381, 383]},
            "format": {"command": "cargo fmt --all -- --check", "github_step": steps["Format check"], "log_lines": [fmt[0] + 1, fmt[1]]},
            "clippy": {"command": "cargo clippy --workspace --all-targets -- -D warnings", "github_step": steps["Clippy"], "log_lines": [clippy[0] + 1, clippy[1]], "finished_line": 481, "locked_or_offline_flags": False},
            "compile_all_targets": {"command": "cargo test --workspace --all-targets --locked --no-run", "github_step": steps["Compile every default test target"], "log_lines": [compile_only[0] + 1, compile_only[1]], "scope": "Compilation does not count as test execution."},
            "authorized_default_regression": {"github_step": steps["Default regression within authorized scope"], "log_lines": [default[0] + 1, default[1]],
                "commands": ["cargo test --workspace --all-targets --exclude cc-semantic --locked", "cargo test -p cc-semantic --lib --locked", "cargo test -p cc-semantic --locked --test artifact_cache --test attempt_lease_contract --test bounded_parallel --test manifest_exact_integration --test p7_cache_allocation_independent_review --test publish_cas --test query_cache --test queue_worker --test retry_worker_layering --test semantic_degrade --test space_switch"],
                "executed_rust_summary_totals": totals([x for x in rust if default[0] + 1 <= x["line"] <= default[1]]),
                "scope": "Original workflow deliberately runs normal-path semantic targets. It does not execute every cc-semantic process-fault, GC/WAL or semantic_runtime recoverable-retry target in the default step; it is not the same argv as the earlier local unfiltered workspace command."},
            "whole_check_rust": {**totals(rust), "population": "Test executions summed across 419 original command summaries, not distinct test IDs; ignored/filtered executions stay separate."},
            "whole_check_python": {"suites": python, "reported_test_executions": sum(x["count"] for x in python), "visible_method_executions": len(methods), "visible_unique_full_ids": len({x["id"] for x in methods}), "visible_methods_non_ok": 0, "nonverbose_summary_only_tests": sum(x["count"] for x in python if x["visible_method_lines"] == 0)},
            "native_source_integrity": python[2], "v15_original_proof": proofs[0],
            "sampler": {"test_id": SAMPLER, "executions": sampler, "source_path": sampler_path, "source_bytes": sampler_sources,
                "interpretation": "The unchanged test executed successfully twice on this GitHub runner. This is positive original evidence for these executions, not a fix or a waiver of the local failures."},
            "step_exit_interpretation": "GitHub API reports success for every original step and raw bash -e command execution continues to later steps; explicit numeric per-command OS return receipts are not present in the decoded log. Native unittest and v15 have their own visible successful terminal results."
        },
        "local_failure_parallel_record": {"receipt": file_record(local_path), "source": P, "complete_inputs": 1091,
            "product_paths_equal_G": True, "command": local_workspace["command"], "exit_code": 101,
            "log": file_record(local_log_path), "rustc": local["rustc"], "completed_at_utc": local["completed_at_utc"],
            "failing_test": SAMPLER, "failure_line": 661, "reason_line": 662, "reason": "live child snapshot unavailable",
            "failing_suite_summary": {"line": 669, "passed": 59, "failed": 1, "ignored": 5, "filtered_out": 0},
            "targeted_followup_log": file_record(ROOT / "validation/sampler-targeted-diagnostic.log"), "targeted_followup_result": "0 passed / 1 failed / 64 filtered out; same live child snapshot unavailable assertion",
            "adjudication": "Original local workspace exit101 and targeted failure remain failures. GitHub success is recorded alongside them with differing Rust toolchain, runner environment, build profile and command scope. Source equality alone does not establish the cause or exempt the failure."},
        "other_CI_jobs": [{"id": j["id"], "name": j["name"], "status": j["status"], "conclusion": j["conclusion"], "steps": j["steps"], "html_url": j["html_url"], "evidence_scope": "Official job and step metadata only; their full raw logs were not independently reviewed in this bounded check-log audit."} for j in jobs["jobs"] if j["id"] != check["id"]],
        "workflow": {"path": workflow_path, "git_blob": tree_g[workflow_path]["oid"], "sha256": sha(git("show", G + ":" + workflow_path)), "url": f"https://github.com/jyqj/codecortex/blob/{G}/{workflow_path}"},
        "official_api_inputs": api_sources,
        "reviewer_parser_attempts": {"initial": file_record(HERE / "reviewer-parser-attempt01.json"), "correction": "Only the new read-only reviewer parser changed to preserve the original nonverbose nine-test source_architecture summary granularity. The original 14 historical_corpus tests are verbose and included in the 650 individually observed methods. No product, original guard or original evidence changed and no test was executed."},
        "limits": ["Read-only saved-log and immutable-source analysis; no workflow dispatch, product execution, test rerun, branch change or worktree mutation.",
            "Actual execution remains checkout0805905f. Complete tree equality provides source-domain attribution to G50d9, without renaming the execution or certifying later A content.",
            "A green CI supplies no new scale, latency, provider, runtime/quality or release acceptance by itself.",
            "No G275 runtime, platform, lifecycle, scale or source-proof evidence is borrowed for this result.",
            "Lifecycle37903250681 and other workflow states were outside this task's original-check-log content review.",
            "Original ignored cases and deliberate execution exclusions were retained, not counted as passed."],
        "todo_accounting": {"closed_by_this_review": 0, "done": 163, "total": 192, "remaining": 29}
    }
    output = HERE / "independent-review.json"
    output.write_text(json.dumps(out, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"status": out["status"], "report": file_record(output), "whole_check_rust": out["original_check"]["whole_check_rust"], "whole_check_python": {k: v for k, v in out["original_check"]["whole_check_python"].items() if k != "suites"}, "domains": domains}, ensure_ascii=False))


if __name__ == "__main__":
    main()
