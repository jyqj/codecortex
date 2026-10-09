import datetime, hashlib, json, os, subprocess, sys, time, traceback
from pathlib import Path
HERE=Path(__file__).resolve().parent
VIEW=Path("/workspace/scratch/a217aaae3bde/p8-cold-only-integration/guard-view")
OUT=HERE/"first-original-intake"
OUT.mkdir(exist_ok=False)
sys.path.insert(0,str(VIEW/"scripts"))
import p8_cold_matrix as cold
import p8_scale_matrix as full
BUILD=HERE/"extracted/11627448529"
SHARD=HERE/"extracted/11628181359"
binary=BUILD/"native-build/p8-scale"
mode_before=oct(binary.stat().st_mode & 0o777)
binary.chmod(0o755)
def snapshot():
    reg=full.read_json(VIEW/"scripts/reviewed-source-registry-v15.json")
    names=set(reg["complete_inputs"])|set(reg["validation_inputs"])|{"scripts/reviewed-source-registry-v15.json","scripts/verify_reviewed_source_v15.py"}
    return {"head":subprocess.check_output(["git","rev-parse","HEAD"],cwd=VIEW,text=True).strip(),
            "index_sha256":hashlib.sha256((VIEW/".git/index").read_bytes()).hexdigest(),
            "inputs":{p:full.file_sha256(VIEW/p) for p in sorted(names)},
            "original_zip_inventory":full.inventory(HERE/"original-zips"),
            "build_inventory":full.inventory(BUILD),"shard_inventory":full.inventory(SHARD)}
before=snapshot()
full.write_new(OUT/"inputs-before.json",before)
started=full.utc();begin=time.monotonic();code=0;error=None;accepted=None
try:
    assert before["head"]=="044c008c9459cfa61e7701db2eb868342c508a39"
    outer,built,binary=cold.validate_build(BUILD,VIEW)
    accepted=cold.validate_shard(SHARD,outer,built,binary,BUILD,VIEW)
    full.write_new(OUT/"accepted-original-1000-0.json",accepted)
    print(json.dumps({"status":"accepted_original_cold_shard","sample_count":len(accepted["measurements"]),
        "scale":accepted["plan"]["files"],"shard":accepted["plan"]["shard"],"study":accepted["study"]}),flush=True)
except BaseException as exc:
    code=1;error=type(exc).__name__+": "+str(exc);traceback.print_exc()
finally:
    after=snapshot();full.write_new(OUT/"inputs-after.json",after)
    if before!=after:
        code=1;error=(error+"; " if error else "")+"input drift"
    record={"schema":"actual-original-cold-intake-v1","started_utc":started,"finished_utc":full.utc(),
            "wall_seconds":time.monotonic()-begin,"exit_code":code,"error":error,"inputs_unchanged":before==after,
            "source":before["head"],"run_id":37954851017,"attempt":1,
            "calls":["p8_cold_matrix.validate_build","p8_cold_matrix.validate_shard"],
            "scope":"Original unchanged functions; executable --hash-file only; no native workload rerun or partial aggregate.",
            "local_binary_mode_before":mode_before,"local_binary_mode_for_original_hash_helper":"0o755",
            "original_ZIP_bytes_unchanged":before["original_zip_inventory"]==after["original_zip_inventory"],
            "accepted_shards":1 if code==0 else 0,"accepted_samples":len(accepted["measurements"]) if code==0 else 0,
            "registered_shards":150,"registered_samples":150,"newly_completed_todos":0,"remaining_todos":29}
    full.write_new(OUT/"execution.json",record)
    print(json.dumps(record),flush=True)
raise SystemExit(code)

