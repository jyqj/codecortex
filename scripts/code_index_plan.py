#!/usr/bin/env python3
"""Validate tasks.json and its TODO and entry-point progress views (stdlib only)."""
import argparse
import collections
import hashlib
import json
import re
from datetime import date
from pathlib import Path

STATUSES = {"todo", "in_progress", "blocked", "done", "deferred"}
PROGRESS_START = "<!-- code-index-progress:start -->"
PROGRESS_END = "<!-- code-index-progress:end -->"


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
    validate_navigation(data, by_id)
    return dict(collections.Counter(t["status"] for t in tasks))


def validate_navigation(data, by_id):
    """Check the chosen navigation without selecting or advancing any task."""
    if not isinstance(data.get("status"), str) or data["status"] not in STATUSES:
        raise ValueError("Unknown plan status")
    if data["status"] == "done":
        if any(task["status"] not in {"done", "deferred"} for task in by_id.values()):
            raise ValueError("Done plan has unfinished tasks")
        for field in ("current_phase", "next_task"):
            if field not in data or data[field] is not None:
                raise ValueError("Done plan requires explicit null " + field)
    else:
        phase = data.get("current_phase")
        if phase not in data["phase_order"]:
            raise ValueError("Unknown current_phase")
        next_id = data.get("next_task")
        next_task = by_id.get(next_id) if isinstance(next_id, str) else None
        if next_task is None:
            raise ValueError("Unknown next_task")
        if next_task["status"] in {"done", "deferred"}:
            raise ValueError("next_task must not be done or deferred")
        if next_task["phase"] != phase:
            raise ValueError("next_task does not match current_phase")
        if any(by_id[dep]["status"] != "done" for dep in next_task["depends_on"]):
            raise ValueError("next_task has unfinished hard dependency")
    recorded_date = data.get("last_implementation_date")
    try:
        canonical_date = date.fromisoformat(recorded_date).isoformat()
    except (TypeError, ValueError) as error:
        raise ValueError("Invalid last_implementation_date; expected YYYY-MM-DD") from error
    if canonical_date != recorded_date:
        raise ValueError("Invalid last_implementation_date; expected YYYY-MM-DD")


def render_progress(data, source_hash, link_prefix):
    tasks = data["tasks"]
    by_id = {task["id"]: task for task in tasks}
    counts = collections.Counter(task["status"] for task in tasks)
    states = ["{} {}".format(counts[state], state)
              for state in ("done", "in_progress", "todo", "blocked", "deferred")
              if counts[state] or state in {"done", "in_progress", "todo"}]
    lines = [
        "Code Index V2 共 **{} 项任务：{}**。".format(len(tasks), " / ".join(states)),
        "",
    ]
    if data["status"] == "done":
        lines += [
            "计划状态：`done`（本轮计划已结束）；更新日期：`{}`。".format(data["last_implementation_date"]),
            "所有任务均已完成或明确延期；当前阶段：无；下一任务：无。",
        ]
    else:
        phase = data["current_phase"]
        next_task = by_id[data["next_task"]]
        # Include unfinished prerequisites of the selected next task and active work.
        # This is a dependency view, not an automatic choice of execution order.
        focus = {next_task["id"]}

        def include(task):
            for dependency in task["depends_on"]:
                if by_id[dependency]["status"] != "done" and dependency not in focus:
                    focus.add(dependency)
                    include(by_id[dependency])

        for task in tasks:
            if task["status"] == "in_progress":
                focus.add(task["id"])
                include(task)
        lines += [
            "当前阶段：**{}｜{}**；计划状态：`{}`；更新日期：`{}`。".format(
                phase, data["phase_names"][phase], data["status"], data["last_implementation_date"]),
            "下一任务：**{}｜{}**（硬依赖已完成）。".format(next_task["id"], next_task["title"]),
            "",
            "| 当前下一项、进行中任务及其未完成前置 | 状态 | 硬依赖（任务状态） |",
            "|---|---|---|",
        ]
        for task in tasks:
            if task["id"] not in focus:
                continue
            dependencies = "、".join("{} ({})".format(dep, by_id[dep]["status"])
                                     for dep in task["depends_on"]) or "无"
            lines.append("| {}｜{} | `{}` | {} |".format(
                task["id"], task["title"], task["status"], dependencies))
    lines += [
        "",
        "进度入口：[重构总览]({0}README.md) · [逐项 TODO]({0}05-TODO.md) · "
        "[唯一任务状态源]({0}tasks.json) · [执行交接]({0}08-HANDOFF.md)。".format(link_prefix),
        "任务完成数不等同发布认证；以各任务证据和适用验证范围为准。",
        "",
        "> 本块由 `scripts/code_index_plan.py --write` 从 `tasks.json` 生成；无参运行校验全部进度入口。",
        "> 源文件 SHA-256：`{}`。".format(source_hash),
    ]
    return "\n".join(lines) + "\n"


def replace_progress(text, progress, path):
    for marker in (PROGRESS_START, PROGRESS_END):
        if text.count(marker) != 1 or not re.search(r"(?m)^" + re.escape(marker) + r"\r?$", text):
            raise ValueError("Progress markers missing, duplicated or not standalone: " + str(path))
    start = text.index(PROGRESS_START)
    end = text.index(PROGRESS_END)
    if start >= end:
        raise ValueError("Progress markers out of order: " + str(path))
    return text[:start] + PROGRESS_START + "\n" + progress + PROGRESS_END + text[end + len(PROGRESS_END):]


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
    targets = {args.root / "05-TODO.md": expected}
    # --root remains the roadmap directory; resolve all views within that repo.
    repo = args.root.resolve().parents[2]
    entries = [(repo / "README.md", "docs/roadmap/code-index-v2/"),
               (args.root / "README.md", ""), (args.root / "08-HANDOFF.md", "")]
    for target, prefix in entries:
        current = target.read_bytes().decode("utf-8")
        targets[target] = replace_progress(current, render_progress(data, digest, prefix), target)
    # Validate every marker before writing any view, so malformed entry points
    # cannot leave the TODO and the other generated blocks partially updated.
    for target, expected in targets.items():
        if args.write:
            target.write_text(expected, encoding="utf-8")
        elif target.read_bytes().decode("utf-8") != expected:
            raise ValueError("Plan view drift in {}; regenerate with --write".format(target))
    print(json.dumps({"status": "passed", "task_count": len(data["tasks"]), "states": statuses,
                      "task_sha256": digest, "views": len(targets)}, ensure_ascii=False))


if __name__ == "__main__":
    main()
