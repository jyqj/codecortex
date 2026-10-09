import datetime,json,subprocess
from pathlib import Path
root=Path("/Users/jin/Desktop/codecortex-rust/artifacts/benchmarks/p8-pr184-native-20261009-root")
GH="/opt/homebrew/bin/gh"
def api(endpoint):
 p=subprocess.run([GH,"api","--method","GET",endpoint],stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=40)
 assert p.returncode==0,p.stderr.decode()
 return json.loads(p.stdout)
pr=api("/repos/jyqj/codecortex/pulls/184")
assert pr["head"]["sha"]=="b356043c2c0c970d000e5f5e32f1dff69f88454f" and pr["state"]=="open"
records=[]
for old,new in [(37923044883,37930625847),(37923045016,37930625741)]:
 r=api("/repos/jyqj/codecortex/actions/runs/"+str(old));j=api("/repos/jyqj/codecortex/actions/runs/"+str(old)+"/jobs?filter=all&per_page=100&page=1");replacement=api("/repos/jyqj/codecortex/actions/runs/"+str(new))
 record={"observed_at_utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"old_run":r,"jobs":j,"replacement":replacement}
 okay=r["head_sha"]=="ae033369f015c416cd7995709c1853497773cf6a" and r["status"] in ("queued","pending") and replacement["head_sha"]=="b356043c2c0c970d000e5f5e32f1dff69f88454f" and r["workflow_id"]==replacement["workflow_id"] and j["total_count"]==len(j["jobs"]) and len(j["jobs"])>0 and all(x["status"] in ("queued","pending") and not x.get("runner_id") and not x.get("steps") for x in j["jobs"])
 record["eligible_all_jobs_unstarted"]=okay
 if okay:
  argv=[GH,"run","cancel",str(old),"--repo","jyqj/codecortex"]
  p=subprocess.run(argv,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=40)
  record["action"]={"argv":argv,"exit_code":p.returncode,"stdout":p.stdout.decode(),"stderr":p.stderr.decode()}
  assert p.returncode==0,record["action"]
 records.append(record)
body=json.dumps({"schema":"p8-pr184-superseded-old-G-unstarted-CI-cleanup-v1","new_head":pr["head"]["sha"],"records":records,"preserved_started_run_ids":[37923045101,37923045152,37923045040],"all_original_scale_runs_untouched":True},indent=2)+"\n"
with (root/"round28-oldG-unstarted-CI-cleanup.json").open("x") as f:f.write(body)
print(json.dumps({"receipt":"round28-oldG-unstarted-CI-cleanup.json","records":[{"old":x["old_run"]["id"],"replacement":x["replacement"]["id"],"eligible":x["eligible_all_jobs_unstarted"],"action":x.get("action")} for x in records]}))
