#!/usr/bin/env python3
"""Read immutable Git objects and saved official API replies; run no product code."""
import argparse
import collections
import datetime
import hashlib
import json
from pathlib import Path
import re
import subprocess

HEAD = "275e8799d4947d297329073eaa3ca675d3fd0777"
CHECKOUT = "d53ea0be17a53d1354ee1d6f0a111c5e588e91f5"
LOG_BLOB = "08b75a9d273b83a56b9f2777241b3194b76e69b9"
LOG_SHA256 = "e054ef90af95bd936a1a408d1f118735c1c0ffdf718c281c782ee2afa7c988c5"
ARCHIVE = "f17b9c41df39972d31d67cfe510d53e6840f92c9"
ARCHIVE_PATH = "artifacts/checkpoints/p8-round18-evidence-20261009/pr173-ci"


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--repo", type=Path, required=True)
    parser.add_argument("--api-inputs", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()

    def git(*words):
        return subprocess.check_output(["git", *words], cwd=args.repo)

    def tree(ref):
        result = {}
        for row in git("ls-tree", "-r", ref).decode().splitlines():
            meta, path = row.split("\t", 1)
            mode, kind, oid = meta.split()
            result[path] = {"mode": mode, "kind": kind, "oid": oid}
        return result

    raw_api = args.api_inputs.read_bytes()
    api = json.loads(raw_api)
    log = git("cat-file", "blob", LOG_BLOB)
    assert len(log) == 1065325
    assert hashlib.sha256(log).hexdigest() == LOG_SHA256
    assert git("rev-parse", f"{ARCHIVE}:{ARCHIVE_PATH}/original-check.log").decode().strip() == LOG_BLOB
    lines = [re.sub(r"\x1b\[[0-9;]*m", "", re.sub(r"^\d{4}-\d\d-\d\dT\S+Z ", "", line))
             for line in log.decode().splitlines()]
    checkout_line = next(i for i, line in enumerate(lines) if "git log -1 --format=%H" in line)
    assert lines[checkout_line + 1] == CHECKOUT

    start = next(i for i, line in enumerate(lines) if line.startswith("##[group]Run # Read-only AST comparator"))
    end = next(i for i in range(start + 1, len(lines)) if lines[i].startswith("##[group]"))
    methods, suites, active_methods = [], [], []
    for i in range(start, end):
        m = re.fullmatch(r"(test_\S+) \(([^)]+)\) \.\.\. (.*)", lines[i])
        if m:
            assert m[3] == "ok", (i + 1, lines[i])
            entry = {"line": i + 1, "id": m[2], "result": m[3]}
            methods.append(entry)
            active_methods.append(entry)
        summary = re.fullmatch(r"Ran (\d+) tests in ([\d.]+)s", lines[i])
        if summary:
            assert int(summary[1]) == len(active_methods)
            assert lines[i + 2] == "OK"
            suites.append({"count": len(active_methods), "unique_ids": len({x["id"] for x in active_methods}),
                           "seconds": float(summary[2]), "summary_line": i + 1, "terminal": "OK",
                           "first_method_line": active_methods[0]["line"], "last_method_line": active_methods[-1]["line"]})
            active_methods = []
    assert not active_methods
    assert [x["count"] for x in suites] == [46, 409, 181]
    assert len(methods) == len({x["id"] for x in methods}) == 636

    rust = []
    for i, line in enumerate(lines):
        m = re.search(r"test result: (\w+)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out;", line)
        if m:
            assert m[1] == "ok"
            rust.append(dict(zip(["passed", "failed", "ignored", "measured", "filtered_out"], map(int, m.groups()[1:]))))
    rust_totals = {key: sum(x[key] for x in rust) for key in rust[0]}
    assert len(rust) == 418
    assert [rust_totals[k] for k in ("passed", "failed", "ignored", "measured")] == [3281, 0, 132, 0]

    proofs = []
    historical = []
    for i, line in enumerate(lines):
        if not line.startswith("{"):
            continue
        try:
            value = json.loads(line)
        except json.JSONDecodeError:
            continue
        if value.get("source_version") == "p8-completion-source-20261009-v15":
            proofs.append({"line": i + 1, "value": value})
        if value.get("verifier_version") == "historical-integrations-v2":
            historical.append({"line": i + 1, "value": value})
    assert len(proofs) == 1 and proofs[0]["value"]["status"] == "passed"
    proof = proofs[0]["value"]
    assert proof["product_source"] == "a402e88d460179afb12e3e2dc066d62cf168c690"
    assert proof["review_source"] == "f66daf6088f6f6a8d990d736ee0c1213032ca175"
    assert proof["complete_inputs"] == 1089

    a = api["official_reads"]["g275_root_tree"]["data"]
    b = api["official_reads"]["g275_ci_checkout_root_tree"]["data"]
    assert not a["truncated"] and not b["truncated"]
    roots_a = {x["path"]: {k: x[k] for k in ("path", "mode", "type", "sha")} for x in a["tree"]}
    roots_b = {x["path"]: {k: x[k] for k in ("path", "mode", "type", "sha")} for x in b["tree"]}
    assert set(roots_a) == set(roots_b) and len(roots_a) == 13
    assert [p for p in sorted(roots_a) if roots_a[p] != roots_b[p]] == ["artifacts"]
    assert git("rev-parse", f"{HEAD}^{{tree}}").decode().strip() == a["sha"]
    assert git("rev-parse", f"{CHECKOUT}^{{tree}}").decode().strip() == b["sha"]
    registry_bytes = git("show", f"{HEAD}:scripts/reviewed-source-registry-v15.json")
    registry = json.loads(registry_bytes)
    t_head, t_checkout = tree(HEAD), tree(CHECKOUT)
    domains = {}
    for name in ("complete_inputs", "validation_inputs"):
        paths = sorted(registry[name])
        assert all(t_head[p] == t_checkout[p] for p in paths)
        assert not any(p.startswith("artifacts/") for p in paths)
        domains[name] = {"count": len(paths), "all_path_mode_type_blob_equal": True,
                         "path_manifest_sha256": hashlib.sha256(("\n".join(paths) + "\n").encode()).hexdigest()}
    assert domains["complete_inputs"]["count"] == 1089 and domains["validation_inputs"]["count"] == 139

    ci_api = api["official_reads"]["g275_ci_jobs"]
    assert ci_api["data"]["total_count"] == len(ci_api["data"]["jobs"]) == 3
    assert all(j["status"] == "completed" and j["conclusion"] == "success" and
               all(s["conclusion"] == "success" for s in j["steps"]) for j in ci_api["data"]["jobs"])
    out = {"schema_version": 1, "status": "accepted_original_ci_scope", "reviewed_at_utc": datetime.datetime.now(datetime.timezone.utc).isoformat(),
           "repository": "jyqj/codecortex", "reviewer": "/root/scale_audit", "head": HEAD, "actual_checkout": CHECKOUT,
           "ci_run_id": 37890756949, "attempt": 1, "check_job_id": 113690916080,
           "ci_url": "https://github.com/jyqj/codecortex/actions/runs/37890756949", "actual_checkout_log_line": checkout_line + 2,
           "original_log": {"git_blob": LOG_BLOB, "bytes": len(log), "sha256": LOG_SHA256, "line_count": len(lines),
                            "url": f"https://github.com/jyqj/codecortex/blob/{ARCHIVE}/{ARCHIVE_PATH}/original-check.log"},
           "python": {"suites_in_order": [dict(name=name, **suite) for name, suite in zip(["resource_harness", "scripts/test_p8_*", "source_integrity"], suites)],
                      "method_executions": len(methods), "unique_full_ids": len({x["id"] for x in methods}), "non_ok": 0},
           "rust": {"summary_count": len(rust), **rust_totals, "population": "executions across original commands, not unique test IDs"},
           "source_proof": proofs[0], "historical_scope_boundaries": historical,
           "complete_source_bridge": {"head_tree": a["sha"], "checkout_tree": b["sha"], "root_entry_count": 13,
                "api_truncated": False, "equal_nonartifact_roots": [roots_a[p] for p in sorted(roots_a) if p != "artifacts"],
                "artifact_roots": {"head": roots_a["artifacts"], "checkout": roots_b["artifacts"]}, "domains": domains,
                "registry_sha256": hashlib.sha256(registry_bytes).hexdigest()},
           "original_ci_jobs": [{k: j[k] for k in ("id", "name", "status", "conclusion", "started_at", "completed_at", "html_url")} for j in ci_api["data"]["jobs"]],
           "api_inputs": {"path": args.api_inputs.name, "sha256": hashlib.sha256(raw_api).hexdigest(), "jobs_url": ci_api["url"], "retrieved_at": ci_api["retrieved_at"]},
           "prior_independent_review": {"git_blob": "875011d4d91976537385f56ac14a6d4998ee6458", "url": f"https://github.com/jyqj/codecortex/blob/{ARCHIVE}/{ARCHIVE_PATH}/independent-review.json"},
           "prior_root_review": {"git_blob": "0f352cd0fb1c986ff742d359210751d881314fe6", "url": f"https://github.com/jyqj/codecortex/blob/{ARCHIVE}/{ARCHIVE_PATH}/root-review.json"},
           "limitations": ["Actual CI checkout remains d53ea0be, never renamed to head G275 or a later merge.",
              "Read-only log/tree analysis; original CI, product and tests were not rerun.",
              "Source proof admits source and validation inputs; runtime, scale, quality and release require independent execution evidence.",
              "132 original ignored Rust executions remain ignored; they were not counted as passed.",
              "A truncated 300-file compare response was not used as completeness evidence; complete Git tree roots and all 1228 registry paths were compared.",
              "Generic raw-log fetch exceeded its 1 MB limit; the existing original Git blob was read locally and matched the official archive object."],
           "todo_accounting": {"newly_closed": 0, "remaining": 29}}
    args.output.write_text(json.dumps(out, ensure_ascii=False, indent=2) + "\n")
    print(json.dumps({"status": out["status"], "output": str(args.output), "bytes": args.output.stat().st_size, "sha256": hashlib.sha256(args.output.read_bytes()).hexdigest()}))


if __name__ == "__main__":
    main()
