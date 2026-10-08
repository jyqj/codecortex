#!/usr/bin/env python3
"""Read back one completed public V02 artifact; never invoke the candidate."""
import argparse,base64,hashlib,json,os,stat,zipfile
from pathlib import Path,PurePosixPath
PARTS=6
PIN={"artifact_id":11539125414,"artifact_name":"p8-joint-public-validation","zip_bytes":1146450,"zip_sha256":"7ea94b1f00b44d8d8151caaabc73f40cf7db57157c3ae5681f6972611e085983","run_id":37752879911,"workflow_head":"bb26deab45f6bc1bc5bbb79f5f3ce4a7f6d585d3","candidate_source":"2cd04485b0e0b23483f64671d56538bbaa47f442","conclusion":"success"}
def need(v,m):
    if not v: raise ValueError(m)
def sha(v): return hashlib.sha256(v).hexdigest()
def oid(v): return hashlib.sha1(b"blob "+str(len(v)).encode()+b"\0"+v).hexdigest()
def jbytes(v): return (json.dumps(v,sort_keys=True,indent=2)+"\n").encode()
def safe(n): return bool(n) and not PurePosixPath(n).is_absolute() and "\\" not in n and all(x not in ("",".","..") for x in n.split("/"))
p=argparse.ArgumentParser();p.add_argument("--part",type=int,choices=range(PARTS),required=True);a=p.parse_args()
root=Path(os.environ["READBACK_ROOT"])
meta_raw=(root/"artifact.json").read_bytes();run_raw=(root/"run.json").read_bytes()
meta=json.loads(meta_raw);run=json.loads(run_raw)
need(meta["id"]==PIN["artifact_id"] and meta["name"]==PIN["artifact_name"] and not meta["expired"],"artifact identity")
need(meta["workflow_run"]["id"]==PIN["run_id"] and meta["workflow_run"]["head_sha"]==PIN["workflow_head"],"artifact origin")
need(meta["size_in_bytes"]==PIN["zip_bytes"] and meta["digest"]=="sha256:"+PIN["zip_sha256"],"service ZIP identity")
need(run["id"]==PIN["run_id"] and run["head_sha"]==PIN["workflow_head"] and run["status"]=="completed" and run["conclusion"]=="success","terminal original run")
raw=(root/"original.zip").read_bytes()
need(len(raw)==PIN["zip_bytes"] and sha(raw)==PIN["zip_sha256"],"exact completed artifact ZIP")
prefix="artifacts/checkpoints/p8-joint-public-validation-20261008/actual/"+str(PIN["run_id"])+"/"
files={}
def add(n,v,origin):
    need(safe(n) and n not in files,"export path collision")
    files[n]=(v,origin)
add(prefix+"metadata/artifact.json",meta_raw,{"kind":"service_metadata"})
add(prefix+"metadata/run.json",run_raw,{"kind":"service_metadata"})
chunks=[]
for index,start in enumerate(range(0,len(raw),786432)):
    v=raw[start:start+786432];name=prefix+"original-archive/part-"+str(index).zfill(3)+".bin"
    add(name,v,{"kind":"lossless_original_zip_chunk","offset":start})
    chunks.append({"path":name,"offset":start,"bytes":len(v),"sha256":sha(v)})
add(prefix+"original-archive/manifest.json",jbytes({"original_zip_bytes":len(raw),"original_zip_sha256":sha(raw),"format":"Concatenate raw chunks in listed order","chunks":chunks}),{"kind":"original_zip_restoration_manifest"})
inventory=[];seen=set()
with zipfile.ZipFile(root/"original.zip") as z:
    need(0<len(z.infolist())<=4096,"bounded complete member count")
    need(sum(i.file_size for i in z.infolist())<=64*1024*1024,"bounded expanded public artifact")
    for i in sorted(z.infolist(),key=lambda x:x.filename):
        n=i.filename;mode=i.external_attr>>16
        need(safe(n) and n not in seen and not i.is_dir() and stat.S_IFMT(mode) in (0,stat.S_IFREG),"unique regular member")
        seen.add(n);v=z.read(i)
        need(len(v)==i.file_size,"member complete bytes")
        inventory.append({"path":n,"bytes":len(v),"sha256":sha(v),"git_blob":oid(v),"zip_mode":mode})
        add(prefix+"raw/"+n,v,{"kind":"exact_original_member","member":n})
need("result.json" in seen,"original result receipt present")
result=json.loads(files[prefix+"raw/result.json"][0])
need(result["source"]==PIN["candidate_source"] and result["status"]=="joint_original_and_canonical_public_validation_passed","actual result identity")
add(prefix+"metadata/original-member-inventory.json",jbytes({"pin":PIN,"file_count":len(inventory),"total_member_bytes":sum(r["bytes"] for r in inventory),"files":inventory}),{"kind":"observed_complete_original_inventory"})
records=[]
for name,(v,origin) in sorted(files.items()):
    encoded=base64.b64encode(v).decode("ascii")
    records.append({"path":name,"bytes":len(v),"sha256":sha(v),"git_blob":oid(v),"encoded_characters":len(encoded),"chunks":(len(encoded)+7999)//8000,**origin})
loads=[0]*PARTS
for record in sorted(records,key=lambda r:(-r["encoded_characters"],r["path"])):
    part=min(range(PARTS),key=lambda i:(loads[i],i));record["part"]=part;loads[part]+=record["encoded_characters"]
need(max(loads)<=6*1024*1024,"per-job readback log budget")
for r in records:
    if r["part"]!=a.part: continue
    encoded=base64.b64encode(files[r["path"]][0]).decode("ascii")
    print("P8_V02_FILE "+json.dumps(r),flush=True)
    for index,start in enumerate(range(0,len(encoded),8000)):
        print("P8_V02_CHUNK "+json.dumps({"path":r["path"],"index":index,"content":encoded[start:start+8000]}),flush=True)
print("P8_V02_MANIFEST "+json.dumps({"schema_version":1,"scope":"Lossless readback only of fixed completed public V02 artifact. No new product, ranking, validation or replay.","original_pin":PIN,"original_member_count":len(inventory),"original_member_bytes":sum(r["bytes"] for r in inventory),"files":records,"file_count":len(records),"total_bytes":sum(r["bytes"] for r in records),"part":a.part,"parts":PARTS,"part_encoded_characters":loads}),flush=True)
