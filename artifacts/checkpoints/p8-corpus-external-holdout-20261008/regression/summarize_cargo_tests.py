#!/usr/bin/env python3
import hashlib,json,pathlib,re,sys
base=pathlib.Path(__file__).parent
name=sys.argv[1]
receipt_path=base/(name+".json")
log_path=base/(name+".log")
output=base/(name+"-summary.json")
if output.exists():raise SystemExit("refuse overwrite")
receipt=json.loads(receipt_path.read_text()); log=log_path.read_text()
marks=list(re.finditer(r"^\s+Running (.+?) \((/[^\n)]+)\)\s*$",log,re.M)); targets=[]
for i,m in enumerate(marks):
    section=log[m.end():marks[i+1].start() if i+1<len(marks) else len(log)]
    results=list(re.finditer(r"^test result: (ok|FAILED)\. (\d+) passed; (\d+) failed; (\d+) ignored; (\d+) measured; (\d+) filtered out; finished in ([0-9.]+)s$",section,re.M))
    if len(results)!=1:raise RuntimeError((m.group(1),len(results)))
    r=results[0]
    row={"target":m.group(1),"executable":m.group(2),"status":r.group(1),"passed":int(r.group(2)),"failed":int(r.group(3)),"ignored":int(r.group(4)),"measured":int(r.group(5)),"filtered_out":int(r.group(6)),"elapsed_seconds":float(r.group(7)),"failed_ids":re.findall(r"^test (.+?) \.\.\. FAILED$",section,re.M),"ignored_tests":[{"id":a,"reason":b} for a,b in re.findall(r"^test (.+?) \.\.\. ignored(?:, (.*))?$",section,re.M)]}
    if len(row["failed_ids"])!=row["failed"] or len(row["ignored_tests"])!=row["ignored"]:raise RuntimeError("count mismatch "+row["target"])
    targets.append(row)
if not targets:raise RuntimeError("no completed targets")
result={"source_commit":receipt["source_identity"]["head"],"argv":receipt["argv"],"exit_code":receipt["exit_code"],"original_log_sha256":hashlib.sha256(log_path.read_bytes()).hexdigest(),"command_receipt_sha256":hashlib.sha256(receipt_path.read_bytes()).hexdigest(),"source_unchanged":receipt["source_unchanged"],"target_count":len(targets),"totals":{k:sum(x[k] for x in targets) for k in ["passed","failed","ignored","measured","filtered_out"]},"failures":[{"target":x["target"],"id":id} for x in targets for id in x["failed_ids"]],"scope":"every selected target result from this complete command, no ignored or filtered tests counted as passing","targets":targets,"observer_sha256":hashlib.sha256(pathlib.Path(__file__).read_bytes()).hexdigest()}
output.write_text(json.dumps(result,indent=2)+"\n")
print(json.dumps({k:result[k] for k in ["target_count","totals","failures"]})); print("summary_sha256",hashlib.sha256(output.read_bytes()).hexdigest())
