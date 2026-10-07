#!/usr/bin/env python3
"""Prepare a fixed G8 evidence-gap ledger; never grant release acceptance.

Task state and repository receipts are declarations, not approval authority.
This offline tool preserves the original 192 definitions, evaluates local and
semantic dependency scopes separately, and retains every missing/partial item.
"""
import argparse
from collections import Counter
import json
import math
import os
from pathlib import Path
import re
import subprocess
import sys

import p8_judge_evidence as common
import p8_release_evidence as support

ROOT = Path(__file__).resolve().parents[1]
DEFINITIONS = ROOT / "artifacts/checkpoints/p8-next-ten-20261008/release-review/original-definitions.json"
DEFINITIONS_SHA256 = "ff2dff22f5064dca04254e8191fdf93a52155acbbaf2c52d03ff147b519690aa"
TASKS = "docs/roadmap/code-index-v2/tasks.json"
MUTABLE = {"status", "evidence", "implementation_notes"}
STATUSES = {"todo", "in_progress", "blocked", "done", "deferred"}
PROFILES = {"local", "semantic"}
CONDITIONS = {"live semantic-effect certification; not required for engineering/fake profile",
              "M4-semantic release; not required for M4-local"}
MAX_RECORD_BYTES = 16 * 1024 * 1024
INPUT_DIGEST_FORMAT = "sha256_sorted_compact_path_sha256_json_no_newline_v1"
require = common.require
fields = common.fields
text = common.text


def commit(value):
    require(isinstance(value, str) and re.fullmatch(r"[0-9a-f]{40}", value),
            "immutable full commit required")
    return value


def parse_object(raw):
    require(len(raw) <= common.MAX_INPUT, "JSON record size limit")
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate JSON key")
            result[key] = value
        return result
    def number(value):
        result = float(value)
        require(math.isfinite(result), "nonfinite JSON number")
        return result
    try:
        result = json.loads(raw, object_pairs_hook=unique, parse_float=number,
            parse_constant=lambda _: (_ for _ in ()).throw(support.Invalid("nonfinite JSON")))
    except (ValueError, UnicodeError, RecursionError) as exc:
        raise support.Invalid("invalid JSON object") from exc
    require(isinstance(result, dict), "JSON object required")
    return result


def git_blob(repo, ref, name):
    """Read one bounded local regular Git blob; all transport protocols disabled."""
    commit(ref)
    name = support.name(name)
    text(name, "Git record path", 1024)
    env = {**os.environ, "GIT_TERMINAL_PROMPT": "0", "GIT_NO_LAZY_FETCH": "1",
           "GIT_LITERAL_PATHSPECS": "1"}
    def run(*args):
        try:
            result = subprocess.run(["git", "-c", "protocol.allow=never", "-C", str(repo), *args],
                capture_output=True, env=env, timeout=20)
        except (OSError, subprocess.TimeoutExpired) as exc:
            raise support.Invalid("local Git read failed") from exc
        require(result.returncode == 0, "local Git object unavailable: " + name)
        return result.stdout
    row = run("ls-tree", "-z", ref, "--", name)
    parts = row.rstrip(b"\0").split(b"\t")
    require(len(parts) == 2 and parts[1].decode() == name, "exact Git file required")
    header = parts[0].split()
    require(len(header) == 3 and header[0] in {b"100644", b"100755"} and header[1] == b"blob",
            "regular Git blob required; symlinks and trees are not evidence")
    size = int(run("cat-file", "-s", header[2].decode()).strip())
    require(0 <= size <= common.MAX_INPUT, "Git blob size limit")
    raw = run("cat-file", "blob", header[2].decode())
    require(len(raw) == size, "Git blob size mismatch")
    return raw


def load_definitions(path=DEFINITIONS):
    raw = support.read_bytes(path, limit=common.MAX_INPUT)
    require(common.sha(raw) == DEFINITIONS_SHA256, "original task/G8 definition snapshot changed")
    return parse_object(raw)


def task_index(definitions, state):
    require(type(state.get("schema_version")) is int and state["schema_version"] == 1
            and type(state.get("task_count")) is int and state["task_count"] == 192
            and isinstance(state.get("tasks"), list) and len(state["tasks"]) == 192,
            "original 192 task inventory required")
    original = {task["id"]: task for task in definitions["tasks"]}
    current = {}
    for task in state["tasks"]:
        require(isinstance(task, dict), "task object required")
        tid = text(task.get("id"), "task ID", 16)
        require(tid in original and tid not in current, "unknown or duplicate original task ID")
        require({key: value for key, value in task.items() if key not in MUTABLE} == original[tid],
                "original task definition changed: " + tid)
        require(isinstance(task.get("status"), str) and task["status"] in STATUSES,
                "unknown task status")
        require(isinstance(task.get("evidence", []), list), "task evidence list required")
        current[tid] = task
    require(set(current) == set(original), "original task inventory differs")
    require(state.get("phase_task_counts") == definitions["phase_task_counts"], "phase task counts differ")
    return original, current


