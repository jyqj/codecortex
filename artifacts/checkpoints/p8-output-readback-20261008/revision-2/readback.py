#!/usr/bin/env python3
"""Read fixed Actions archives only; never rebuild, requery, or alter results."""
import argparse,base64,hashlib,json,stat,zipfile
from pathlib import Path,PurePosixPath

def need(value,message):
    if not value:
        raise ValueError(message)
def sha(raw):
    return hashlib.sha256(raw).hexdigest()
def jsonbytes(value):
    return (json.dumps(value,sort_keys=True,indent=2,ensure_ascii=True)+"\n").encode()

p=argparse.ArgumentParser()
p.add_argument("--input",type=Path,required=True)
p.add_argument("--pins",type=Path,required=True)
p.add_argument("--part",type=int,required=True,choices=range(8))
a=p.parse_args()
pins=json.loads(a.pins.read_bytes())
need(pins["schema_version"]==1 and pins["parts"]==8,"pin schema")
files={}
omitted=[]
archives=[]
total=0
def add(path,raw,origin):
    global total
    q=PurePosixPath(path)
    need(path and not q.is_absolute() and "\\" not in path
         and all(c not in ("",".","..") for c in path.split("/")),"unsafe output path")
    need(path not in files,"duplicate output path")
    total+=len(raw)
    need(total<=50*1024*1024,"expanded aggregate bound")
    files[path]=(raw,origin)
for pin in pins["artifacts"]:
    label=str(pin["id"])
    meta_raw=(a.input/(label+".json")).read_bytes()
    meta=json.loads(meta_raw)
    need(meta["id"]==pin["id"] and meta["name"]==pin["name"]
         and not meta["expired"],"artifact identity")
    need(meta["workflow_run"]["id"]==pin["run"]
         and meta["workflow_run"]["head_sha"]==pin["head"],"artifact source")
    need(meta["size_in_bytes"]==pin["bytes"]
         and meta["digest"]=="sha256:"+pin["sha256"],"artifact metadata digest")
    run_raw=(a.input/(label+"-run.json")).read_bytes()
    run=json.loads(run_raw)
    need(run["id"]==pin["run"] and run["head_sha"]==pin["head"]
         and run["status"]=="completed"
         and run["conclusion"]==pin["conclusion"],"actual terminal run")
    data=(a.input/(label+".zip")).read_bytes()
    need(len(data)==pin["bytes"] and sha(data)==pin["sha256"],"original ZIP bytes")
    prefix=pin["prefix"]
    add(prefix+"metadata/artifact.json",meta_raw,{"kind":"service_metadata","artifact":pin["id"]})
    add(prefix+"metadata/run.json",run_raw,{"kind":"service_metadata","artifact":pin["id"]})
    if pin["retain_zip"]:
        need(len(data)<=2*1024*1024,"retained archive size")
        add(prefix+"original-artifact.zip",data,{"kind":"original_zip","artifact":pin["id"]})
    seen=set()
    member_count=0
    member_bytes=0
    exclusions=pin.get("binary_members",{})
    observed_exclusions=set()
    with zipfile.ZipFile(a.input/(label+".zip")) as z:
        need(len(z.infolist())<=2000,"member count bound")
        need(sum(i.file_size for i in z.infolist())<=100*1024*1024,"expanded ZIP bound")
        for info in sorted(z.infolist(),key=lambda i:i.filename):
            name=info.filename
            need(name and not PurePosixPath(name).is_absolute() and "\\" not in name
                 and all(c not in ("",".","..") for c in name.split("/")),"unsafe ZIP member")
            need(name not in seen and not info.is_dir()
                 and not stat.S_ISLNK(info.external_attr>>16),"regular unique member")
            seen.add(name)
            raw=z.read(info)
            need(len(raw)==info.file_size,"member byte length")
            origin={"kind":"original_member","artifact":pin["id"],"member":name}
            member_count+=1
            member_bytes+=len(raw)
            if name in exclusions:
                need({"bytes":len(raw),"sha256":sha(raw)}==exclusions[name],"omitted binary pin")
                observed_exclusions.add(name)
                omitted.append({**origin,**exclusions[name],"retention":"Exact full original Actions ZIP, not reencoded into Git"})
            else:
                add(prefix+"raw/"+name,raw,origin)
    need(observed_exclusions==set(exclusions),"binary exclusion inventory")
    archives.append({**pin,"actual_member_count":member_count,"actual_member_bytes":member_bytes})
records=[]
for path,(raw,origin) in sorted(files.items()):
    encoded=base64.b64encode(raw).decode("ascii")
    records.append({"path":path,"bytes":len(raw),"sha256":sha(raw),
      "git_blob":hashlib.sha1(b"blob "+str(len(raw)).encode()+b"\0"+raw).hexdigest(),
      "encoded_characters":len(encoded),"chunks":(len(encoded)+7999)//8000,**origin})
loads=[0]*8
for record in sorted(records,key=lambda r:(-r["encoded_characters"],r["path"])):
    part=min(range(8),key=lambda n:(loads[n],n))
    record["part"]=part
    loads[part]+=record["encoded_characters"]
need(max(loads)<=6*1024*1024,"per-part log bound")
manifest={"schema_version":1,"scope":"Exact Actions archive and member readback, no evaluation",
 "archives":archives,"files":records,"file_count":len(records),"total_bytes":total,
 "omitted_binary_bodies":omitted,"part":a.part,"parts":8,"part_encoded_characters":loads}
for r in records:
    if r["part"]!=a.part:
        continue
    encoded=base64.b64encode(files[r["path"]][0]).decode("ascii")
    print("P8_RB_FILE "+json.dumps(r,ensure_ascii=True),flush=True)
    for i,start in enumerate(range(0,len(encoded),8000)):
        print("P8_RB_CHUNK "+json.dumps({"path":r["path"],"index":i,
              "content":encoded[start:start+8000]},ensure_ascii=True),flush=True)
print("P8_RB_MANIFEST "+json.dumps(manifest,ensure_ascii=True),flush=True)
