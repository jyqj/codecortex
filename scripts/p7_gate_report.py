#!/usr/bin/env python3
"""Build a reproducible G7 review dossier; never grant task or release approval."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path, PurePosixPath
import re
import stat
import subprocess
import sys

from p7_build_identity import json_bytes, source_snapshot

TASKS = "docs/roadmap/code-index-v2/tasks.json"
DECISION = "artifacts/checkpoints/p789-blocking-analysis-20261002/DECISIONS-RECORDED.json"
TASK_IDS = {f"P7-{number:03}" for number in range(1, 21)}
HARD_G7 = {f"P7-{number:03}" for number in range(1, 18)} | {"P7-019"}
KINDS = {"implementation", "current_validation", "blocked_disposition", "scope_audit"}
STATES = {"todo", "in_progress", "done", "blocked", "deferred"}
SHA = re.compile(r"[0-9a-f]{64}\Z")
COMMIT = re.compile(r"[0-9a-f]{40}\Z")


def require(condition, message):
    if not condition:
        raise ValueError(message)


def unique_object(pairs):
    result = {}
    for key, value in pairs:
        require(key not in result, "duplicate JSON field: " + key)
        result[key] = value
    return result


def parse(raw):
    def invalid_constant(value):
        raise ValueError("non-finite JSON constant: " + value)
    return json.loads(raw, object_pairs_hook=unique_object, parse_constant=invalid_constant)


def keys(value, expected, name):
    require(isinstance(value, dict) and set(value) == set(expected), "invalid " + name + " fields")


def digest(data):
    return hashlib.sha256(data).hexdigest()


def regular_path(root, value):
    require(isinstance(value, str) and value and "\\" not in value, "invalid evidence path")
    path = PurePosixPath(value)
    require(path.parts and not path.is_absolute() and ".." not in path.parts and str(path) == value,
            "evidence path must be canonical and relative")
    require(path.parts[0] in {"artifacts", "docs", "scripts", "crates"}, "unsupported evidence root")
    current = root
    for part in path.parts:
        current = current / part
        require(not current.is_symlink(), "symlink evidence path")
    require(stat.S_ISREG(current.lstat().st_mode), "evidence must be a regular file")
    return current


def reference(root, value):
    keys(value, {"path", "sha256"}, "evidence reference")
    require(isinstance(value["sha256"], str) and SHA.fullmatch(value["sha256"]), "invalid evidence digest")
    path = regular_path(root, value["path"])
    require(path.stat().st_size <= 64 * 1024 * 1024, "evidence file exceeds 64 MiB bound")
    h = hashlib.sha256()
    with path.open("rb") as stream:
        for block in iter(lambda: stream.read(1024 * 1024), b""):
            h.update(block)
    require(h.hexdigest() == value["sha256"], "evidence digest drift: " + value["path"])
    return dict(value, bytes=path.stat().st_size)


def task_map(data):
    tasks = data.get("tasks", [])
    by_id = {task["id"]: task for task in tasks}
    require(len(tasks) == len(by_id) == data.get("task_count"), "task count or identity drift")
    require(TASK_IDS <= set(by_id), "incomplete P7 task inventory")
    require(all(task["status"] in STATES for task in tasks), "invalid task status")
    require(set(by_id["P7-020"]["depends_on"]) == HARD_G7, "original G7 hard dependencies changed")
    for task in tasks:
        require(all(dep in by_id for dep in task["depends_on"]), "missing hard dependency")
        if task["status"] == "done":
            require(task.get("evidence") and all(by_id[dep]["status"] == "done" for dep in task["depends_on"]),
                    "done task lacks evidence or has unfinished hard dependency")
    return by_id


def git(root, *args):
    return subprocess.check_output(["git", *args], cwd=root, stderr=subprocess.PIPE)


def collect(root, index_path):
    root = Path(root).resolve(strict=True)
    with Path(index_path).open("rb") as index_file:
        raw = index_file.read(256 * 1024 + 1)
    require(len(raw) <= 256 * 1024, "evidence index exceeds 256 KiB bound")
    index = parse(raw)
    keys(index, {"schema_version", "baseline_sha", "tasks_sha256", "decision", "entries", "wiring"}, "index")
    require(type(index["schema_version"]) is int and index["schema_version"] == 1, "unsupported index version")
    require(isinstance(index["baseline_sha"], str) and COMMIT.fullmatch(index["baseline_sha"]), "baseline must be an immutable full commit")
    current_raw = (root / TASKS).read_bytes()
    require(digest(current_raw) == index["tasks_sha256"], "task authority changed; rebuild the explicit index")
    current = parse(current_raw)
    tasks = task_map(current)
    baseline_raw = git(root, "show", index["baseline_sha"] + ":" + TASKS)
    baseline = task_map(parse(baseline_raw))
    require(set(tasks) == set(baseline), "original task inventory changed")
    # Status/evidence may advance, but this reporter cannot silently replace
    # original acceptance or dependency conditions to make a dossier pass.
    for task_id in TASK_IDS:
        for field in ("acceptance", "validations", "depends_on", "conditional_dependencies", "conditional", "required_for"):
            require(tasks[task_id].get(field) == baseline[task_id].get(field), "original task condition changed: " + task_id + "/" + field)
    decision_ref = reference(root, index["decision"])
    require(decision_ref["path"] == DECISION, "wrong authorization decision record")
    require((root / DECISION).read_bytes() == git(root, "show", index["baseline_sha"] + ":" + DECISION), "authorization decision differs from fixed baseline")
    decision = parse((root / DECISION).read_bytes())
    selected = [item for item in decision["decisions"] if item["id"] == "D1+D2"]
    require(len(selected) == 1 and "P7-018" in selected[0]["affected_tasks"], "missing original live disposition")
    require(tasks["P7-018"]["status"] == "blocked", "current no-live disposition must remain blocked")

    require(isinstance(index["entries"], list) and len(index["entries"]) <= 20, "invalid entry inventory")
    entries = {}
    for entry in index["entries"]:
        keys(entry, {"task_id", "kind", "scope", "source_commit", "artifacts", "limitations"}, "task entry")
        task_id = entry["task_id"]
        require(task_id in TASK_IDS and task_id not in entries, "unknown or duplicate task entry")
        require(entry["kind"] in KINDS, "invalid advancement kind")
        require(isinstance(entry["scope"], str) and entry["scope"].strip(), "missing scoped observation")
        require(isinstance(entry["limitations"], list) and entry["limitations"] and all(isinstance(x, str) and x.strip() for x in entry["limitations"]), "missing explicit limits")
        require(isinstance(entry["source_commit"], str) and COMMIT.fullmatch(entry["source_commit"]), "task source must be immutable")
        git(root, "cat-file", "-e", entry["source_commit"] + "^{commit}")
        require(isinstance(entry["artifacts"], list) and 1 <= len(entry["artifacts"]) <= 32, "missing or excessive task evidence")
        require(len({ref["path"] for ref in entry["artifacts"]}) == len(entry["artifacts"]), "duplicate artifact reference")
        if task_id == "P7-018":
            require(entry["kind"] == "blocked_disposition", "blocked live task cannot be counted as implementation")
        changed = git(root, "diff", "--name-only", entry["source_commit"], "HEAD", "--", "crates", "Cargo.toml", "Cargo.lock").decode().splitlines()
        entries[task_id] = dict(entry, artifacts=[reference(root, ref) for ref in entry["artifacts"]],
                               task_status=tasks[task_id]["status"], source_inputs_equal_to_current=not changed,
                               source_paths_different_from_current=changed,
                               evidence_integrity="verified_bytes_only; execution semantics require scoped review")
    require(isinstance(index["wiring"], list) and len(index["wiring"]) == 13, "all 13 wiring rows are required")
    wiring = {}
    for item in index["wiring"]:
        keys(item, {"id", "task_id", "status", "note", "artifacts"}, "wiring row")
        require(type(item["id"]) is int and 1 <= item["id"] <= 13 and item["id"] not in wiring, "invalid wiring identity")
        require(item["task_id"] in TASK_IDS and item["status"] in {"implemented", "partial", "transferred", "not_reviewed"}, "invalid wiring disposition")
        require(isinstance(item["note"], str) and item["note"].strip(), "missing wiring explanation")
        require(isinstance(item["artifacts"], list) and 1 <= len(item["artifacts"]) <= 16, "missing wiring evidence")
        wiring[item["id"]] = dict(item, artifacts=[reference(root, ref) for ref in item["artifacts"]])

    snapshot = source_snapshot(root)
    counts = dict(Counter(task["status"] for task in tasks.values()))
    remaining = [task_id for task_id, task in tasks.items() if task["status"] not in {"done", "deferred"}]
    hard_open = sorted(task_id for task_id in HARD_G7 if tasks[task_id]["status"] != "done")
    missing = sorted(HARD_G7 - set(entries))
    drifted = sorted(task_id for task_id, entry in entries.items() if task_id in HARD_G7 and not entry["source_inputs_equal_to_current"])
    accepted = sorted(task_id for task_id in TASK_IDS if tasks[task_id]["status"] == "done" and baseline[task_id]["status"] != "done")
    return {
        "schema_version": 1, "report_kind": "G7_review_dossier", "index_sha256": digest(raw),
        "reporter_sha256": digest(Path(__file__).read_bytes()),
        "tasks_sha256": digest(current_raw), "baseline_sha": index["baseline_sha"],
        "source": {key: value for key, value in snapshot.items() if key != "inputs"},
        "counts": counts, "unfinished_count": len(remaining), "unfinished_task_ids": remaining,
        "progress": {"accepted_since_baseline": accepted,
                     "implemented_or_currently_validated": sorted(task_id for task_id, entry in entries.items() if entry["kind"] in {"implementation", "current_validation"}),
                     "blocked_dispositions": sorted(task_id for task_id, entry in entries.items() if entry["kind"] == "blocked_disposition"),
                     "scope_audits_not_counted_as_implementation": sorted(task_id for task_id, entry in entries.items() if entry["kind"] == "scope_audit")},
        "engineering": {"status": "not_accepted", "unfinished_hard_dependencies": hard_open,
                        "missing_current_submissions": missing, "mixed_source_submissions": drifted,
                        "missing_unfinished_current_submissions": sorted(set(missing) & set(hard_open)),
                        "accepted_tasks_without_current_submission": sorted(set(missing) - set(hard_open)),
                        "wiring_rows_requiring_review": [number for number, item in sorted(wiring.items()) if item["status"] != "implemented"],
                        "review": "required; artifact integrity and task statuses cannot certify V15-V19, default package behavior or release scope"},
        "live": {"status": "blocked", "decision": decision_ref, "decision_id": "D1+D2",
                 "provider_calls": "not_run", "quality_delta": None, "monetary_cost": None,
                 "scope": "live provider leg only; offline V18 and other engineering evidence remain separately assessable",
                 "validations": [{"id": value, "status": "blocked", "run_id": None} for value in ["V15", "V18", "V19", "V20"]]},
        "task_submissions": list(entries.values()), "wiring": [wiring[number] for number in sorted(wiring)],
        "release_scope": {"M3_engineering": "no new approval from this dossier; assess current engineering evidence",
                          "M4_semantic": "no live semantic benefit certified; live authorization and evidence remain absent"},
        "limits": ["No task status, acceptance, gate, provider configuration or live manifest is written by this command.",
                   "Per-task implementation and current validation counts are explicit submitted work, not new test counts or automatic task completion.",
                   "Equal source bytes and artifact digests are provenance checks, not proof that a compiler or test actually used them.",
                   "A successful command exit means the dossier was generated; engineering remains not_accepted and live remains blocked."],
    }


def markdown(report):
    rows = ["# P7 / G7 当前复核材料", "", f"当前未完成 **{report['unfinished_count']} 项**；状态分布：`{json.dumps(report['counts'], ensure_ascii=False)}`。", "",
            "工程 Gate：**未验收**。本命令只核对固定任务条件、证据字节和引用，不能凭状态或 hash 批准功能/发行。", "",
            "live：**blocked**，沿用 D1+D2；未调用真实 provider、没有真实效果或金额证据。", "",
            "| 原任务 | 当前状态 | 本轮交付类型 | 观察范围 |", "|---|---|---|---|"]
    def cell(text): return str(text).replace("|", "\\|").replace("\n", " ")
    for entry in report["task_submissions"]:
        rows.append("| " + " | ".join(cell(entry[key]) for key in ("task_id", "task_status", "kind", "scope")) + " |")
    rows += ["", "## 尚需闭合", "",
             "硬依赖：" + (", ".join(report["engineering"]["unfinished_hard_dependencies"]) or "任务状态均已完成，仍需Gate独立验收") + "。",
             "尚未验收且未提交本轮证据：" + (", ".join(report["engineering"]["missing_unfinished_current_submissions"]) or "无") + "。",
             "来源与当前整合输入不同的记录：" + (", ".join(report["engineering"]["mixed_source_submissions"]) or "无") + "；它们仍保持各自原源码的观察范围。", "",
             "已验收、未重复提交本轮证据：" + (", ".join(report["engineering"]["accepted_tasks_without_current_submission"]) or "无") + "。缺少本轮条目不会重开这些任务；原验收记录继续保留。", "",
             "## 13 项接线对账", "", "| 项 | 负责任务 | 当前判断 | 依据与剩余事项 |", "|---:|---|---|---|"]
    for row in report["wiring"]:
        rows.append("| " + " | ".join(cell(row[key]) for key in ("id", "task_id", "status", "note")) + " |")
    rows += ["", "每项原始证据的路径、SHA-256、固定源码、差异路径和限制见同目录 report.json。", "",
             "生成成功不等于 G7 通过；本报告没有更新任务状态，也没有将 fake 观察转成真实语义收益。", ""]
    return "\n".join(rows)


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument("--evidence-index", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    report = collect(args.root, args.evidence_index)
    args.output.mkdir(parents=True, exist_ok=False)
    (args.output / "report.json").write_bytes(json_bytes(report))
    (args.output / "README.md").write_text(markdown(report), encoding="utf-8")
    print(json.dumps({"dossier_written": True, "engineering_status": report["engineering"]["status"],
                      "live_status": report["live"]["status"], "unfinished_count": report["unfinished_count"]}))


if __name__ == "__main__":
    try:
        main()
    except (ValueError, OSError, subprocess.SubprocessError, KeyError, TypeError) as error:
        print("G7 dossier rejected: " + str(error), file=sys.stderr)
        sys.exit(2)
