#!/usr/bin/env python3
"""Preserve fixed accepted raw bytes and generate the original task progress views."""
import argparse
from collections import Counter
import hashlib
import json
from pathlib import Path, PurePosixPath
import shutil
import stat
import subprocess
import sys
import zipfile

BASE = "artifacts/checkpoints/p7-final-acceptance-20261008"
SOURCE = "ffdc6f0f97db78cc25a6c026904e7c2adde05d14"
ORIGINAL_TASKS_SHA = "a7ebacf3903f3f405e3c5ba4d19f5590107467b55ede35c4d675516c07cf4e68"
FINAL_TASKS_SHA = "b5a33e7827a6a1b60b71194348d61eb629385c48c1b970f434f330b2244a0812"
PROPOSAL_SHA = "22d85d4f9a6a40ebae76946788ff510fcd8108a521e88de283a46a24b1b6622d"
TARGETS = ["P7-014", "P7-015", "P7-016", "P7-017", "P7-019", "P7-020", "P8-001"]

def digest(raw):
    return hashlib.sha256(raw).hexdigest()

def encoded(value):
    return (json.dumps(value, ensure_ascii=False, indent=2) + "\n").encode("utf-8")

def safe_path(value):
    p = PurePosixPath(value)
    assert value and not p.is_absolute() and ".." not in p.parts and str(p) == value
    return p

def write(root, relative, raw):
    path = root.joinpath(*safe_path(relative).parts)
    path.parent.mkdir(parents=True, exist_ok=True)
    assert not path.is_symlink()
    path.write_bytes(raw)
    path.chmod(0o644)
    return path

