#!/usr/bin/env python3
"""Bundle frozen public Round 31 evidence without altering original inputs."""
import datetime,gzip,hashlib,io,json,pathlib,re,stat,tarfile
ROOT=pathlib.Path("/workspace/scratch/a217aaae3bde")
OUT=ROOT/"p8-db-lock-observation-integration"
cfg=json.loads((OUT/"round31-public-bundle-inputs.json").read_text())
receipt=json.loads((ROOT/cfg["source_admission_receipt"]).read_text())
assert receipt["source_before"]==cfg["diagnostic_G"] and receipt["source_after"]==cfg["diagnostic_G"]
assert receipt["all_input_bytes_modes_unchanged"] and isinstance(receipt["returncode"],int)
files=cfg["files"]
def sha(raw): return hashlib.sha256(raw).hexdigest()
def oid(raw): return hashlib.sha1(b"blob "+str(len(raw)).encode()+b"\0"+raw).hexdigest()
records=[];payloads={};protected={}
for declared in sorted(files,key=lambda x:x["target"]):
    rel=pathlib.PurePosixPath(declared["source"]);target=pathlib.PurePosixPath(declared["target"])
    assert not rel.is_absolute() and ".." not in rel.parts and not target.is_absolute() and ".." not in target.parts
    assert not any("private-transfer" in part for part in rel.parts),"private transfer source selected"
    source=ROOT/pathlib.Path(rel)
    for candidate in [source,*source.parents]:
        if candidate==ROOT.parent:break
        assert not candidate.is_symlink(),str(candidate)
    assert source.is_file() and str(target) not in payloads
    st=source.stat();raw=source.read_bytes();digest=sha(raw);blob=oid(raw)
    if "source_mode" in declared:assert stat.S_IMODE(st.st_mode)==int(declared["source_mode"],8),(str(rel),"declared source mode changed")
    git_mode="100755" if st.st_mode&0o111 else "100644"
    for key,actual in [("bytes",len(raw)),("sha256",digest),("git_blob",blob)]:
        if key in declared:assert declared[key]==actual,(str(rel),key)
    if "mode" in declared:
        expected=declared["mode"]
        if isinstance(expected,int):expected="100755" if expected&0o111 else "100644"
        assert expected==git_mode,(str(rel),"mode")
    if not str(target).endswith(".tar.gz"):
        checks={"private_file_identifier":len(re.findall(rb"file[-_][A-Za-z0-9]{20,}",raw)),"private_library_uri":len(re.findall(rb"library://[A-Za-z0-9]",raw)),"signed_blob_transfer":len(re.findall(rb"[?&]sig=[A-Za-z0-9%+/=]{16,}",raw))}
        assert not any(checks.values()),(str(rel),"unexpected private marker",checks)
    else:checks={"opaque_container_bound_to_preverified_sha256":True}
    record={"source":str(rel),"target":str(target),"bytes":len(raw),"sha256":digest,"git_blob":blob,"git_mode":git_mode,"source_mode":oct(stat.S_IMODE(st.st_mode)),"tar_mode":stat.S_IMODE(st.st_mode),"public_reference_scan":checks}
    records.append(record);payloads[str(target)]=raw;protected[str(rel)]=(digest,st.st_ino,st.st_size,stat.S_IMODE(st.st_mode))
bundle=OUT/"round31-public-evidence.tar.gz"
assert not bundle.exists(),"immutable bundle already exists"
with bundle.open("xb") as dst:
    with gzip.GzipFile(fileobj=dst,mode="wb",filename="",mtime=0,compresslevel=9) as compressed:
        with tarfile.open(fileobj=compressed,mode="w",format=tarfile.USTAR_FORMAT) as tar:
            for record in records:
                info=tarfile.TarInfo(record["target"]);info.type=tarfile.REGTYPE;info.size=record["bytes"];info.mode=record["tar_mode"];info.mtime=0;info.uid=0;info.gid=0;info.uname="";info.gname=""
                tar.addfile(info,io.BytesIO(payloads[record["target"]]))
members=[]
with tarfile.open(bundle,"r:gz") as tar:
    infos=tar.getmembers();assert [m.name for m in infos]==[r["target"] for r in records]
    for info,record in zip(infos,records):
        assert info.isfile() and info.mode==record["tar_mode"] and info.size==record["bytes"] and info.uid==0 and info.gid==0 and info.mtime==0
        raw=tar.extractfile(info).read()
        assert raw==payloads[info.name] and sha(raw)==record["sha256"] and oid(raw)==record["git_blob"]
        members.append({"target":info.name,"bytes":len(raw),"sha256":sha(raw),"git_blob":oid(raw),"mode":info.mode})
for path,before in protected.items():
    src=ROOT/path;st=src.stat()
    assert (sha(src.read_bytes()),st.st_ino,st.st_size,stat.S_IMODE(st.st_mode))==before,path
bundle_raw=bundle.read_bytes()
inventory={"schema":"p8-round31-public-evidence-inventory-v1","created_at":datetime.datetime.now(datetime.timezone.utc).isoformat(),"source_C":cfg["source_C"],"diagnostic_G":cfg["diagnostic_G"],"task_status":cfg["task_status"],"files":records,"file_count":len(records),"total_uncompressed_payload_bytes":sum(r["bytes"] for r in records),"container":{"filename":bundle.name,"bytes":len(bundle_raw),"sha256":sha(bundle_raw),"git_blob":oid(bundle_raw),"format":"USTAR + gzip; sorted targets, fixed zero ownership/timestamps, original regular-file modes"},"exclusions":cfg["excluded"],"native_reexecution":False,"original_inputs_unchanged":True,"source_admission_returncode":receipt["returncode"],"scope":"Public original logs, recorded acceptance executions and independent reviews; original GitHub ZIPs remain at their original immutable artifact references."}
inventory_raw=(json.dumps(inventory,indent=2,sort_keys=True)+"\n").encode()
(OUT/"round31-public-evidence-inventory.json").write_bytes(inventory_raw)
readback={"schema":"p8-round31-public-evidence-archive-readback-v1","status":"passed_exact_member_bytes_modes_and_source_preservation","container":inventory["container"],"inventory_sha256":sha(inventory_raw),"member_count":len(members),"members":members,"protected_original_files":len(protected),"original_inputs_unchanged":True,"disk_extraction_performed":False,"native_or_helper_reexecution":False,"todo_closures":0,"todo_remaining":29}
raw=(json.dumps(readback,indent=2,sort_keys=True)+"\n").encode();(OUT/"round31-public-evidence-readback.json").write_bytes(raw)
print(json.dumps({"status":readback["status"],"file_count":len(records),"payload_bytes":inventory["total_uncompressed_payload_bytes"],"container":inventory["container"],"inventory_bytes":len(inventory_raw),"inventory_sha256":sha(inventory_raw),"readback_bytes":len(raw),"readback_sha256":sha(raw),"source_admission_returncode":receipt["returncode"]}))

