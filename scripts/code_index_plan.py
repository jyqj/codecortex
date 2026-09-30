#!/usr/bin/env python3
"""Validate tasks.json and render its single-source Markdown view (stdlib only)."""
import argparse
import collections
import hashlib
import json
import re
from pathlib import Path

STATUSES = {"todo", "in_progress", "blocked", "done", "deferred"}


def validate(data, root):
    tasks = data["tasks"]
    by_id = {t["id"]: t for t in tasks}
    if len(by_id) != len(tasks):
        raise ValueError("Duplicate task IDs")
    allowed_validations = set(re.findall(r"\bV\d{2}\b", (root / "06-VALIDATION.md").read_text()))
    visiting, visited = set(), set()

    def visit(key):
        if key in visiting:
            raise ValueError("Dependency cycle at " + key)
        if key in visited:
            return
        visiting.add(key)
        task = by_id[key]
        dependencies = task["depends_on"] + [d["task"] for d in task.get("conditional_dependencies", [])]
        for dependency in dependencies:
            if dependency not in by_id:
                raise ValueError("Unknown dependency " + dependency)
            visit(dependency)
        visiting.remove(key)
        visited.add(key)

    for task in tasks:
        for field in ["id", "phase", "batch", "title", "scope", "steps", "acceptance", "validations", "rollback"]:
            if not task.get(field):
                raise ValueError("Missing " + field + " in " + task["id"])
        if task["status"] not in STATUSES:
            raise ValueError("Unknown status in " + task["id"])
        if not set(task["validations"]) <= allowed_validations:
            raise ValueError("Unknown validation in " + task["id"])
        if task["status"] == "done":
            if not task.get("evidence"):
                raise ValueError("Done task lacks evidence: " + task["id"])
            for dep in task["depends_on"]:
                if by_id[dep]["status"] != "done":
                    raise ValueError("Done task has unfinished hard dependency: " + task["id"])
        visit(task["id"])
    if data["task_count"] != len(tasks):
        raise ValueError("Task count drift")
    counts = collections.Counter(t["phase"] for t in tasks)
    if dict(counts) != data["phase_task_counts"]:
        raise ValueError("Phase counts drift")
    return dict(collections.Counter(t["status"] for t in tasks))


def render(data, source_hash):
    tasks = data["tasks"]
    lines = ["# 05｜逐项重构 TODO（由 tasks.json 派生）", "",
             "> 任务总数：{}；源文件 SHA-256：`{}`。".format(len(tasks), source_hash),
             "> 状态只改 tasks.json；使用 scripts/code_index_plan.py --write 生成本页。", "",
             "## 总览", "", "| Phase | 主题 | done / 总数 |", "|---|---|---:|"]
    for phase in data["phase_order"]:
        subset = [t for t in tasks if t["phase"] == phase]
        lines.append("| {} | {} | {} / {} |".format(phase, data["phase_names"][phase], sum(t["status"] == "done" for t in subset), len(subset)))
    for phase in data["phase_order"]:
        lines += ["", "## {}｜{}".format(phase, data["phase_names"][phase])]
        for t in [x for x in tasks if x["phase"] == phase]:
            checked = "x" if t["status"] == "done" else " "
            lines += ["", "### [{}] {}｜{}".format(checked, t["id"], t["title"]), "",
                      "状态：`{}`；批次：`{}`；优先级：`{}`。".format(t["status"], t["batch"], t.get("priority", "normal")),
                      "范围：" + "；".join("`" + p + "`" for p in t["scope"]),
                      "硬依赖：" + (", ".join(t["depends_on"]) or "无")]
            for key, title in [("steps", "步骤"), ("deliverables", "交付物"), ("acceptance", "验收"), ("validations", "验证")]:
                lines.append(title + "：" + "；".join(t.get(key, [])))
            lines.append("回滚：" + t["rollback"])
            if t.get("conditional"):
                lines.append("条件：" + t["conditional"])
            if t.get("conditional_dependencies"):
                lines.append("条件依赖：" + json.dumps(t["conditional_dependencies"], ensure_ascii=False))
            lines.append("证据：" + (json.dumps(t.get("evidence", []), ensure_ascii=False) if t.get("evidence") else "尚无"))
            if t.get("implementation_notes"):
                lines.append("实施备注：" + t["implementation_notes"])
    return "\n".join(lines) + "\n"


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--write", action="store_true")
    parser.add_argument("--root", type=Path, default=Path(__file__).resolve().parent.parent / "docs/roadmap/code-index-v2")
    args = parser.parse_args()
    source = args.root / "tasks.json"
    raw = source.read_bytes()
    data = json.loads(raw)
    statuses = validate(data, args.root)
    digest = hashlib.sha256(raw).hexdigest()
    expected = render(data, digest)
    target = args.root / "05-TODO.md"
    if args.write:
        target.write_text(expected, encoding="utf-8")
    elif target.read_text(encoding="utf-8") != expected:
        raise ValueError("TODO drift; regenerate with --write")
    print(json.dumps({"status": "passed", "task_count": len(data["tasks"]), "states": statuses, "task_sha256": digest}, ensure_ascii=False))


if __name__ == "__main__":
    main()
