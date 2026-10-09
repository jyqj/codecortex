#!/usr/bin/env python3
"""Receive the first complete original shard using unchanged b107 once."""
from datetime import datetime, timezone
from pathlib import Path
import json
import os
import subprocess
import time
import run_original_build_init as original

ROOT = Path(__file__).resolve().parent
ARTIFACT = 11633585491
OUT = ROOT / "first-original-shard-intake"


def snapshot():
    result = original.snapshot()
    result["validated_state_sha256"] = original.sha(ROOT / "validated/state.json")
    result["shard_metadata_sha256"] = original.sha(ROOT / f"official-artifact-{ARTIFACT}.json")
    return result


def main():
    assert not (ROOT / "validated/shards" / str(ARTIFACT)).exists()
    OUT.mkdir(exist_ok=False)
    before = snapshot()
    (OUT / "inputs-before.json").write_text(json.dumps(before, sort_keys=True, indent=2) + "\n")
    argv = ["python3", "-B", str(original.HELPER), "shard", "--state", str(ROOT / "validated"),
            "--archive", str(ROOT / "original-zips" / f"artifact-{ARTIFACT}.zip"),
            "--metadata", str(ROOT / f"official-artifact-{ARTIFACT}.json")]
    started = datetime.now(timezone.utc).isoformat()
    begin = time.monotonic()
    result = subprocess.run(argv, cwd=ROOT, env={**os.environ, "GIT_OPTIONAL_LOCKS": "0"}, capture_output=True)
    finished = datetime.now(timezone.utc).isoformat()
    wall = time.monotonic() - begin
    (OUT / "original.stdout").write_bytes(result.stdout)
    (OUT / "original.stderr").write_bytes(result.stderr)
    after = snapshot()
    (OUT / "inputs-after.json").write_text(json.dumps(after, sort_keys=True, indent=2) + "\n")
    review_path = ROOT / "validated/shards" / str(ARTIFACT) / "review.json"
    review = json.loads(review_path.read_bytes()) if review_path.exists() else {}
    code = result.returncode if before == after else 1
    accepted = code == 0 and review.get("status") == "passed_original_validate_shard"
    if not accepted:
        code = code or 1
    record = {"schema": "actual-original-full-scale-shard-intake-v1", "argv": argv,
              "source": before["head"], "run_id": 37962416564, "attempt": 1,
              "artifact_id": ARTIFACT, "started_utc": started, "finished_utc": finished,
              "wall_seconds": wall, "original_exit_code": result.returncode, "wrapper_exit_code": code,
              "inputs_unchanged": before == after, "original_status": review.get("status"),
              "accepted_shards": 1 if accepted else 0, "accepted_samples": review.get("sample_count", 0) if accepted else 0,
              "registered_shards": 150, "registered_samples": 1500,
              "newly_completed_todos": 0, "remaining_todos": 29,
              "scope": "Unchanged original helper validates full original ZIP, source/run/engine and all original shard outputs. No prefix admission, workload replay or partial aggregate.",
              "stdout_bytes": len(result.stdout), "stderr_bytes": len(result.stderr)}
    (OUT / "execution.json").write_text(json.dumps(record, sort_keys=True, indent=2) + "\n")
    print(json.dumps(record), flush=True)
    return code


if __name__ == "__main__":
    raise SystemExit(main())