def run(root, output, label, command):
    result = subprocess.run(command, cwd=root, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
    write(output, BASE + "/finalization/" + label + ".stdout.log", result.stdout)
    write(output, BASE + "/finalization/" + label + ".stderr.log", result.stderr)
    write(output, BASE + "/finalization/" + label + ".json", encoded({
        "command": command, "cwd": str(root), "exit_code": result.returncode,
        "stdout_sha256": digest(result.stdout), "stderr_sha256": digest(result.stderr)}))
    assert result.returncode == 0, label + " failed; raw output retained"
    print("P7_FINALIZATION_STEP " + json.dumps({"label": label, "exit_code": result.returncode}), flush=True)
    return result

def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--audit-root", type=Path, required=True)
    parser.add_argument("--candidate", type=Path, required=True)
    parser.add_argument("--downloads", type=Path, required=True)
    parser.add_argument("--output", type=Path, required=True)
    args = parser.parse_args()
    assert sys.flags.optimize == 0
    audit = args.audit_root.resolve(strict=True)
    root = args.candidate.resolve(strict=True)
    output = args.output.resolve()
    output.mkdir(parents=True, exist_ok=False)
    assert subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=root).decode().strip() == SOURCE
    assert not subprocess.check_output(["git", "status", "--porcelain"], cwd=root)
    sys.path.insert(0, str(root / "scripts"))
    from p7_build_identity import source_snapshot

    before_source = source_snapshot(root)
    assert before_source["input_count"] == 795
    assert before_source["manifest_sha256"] == "5b0f2f129500327a23a44a10b8f4530db6260273481698c9bb5c7cbe65fbc114"
    tracked = subprocess.check_output(["git", "ls-files", "-z", "--", "scripts", ".github/workflows", "tests/source_integrity"], cwd=root)
    frozen_validation = {p.decode(): digest((root / p.decode()).read_bytes()) for p in tracked.split(b"\0") if p}
    task_path = "docs/roadmap/code-index-v2/tasks.json"
    original_raw = (root / task_path).read_bytes()
    assert digest(original_raw) == ORIGINAL_TASKS_SHA
    original = json.loads(original_raw)
    proposal_raw = (audit / BASE / "acceptance-proposal.json").read_bytes()
    assert digest(proposal_raw) == PROPOSAL_SHA
    proposal = json.loads(proposal_raw)
    proposed_raw = (audit / BASE / "tasks-proposed.json").read_bytes()
    assert digest(proposed_raw) == FINAL_TASKS_SHA
    proposed = json.loads(proposed_raw)
    assert proposal["transition"]["proposed_tasks_sha256"] == FINAL_TASKS_SHA
    assert original["task_count"] == proposed["task_count"] == 192
    assert [t["id"] for t in original["tasks"]] == [t["id"] for t in proposed["tasks"]]
    changed = []
    for old, new in zip(original["tasks"], proposed["tasks"]):
        assert set(old) == set(new)
        allowed = {"status", "evidence", "implementation_notes"} if old["id"] in TARGETS else set()
        for key in old:
            if key not in allowed:
                assert old[key] == new[key], old["id"] + "/" + key + " changed"
        if old["id"] in TARGETS:
            assert old["status"] != "done" and new["status"] == "done"
            assert new["evidence"][:-1] == old["evidence"]
            assert new["implementation_notes"].startswith(old.get("implementation_notes", "") + "\n")
            changed.append(old["id"])
    assert changed == TARGETS
    for key in original:
        if key not in {"tasks", "current_phase", "next_task"}:
            assert original[key] == proposed[key], "top-level contract drift: " + key
    by_id = {t["id"]: t for t in proposed["tasks"]}
    for t in proposed["tasks"]:
        if t["status"] == "done":
            assert t["evidence"] and all(by_id[d]["status"] == "done" for d in t["depends_on"])
    assert Counter(t["status"] for t in proposed["tasks"]) == {"done":160, "in_progress":19, "todo":12, "blocked":1}
    assert by_id["P7-018"] == next(t for t in original["tasks"] if t["id"] == "P7-018")

    spec_raw = (audit / BASE / "ci-audit/raw-preservation-spec.json").read_bytes()
    assert digest(spec_raw) == "5d064e780286f42361e7bca4ba5e508467f3c536e82262abcab42359ae6dcfe7"
    spec = json.loads(spec_raw)
    assert spec["file_count"] == len(spec["files"]) == 265
    assert spec["total_bytes"] == 13107978
    archives = {a["kind"]: a for a in spec["archives"]}
    copied = []
    for kind, meta in archives.items():
        archive_path = args.downloads / meta["file_name"]
        assert archive_path.stat().st_size == meta["bytes"]
        assert digest(archive_path.read_bytes()) == meta["sha256"]
        with zipfile.ZipFile(archive_path) as z:
            assert len(z.namelist()) == len(set(z.namelist()))
            for item in (f for f in spec["files"] if f["kind"] == kind):
                safe_path(item["member"])
                info = z.getinfo(item["member"])
                mode = info.external_attr >> 16
                assert not info.is_dir() and not stat.S_ISLNK(mode)
                raw = z.read(info)
                assert len(raw) == item["bytes"] and digest(raw) == item["sha256"]
                assert raw.decode("utf-8").encode("utf-8") == raw
                write(output, item["path"], raw)
                copied.append(item)
    assert len(copied) == 265 and sum(x["bytes"] for x in copied) == 13107978
    preservation = {"schema_version":1, "status":"passed", "source_commit":SOURCE,
                    "spec_sha256":digest(spec_raw), "files":copied,
                    "file_count":len(copied), "total_bytes":sum(x["bytes"] for x in copied),
                    "scope":spec["scope"], "exclusions":spec["exclusions"]}
    write(output, BASE + "/raw-preservation-receipt.json", encoded(preservation))

    assert not (root / ".github/workflows/p7-fixed-artifact-audit.yml").exists()
    shutil.copytree(audit / BASE, root / BASE, dirs_exist_ok=True)
    shutil.copytree(output / "artifacts", root / "artifacts", dirs_exist_ok=True)
    acceptance = json.loads((root / BASE / "acceptance.json").read_bytes())
    assert acceptance["decision"] == "accepted_original_seven_task_scope"
    assert acceptance["proposal_sha256"] == PROPOSAL_SHA
    assert acceptance["task_after_sha256"] == FINAL_TASKS_SHA
    manual = acceptance["non_author_g7_review"]
    manual_path = root.joinpath(*safe_path(manual["path"]).parts)
    assert digest(manual_path.read_bytes()) == manual["sha256"]
    assert acceptance["source_commit"] == SOURCE and acceptance["newly_completed"] == TARGETS
    write(root, task_path, proposed_raw)

    gate_json_path = "docs/roadmap/code-index-v2/P7-REMAINING-GATES.json"
    old_gate = json.loads((root / gate_json_path).read_bytes())
    gate = json.loads(json.dumps(old_gate))
    gate["current_G7_acceptance_20261008"] = {
        "source_commit": SOURCE,
        "status": "accepted_original_engineering_and_fake_scope",
        "acceptance": BASE + "/acceptance.json",
        "non_author_review": manual,
        "tasks_newly_completed": TARGETS,
        "original_task_count":192, "done":160, "unfinished":32,
        "live":"blocked; original D1+D2 unchanged",
        "full_V19":"open; existing corpus/custody/facet/report/quality limitations retained",
        "G8":"not_accepted; P8-001 is a fixed DEV input-lock execution, not release quality",
        "historical_fields":"All previously stored gate rows and scoped observations remain exact historical records."}
    for key in old_gate:
        assert gate[key] == old_gate[key]
    write(root, gate_json_path, encoded(gate))
    gate_md_path = "docs/roadmap/code-index-v2/P7-REMAINING-GATES.md"
    old_md = (root / gate_md_path).read_text(encoding="utf-8")
    current_md = """# P7 current engineering acceptance and remaining gates

## 2026-10-08 current decision

Original tasks P7-014, P7-015, P7-016, P7-017, P7-019, P7-020 and P8-001 are accepted in dependency order at fixed source ffdc6f0f97db78cc25a6c026904e7c2adde05d14: **160 done / 32 unfinished out of 192**.

The current G7 decision accepts the original engineering/fake scope. P7-018 remains **blocked** under the unchanged D1+D2 decision. Full V19 corpus/custody/quality certification and G8 release acceptance remain open. P8-001 freezes and actually runs a default DEV candidate with the original scorer; its original gate remains baseline_recorded_not_quality_certified.

Acceptance and exact current evidence: [manual acceptance](../../../""" + BASE + """/acceptance.json), [non-author G7 review](../../../""" + BASE + """/g7-non-author-review.json), [13-row reconciliation](../../../""" + BASE + """/wiring-reconciliation.json), and [integrity-only dossier](../../../""" + BASE + """/generated-final/report.json). The unchanged dossier generator deliberately reports not_accepted; the separate non-author reviewer record carries the scoped G7 decision.

Offline evidence is **four full 14-tool matrices total plus four separate reopens**, across two packages and two configurations per package. There are eight product roots, zero external-network attempts and eight separately reported anonymous local IPC operations.

Rows7 and13 retain their original conditional dispositions: persistent historical GC totals and switch-log truncation have not been implemented and are not claimed. Row9 is now resolved by actual target-space bounded reclaim in three production entry paths, with the original tests and independent source review retained.

## Historical gate records and original limitations

The following records describe their own frozen historical sources. Their earlier open/prepared statements are retained for audit; the current decision above supersedes only the seven listed task states and scoped G7 engineering decision. It does not close their separately identified full-quality, live or release limitations.

"""
    write(root, gate_md_path, (current_md + old_md).encode("utf-8"))
    run(root, output, "plan-write", [sys.executable, "scripts/code_index_plan.py", "--write"])
    run(root, output, "plan-check", [sys.executable, "scripts/code_index_plan.py"])
    run(root, output, "p8-facts-check", [sys.executable, "scripts/p8_facts.py", "--check"])
    dossier_dir = root / BASE / "generated-final"
    assert not dossier_dir.exists()
    run(root, output, "g7-original-dossier", [sys.executable, "scripts/p7_gate_report.py",
        "--evidence-index", BASE + "/evidence-index.json", "--output", BASE + "/generated-final"])
    dossier = json.loads((dossier_dir / "report.json").read_bytes())
    assert dossier["engineering"]["status"] == "not_accepted"
    assert dossier["engineering"]["unfinished_hard_dependencies"] == []
    assert dossier["engineering"]["wiring_rows_requiring_review"] == [7,13]
    assert dossier["live"]["status"] == "blocked" and dossier["unfinished_count"] == 32
    assert dossier["tasks_sha256"] == FINAL_TASKS_SHA
    assert dossier["engineering"]["mixed_source_submissions"] == []
    after_source = source_snapshot(root)
    assert before_source == after_source
    for p, expected in frozen_validation.items():
        assert digest((root / p).read_bytes()) == expected, "validation input changed: " + p
    assert not (root / ".github/workflows/p7-fixed-artifact-audit.yml").exists()

    views = [task_path, "README.md", "docs/roadmap/code-index-v2/README.md",
             "docs/roadmap/code-index-v2/05-TODO.md", "docs/roadmap/code-index-v2/08-HANDOFF.md",
             gate_json_path, gate_md_path, BASE + "/generated-final/report.json",
             BASE + "/generated-final/README.md"]
    for path in views:
        write(output, path, (root / path).read_bytes())
    receipt = {"schema_version":1, "status":"passed", "source_commit":SOURCE,
               "before_tasks_sha256":ORIGINAL_TASKS_SHA, "after_tasks_sha256":FINAL_TASKS_SHA,
               "newly_completed":TARGETS, "task_count":192,
               "before_counts":dict(Counter(t["status"] for t in original["tasks"])),
               "after_counts":dict(Counter(t["status"] for t in proposed["tasks"])),
               "unfinished_before":39, "unfinished_after":32,
               "original_task_fields_unchanged_except_status_append_only_evidence_notes":True,
               "all_hard_dependencies_satisfied":True, "P7_018_unchanged":True,
               "source_snapshot_unchanged":before_source == after_source,
               "complete_source_inputs":795, "validation_files_unchanged":len(frozen_validation),
               "utility_workflow_absent_from_candidate":True,
               "reporter_unchanged":True, "dossier_engineering_status":"not_accepted",
               "separate_manual_g7_acceptance":manual,
               "view_files":[{"path":p,"sha256":digest((root / p).read_bytes())} for p in views],
               "working_tree_status":subprocess.check_output(["git","status","--porcelain"],cwd=root).decode(),
               "scope":"Apply previously reviewed task decisions and generate original views; no product rerun or automated acceptance."}
    write(output, BASE + "/task-transition-receipt.json", encoded(receipt))
    print("P7_FINALIZATION_RESULT " + json.dumps({k:v for k,v in receipt.items() if k not in {"working_tree_status","view_files"}}, ensure_ascii=True), flush=True)

if __name__ == "__main__":
    main()
