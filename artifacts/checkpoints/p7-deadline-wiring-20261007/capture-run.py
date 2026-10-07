"""Capture one current-source command; no stored receipt grants authority."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import sys
import time

ROOT = Path.cwd()
OUT = Path(__file__).resolve().parent
LABEL, COMMAND = sys.argv[1], sys.argv[2:]
assert re.fullmatch(r"[a-z0-9-]+", LABEL) and COMMAND


def digest(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def git(*args):
    return subprocess.check_output(["git", *args], cwd=ROOT).decode().strip()


def source_map():
    names = git("ls-files", "-z", "Cargo.toml", "Cargo.lock", "crates").split("\0")
    return {name: digest(ROOT / name) for name in sorted(names) if name}


def extra_literal_inputs(inputs):
    found = {}
    pending = [ROOT / name for name in inputs if name.endswith(".rs")]
    visited = set()
    pattern = r'(?:include(?:_str|_bytes)?!\s*\(\s*|#\[path\s*=\s*)"([^"\n]+)"'
    while pending:
        source = pending.pop()
        if source in visited:
            continue
        visited.add(source)
        for relative in re.findall(pattern, source.read_text()):
            path = (source.parent / relative).resolve()
            if path.is_file() and path.is_relative_to(ROOT):
                name = str(path.relative_to(ROOT))
                if name not in inputs:
                    found[name] = digest(path)
                if path.suffix == ".rs":
                    pending.append(path)
    return dict(sorted(found.items()))


before = source_map()
extra_before = extra_literal_inputs(before)
head_before, tree_before = git("rev-parse", "HEAD"), git("rev-parse", "HEAD^{tree}")
status_before = git("status", "--porcelain")
manifest = OUT / (LABEL + "-source-sha256.json")
assert not manifest.exists(), "evidence labels are immutable"
manifest.write_text(json.dumps(before, sort_keys=True, indent=2) + "\n")
env = os.environ.copy()
raw = OUT / "raw" / LABEL
trace_keys = set()
for source in ROOT.glob("crates/cc-server/tests/p7*.rs"):
    trace_keys.update(re.findall(r'"([A-Z0-9_]+EVIDENCE_DIR)"', source.read_text()))
trace_keys.update(["CACHE_KEY_REVIEW_OUTPUT", "V11_REVIEW_OUTPUT"])
for key in sorted(trace_keys):
    env[key] = str(raw / key)
    Path(env[key]).mkdir(parents=True)
started = time.time()
log = OUT / (LABEL + ".log")
with log.open("wb") as stream:
    result = subprocess.run(COMMAND, cwd=ROOT, env=env, stdout=stream, stderr=subprocess.STDOUT)
text = log.read_text(errors="replace")
executables = {}
for line in text.splitlines():
    try:
        event = json.loads(line)
    except json.JSONDecodeError:
        continue
    if isinstance(event, dict) and event.get("reason") == "compiler-artifact" and event.get("executable"):
        path = Path(event["executable"])
        executables[str(path)] = {
            "sha256": digest(path), "bytes": path.stat().st_size,
            "target": event["target"], "profile": event["profile"],
            "manifest_path": event["manifest_path"], "features": event["features"],
            "fresh": event["fresh"],
        }
observed_binary = None
if "lifecycle_stdio.py" in " ".join(COMMAND):
    binary = Path(COMMAND[2])
    observed_binary = {"path": str(binary), "bytes": binary.stat().st_size, "sha256": digest(binary)}
summaries = [
    {"status": match[0], "passed": int(match[1]), "failed": int(match[2]), "ignored": int(match[3])}
    for match in re.findall(r"test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored", text)
]
receipt = {
    "schema_version": 1, "label": LABEL, "executed_by": "/root/todo_audit",
    "source_commit": head_before, "source_tree": tree_before,
    "source_commit_after": git("rev-parse", "HEAD"),
    "status_before": status_before, "status_after": git("status", "--porcelain"),
    "command": COMMAND, "cwd": str(ROOT), "exit_code": result.returncode,
    "duration_seconds": round(time.time() - started, 3),
    "source_count": len(before), "source_manifest": manifest.name,
    "source_scope": "tracked crates/** and root Cargo inputs; additional literal repository includes below",
    "source_manifest_sha256": digest(manifest), "source_unchanged": before == source_map(),
    "extra_literal_inputs": extra_before,
    "extra_literal_inputs_unchanged": extra_before == extra_literal_inputs(before),
    "log": log.name, "log_sha256": digest(log), "log_bytes": log.stat().st_size,
    "test_summaries": summaries, "executables": executables,
    "explicit_lifecycle_binary": observed_binary,
    "raw_artifacts": {str(path.relative_to(OUT)): {"sha256": digest(path), "bytes": path.stat().st_size}
                      for path in sorted(raw.rglob("*")) if path.is_file()},
    "environment": {key: env.get(key) for key in ["CARGO_TARGET_DIR", "CARGO_PROFILE_DEV_DEBUG", "CARGO_PROFILE_TEST_DEBUG", "CARGO_INCREMENTAL", *sorted(trace_keys)]},
    "capture_script_sha256": digest(Path(__file__)),
}
(OUT / (LABEL + "-receipt.json")).write_text(json.dumps(receipt, indent=2) + "\n")
print(json.dumps({key: receipt[key] for key in ["label", "source_commit", "exit_code", "duration_seconds", "source_count", "source_unchanged", "test_summaries"]}), flush=True)
if result.returncode:
    relevant = [line for line in text.splitlines() if not line.startswith("{")]
    print("\n".join(relevant)[-9000:], flush=True)
sys.exit(result.returncode)
