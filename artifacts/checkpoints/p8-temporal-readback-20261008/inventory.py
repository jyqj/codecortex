#!/usr/bin/env python3
"""Read fixed completed pre-author artifacts without product execution or scoring."""
import base64,hashlib,json,os,stat,zipfile
from pathlib import Path,PurePosixPath
ROOT=Path(os.environ["READBACK_ROOT"])
MODE=os.environ["READBACK_MODE"]
PINS=json.loads((Path(__file__).parent/"pins.json").read_bytes())
PIN=PINS[MODE]
def need(value,message):
    if not value: raise ValueError(message)
def digest(data): return hashlib.sha256(data).hexdigest()
def record(name,data,mode=33188):
    return {"path":name,"bytes":len(data),"sha256":digest(data),"git_blob":hashlib.sha1(b"blob "+str(len(data)).encode()+b"\0"+data).hexdigest(),"zip_mode":mode}
def export(rec,data):
    encoded=base64.b64encode(data).decode()
    chunks=[encoded[i:i+32768] for i in range(0,len(encoded),32768)] or [""]
    print("P8_PRESEAL_FILE "+json.dumps({"mode":MODE,**rec,"chunks":len(chunks)},sort_keys=True),flush=True)
    for number,payload in enumerate(chunks):
        print("P8_PRESEAL_CHUNK "+json.dumps({"mode":MODE,"path":rec["path"],"number":number,"base64":payload},sort_keys=True),flush=True)
meta_raw=(ROOT/"artifact.json").read_bytes(); run_raw=(ROOT/"run.json").read_bytes()
meta=json.loads(meta_raw); run=json.loads(run_raw)
need(meta["id"]==PIN["artifact_id"] and meta["name"]==PIN["artifact_name"] and not meta["expired"],"artifact identity")
need(meta["workflow_run"]["id"]==PIN["run_id"] and meta["workflow_run"]["head_sha"]==PIN["workflow_head"],"artifact origin")
need(meta["size_in_bytes"]==PIN["zip_bytes"] and meta["digest"]=="sha256:"+PIN["zip_sha256"],"service ZIP identity")
need(run["id"]==PIN["run_id"] and run["head_sha"]==PIN["workflow_head"] and run["status"]=="completed" and run["conclusion"]=="success" and run["run_attempt"]==1,"terminal original run identity")
zip_path=ROOT/"original.zip"
need(zip_path.stat().st_size==PIN["zip_bytes"],"ZIP size")
hasher=hashlib.sha256()
with zip_path.open("rb") as f:
    while block:=f.read(1024*1024): hasher.update(block)
need(hasher.hexdigest()==PIN["zip_sha256"],"original ZIP digest")
records=[]; seen=set(); selected={}
select={"before.json","after.json","result.json","source-admission.json","source-lock.json"}
with zipfile.ZipFile(zip_path) as archive:
    entries=archive.infolist()
    summary={"scope":"ZIP central-directory metadata only; member content verification pending","mode":MODE,"file_count":len(entries),"total_member_bytes":sum(x.file_size for x in entries),"largest":[{"path":x.filename,"bytes":x.file_size,"compressed":x.compress_size} for x in sorted(entries,key=lambda x:x.file_size,reverse=True)[:20]],"prefix_counts":{}}
    for entry in entries:
        prefix=entry.filename.split("/")[0]
        summary["prefix_counts"][prefix]=summary["prefix_counts"].get(prefix,0)+1
    print("P8_PRESEAL_ZIP_METADATA "+json.dumps(summary,sort_keys=True),flush=True)
    raise SystemExit(0)
    for entry in sorted(entries,key=lambda x:x.filename):
        name=entry.filename
        need(name and not PurePosixPath(name).is_absolute() and "\\" not in name and all(c not in ("",".","..") for c in name.split("/")),"safe member path")
        need(name not in seen and not entry.is_dir() and not stat.S_ISLNK(entry.external_attr>>16),"unique regular member")
        seen.add(name); length=0; sha=hashlib.sha256(); git=hashlib.sha1(b"blob "+str(entry.file_size).encode()+b"\0"); parts=[]
        with archive.open(entry) as stream:
            while block:=stream.read(1024*1024):
                length+=len(block); sha.update(block); git.update(block)
                if name in select: parts.append(block)
        need(length==entry.file_size,"actual complete member length")
        rec={"path":name,"bytes":length,"sha256":sha.hexdigest(),"git_blob":git.hexdigest(),"zip_mode":entry.external_attr>>16}
        records.append(rec)
        if name in select: selected[name]=(rec,b"".join(parts))
need("result.json" in selected,"original result exists")
need(sum(len(value[1]) for value in selected.values())<=3*1024*1024,"bounded initial metadata export")
inventory={"schema_version":1,"scope":"Exact original pre-author bytes; no new query, replay, scoring or task acceptance","mode":MODE,"pin":PIN,"files":records,"file_count":len(records),"total_member_bytes":sum(x["bytes"] for x in records),"artifact_metadata":record("service/artifact.json",meta_raw),"run_metadata":record("service/run.json",run_raw)}
inventory_raw=(json.dumps(inventory,sort_keys=True,indent=2)+"\n").encode()
export(record("readback/original-inventory.json",inventory_raw),inventory_raw)
for name,(rec,data) in selected.items(): export(rec,data)
export(record("service/artifact.json",meta_raw),meta_raw)
export(record("service/run.json",run_raw),run_raw)
print("P8_PRESEAL_SUMMARY "+json.dumps({"mode":MODE,"file_count":len(records),"total_member_bytes":inventory["total_member_bytes"],"inventory_sha256":digest(inventory_raw),"initial_metadata_files":sorted(selected),"original_status":json.loads(selected["result.json"][1]).get("status"),"new_product_queries":0,"new_replays":0},sort_keys=True),flush=True)
