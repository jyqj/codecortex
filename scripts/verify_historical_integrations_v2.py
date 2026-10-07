#!/usr/bin/env python3
"""Verify frozen integrations and unchanged task definitions without freezing progress."""
import argparse
import hashlib
import json
import os
from pathlib import Path, PurePosixPath
import re
import stat
import subprocess
import sys


ROOT = Path(__file__).resolve().parent.parent
VERSION = "historical-integrations-v2"
PACKING_SOURCE = "90858afae647a513537bf118932a7ba5020ee98b"
PACKING_INTEGRATION = "37dd042eaa1209a86e0cafdcd92ae77e036e76f5"
PACKING_WORKFLOW_SOURCE = "156e3ac13ddc6aa2b0208c8805c8c79bd2a0a38d"
E3_SOURCE = "e3c04fed903c4d0e3c26b5d7cf0e055a6f7c5207"
E3_INTEGRATION = "88f2cf099c8b81f3acef485fd5ac9b01c63ce790"
TASK_BASE = "6d02d77f018a5965a6f289b0b43558ed4b9f8322"
TASK_BASE_SHA256 = "cc453fe700a01dd756d35e543d2ceae1f0b748bf97e9542ba0519767f5359ccc"
TASK_PATH = "docs/roadmap/code-index-v2/tasks.json"
GATES_PATH = "docs/roadmap/code-index-v2/P7-REMAINING-GATES.json"
PACKING_REPORT = "docs/checkpoints/2026-10-03-packing-integration"
E3_REPORT = "docs/checkpoints/2026-10-03-fixed-e3-integration"
FORBIDDEN = "artifacts/checkpoints/localwidth4-100k-paired-20261003"
TASK_PROGRESS = frozenset({"status", "evidence", "implementation_notes"})
PLAN_PROGRESS = frozenset({"status", "current_phase", "next_task",
                           "last_implementation_date", "execution_note"})
STATUSES = frozenset({"todo", "in_progress", "blocked", "done", "deferred"})
LEGACY_FILES = (
    "scripts/current-source-registry-v1.json",
    "scripts/current-source-registry-v2.json",
    "scripts/current-source-registry-v3.json",
    "scripts/verify_current_source.py",
    "scripts/verify_current_source_v2.py",
    "scripts/verify_current_source_v3.py",
    "scripts/p0_historical_corpus.py",
    "scripts/verify_fixed_e3_integration.py",
    "scripts/verify_packing_integration.py",
)


class VerificationError(ValueError):
    """A fixed identity or current plan contract did not match."""


def require(condition, message):
    if not condition:
        raise VerificationError(message)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def canonical(value):
    # Unlike Python equality, JSON keeps true distinct from 1 at every depth.
    return json.dumps(value, ensure_ascii=False, sort_keys=True,
                      separators=(",", ":"), allow_nan=False)


def parse_json(raw):
    def unique(pairs):
        result = {}
        for key, value in pairs:
            require(key not in result, "duplicate JSON key: " + key)
            result[key] = value
        return result

    def invalid_constant(value):
        raise VerificationError("non-JSON constant: " + value)

    return json.loads(raw, object_pairs_hook=unique, parse_constant=invalid_constant)


def relative_path(path):
    require(isinstance(path, str) and bool(path) and "\\" not in path
            and not any(ord(char) < 32 for char in path), "invalid relative path")
    parsed = PurePosixPath(path)
    require(not parsed.is_absolute() and str(parsed) == path
            and all(part not in {".", ".."} for part in parsed.parts),
            "unsafe relative path: " + path)
    return parsed.parts


def read_regular(root, path):
    parts = relative_path(path)
    target = Path(root)
    for index, part in enumerate(parts):
        target = target / part
        mode = target.lstat().st_mode
        expected = stat.S_ISREG if index == len(parts) - 1 else stat.S_ISDIR
        require(expected(mode), "non-regular path component: " + path)
    return target.read_bytes()


def fixed_file(root, path, expected):
    raw = read_regular(root, path)
    require(raw == expected, "frozen file changed: " + path)
    return raw