def dependencies(original, tid, profile):
    task = original[tid]
    selected = list(task["depends_on"])
    for condition in task.get("conditional_dependencies", []):
        require(condition["when"] in CONDITIONS, "unknown original conditional scope")
        if profile == "semantic":
            selected.append(condition["task"])
    require(len(selected) == len(set(selected)) and set(selected) <= set(original),
            "duplicate or unknown dependency")
    return selected


def closure(original, profile):
    visiting, visited = set(), set()
    def walk(tid):
        require(tid not in visiting, "dependency cycle")
        if tid in visited:
            return
        visiting.add(tid)
        for dep in dependencies(original, tid, profile):
            walk(dep)
        visiting.remove(tid)
        visited.add(tid)
    walk("P8-020")
    visited.remove("P8-020")
    return visited


def pointer(value, path):
    if path is None:
        return None
    for key in path:
        if not isinstance(value, dict) or key not in value:
            return None
        value = value[key]
    return value if isinstance(value, str) and len(value.encode()) <= 256 else None


def validate_manifest(manifest, original, coverage):
    fields(manifest, {"schema_version", "run_id", "state_commit", "candidate", "records"})
    require(type(manifest["schema_version"]) is int and manifest["schema_version"] == 1,
            "unsupported release-review schema")
    text(manifest["run_id"], "run ID", 128)
    commit(manifest["state_commit"])
    fields(manifest["candidate"], {"source_commit", "input_digest", "input_digest_format"})
    commit(manifest["candidate"]["source_commit"])
    common.hash_value(manifest["candidate"]["input_digest"])
    require(manifest["candidate"]["input_digest_format"] == INPUT_DIGEST_FORMAT,
            "unsupported candidate input digest format")
    require(isinstance(manifest["records"], list) and len(manifest["records"]) <= 128,
            "bounded evidence records required")
    seen = set()
    for record in manifest["records"]:
        fields(record, {"id", "task", "profile", "scope", "coverage", "record",
                        "status_path", "source_path", "input_digest_path", "input_digest_format"})
        rid = text(record["id"], "record ID", 128)
        require(rid not in seen, "duplicate record ID")
        seen.add(rid)
        tid = text(record["task"], "record task", 16)
        require(tid in original, "unknown record task")
        require(isinstance(record["profile"], str) and record["profile"] in PROFILES | {"both"},
                "unknown evidence profile")
        require(isinstance(record["scope"], str)
                and record["scope"] in {"scoped_engineering", "release_validation"}, "unknown evidence scope")
        labels = record["coverage"]
        require(isinstance(labels, list) and all(isinstance(label, str) and label in coverage for label in labels)
                and len(labels) == len(set(labels)), "unknown or duplicate coverage declaration")
        ref = record["record"]
        fields(ref, {"commit", "path", "sha256"})
        commit(ref["commit"])
        support.name(ref["path"])
        common.hash_value(ref["sha256"])
        require(record["input_digest_format"] is None or isinstance(record["input_digest_format"], str)
                and record["input_digest_format"] in {INPUT_DIGEST_FORMAT, "opaque_record_sha256"},
                "unknown record input digest format")
        for key in ["status_path", "source_path", "input_digest_path"]:
            path = record[key]
            require(path is None or isinstance(path, list) and 1 <= len(path) <= 8
                    and all(isinstance(item, str) and item and len(item) <= 128 for item in path),
                    "bounded object-key evidence path required")


