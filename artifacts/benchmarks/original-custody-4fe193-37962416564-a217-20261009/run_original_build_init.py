#!/usr/bin/env python3
"""Run the frozen original build intake once and retain its complete receipt."""
from datetime import datetime, timezone
from pathlib import Path
import hashlib
import json
import os
import subprocess
import sys
import time

ROOT = Path(__file__).resolve().parent
VIEW = ROOT / "required-source-view"
OUT = ROOT / "first-original-build-intake"
HELPER = ROOT / "review_scale_fixed_source.py"


def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()


def write(name, value):
    (OUT / name).write_text(json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + "\n")


def snapshot():
    reg = json.loads((VIEW / "scripts/reviewed-source-registry-v15.json").read_bytes())
    names = set(reg["complete_inputs"]) | set(reg["validation_inputs"]) | {
        "scripts/reviewed-source-registry-v15.json", "scripts/verify_reviewed_source_v15.py"}
    git_dir = Path(subprocess.check_output(
        ["git", "rev-parse", "--absolute-git-dir"], cwd=VIEW, text=True).strip())
    return {
        "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=VIEW, text=True).strip(),
        "tree": subprocess.check_output(["git", "rev-parse", "HEAD^{tree}"], cwd=VIEW, text=True).strip(),
        "index_sha256": sha(git_dir / "index"),
        "inputs": {n: sha(VIEW / n) for n in sorted(names)},
        "original_zips": {p.name: sha(p) for p in sorted((ROOT / "original-zips").glob("*.zip"))},
        "helper_sha256": sha(HELPER),
        "official_metadata_sha256": sha(ROOT / "official-artifact-11632527002.json"),
    }


def main():
    assert not (ROOT / "validated").exists()
    OUT.mkdir(exist_ok=False)
    before = snapshot()
    write("inputs-before.json", before)
    assert before["head"] == "4fe927488d5cb7f26bd27c7624745fb1b6ca202b"
    assert before["tree"] == "d8891f547c92044fd40b63165d6ef54cb6de8bab"
    assert before["helper_sha256"] == "b10728501358eb2a55a043d9a67e18cb6105a19398b84baae3b0689a9b0d0fee"
    argv = ["python3", "-B", str(HELPER), "init", "--root", str(VIEW),
            "--source", before["head"], "--run-id", "37962416564", "--state", str(ROOT / "validated"),
            "--archive", str(ROOT / "original-zips/artifact-11632527002.zip"),
            "--metadata", str(ROOT / "official-artifact-11632527002.json")]
    started = datetime.now(timezone.utc).isoformat()
    begin = time.monotonic()
    result = subprocess.run(argv, cwd=ROOT, env={**os.environ, "GIT_OPTIONAL_LOCKS": "0"}, capture_output=True)
    finished = datetime.now(timezone.utc).isoformat()
    wall = time.monotonic() - begin
    (OUT / "original.stdout").write_bytes(result.stdout)
    (OUT / "original.stderr").write_bytes(result.stderr)
    after = snapshot()
    write("inputs-after.json", after)
    code = result.returncode if before == after else 1
    record = {"schema": "actual-original-full-scale-build-intake-v1", "argv": argv,
              "source": before["head"], "run_id": 37962416564, "attempt": 1,
              "artifact_id": 11632527002, "started_utc": started, "finished_utc": finished,
              "wall_seconds": wall, "original_exit_code": result.returncode, "wrapper_exit_code": code,
              "inputs_unchanged": before == after, "guard_invocations": 0,
              "scope": "Original frozen b107 init and unchanged full-study build validator. Retained executable only for original hash-file provenance; no measured workload or test replay.",
              "accepted_shards": 0, "accepted_samples": 0, "newly_completed_todos": 0, "remaining_todos": 29,
              "stdout_bytes": len(result.stdout), "stderr_bytes": len(result.stderr)}
    write("execution.json", record)
    print(json.dumps(record), flush=True)
    if result.returncode:
        sys.stdout.buffer.write(result.stdout)
        sys.stderr.buffer.write(result.stderr)
    return code


if __name__ == "__main__":
    raise SystemExit(main())
