#!/usr/bin/env python3
"""Export exact existing derived reports and body-free raw segments; never execute or rescore."""
import argparse,base64,hashlib,json,os,stat,zipfile
from pathlib import Path,PurePosixPath
p=argparse.ArgumentParser(); p.add_argument("--part",type=int,choices=range(6),required=True); a=p.parse_args()
root=Path(os.environ["READBACK_ROOT"]); pin=json.loads((Path(__file__).parent/"inventory-pins.json").read_bytes())
def need(v,m):
    if not v: raise ValueError(m)
def sha(v): return hashlib.sha256(v).hexdigest()
def safe(n): return n and not PurePosixPath(n).is_absolute() and "\\" not in n and all(c not in ("",".","..") for c in n.split("/"))
archive=(root/"original.zip").read_bytes()
need(len(archive)==pin["zip_bytes"] and sha(archive)==pin["zip_sha256"],"original ZIP identity")
prefix="artifacts/checkpoints/p8-release-evidence-20261008/runs/"+str(pin["run_id"])+"/"
files={}; subset=[]
def add(path,raw,origin):
    need(safe(path) and path not in files,"safe unique export")
    files[path]=(raw,origin)
with zipfile.ZipFile(root/"original.zip") as z:
    need(len(z.namelist())==len(set(z.namelist())),"unique member set")
    raw_index=z.read("external/external-reference-package/index.json")
    need(sha(raw_index)=="09d56a71921b8debd2c1740a941310f1c5d1e8709111676d7543bf550899e0d0","fixed original restoration index")
    index=json.loads(raw_index); need(index["source"]==pin["candidate_source"],"original candidate source")
    for record in index["original_files"]:
        path=record["path"]; need(safe(path),"original path")
        if "/raw/" not in path and path.endswith("queries.jsonl"):
            continue
        segments=record.get("segments")
        if "/raw/" not in path:
            need(isinstance(segments,list) and segments and all("payload_sha256" in s for s in segments),"report must not contain a corpus reference")
            chunks=[]
            for s in segments:
                info=z.getinfo("external/external-reference-package/payload/"+s["payload_sha256"])
                need(not info.is_dir() and not stat.S_ISLNK(info.external_attr>>16),"regular payload")
                raw=z.read(info); need(len(raw)==s["bytes"] and sha(raw)==s["payload_sha256"],"payload identity")
                chunks.append(raw)
            raw=b"".join(chunks); need(len(raw)==record["bytes"] and sha(raw)==record["sha256"],"exact original report")
            add(prefix+"derived-original/"+path,raw,{"kind":"exact_original_report_from_original_payload","original_path":path})
            subset.append(record)
        elif path.endswith(("/000000.json","/000001.json")):
            for number,s in enumerate(segments):
                if "payload_sha256" not in s: continue
                raw=z.read("external/external-reference-package/payload/"+s["payload_sha256"])
                need(len(raw)==s["bytes"] and sha(raw)==s["payload_sha256"],"original raw segment identity")
                add(prefix+"diagnostic-original-segments/"+path+"/segment-"+str(number).zfill(3)+".fragment",raw,
                    {"kind":"exact_original_body_free_fragment_not_complete_raw_response","original_path":path,"segment":number})
            subset.append(record)
    add(prefix+"derived-original/selection.json",(json.dumps({"scope":"All86 existing nonraw nonquery reports plus original body-free fragments of first2 raw responses/profile. No corpus insertion, new scoring or new query.","original_zip":pin,"index_sha256":sha(raw_index),"original_records":subset},indent=2,sort_keys=True)+"\n").encode(),{"kind":"selection_and_original_identity_map"})
records=[]
for path,(raw,origin) in sorted(files.items()):
    encoded=base64.b64encode(raw).decode()
    records.append({"path":path,"bytes":len(raw),"sha256":sha(raw),"git_blob":hashlib.sha1(b"blob "+str(len(raw)).encode()+b"\0"+raw).hexdigest(),"encoded_characters":len(encoded),"chunks":(len(encoded)+7999)//8000,**origin})
loads=[0]*6
for r in sorted(records,key=lambda r:(-r["encoded_characters"],r["path"])):
    part=min(range(6),key=lambda n:(loads[n],n));r["part"]=part;loads[part]+=r["encoded_characters"]
need(max(loads)<=6*1024*1024,"bounded readback log")
for r in records:
    if r["part"]!=a.part:continue
    encoded=base64.b64encode(files[r["path"]][0]).decode()
    print("P8_RB_FILE "+json.dumps(r),flush=True)
    for i,start in enumerate(range(0,len(encoded),8000)):
        print("P8_RB_CHUNK "+json.dumps({"path":r["path"],"index":i,"content":encoded[start:start+8000]}),flush=True)
print("P8_RB_MANIFEST "+json.dumps({"schema_version":3,"scope":"Exact original report/fragment readback only; no evaluation","original_pin":pin,"files":records,"file_count":len(records),"total_bytes":sum(r["bytes"] for r in records),"part":a.part,"parts":6,"part_encoded_characters":loads}),flush=True)
