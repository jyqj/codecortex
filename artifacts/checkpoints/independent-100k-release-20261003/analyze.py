#!/usr/bin/env python3
"""Summarize retained observations without changing outcome or deadlines."""
import collections,gzip,hashlib,json,pathlib
def load_json(path):
    if path.exists():return json.loads(path.read_text())
    with gzip.open(str(path)+".gz","rt") as f:return json.load(f)

OUT=pathlib.Path(__file__).resolve().parent
report={"limitations":["/proc/<pid>/task/<tid>/children unavailable in this mount: descendant coverage incomplete; root RSS must not be called full process-tree RSS","20ms requested sampler interval; Python/GIL/IO pauses and sampling gaps can miss transient peaks; VmHWM is root-process kernel high-water only","RSS counts resident pages, not PSS; known-process RSS sums can double-count shared pages","build resources measure cgroup memory including page cache/other processes, not build process-tree RSS","single measured successful 100k run, simple one-function files and deterministic synthetic constant vectors; no statistical tail, realistic corpus or complete V20 claim"],"attempts":[]}
for name,prefix in [("attempt01",OUT/"attempt01"),("attempt02",OUT/"attempt02"),("attempt03",OUT)]:
    if not (prefix/"summary.json").exists() and not (prefix/"summary.json.gz").exists():continue
    summary=load_json(prefix/"summary.json");case=prefix/"live/n100000"
    result={"name":name,"status":summary["status"],"wall_seconds":summary["wall_seconds"],"failures":summary["failures"],"not_run":summary["not_run"],"phases":{},"runner_exit_code":0 if summary["status"]=="passed_declared_100k_local_scope" else 1}
    for resources in [case/"resources.jsonl.gz",case/"reopen/resources.jsonl.gz"]:
        if not resources.exists():continue
        previous=None
        with gzip.open(resources,"rt") as f:
            for line in f:
                row=json.loads(line);phase=row["phase"];phase=("reopen/" if resources.parent.name=="reopen" else "")+phase
                values=result["phases"].setdefault(phase,{"samples":0,"peak_product_root_rss_bytes":0,"peak_product_root_vmhwm_bytes":0,"peak_known_product_model_runner_sum_rss_bytes":0,"peak_cgroup_memory_current_bytes":0,"tree_coverage_error_samples":0,"maximum_sample_gap_ms":0})
                values["samples"]+=1
                for key,field in [("peak_product_root_rss_bytes","rss_bytes"),("peak_product_root_vmhwm_bytes","vmhwm_bytes")]:values[key]=max(values[key],(row.get("product") or {}).get(field) or 0)
                values["peak_known_product_model_runner_sum_rss_bytes"]=max(values["peak_known_product_model_runner_sum_rss_bytes"],sum((row.get(p) or {}).get("rss_bytes") or 0 for p in ["product","model","runner"]))
                values["peak_cgroup_memory_current_bytes"]=max(values["peak_cgroup_memory_current_bytes"],row["cgroup_memory_current"])
                if row.get("product_tree",{}).get("coverage_errors"):values["tree_coverage_error_samples"]+=1
                if previous is not None:values["maximum_sample_gap_ms"]=max(values["maximum_sample_gap_ms"],(row["time_ns"]-previous)/1e6)
                previous=row["time_ns"]
    http=case/"http.jsonl.gz";events=collections.Counter();items=collections.Counter()
    if http.exists():
        with gzip.open(http,"rt") as f:
            for line in f:
                row=json.loads(line);events[row["event"]]+=1;items[row["event"]]+=row["input_count"]
    result["mock_http_events"]=dict(events);result["mock_http_items"]=dict(items)
    result["peak_product_root_rss_bytes"]=max((x["peak_product_root_rss_bytes"] for x in result["phases"].values()),default=None)
    report["attempts"].append(result)
report["build_resources"]={"max_cgroup_memory_current":max(json.loads(line)["cgroup_memory_current"] for line in (OUT/"build-resources.jsonl").read_text().splitlines())}
(OUT/"resource-analysis.json").write_text(json.dumps(report,indent=2)+"\n")
print(json.dumps({"attempts":[{k:r[k] for k in ["name","status","wall_seconds","peak_product_root_rss_bytes","runner_exit_code"]} for r in report["attempts"]],"limitations":report["limitations"]},indent=2))
