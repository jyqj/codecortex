#!/usr/bin/env python3
"""Post-failure read-only inspection; equivalent code was run as a heredoc."""
import importlib.util,json,pathlib,sys,time,gzip
sys.dont_write_bytecode=True
out=pathlib.Path(__file__).resolve().parent;root=out.parents[2];case=out/"live/n100000"
spec=importlib.util.spec_from_file_location("driver",root/"scripts/p7_release_resource_preparation.py");driver=importlib.util.module_from_spec(spec);spec.loader.exec_module(driver)
t=time.monotonic();r=driver.db_report(case/"repo");r["inspection_wall_seconds"]=time.monotonic()-t;r["inspection_phase"]="after failed ready drain and cleanup; not a successful ready manifest checkpoint"
driver.write(case/"failure-db.json",r)
last_status=None
with gzip.open(case/"rpc.jsonl.gz","rt") as f:
    for line in f:
        row=json.loads(line);result=row.get("payload",{}).get("result",{}).get("structuredContent",{});data=result.get("result",result)
        if isinstance(data,dict) and "retrieval" in data:last_status=data
if last_status:driver.write(case/"last-observed-status.json",last_status)
driver.write(out/"retained-local-data.json",{"source":driver.totals(case/"repo/src"),"database_files":r.get("files"),"semantic_cache":driver.totals(case/"cache"),"inspection_wall_seconds":r["inspection_wall_seconds"]})
print(json.dumps({"counts":r.get("counts"),"integrity":r.get("integrity"),"FK_errors":r.get("foreign_key_errors")}))
