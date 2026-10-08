#!/usr/bin/env python3
"""Read back four fixed completed preseal artifacts; no product or scorer execution."""
import argparse,base64,hashlib,json,os,stat,zipfile
from pathlib import Path,PurePosixPath
PARTS=16
p=argparse.ArgumentParser();p.add_argument("--part",type=int,choices=range(PARTS),required=True);a=p.parse_args()
root=Path(os.environ["READBACK_ROOT"])
observations=json.loads((Path(__file__).parent/"original-inventories.json").read_bytes())
def need(v,m):
    if not v: raise ValueError(m)
def sha(v):return hashlib.sha256(v).hexdigest()
def oid(v):return hashlib.sha1(b"blob "+str(len(v)).encode()+b"\0"+v).hexdigest()
def jb(v):return (json.dumps(v,sort_keys=True,indent=2)+"\n").encode()
def safe(n):return bool(n) and not PurePosixPath(n).is_absolute() and "\\" not in n and all(c not in ("",".","..") for c in n.split("/"))
files={};selection={}
def add(n,v,origin):
    need(safe(n) and n not in files,"unique export path")
    files[n]=(v,origin)
for key,observed in sorted(observations.items()):
    pin=observed["pin"];directory=root/key
    prefix="artifacts/checkpoints/p8-final-preseal-evidence-20261008/runs/37754844471/"+key+"/"
    meta_raw=(directory/"artifact.json").read_bytes();run_raw=(directory/"run.json").read_bytes()
    meta=json.loads(meta_raw);run=json.loads(run_raw)
    need(meta["id"]==pin["artifact_id"] and meta["name"]==pin["artifact_name"] and not meta["expired"],"artifact identity")
    need(meta["workflow_run"]["id"]==pin["run_id"] and meta["workflow_run"]["head_sha"]==pin["workflow_head"],"artifact origin")
    need(meta["size_in_bytes"]==pin["zip_bytes"] and meta["digest"]=="sha256:"+pin["zip_sha256"],"service ZIP digest")
    need(run["id"]==pin["run_id"] and run["head_sha"]==pin["workflow_head"] and run["status"]=="completed" and run["conclusion"]=="success","original terminal success")
    archive=(directory/"original.zip").read_bytes()
    need(len(archive)==pin["zip_bytes"] and sha(archive)==pin["zip_sha256"],"original archive bytes")
    add(prefix+"metadata/artifact.json",meta_raw,{"kind":"original_service_metadata","artifact":key})
    add(prefix+"metadata/run.json",run_raw,{"kind":"original_service_metadata","artifact":key})
    add(prefix+"metadata/original-member-inventory.json",jb(observed),{"kind":"prior_actual_original_inventory","artifact":key})
    chunks=[]
    for index,start in enumerate(range(0,len(archive),786432)):
        data=archive[start:start+786432];name=prefix+"original-archive/part-"+str(index).zfill(3)+".bin"
        add(name,data,{"kind":"lossless_original_zip_chunk","artifact":key,"offset":start})
        chunks.append({"path":name,"offset":start,"bytes":len(data),"sha256":sha(data)})
    add(prefix+"original-archive/manifest.json",jb({"original_pin":pin,"original_zip_bytes":len(archive),"original_zip_sha256":sha(archive),"chunks":chunks,"format":"Concatenate all raw chunks in this exact order"}),{"kind":"original_zip_restoration_manifest","artifact":key})
    expected={r["path"]:r for r in observed["files"]};seen=set();exported=[];omitted=[]
    with zipfile.ZipFile(directory/"original.zip") as z:
        need(len(z.infolist())==observed["file_count"] and sum(i.file_size for i in z.infolist())==observed["total_member_bytes"],"complete original members and bytes")
        witness={}
        if key=="synthetic":
            for arm in json.loads(z.read("synthetic-positive-witnesses.json")):
                for rep in arm["repetitions"]:
                    name="runs/"+arm["arm"]+"/"+rep["raw_path"]
                    need(name not in witness,"unique original positive witness")
                    witness[name]=rep["raw_sha256"]
            need(len(witness)==6,"all six original actual positive witness responses")
        diagnostics={"runs/"+arm+"/raw/000000.json" for arm in ("candidate_local_default","rg_baseline")}
        diagnostics|={"replay/"+arm+"-raw-drift-control/raw/000000.json" for arm in ("candidate_local_default","rg_baseline")}
        for info in sorted(z.infolist(),key=lambda i:i.filename):
            n=info.filename;mode=info.external_attr>>16
            need(safe(n) and n not in seen and not info.is_dir() and stat.S_IFMT(mode) in (0,stat.S_IFREG),"unique regular safe original member")
            seen.add(n);v=z.read(info)
            actual={"path":n,"bytes":len(v),"sha256":sha(v),"git_blob":oid(v),"zip_mode":mode}
            need(actual==expected[n],"original member differs from independent prior inventory")
            if n in witness:need(sha(v)==witness[n],"actual original positive witness hash")
            omit_binary=n in ("package/builds/evaluator/cc-eval","package/builds/product/codecortex","seal-controls/binary-copy/product")
            omit_raw="/raw/" in n and n not in witness and n not in diagnostics
            if omit_binary or omit_raw:
                omitted.append({"path":n,"reason":"binary_kept_in_complete_original_zip" if omit_binary else "raw_kept_in_complete_original_zip"})
            else:
                add(prefix+"raw/"+n,v,{"kind":"exact_original_member","artifact":key,"member":n});exported.append(n)
    need(seen==set(expected),"complete original member set")
    selection[key]={"pin":pin,"original_members":observed["file_count"],"original_bytes":observed["total_member_bytes"],"exported_members":exported,"not_duplicated_as_plain_files":omitted,"scope":"Every original byte remains in lossless ZIP chunks. All nonbinary nonraw files plus six actual positive raw witnesses and first raw/drift controls are duplicated for independent review."}
add("artifacts/checkpoints/p8-final-preseal-evidence-20261008/runs/37754844471/selection.json",jb(selection),{"kind":"complete_export_selection"})
records=[]
for name,(v,origin) in sorted(files.items()):
    encoded=base64.b64encode(v).decode("ascii")
    records.append({"path":name,"bytes":len(v),"sha256":sha(v),"git_blob":oid(v),"encoded_characters":len(encoded),"chunks":(len(encoded)+7999)//8000,**origin})
loads=[0]*PARTS
for r in sorted(records,key=lambda r:(-r["encoded_characters"],r["path"])):
    part=min(range(PARTS),key=lambda i:(loads[i],i));r["part"]=part;loads[part]+=r["encoded_characters"]
need(max(loads)<=6*1024*1024,"bounded per-job original readback")
for r in records:
    if r["part"]!=a.part:continue
    encoded=base64.b64encode(files[r["path"]][0]).decode("ascii")
    print("P8_RB_FILE "+json.dumps(r),flush=True)
    for index,start in enumerate(range(0,len(encoded),8000)):
        print("P8_RB_CHUNK "+json.dumps({"path":r["path"],"index":index,"content":encoded[start:start+8000]}),flush=True)
print("P8_RB_MANIFEST "+json.dumps({"schema_version":1,"scope":"Lossless fixed original preseal evidence only; no candidate execution, scoring or new query","original_pins":{k:v["pin"] for k,v in observations.items()},"files":records,"file_count":len(records),"total_bytes":sum(r["bytes"] for r in records),"part":a.part,"parts":PARTS,"part_encoded_characters":loads}),flush=True)
