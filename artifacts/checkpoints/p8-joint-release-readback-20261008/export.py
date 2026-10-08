#!/usr/bin/env python3
"""Lossless readback of one fixed original Actions ZIP and its metadata, no execution."""
import argparse,base64,hashlib,json,stat,zipfile,os
from pathlib import Path,PurePosixPath
p=argparse.ArgumentParser()
p.add_argument("--part",type=int,choices=range(12),required=True)
a=p.parse_args()
root=Path(os.environ["READBACK_ROOT"])
here=Path(__file__).parent
observed=json.loads((here/"original-inventory.json").read_bytes())
pin=observed["pin"]
prefix="artifacts/checkpoints/p8-release-evidence-20261008/runs/"+str(pin["run_id"])+"/"
def need(v,m):
    if not v: raise ValueError(m)
def sha(v): return hashlib.sha256(v).hexdigest()
def jbytes(v): return (json.dumps(v,sort_keys=True,indent=2)+"\n").encode()
def safe(n):
    return n and not PurePosixPath(n).is_absolute() and "\\" not in n and all(c not in ("",".","..") for c in n.split("/"))
meta_raw=(root/"artifact.json").read_bytes(); run_raw=(root/"run.json").read_bytes()
meta=json.loads(meta_raw); run=json.loads(run_raw)
need(meta["id"]==pin["artifact_id"] and meta["name"]==pin["artifact_name"] and not meta["expired"],"artifact identity")
need(meta["workflow_run"]["id"]==pin["run_id"] and meta["workflow_run"]["head_sha"]==pin["workflow_head"],"artifact origin")
need(meta["size_in_bytes"]==pin["zip_bytes"] and meta["digest"]=="sha256:"+pin["zip_sha256"],"service ZIP identity")
need(run["id"]==pin["run_id"] and run["head_sha"]==pin["workflow_head"] and run["status"]=="completed" and run["conclusion"]==pin["conclusion"],"terminal original run")
raw=(root/"original.zip").read_bytes()
need(len(raw)==pin["zip_bytes"] and sha(raw)==pin["zip_sha256"],"original ZIP bytes")
files={}
def add(path,v,origin):
    need(safe(path) and path not in files,"export path collision")
    files[path]=(v,origin)
add(prefix+"metadata/artifact.json",meta_raw,{"kind":"service_metadata"})
add(prefix+"metadata/run.json",run_raw,{"kind":"service_metadata"})
add(prefix+"metadata/original-member-inventory.json",jbytes(observed),{"kind":"verified_complete_original_inventory"})
chunks=[]
for index,start in enumerate(range(0,len(raw),786432)):
    data=raw[start:start+786432]
    name=prefix+"original-archive/part-"+str(index).zfill(3)+".bin"
    add(name,data,{"kind":"lossless_original_zip_chunk","offset":start})
    chunks.append({"path":name,"offset":start,"bytes":len(data),"sha256":sha(data)})
add(prefix+"original-archive/manifest.json",jbytes({"schema_version":1,"format":"Ordered raw ZIP byte chunks; concatenate exactly in this order","original_zip_bytes":len(raw),"original_zip_sha256":sha(raw),"chunks":chunks}),{"kind":"original_zip_restoration_manifest"})
expected={r["path"]:r for r in observed["files"]}; seen=set()
with zipfile.ZipFile(root/"original.zip") as z:
    need(len(z.infolist())==observed["file_count"],"complete original member count")
    need(sum(i.file_size for i in z.infolist())==observed["total_member_bytes"],"complete original byte count")
    for i in sorted(z.infolist(),key=lambda x:x.filename):
        n=i.filename
        need(safe(n) and n not in seen and not i.is_dir() and not stat.S_ISLNK(i.external_attr>>16),"unique regular member")
        seen.add(n); v=z.read(i)
        rec={"path":n,"bytes":len(v),"sha256":sha(v),"git_blob":hashlib.sha1(b"blob "+str(len(v)).encode()+b"\0"+v).hexdigest(),"zip_mode":i.external_attr>>16}
        need(rec==expected[n],"original member differs from prior actual inventory")
        if not n.startswith("external/external-reference-package/payload/") and n!="candidate-package/candidate-tools.zip":
            add(prefix+"raw/"+n,v,{"kind":"exact_original_member","member":n})
need(seen==set(expected),"complete member set")
records=[]
for name,(v,origin) in sorted(files.items()):
    encoded=base64.b64encode(v).decode("ascii")
    records.append({"path":name,"bytes":len(v),"sha256":sha(v),"git_blob":hashlib.sha1(b"blob "+str(len(v)).encode()+b"\0"+v).hexdigest(),"encoded_characters":len(encoded),"chunks":(len(encoded)+7999)//8000,**origin})
loads=[0]*12
for record in sorted(records,key=lambda r:(-r["encoded_characters"],r["path"])):
    part=min(range(12),key=lambda i:(loads[i],i))
    record["part"]=part
    loads[part]+=record["encoded_characters"]
need(max(loads)<=6*1024*1024,"per-job readback log budget")
for r in records:
    if r["part"]!=a.part: continue
    encoded=base64.b64encode(files[r["path"]][0]).decode("ascii")
    print("P8_RB_FILE "+json.dumps(r),flush=True)
    for index,start in enumerate(range(0,len(encoded),8000)):
        print("P8_RB_CHUNK "+json.dumps({"path":r["path"],"index":index,"content":encoded[start:start+8000]}),flush=True)
print("P8_RB_MANIFEST "+json.dumps({"schema_version":2,"scope":"Lossless original archive, exact metadata members and complete original hashes. No new candidate execution, ranking or replay.","original_pin":pin,"original_member_count":observed["file_count"],"original_member_bytes":observed["total_member_bytes"],"files":records,"file_count":len(records),"total_bytes":sum(r["bytes"] for r in records),"part":a.part,"parts":12,"part_encoded_characters":loads}),flush=True)
