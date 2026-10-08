#!/usr/bin/env python3
"""Inventory fixed original Actions bytes; do not build, execute queries or rescore."""
import argparse,base64,hashlib,json,stat,zipfile
from pathlib import Path,PurePosixPath
import os
root=Path(os.environ["READBACK_ROOT"])
parser=argparse.ArgumentParser(); parser.add_argument("--key",required=True,choices=("synthetic","prepare","rust-prepare","rust-synthetic")); args=parser.parse_args()
pin=json.loads((Path(__file__).parent/"inventory-pins.json").read_bytes())[args.key]
def need(v,m):
    if not v: raise ValueError(m)
def digest(v): return hashlib.sha256(v).hexdigest()
meta_raw=(root/"artifact.json").read_bytes()
run_raw=(root/"run.json").read_bytes()
meta=json.loads(meta_raw); run=json.loads(run_raw)
need(meta["id"]==pin["artifact_id"] and meta["name"]==pin["artifact_name"] and not meta["expired"],"artifact identity")
need(meta["workflow_run"]["id"]==pin["run_id"] and meta["workflow_run"]["head_sha"]==pin["workflow_head"],"artifact origin")
need(meta["size_in_bytes"]==pin["zip_bytes"] and meta["digest"]=="sha256:"+pin["zip_sha256"],"service ZIP identity")
need(run["id"]==pin["run_id"] and run["head_sha"]==pin["workflow_head"] and run["status"]=="completed" and run["conclusion"]==pin["conclusion"],"terminal original run")
raw=(root/"original.zip").read_bytes()
need(len(raw)==pin["zip_bytes"] and digest(raw)==pin["zip_sha256"],"original ZIP bytes")
records=[]; seen=set()
with zipfile.ZipFile(root/"original.zip") as z:
    need(len(z.infolist())<=10000,"member count")
    need(sum(i.file_size for i in z.infolist())<=500*1024*1024,"expanded byte bound")
    for i in sorted(z.infolist(),key=lambda x:x.filename):
        n=i.filename
        need(n and not PurePosixPath(n).is_absolute() and "\\" not in n and all(c not in ("",".","..") for c in n.split("/")),"member path")
        need(n not in seen and not i.is_dir() and not stat.S_ISLNK(i.external_attr>>16),"unique regular member")
        seen.add(n); v=z.read(i)
        need(len(v)==i.file_size,"actual member length")
        rec={"path":n,"bytes":len(v),"sha256":digest(v),"git_blob":hashlib.sha1(b"blob "+str(len(v)).encode()+b"\0"+v).hexdigest(),"zip_mode":i.external_attr>>16}
        records.append(rec)
        print("P8_RELEASE_MEMBER "+json.dumps(rec,sort_keys=True),flush=True)
        if n in ("result.json","ptv2-shared-rust-environment.json"):
            need(len(v)<=100000,"small metadata bound")
            print("P8_RELEASE_METADATA "+json.dumps({"path":n,"value":json.loads(v)},sort_keys=True),flush=True)
            encoded=base64.b64encode(v).decode("ascii")
            for index,start in enumerate(range(0,len(encoded),8000)):
                print("P8_RELEASE_METADATA_BYTES "+json.dumps({"path":n,"index":index,"content":encoded[start:start+8000]}),flush=True)
print("P8_RELEASE_INVENTORY "+json.dumps({"schema_version":1,"scope":"Original byte inventory only; no new ranking or replay","key":args.key,"pin":pin,"files":records,"file_count":len(records),"total_member_bytes":sum(r["bytes"] for r in records),"metadata":{"artifact_sha256":digest(meta_raw),"run_sha256":digest(run_raw)}} ,sort_keys=True),flush=True)
