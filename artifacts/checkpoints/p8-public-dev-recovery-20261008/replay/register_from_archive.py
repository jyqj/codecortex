#!/usr/bin/env python3
import base64,hashlib,json,os,pathlib,stat,subprocess,zipfile
root=pathlib.Path.cwd()
tmp=pathlib.Path(os.environ["P8_REGISTER_ROOT"])
tmp.mkdir(parents=True,exist_ok=False)
downloads=pathlib.Path(os.environ["P8_REGISTER_DOWNLOADS"])
meta=json.loads((downloads/"artifact.json").read_bytes())
assert meta["id"]==11533204793 and meta["name"]=="p8-original-corpus-replay-and-projection"
assert not meta["expired"] and meta["size_in_bytes"]==1056926
assert meta["digest"]=="sha256:b21f52f43e1b93185377c61d93cab466dd971233c77101a54aa18f3a104733fe"
assert meta["workflow_run"]["id"]==37742080132 and meta["workflow_run"]["head_sha"]=="9a488ae727501fea1769cc5337fa1e987ae8883e"
def sha(raw):
    return hashlib.sha256(raw).hexdigest()
data=(downloads/"artifact.zip").read_bytes()
assert len(data)==1056926 and sha(data)=="b21f52f43e1b93185377c61d93cab466dd971233c77101a54aa18f3a104733fe"
unpacked=tmp/"restored"
unpacked.mkdir()
mode_fields={}
with zipfile.ZipFile(downloads/"artifact.zip") as z:
    infos=z.infolist()
    assert len(infos)==312 and len(set(i.filename for i in infos))==312
    assert sum(i.file_size for i in infos)==5307226
    for i in infos:
        p=pathlib.PurePosixPath(i.filename)
        assert not p.is_absolute() and "\\" not in i.filename and all(c not in ("",".","..") for c in i.filename.split("/"))
        assert not i.is_dir() and not stat.S_ISLNK(i.external_attr>>16)
        raw=z.read(i)
        assert len(raw)==i.file_size
        dest=unpacked/i.filename
        dest.parent.mkdir(parents=True,exist_ok=True)
        with dest.open("xb") as f:
            f.write(raw)
        dest.chmod(0o644)
        mode_fields[i.filename]=(i.external_attr>>16)&0o777
projection=unpacked/"reviewed"
original=unpacked/"original"
project_raw=(projection/"receipt.json").read_bytes()
assert sha(project_raw)=="6864e62107b8be888f3d91968d627f72c7088802241bbee5c68b91f79cf687df"
assert sha((original/"receipt.json").read_bytes())=="140e5a43874c5678f2b222e0d9e2dd30e34ffe3f1669ba305db6a15c53dfc2f1"
inventory=json.loads(project_raw)["canonical_product_inventory"]
mode_changes=[]
for item in inventory:
    name=item["path"]
    assert name.startswith("crates/cc-eval/benchmarks/public-v19/") and all(c not in ("",".","..") for c in name.split("/"))
    target=projection/name
    raw=target.read_bytes()
    assert len(raw)==item["bytes"] and sha(raw)==item["sha256"]
    mode=int(item["mode"],8)
    assert mode in (0o644,0o755)
    before=target.stat().st_mode&0o777
    target.chmod(mode)
    if before!=mode:
        mode_changes.append({"path":name,"sha256":sha(raw),"bytes":len(raw),"zip_mode_field":mode_fields["reviewed/"+name],"transport_copy_mode":oct(before),"restored_declared_mode":oct(mode)})
assert len(mode_changes)==3
public=tmp/"public"
public.mkdir()
(public/"mode-restoration.json").write_text(json.dumps({"source_artifact":11533204793,"original_zip_sha256":sha(data),"projection_receipt_sha256":sha(project_raw),"changes":mode_changes,"scope":"Restore declared original executable modes in a new copy using the fixed actual projection inventory. The original downloaded ZIP is unchanged; transport permissions are not claimed preserved."},indent=2)+"\n")
script=root/"artifacts/checkpoints/p8-public-dev-recovery-20261008/replay/register_reviewed_public_dev.py"
assert sha(script.read_bytes())=="bd7326e5c9e7b6439412c9ab142397b3377f1ba5f2975d5762571a8826fd0100"
command=["python3",str(script),"--projection-output",str(projection),"--original-output",str(original),"--projection-receipt-sha256",sha(project_raw),"--original-receipt-sha256",sha((original/"receipt.json").read_bytes()),"--output",str(public/"registration")]
done=subprocess.run(command,stdout=subprocess.PIPE,stderr=subprocess.PIPE)
(public/"registration.stdout").write_bytes(done.stdout)
(public/"registration.stderr").write_bytes(done.stderr)
(public/"command.json").write_text(json.dumps({"argv":command,"exit_code":done.returncode,"stdout_sha256":sha(done.stdout),"stderr_sha256":sha(done.stderr),"script_sha256":sha(script.read_bytes())},indent=2)+"\n")
print("P8_REGISTER_EXIT "+str(done.returncode),flush=True)
if done.returncode:
    print(done.stderr.decode("utf-8","replace")[-15000:],flush=True)
files=[]
for p in sorted(public.rglob("*")):
    if p.is_file():
        assert not p.is_symlink()
        raw=p.read_bytes()
        encoded=base64.b64encode(raw).decode()
        record={"path":p.relative_to(public).as_posix(),"bytes":len(raw),"sha256":sha(raw),"git_blob":hashlib.sha1(b"blob "+str(len(raw)).encode()+b"\0"+raw).hexdigest(),"chunks":(len(encoded)+7999)//8000}
        files.append(record)
        print("P8_REGISTER_FILE "+json.dumps(record),flush=True)
        for i,start in enumerate(range(0,len(encoded),8000)):
            print("P8_REGISTER_CHUNK "+json.dumps({"path":record["path"],"index":i,"content":encoded[start:start+8000]}),flush=True)
print("P8_REGISTER_MANIFEST "+json.dumps({"files":files,"file_count":len(files),"total_bytes":sum(f["bytes"] for f in files),"exit_code":done.returncode}),flush=True)
raise SystemExit(done.returncode)
