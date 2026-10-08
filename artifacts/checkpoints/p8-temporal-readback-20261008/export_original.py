#!/usr/bin/env python3
"""Read fixed completed pre-author artifacts without product execution or scoring."""
import argparse,base64,gzip,hashlib,json,os,stat,zipfile
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

parser=argparse.ArgumentParser(); parser.add_argument("--part",type=int,required=True); args=parser.parse_args()
need(0<=args.part<8 and MODE=="synthetic-control","fixed readback part and original mode")
def jb(value): return (json.dumps(value,sort_keys=True,ensure_ascii=False,indent=2)+"\n").encode()
records=[]; seen=set(); original={}; runtime=[]; reused_package=[]; mutation=None
with zipfile.ZipFile(zip_path) as archive:
    entries=archive.infolist()
    need(len(entries)==53199 and sum(x.file_size for x in entries)==1395874461,"independently observed exact original archive closure")
    original_product=archive.read("package/builds/product/codecortex")
    need(digest(original_product)=="f9337b0347727a31a7b147497763b9c0765a725f207c6245b3715cd4749f79f0","pinned original product bytes for copy-control reconstruction")
    for entry in sorted(entries,key=lambda x:x.filename):
        name=entry.filename
        need(name and not PurePosixPath(name).is_absolute() and "\\" not in name and all(c not in ("",".","..") for c in name.split("/")),"safe member path")
        need(name not in seen and not entry.is_dir() and not stat.S_ISLNK(entry.external_attr>>16),"unique regular member")
        seen.add(name); length=0; sha=hashlib.sha256(); git=hashlib.sha1(b"blob "+str(entry.file_size).encode()+b"\0")
        keep=not name.startswith(("runtime-home/","package/")); parts=[]
        with archive.open(entry) as stream:
            while block:=stream.read(1024*1024):
                length+=len(block); sha.update(block); git.update(block)
                if keep: parts.append(block)
        need(length==entry.file_size,"complete original member")
        rec={"path":name,"bytes":length,"sha256":sha.hexdigest(),"git_blob":git.hexdigest(),"zip_mode":entry.external_attr>>16}
        records.append(rec)
        if name.startswith("runtime-home/"): runtime.append(rec)
        elif name.startswith("package/"): reused_package.append(rec)
        elif name=="seal-controls/binary-copy/product":
            data=b"".join(parts)
            need(data.startswith(original_product) and len(data)-len(original_product)==24,"actual original copied-binary mutation shape")
            mutation={"original_record":rec,"base_member":"builds/product/codecortex","base_sha256":digest(original_product),"base_bytes":len(original_product),"append_base64":base64.b64encode(data[len(original_product):]).decode(),"method":"Actual complete byte prefix compared to original packaged binary; exact24-byte suffix retained. No rewritten original evidence."}
        else: original[name]=b"".join(parts)
need(len(runtime)==52074 and len(reused_package)==30 and mutation is not None and len(original)==1094,"complete original member partition")
runtime_raw=jb({"schema_version":1,"scope":"Every actual runtime-home member was fully read and hashed; cached file bodies are not separately copied into Git by this readback","files":runtime,"file_count":len(runtime)})
compressed=gzip.compress(runtime_raw,mtime=0)
compressed_parts=[]
for index,start in enumerate(range(0,len(compressed),786432)):
    name="readback/runtime-member-index.json.gz.part-"+str(index).zfill(3)
    data=compressed[start:start+786432]; original[name]=data; compressed_parts.append(record(name,data))
runtime_recipe={"raw_bytes":len(runtime_raw),"raw_sha256":digest(runtime_raw),"gzip_bytes":len(compressed),"gzip_sha256":digest(compressed),"parts":compressed_parts,"file_count":len(runtime),"body_retention":"Original complete artifact remains at the fixed Actions artifact ID/SHA. This Git readback preserves the complete content inventory, not the installed Rust cache/docs bodies."}
non_runtime=[r for r in records if not r["path"].startswith("runtime-home/")]
index={"schema_version":1,"mode":MODE,"pin":PIN,"file_count":len(records),"total_member_bytes":sum(x["bytes"] for x in records),"actual_complete_member_hashing":True,"non_runtime_files":non_runtime,"runtime_inventory":runtime_recipe}
original["readback/original-member-index.json"]=jb(index)
original["readback/package-and-copy-reconstruction.json"]=jb({"schema_version":1,"exact_joint_package":{"source":"2cd04485b0e0b23483f64671d56538bbaa47f442","manifest_sha256":"1dce263321c34dc60e69132d7e386ffa90c9d0436c1be5759efd7fefc957958b","archive_sha256":"acb16c6c8b34dc513569d4667b0dff721713f791eb7c7752791ad2430ed438f3","archive_bytes":18801283,"original_release_run":37749276827},"reused_package_members":reused_package,"copied_binary_control":mutation})
original["service/artifact.json"]=meta_raw; original["service/run.json"]=run_raw
expected={r["path"]:r for r in non_runtime}
files=[]
for name,data in sorted(original.items()):
    rec=record(name,data,expected.get(name,{}).get("zip_mode",33188))
    if name in expected: need(rec==expected[name],"export original member drift")
    files.append(rec)
bins=[[] for _ in range(8)]; totals=[0]*8
for rec in sorted(files,key=lambda r:(-r["bytes"],r["path"])):
    part=min(range(8),key=lambda i:(totals[i],i)); bins[part].append(rec); totals[part]+=rec["bytes"]
manifest={"schema_version":1,"scope":"Exact original synthetic measurement/control evidence and complete member content inventory; no new query or replay. Runtime cache bodies retained only in original service artifact; packaged binaries referenced losslessly to separately preserved exact joint package.","pin":PIN,"original_member_count":len(records),"original_member_bytes":sum(x["bytes"] for x in records),"export_files":len(files),"export_bytes":sum(r["bytes"] for r in files),"runtime_recipe":runtime_recipe,"parts":totals,"files":[{**r,"part":part} for part,items in enumerate(bins) for r in sorted(items,key=lambda x:x["path"])]}
manifest_raw=jb(manifest)
if args.part==0: export(record("readback/export-manifest.json",manifest_raw),manifest_raw)
for rec in sorted(bins[args.part],key=lambda r:r["path"]): export(rec,original[rec["path"]])
print("P8_PRESEAL_EXPORT_SUMMARY "+json.dumps({"part":args.part,"export_files":len(files),"export_bytes":sum(totals),"part_files":len(bins[args.part]),"part_bytes":totals[args.part],"manifest_sha256":digest(manifest_raw),"original_member_count":len(records),"original_member_bytes":sum(r["bytes"] for r in records),"new_queries":0,"new_replays":0},sort_keys=True),flush=True)
