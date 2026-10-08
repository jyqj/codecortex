#!/usr/bin/env python3
"""Verify the exact published adaptation allowlists before replaying evidence."""
import hashlib
import json
import os
from pathlib import Path
import subprocess
import sys
sys.dont_write_bytecode = True

SOURCE = Path(os.environ["P7_AUDIT_SOURCE_ROOT"]).resolve(strict=True)
AUDIT = Path(__file__).resolve().parent
OUT = Path(os.environ["P7_AUDIT_RESULTS_ROOT"]).resolve()
HEAD = "ffdc6f0f97db78cc25a6c026904e7c2adde05d14"
TREE = "9e59b41540eb3769e9ca0a787c9ed60441259d20"
MERGE = "683d8882c108e4e9be68ebac127f3c709e329711"

def git(*args):
    return subprocess.check_output(["git", *args], cwd=SOURCE)

def digest(raw):
    return hashlib.sha256(raw).hexdigest()

def regular(path):
    assert path.is_file() and not path.is_symlink(), str(path)
    return path.read_bytes()

assert sys.flags.optimize == 0
assert git("rev-parse", "HEAD").decode().strip() == HEAD
assert git("rev-parse", "HEAD^{tree}").decode().strip() == TREE
assert git("rev-parse", MERGE + "^{tree}").decode().strip() == TREE
assert not git("status", "--porcelain")
rows = []
plan = json.loads(regular(AUDIT / "audit-adaptation.json"))
for item in plan["files"]:
    raw = git("show", HEAD + ":" + item["original_path"])
    assert git("rev-parse", HEAD + ":" + item["original_path"]).decode().strip() == item["original_git_blob"]
    text = raw.decode()
    observations = []
    for change in item["transforms"]:
        count = text.count(change["before"])
        assert count == change["expected_count"], (item["output"], change, count)
        text = text.replace(change["before"], change["after"])
        observations.append({"before":change["before"], "count":count})
    target = AUDIT / item["output"]
    assert text.encode() == regular(target), "unlisted auditor change: " + item["output"]
    rows.append({"file":item["output"], "original_sha256":digest(raw),
                 "current_sha256":digest(text.encode()), "transforms":observations,
                 "all_bytes_reconstructed_by_explicit_allowlist":True})
plan = json.loads(regular(AUDIT / "closeout-adaptation.json"))
raw = git("show", HEAD + ":" + plan["original_path"])
assert digest(raw) == plan["original_sha256"]
assert git("rev-parse", HEAD + ":" + plan["original_path"]).decode().strip() == plan["original_git_blob"]
text = raw.decode()
for change in plan["replacements"]:
    assert text.count(change["from"]) == 1, change
    text = text.replace(change["from"], change["to"], 1)
target = AUDIT / Path(plan["new_path"]).name
assert text.encode() == regular(target)
assert digest(text.encode()) == plan["new_sha256"]
rows.append({"file":target.name, "original_sha256":digest(raw),
             "current_sha256":digest(text.encode()),
             "transforms":len(plan["replacements"]),
             "all_bytes_reconstructed_by_explicit_allowlist":True})
sys.path.insert(0, str(SOURCE / "scripts"))
from p7_build_identity import source_snapshot, json_bytes
snapshot = source_snapshot(SOURCE)
assert snapshot["input_count"] == 795
assert snapshot["source_commit"] == HEAD and snapshot["source_tree"] == TREE
assert snapshot["manifest_sha256"] == "5b0f2f129500327a23a44a10b8f4530db6260273481698c9bb5c7cbe65fbc114"
OUT.mkdir(parents=True, exist_ok=True)
(OUT / "candidate-source-before.json").write_bytes(json_bytes(snapshot))
record = {"schema_version":1, "source_commit":HEAD, "source_tree":TREE,
          "script_sha256":digest(Path(__file__).read_bytes()), "adaptations":rows,
          "source_input_count":795, "assert_optimization":sys.flags.optimize,
          "task_acceptance":"not_granted; byte reconstruction only"}
(OUT / "adapter-integrity.json").write_bytes(json_bytes(record))
print("P7_ADAPTER_INTEGRITY " + json.dumps(record, ensure_ascii=True))
