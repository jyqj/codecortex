#!/usr/bin/env python3
"""Receive selected complete original cold shards with the unchanged driver."""
from pathlib import Path
import hashlib
import json
import subprocess
import sys
import time
import traceback

HERE = Path(__file__).resolve().parent
VIEW = HERE.parent / "p8-cold-only-integration/guard-view"
sys.path.insert(0, str(VIEW / "scripts"))
import p8_cold_matrix as cold
import p8_scale_matrix as full


def main():
    plan_path = Path(sys.argv[1]).resolve()
    plan = json.loads(plan_path.read_bytes())
    assert plan["source"] == "044c008c9459cfa61e7701db2eb868342c508a39"
    assert plan["run_id"] == 37954851017 and plan["attempt"] == 1
    cells = plan["shards"]
    assert len({(c["scale"], c["rep"]) for c in cells}) == len(cells)
    assert len({c["artifact_id"] for c in cells}) == len(cells)
    assert all(c["scale"] in (1000, 5000, 10000, 50000, 100000) and 0 <= c["rep"] < 30 for c in cells)
    out = HERE / plan["output"]
    assert out.parent == HERE
    out.mkdir(exist_ok=False)
    build = HERE / "extracted/11627448529"

    def snapshot():
        reg = full.read_json(VIEW / "scripts/reviewed-source-registry-v15.json")
        names = set(reg["complete_inputs"]) | set(reg["validation_inputs"]) | {
            "scripts/reviewed-source-registry-v15.json", "scripts/verify_reviewed_source_v15.py"}
        return {
            "head": subprocess.check_output(["git", "rev-parse", "HEAD"], cwd=VIEW, text=True).strip(),
            "index_sha256": hashlib.sha256((VIEW / ".git/index").read_bytes()).hexdigest(),
            "inputs": {p: full.file_sha256(VIEW / p) for p in sorted(names)},
            "batch_plan_sha256": full.file_sha256(plan_path),
            "original_zips": {p.name: full.file_sha256(p) for p in sorted((HERE / "original-zips").glob("*.zip"))},
            "build_inventory": full.inventory(build),
            "selected_shards": {str(c["artifact_id"]): full.inventory(HERE / "extracted" / str(c["artifact_id"])) for c in cells},
        }

    before = snapshot()
    full.write_new(out / "inputs-before.json", before)
    started, begin, code, error, accepted = full.utc(), time.monotonic(), 0, None, []
    try:
        assert before["head"] == plan["source"]
        outer, built, binary = cold.validate_build(build, VIEW)
        for cell in cells:
            result = cold.validate_shard(HERE / "extracted" / str(cell["artifact_id"]), outer, built, binary, build, VIEW)
            assert result["plan"]["files"] == [cell["scale"]]
            assert result["plan"]["shard"] == {"count": 30, "index": cell["rep"]}
            assert len(result["measurements"]) == 1
            full.write_new(out / f"accepted-original-{cell['scale']}-{cell['rep']}.json", result)
            accepted.append({**cell, "samples": 1})
    except BaseException as exc:
        code, error = 1, type(exc).__name__ + ": " + str(exc)
        traceback.print_exc()
    finally:
        after = snapshot()
        full.write_new(out / "inputs-after.json", after)
        if before != after:
            code, error = 1, (error + "; " if error else "") + "input drift"
        record = {"schema": "actual-original-cold-batch-intake-v1", "started_utc": started,
                  "finished_utc": full.utc(), "wall_seconds": time.monotonic() - begin,
                  "exit_code": code, "error": error, "inputs_unchanged": before == after,
                  "source": before["head"], "run_id": 37954851017, "attempt": 1,
                  "calls": ["p8_cold_matrix.validate_build", "p8_cold_matrix.validate_shard"],
                  "scope": "Original unchanged functions; retained executable hash-file only; no workload execution or partial aggregate. Complete original ZIPs only in immutable inventory.",
                  "accepted_originals": accepted, "newly_accepted_shards": len(accepted) if code == 0 else 0,
                  "prior_accepted_shards": plan["prior_accepted_shards"],
                  "total_accepted_shards": plan["prior_accepted_shards"] + (len(accepted) if code == 0 else 0),
                  "registered_shards": 150, "registered_samples": 150,
                  "newly_completed_todos": 0, "remaining_todos": 29}
        full.write_new(out / "execution.json", record)
        print(json.dumps(record), flush=True)
    return code


if __name__ == "__main__":
    raise SystemExit(main())
