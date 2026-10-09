"""Accept the original 100k rep0 cold shard using the frozen driver."""
import hashlib
import json
from pathlib import Path
import subprocess
import sys
import time
import traceback

HERE = Path(__file__).resolve().parent
VIEW = HERE.parent / "p8-cold-only-integration/guard-view"
OUT = HERE / "first-100k-original-intake"
OUT.mkdir(exist_ok=False)
sys.path.insert(0, str(VIEW / "scripts"))
import p8_cold_matrix as cold
import p8_scale_matrix as full

BUILD = HERE / "extracted/11627448529"
SHARDS = {100000: 11632550602}


def snapshot():
    reg = full.read_json(VIEW / "scripts/reviewed-source-registry-v15.json")
    names = set(reg["complete_inputs"]) | set(reg["validation_inputs"]) | {
        "scripts/reviewed-source-registry-v15.json", "scripts/verify_reviewed_source_v15.py"}
    return {
        "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=VIEW, text=True).strip(),
        "index_sha256": hashlib.sha256((VIEW / ".git/index").read_bytes()).hexdigest(),
        "inputs": {p: full.file_sha256(VIEW / p) for p in sorted(names)},
        "original_zip_inventory": full.inventory(HERE / "original-zips"),
        "build_inventory": full.inventory(BUILD),
        "shard_inventories": {str(n): full.inventory(HERE / "extracted" / str(n)) for n in SHARDS.values()},
    }


before = snapshot()
full.write_new(OUT / "inputs-before.json", before)
started, begin, code, error, accepted = full.utc(), time.monotonic(), 0, None, []
try:
    assert before["head"] == "044c008c9459cfa61e7701db2eb868342c508a39"
    outer, built, binary = cold.validate_build(BUILD, VIEW)
    for scale, artifact in SHARDS.items():
        result = cold.validate_shard(HERE / "extracted" / str(artifact), outer, built, binary, BUILD, VIEW)
        assert result["plan"]["files"] == [scale] and result["plan"]["shard"] == {"count": 30, "index": 0}
        assert len(result["measurements"]) == 1
        full.write_new(OUT / f"accepted-original-{scale}-0.json", result)
        accepted.append({"artifact_id": artifact, "scale": scale, "shard": 0, "samples": 1})
except BaseException as exc:
    code, error = 1, type(exc).__name__ + ": " + str(exc)
    traceback.print_exc()
finally:
    after = snapshot()
    full.write_new(OUT / "inputs-after.json", after)
    if before != after:
        code, error = 1, (error + "; " if error else "") + "input drift"
    record = {
        "schema": "actual-original-cold-intake-v1", "started_utc": started,
        "wrapper_revision": "New immutable 100k rep0 intake, derived from corrected v2 plan shape. Earlier four accepted shards and prior wrapper history stay separate. No native workload rerun.",
        "finished_utc": full.utc(), "wall_seconds": time.monotonic() - begin,
        "exit_code": code, "error": error, "inputs_unchanged": before == after,
        "source": before["head"], "run_id": 37954851017, "attempt": 1,
        "calls": ["p8_cold_matrix.validate_build", "p8_cold_matrix.validate_shard"],
        "scope": "Original unchanged functions; retained executable --hash-file only; no workload execution or partial aggregate.",
        "accepted_originals": accepted, "newly_accepted_shards": len(accepted) if code == 0 else 0,
        "prior_accepted_shards": 4, "total_accepted_shards": 4 + len(accepted) if code == 0 else 4,
        "registered_shards": 150, "registered_samples": 150,
        "newly_completed_todos": 0, "remaining_todos": 29,
    }
    full.write_new(OUT / "execution.json", record)
    print(json.dumps(record), flush=True)
raise SystemExit(code)