def assess(definitions, state, manifest, raw_records):
    original, current = task_index(definitions, state)
    gate = definitions["g8"]
    coverage = set(gate["validations"] + gate["release_checks"])
    validate_manifest(manifest, original, coverage)
    require(set(raw_records) == {record["id"] for record in manifest["records"]},
            "raw evidence inventory differs")
    require(sum(len(raw) for raw in raw_records.values()) <= MAX_RECORD_BYTES, "total evidence byte limit")
    records = []
    for record in manifest["records"]:
        raw = raw_records[record["id"]]
        require(common.sha(raw) == record["record"]["sha256"], "record digest mismatch: " + record["id"])
        value = parse_object(raw)
        status = pointer(value, record["status_path"])
        source = pointer(value, record["source_path"])
        inputs = pointer(value, record["input_digest_path"])
        gaps = []
        if status != "passed":
            gaps.append("status_not_full_release_pass")
        if source is None:
            gaps.append("candidate_source_unavailable")
        elif source != manifest["candidate"]["source_commit"]:
            gaps.append("candidate_source_different")
        if inputs is None:
            gaps.append("candidate_input_digest_unavailable")
        elif record["input_digest_format"] != INPUT_DIGEST_FORMAT:
            gaps.append("candidate_input_digest_not_comparable")
        elif inputs != manifest["candidate"]["input_digest"]:
            gaps.append("candidate_input_digest_different")
        if record["scope"] != "release_validation":
            gaps.append("scoped_engineering_only")
        records.append({**record, "observed_status": status, "observed_source": source,
            "observed_input_digest": inputs, "bytes": len(raw), "gaps": gaps,
            "declared_metadata_consistent": not gaps, "authenticity": "not_verified",
            "release_effect": "none"})
    profiles = {}
    for profile in sorted(PROFILES):
        direct = dependencies(original, "P8-020", profile)
        transitive = closure(original, profile)
        unfinished = [{"task": tid, "declared_status": current[tid]["status"]}
                      for tid in sorted(transitive) if current[tid]["status"] != "done"]
        selected = [row for row in records if row["profile"] in {profile, "both"}
                    and row["task"] in transitive | {"P8-020"}]
        consistent = [row for row in selected if row["declared_metadata_consistent"]]
        declared_coverage = {label for row in consistent for label in row["coverage"]}
        missing_tasks = [tid for tid in direct if not any(row["task"] == tid for row in consistent)]
        gaps = sorted(coverage - declared_coverage)
        external = ["independent_release_acceptance_not_verified", "current_metrics_and_raw_replay_not_executed",
                    "candidate_source_integrity_not_executed_by_this_tool"]
        if profile == "semantic":
            external.append("live_provider_authority_clean_custody_and_budget_not_verified")
        profiles[profile] = {"status": "blocked" if unfinished or missing_tasks or gaps else "not_accepted",
            "release_certified": False, "direct_dependencies": direct,
            "transitive_dependency_count": len(transitive), "unfinished_dependencies": unfinished,
            "direct_tasks_without_consistent_release_metadata": missing_tasks,
            "declared_coverage_gaps": gaps, "coverage_authenticity": "not_verified",
            "records_selected": [row["id"] for row in selected], "external_blockers": external}
    counts = Counter(task["status"] for task in current.values())
    return {"schema_version": 1, "kind": "p8_g8_release_review_preparation", "run_id": manifest["run_id"],
        "status": "not_accepted", "exit_code": 1, "release_certified": False, "todo_completed": False,
        "definitions_source": definitions["source_commit"], "definitions_sha256": DEFINITIONS_SHA256,
        "state_commit": manifest["state_commit"], "candidate_declaration": manifest["candidate"],
        "candidate_source_verification": "not_run_by_this_tool", "task_state_authority": "repository_declarations_only",
        "task_counts": {status: counts[status] for status in sorted(STATUSES)},
        "original_task_count": len(current), "remaining_not_done": len(current) - counts["done"],
        "g8_original": gate, "p8_020_original": original["P8-020"], "profiles": profiles, "records": records,
        "optional_judge": {"task": "P8-014", "declared_status": current["P8-014"]["status"],
                           "blocks_local_or_semantic": False, "execution": "not_verified"},
        "p9_blocks_g8": False, "model_calls": 0, "network_calls": 0, "input_mutations": 0}


def _review(repo, manifest_path, output):
    manifest, manifest_raw = common.read_object(manifest_path)
    definitions = load_definitions()
    original = {task["id"]: task for task in definitions["tasks"]}
    gate = definitions["g8"]
    validate_manifest(manifest, original, set(gate["validations"] + gate["release_checks"]))
    state_raw = git_blob(repo, manifest["state_commit"], TASKS)
    state = parse_object(state_raw)
    metadata = output / "metadata"
    metadata.mkdir(mode=0o700)
    common.private_write(metadata / "manifest.json", manifest_raw)
    raw_records, total = {}, 0
    for index, record in enumerate(manifest["records"]):
        raw = git_blob(repo, record["record"]["commit"], record["record"]["path"])
        total += len(raw)
        require(total <= MAX_RECORD_BYTES, "total evidence byte limit")
        raw_records[record["id"]] = raw
        common.private_write(metadata / (f"record-{index:04d}.json"), raw)
    result = assess(definitions, state, manifest, raw_records)
    result["manifest_sha256"] = common.sha(manifest_raw)
    result["task_state_sha256"] = common.sha(state_raw)
    return result


def review(repo, manifest, output):
    return common.local_run(output, [manifest], lambda out: _review(support.path(repo), support.path(manifest), out))


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--repo", type=Path, default=ROOT)
    parser.add_argument("--manifest", required=True)
    parser.add_argument("--output", required=True)
    args = parser.parse_args(argv)
    try:
        result = review(args.repo, args.manifest, args.output)
    except (support.Invalid, OSError) as exc:
        result = {"status": "invalid_input", "exit_code": 2, "error": str(exc)[:2048], "release_certified": False}
    print(json.dumps(result, ensure_ascii=False, sort_keys=True, allow_nan=False))
    return result["exit_code"]


if __name__ == "__main__":
    sys.exit(main())
