import collections,hashlib,json
from pathlib import Path
root=Path("/Users/jin/Desktop/codecortex-rust/artifacts/benchmarks/p8-pr184-native-20261009-root")
folder=root/"formal275e-100k-failure-reception"
raw=(folder/"members/native/raw.jsonl").read_bytes()
rows=[json.loads(x) for x in raw.splitlines()]
events=collections.Counter(r.get("event","<missing>") for r in rows)
finished=[r for r in rows if r.get("event")=="build_finished"]
started={r["label"]:r for r in rows if r.get("event")=="build_started"}
finished_labels={r["label"] for r in finished}
keys_by_event={e:sorted({k for r in rows if r.get("event","<missing>")==e for k in r}) for e in events}
def brief(r):
 report=r.get("report",{})
 return {"label":r.get("label"),"full":r.get("full"),"start_us":r.get("start_us"),"end_us":r.get("end_us"),"wall_us":r.get("wall_us"),"build_timing":report.get("build_timing"),"phase_timing":report.get("phase_timing"),"files_parsed":report.get("files_parsed"),"files_scanned":report.get("files_scanned"),"dirty_plan":report.get("dirty_plan"),"resolution_freshness":report.get("resolution_freshness")}
obs={"schema":"p8-formal275e-failed100k-timing-observation-v1","scope":"Derived only from the original failed shard; incomplete observations are not complete acceptance or a new run.","raw":{"bytes":len(raw),"sha256":hashlib.sha256(raw).hexdigest()},"event_counts":dict(events),"keys_by_event":keys_by_event,"finished_builds":len(finished),"finished_wall_sum_us":sum(r.get("wall_us",0) for r in finished),"full_builds":[brief(r) for r in finished if r.get("full")],"longest_12_builds":[brief(r) for r in sorted(finished,key=lambda r:r.get("wall_us",0),reverse=True)[:12]],"unmatched_started_builds":[started[x] for x in started if x not in finished_labels],"last_event":rows[-1],"sample_summaries":[{k:v for k,v in r.items() if not isinstance(v,(dict,list))} for r in rows if r.get("event") not in ("build_started","build_finished","input","run_started")]}
body=json.dumps(obs,indent=2)+"\n"
with (folder/"timing-observation.json").open("x") as f:f.write(body)
print(json.dumps({"bytes":len(body.encode()),"sha256":hashlib.sha256(body.encode()).hexdigest(),"observation":obs}))
paths=[]
for phase in ("P","P2","P3"):
 paths.append(root/("validation-"+phase+"-main4ef-runner.py"))
 paths.extend(sorted((root/("validation-"+phase+"-main4ef-results")).iterdir()))
 paths.append(root/("main4ef-"+phase+"-checkout.json"))
paths.append(root/"duplicate-platform-aggregate-cancel.json")
items=[]
for p in paths:
 raw=p.read_bytes();items.append({"path":str(p.relative_to(root)),"bytes":len(raw),"sha256":hashlib.sha256(raw).hexdigest()})
body=json.dumps({"schema":"p8-round27-native-files-inventory-v1","files":items},indent=2)+"\n"
with (root/"round27-native-inventory.json").open("x") as f:f.write(body)
print(json.dumps({"inventory_bytes":len(body.encode()),"inventory_sha256":hashlib.sha256(body.encode()).hexdigest(),"files":items}))