class GitObjects:
    """Read immutable Git objects; fetch only named missing provenance commits."""

    def __init__(self, root):
        self.root = Path(root)
        self.cache = {}

    def git(self, *args, data=None):
        return subprocess.check_output(["git", *args], cwd=self.root, input=data)

    def ensure(self, refs):
        refs = sorted(set(refs))
        require(all(isinstance(ref, str) and re.fullmatch(r"[0-9a-f]{40}", ref)
                    for ref in refs), "provenance requires fixed full commit SHAs")
        missing = [ref for ref in refs if subprocess.run(
            ["git", "cat-file", "-e", ref + "^{commit}"], cwd=self.root,
            stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode]
        if missing:
            subprocess.run(["git", "fetch", "--no-tags", "origin", *missing],
                           cwd=self.root, check=True)

    def blobs(self, pairs):
        pairs = list(pairs)
        missing = list(dict.fromkeys(pair for pair in pairs if pair not in self.cache))
        for ref, path in missing:
            require(re.fullmatch(r"[0-9a-f]{40}", ref), "non-fixed Git reference")
            relative_path(path)
        if missing:
            requests = "".join(ref + ":" + path + "\n" for ref, path in missing).encode()
            raw = self.git("cat-file", "--batch", data=requests)
            offset = 0
            for pair in missing:
                end = raw.find(b"\n", offset)
                require(end >= offset, "truncated Git blob header")
                header = raw[offset:end].decode().split()
                require(len(header) == 3 and header[1] == "blob"
                        and re.fullmatch(r"[0-9a-f]{40}", header[0])
                        and header[2].isdigit(), "missing or non-blob Git input: " + str(pair))
                size, offset = int(header[2]), end + 1
                body = raw[offset:offset + size]
                offset += size
                require(len(body) == size and raw[offset:offset + 1] == b"\n",
                        "truncated Git blob body")
                self.cache[pair] = body
                offset += 1
            require(offset == len(raw), "unexpected trailing Git blob data")
        return [self.cache[pair] for pair in pairs]

    def blob(self, ref, path):
        return self.blobs([(ref, path)])[0]

    def inputs(self, ref):
        raw = self.git("ls-tree", "-r", "--name-only", "-z", ref, "--",
                       "crates", "Cargo.toml", "Cargo.lock")
        return {path.decode() for path in raw.split(b"\0") if path}


def task_states(snapshot):
    require(isinstance(snapshot, dict) and isinstance(snapshot.get("tasks"), list),
            "invalid task snapshot")
    rows = snapshot["tasks"]
    require(all(isinstance(row, dict) and isinstance(row.get("id"), str)
                and isinstance(row.get("status"), str) for row in rows),
            "invalid task identity/status")
    result = {row["id"]: row["status"] for row in rows}
    require(len(result) == len(rows), "duplicate task identities")
    return result


def historical_states(snapshot, preserved, label, complete):
    states = task_states(snapshot)
    require(isinstance(preserved, dict), "invalid historical state manifest")
    require(all(key in states and states[key] == value for key, value in preserved.items()),
            label + " historical task states changed")
    if complete:
        require(set(states) == set(preserved), label + " historical task inventory changed")


def current_task_definitions(current, baseline):
    """Allow explicitly named progress fields; freeze every other JSON value."""
    old_states, new_states = task_states(baseline), task_states(current)
    require(len(old_states) == 192 and len(new_states) == 192,
            "current task inventory must retain all 192 tasks")
    require(set(current) == set(baseline), "current plan fields changed")
    old_rows, rows = baseline["tasks"], current["tasks"]
    require([row["id"] for row in rows] == [row["id"] for row in old_rows],
            "current task identities or order changed")
    strip_plan = lambda value: {key: item for key, item in value.items()
                                if key not in PLAN_PROGRESS and key != "tasks"}
    require(canonical(strip_plan(current)) == canonical(strip_plan(baseline)),
            "current plan definition changed")
    for old, row in zip(old_rows, rows):
        require(set(old) <= set(row) <= set(old) | TASK_PROGRESS,
                "current task fields changed: " + old["id"])
        immutable = lambda value: {key: item for key, item in value.items()
                                   if key not in TASK_PROGRESS}
        require(canonical(immutable(row)) == canonical(immutable(old)),
                "current task definition changed: " + old["id"])
        require(row["status"] in STATUSES and isinstance(row["evidence"], list),
                "invalid current task progress: " + old["id"])
        require("implementation_notes" not in row
                or isinstance(row["implementation_notes"], str),
                "invalid current implementation notes: " + old["id"])
    require(isinstance(current["status"], str) and current["status"] in STATUSES,
            "invalid current plan status")
    require(isinstance(current["execution_note"], str), "invalid current execution note")
    require(isinstance(current["last_implementation_date"], str), "invalid current progress date")
    for field in ("current_phase", "next_task"):
        require(current[field] is None or isinstance(current[field], str),
                "invalid current navigation field: " + field)
    return [{"task_id": key, "baseline": value, "current": new_states[key]}
            for key, value in old_states.items() if value != new_states[key]]


def current_gate_checks(root, gates):
    require(isinstance(gates.get("rows"), list) and len(gates["rows"]) == 37,
            "current remaining-gate row count changed")
    require(gates["full_gate_status"]["V19"] == "open", "current V19 must remain open")
    scope = gates["fixed_e3_local_attempt_width4_acceptance"]
    require(scope["strict_causal_speedup"] is False, "strict causal speedup not established")
    require(scope["statistical_significance"] is False, "statistical significance not established")
    require(not os.path.lexists(Path(root) / FORBIDDEN), "excluded historical paired directory present")


def verify(root=ROOT):
    git = GitObjects(root)
    git.ensure([PACKING_INTEGRATION, PACKING_SOURCE, PACKING_WORKFLOW_SOURCE,
                E3_INTEGRATION, E3_SOURCE, TASK_BASE])
    for path, raw in zip(LEGACY_FILES, git.blobs((TASK_BASE, p) for p in LEGACY_FILES)):
        fixed_file(root, path, raw)
    manifest_paths = [PACKING_REPORT + "/source-manifest.json",
                      E3_REPORT + "/production-identity.json",
                      E3_REPORT + "/imported-identity.json",
                      E3_REPORT + "/final-tooling-identity.json"]
    manifest_refs = [PACKING_INTEGRATION, E3_INTEGRATION, E3_INTEGRATION, E3_INTEGRATION]
    raw_manifests = git.blobs(zip(manifest_refs, manifest_paths))
    packing, product, imports, tooling = [parse_json(fixed_file(root, path, raw))
        for path, raw in zip(manifest_paths, raw_manifests)]
    require(packing["fixed_product_source"] == PACKING_SOURCE, "packing product pin changed")
    require(product["source_commit"] == E3_SOURCE, "E3 product pin changed")
    git.ensure(row["source_commit"] for row in
               packing["crate_inputs"] + packing["evidence_files"] + imports["files"])

    expected = {row["path"]: row for row in packing["crate_inputs"]}
    require(git.inputs(PACKING_INTEGRATION) == set(expected),
            "historical packing input inventory changed")
    actual = git.blobs((PACKING_INTEGRATION, path) for path in expected)
    originals = git.blobs((row["source_commit"], path) for path, row in expected.items())
    for (path, row), raw, original in zip(expected.items(), actual, originals):
        require(sha(raw) == row["sha256"], "historical packing input changed: " + path)
        if row.get("adaptation") == "four_cloned_ref_to_slice_refs":
            require(sha(original) == row["original_sha256"], "original adapted test changed")
            fixed_file(root, PACKING_REPORT + "/original-tests/qname_db_independent_review.rs", original)
            head, tail = original.split(b"&[original.clone()]", 1)
            original = head + b"&[original.clone()]" + tail.replace(
                b"&[original.clone()]", b"std::slice::from_ref(&original)").replace(
                b"&[repeated.clone()]", b"std::slice::from_ref(&repeated)")
        require(raw == original, "packing input provenance changed: " + path)
        if row["category"] == "product":
            require(row["source_commit"] == PACKING_SOURCE, "packing product source changed")
    def is_product(path):
        return path in ("Cargo.toml", "Cargo.lock") or "/src/" in path or (
            path.startswith("crates/") and path.endswith(("/Cargo.toml", "/build.rs")))
    require({p for p in git.inputs(PACKING_SOURCE) if is_product(p)} ==
            {p for p, row in expected.items() if row["category"] == "product"},
            "packing production inventory changed")
    evidence = packing["evidence_files"]
    for row, original in zip(evidence, git.blobs((r["source_commit"], r["path"]) for r in evidence)):
        raw = fixed_file(root, row["path"], original)
        require(sha(raw) == row["sha256"], "packing evidence fixity changed: " + row["path"])
    for row in packing["unchanged_workflows"]:
        require(git.blob(PACKING_INTEGRATION, row["path"]) ==
                git.blob(PACKING_WORKFLOW_SOURCE, row["path"]), "historical workflow changed")
    historical_states(parse_json(git.blob(PACKING_INTEGRATION, TASK_PATH)),
                      packing["preserved_task_states"], "packing", complete=True)

    e3_inputs = {row["path"]: row for row in product["files"]}
    require(git.inputs(E3_SOURCE) == set(e3_inputs), "historical E3 inventory differs")
    for (path, row), raw in zip(e3_inputs.items(), git.blobs((E3_SOURCE, p) for p in e3_inputs)):
        require(sha(raw) == row["sha256"], "historical E3 bytes differ: " + path)
    for row, original in zip(imports["files"], git.blobs(
            (r["source_commit"], r["path"]) for r in imports["files"])):
        raw = fixed_file(root, row["path"], original)
        require(sha(raw) == row["sha256"], "E3 import bytes differ: " + row["path"])
    e3_tasks = git.blob(E3_INTEGRATION, TASK_PATH)
    task_rows = [row for row in tooling["files"] if row["path"] == TASK_PATH]
    require(len(task_rows) == 1 and sha(e3_tasks) == task_rows[0]["sha256"]
            and len(e3_tasks) == task_rows[0]["bytes"], "E3 task snapshot tooling identity changed")
    historical_states(parse_json(e3_tasks), imports["preserved_task_states"], "E3", complete=False)

    baseline_raw = git.blob(TASK_BASE, TASK_PATH)
    require(sha(baseline_raw) == TASK_BASE_SHA256, "fixed task-definition baseline changed")
    current_raw = read_regular(root, TASK_PATH)
    changes = current_task_definitions(parse_json(current_raw), parse_json(baseline_raw))
    current_gate_checks(root, parse_json(read_regular(root, GATES_PATH)))
    # The current generator owns valid status/dependency/navigation and all four
    # derived views. Run its normal check, with no write mode or alternate root.
    subprocess.run([sys.executable, "-B", "scripts/code_index_plan.py"], cwd=root,
                   env=dict(os.environ, PYTHONDONTWRITEBYTECODE="1"), check=True)
    return {"schema_version": 2, "verifier_version": VERSION, "status": "passed",
            "scope": "historical_integrity_and_current_task_definitions_only",
            "packing_source": PACKING_SOURCE, "packing_integration": PACKING_INTEGRATION,
            "packing_inputs": len(expected), "packing_evidence_files": len(evidence),
            "e3_source": E3_SOURCE, "e3_integration": E3_INTEGRATION,
            "e3_inputs": len(e3_inputs), "e3_imported_files": len(imports["files"]),
            "task_definition_base": TASK_BASE, "task_definition_base_sha256": TASK_BASE_SHA256,
            "task_count": 192, "current_task_sha256": sha(current_raw),
            "state_changes_since_baseline": changes, "legacy_files_unchanged": len(LEGACY_FILES),
            "current_plan_check": "passed", "full_P7": "open", "V19": "open",
            "quality_and_100k": "not_inherited", "fault_runtime_tests": "not_run",
            "evidence_content_acceptance": "not_granted_by_this_verifier"}


def main():
    argparse.ArgumentParser(description=__doc__).parse_args()
    try:
        require(__debug__, "optimized Python cannot run this verifier")
        print(json.dumps(verify(), ensure_ascii=False, sort_keys=True))
        return 0
    except (OSError, ValueError, KeyError, TypeError, subprocess.CalledProcessError) as error:
        print(json.dumps({"status": "failed", "verifier_version": VERSION,
                          "error": str(error)}, ensure_ascii=False), file=sys.stderr)
        return 1


if __name__ == "__main__":
    raise SystemExit(main())
