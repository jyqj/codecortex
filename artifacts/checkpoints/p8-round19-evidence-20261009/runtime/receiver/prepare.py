#!/usr/bin/env python3
"""Exact275e original runtime ZIP/source preparation; no product or native replay."""
import hashlib,json,os,pathlib,shutil,stat,subprocess,sys,time,zipfile
if sys.flags.optimize:
    raise SystemExit("optimized Python is forbidden")
sys.dont_write_bytecode=True
P=pathlib.Path
HERE=P(__file__).resolve().parent
REG=json.loads((HERE/"registration.json").read_text())
ROOT=P.cwd().resolve()
OWNED=ROOT/"artifacts/checkpoints/p8-round19-evidence-intake-20261009/runtime275e-author"
OUT=OWNED/"evidence"
RESERVE=512*1024**2
SOURCE="275e8799d4947d297329073eaa3ca675d3fd0777"
RUN=37890757030
CREATED=False

def require(ok,message):
    if not ok:
        raise ValueError(message)

def sha(path):
    h=hashlib.sha256()
    with P(path).open("rb") as stream:
        for data in iter(lambda:stream.read(1024*1024),b""):
            h.update(data)
    return h.hexdigest()

def save(path,value):
    path.parent.mkdir(parents=True,exist_ok=True)
    with path.open("x") as stream:
        json.dump(value,stream,sort_keys=True,indent=2)
        stream.write("\n")

def free():
    return shutil.disk_usage(ROOT).free

def owned(path):
    require(OUT in path.parents,"output outside owned evidence directory")
    parent=path.parent
    while not parent.exists():
        parent=parent.parent
    require(parent.resolve()==parent,"symlinked output ancestor")

def api(relative,path):
    require(relative.startswith("/repos/jyqj/codecortex/"),"foreign API path")
    owned(path)
    path.parent.mkdir(parents=True,exist_ok=True)
    with path.open("xb") as stream:
        result=subprocess.run(["gh","api",relative],stdout=stream,
            stderr=subprocess.DEVNULL,timeout=600,check=False)
    require(result.returncode==0,"original GitHub artifact/API transfer failed")
    return path

def members(archive):
    with zipfile.ZipFile(archive) as z:
        infos=z.infolist()
        require(len({i.filename for i in infos})==len(infos),"duplicate ZIP name")
        result={}
        for info in infos:
            q=pathlib.PurePosixPath(info.filename)
            require(not info.is_dir() and not q.is_absolute() and ".." not in q.parts
                and q.as_posix()==info.filename and "\\" not in info.filename,
                "unsafe ZIP member")
            require(stat.S_IFMT(info.external_attr>>16) in (0,stat.S_IFREG)
                and not info.flag_bits&1,"encrypted/nonregular ZIP member")
            h=hashlib.sha256(); count=0
            with z.open(info) as source:
                for block in iter(lambda:source.read(1024*1024),b""):
                    h.update(block); count+=len(block)
            require(count==info.file_size,"ZIP scanned size differs")
            result[info.filename]={"bytes":count,"sha256":h.hexdigest(),"crc32":"%08x"%info.CRC}
    return result

def expand(archive,dest,expected):
    owned(dest)
    dest.mkdir(parents=True,exist_ok=False)
    with zipfile.ZipFile(archive) as z:
        for name,row in expected.items():
            path=dest/name
            owned(path)
            path.parent.mkdir(parents=True,exist_ok=True)
            with z.open(name) as source,path.open("xb") as target:
                shutil.copyfileobj(source,target,1024*1024)
            require(path.stat().st_size==row["bytes"] and sha(path)==row["sha256"],"expanded bytes differ")
    return dest

def git(*args,cwd=ROOT):
    return subprocess.check_output(["git",*args],cwd=cwd)

def source_copy():
    """Own Git metadata and immutable files; shared object storage is read-only."""
    source=OUT/"source"
    source.mkdir()
    subprocess.run(["git","init","--quiet",str(source)],check=True,stdout=subprocess.DEVNULL,stderr=subprocess.DEVNULL)
    common=P(git("rev-parse","--git-common-dir").decode().strip())
    if not common.is_absolute():
        common=ROOT/common
    objects=(common/"objects").resolve(strict=True)
    (source/".git/objects/info/alternates").write_text(str(objects)+"\n")
    # This HEAD/index belongs only to the new receiver source directory.
    subprocess.run(["git","-C",str(source),"update-ref","HEAD",SOURCE],check=True)
    subprocess.run(["git","-C",str(source),"read-tree",SOURCE],check=True)
    selectors=["Cargo.toml","Cargo.lock","crates",".cargo","rust-toolchain","rust-toolchain.toml","scripts"]
    entries=git("ls-tree","-r","-z",SOURCE,"--",*selectors)
    manifest={}
    for entry in entries.split(b"\0"):
        if not entry: continue
        metadata,name=entry.split(b"\t",1)
        mode,kind,oid=metadata.decode().split()
        name=name.decode()
        require(kind=="blob" and mode in ("100644","100755"),"nonregular source input")
        path=source/name
        owned(path)
        path.parent.mkdir(parents=True,exist_ok=True)
        data=git("cat-file","blob",oid)
        actual=hashlib.sha1(b"blob "+str(len(data)).encode()+b"\0"+data).hexdigest()
        require(actual==oid,"source Git blob bytes differ")
        with path.open("xb") as stream: stream.write(data)
        path.chmod(0o755 if mode=="100755" else 0o644)
        manifest[name]={"git_blob":oid,"git_mode":mode,"bytes":len(data),"sha256":hashlib.sha256(data).hexdigest()}
    require(git("rev-parse","HEAD",cwd=source).decode().strip()==SOURCE,"owned source head differs")
    require(git("rev-parse","HEAD^{tree}",cwd=source).decode().strip()==REG["source_tree"],"owned source tree differs")
    save(OUT/"source-copy.json",manifest)
    return source

def controls():
    require(HERE==OWNED and HERE.resolve()==HERE,"wrong owned controller root")
    require(REG["source"]==SOURCE and REG["run"]==RUN and REG["attempt"]==1,"wrong registration")
    found={}
    for name,pin in REG["control_files"].items():
        path=HERE/name
        require(path.is_file() and not path.is_symlink()
            and path.stat().st_size==pin["bytes"] and sha(path)==pin["sha256"],"control input differs")
        found[name]=pin
    return found

def run():
    global CREATED
    before=controls()
    require(not OUT.exists() and OUT.parent.resolve()==OUT.parent,"evidence exists or noncanonical")
    require(free()>=RESERVE+sum(x["bytes"] for x in REG["artifacts"])+128*1024**2,"not_run: insufficient ZIP reserve")
    OUT.mkdir()
    CREATED=True
    save(OUT/"registration.json",REG)
    r=json.loads(api("/repos/jyqj/codecortex/actions/runs/%d"%RUN,OUT/"api/run.json").read_text())
    require(r["id"]==RUN and r["head_sha"]==SOURCE and r["run_attempt"]==1
        and r["status"]=="completed","original run not exact terminal attempt")
    js=json.loads(api("/repos/jyqj/codecortex/actions/runs/%d/jobs?per_page=100&page=1"%RUN,OUT/"api/jobs.json").read_text())
    ars=json.loads(api("/repos/jyqj/codecortex/actions/runs/%d/artifacts?per_page=100&page=1"%RUN,OUT/"api/artifacts.json").read_text())
    require(js["total_count"]==len(js["jobs"])==6 and len({j["id"] for j in js["jobs"]})==6,"incomplete original jobs")
    require({j["id"] for j in js["jobs"]}==set(REG["job_ids"]) and all(j["status"]=="completed" for j in js["jobs"]),"original job identity or terminal differs")
    require(ars["total_count"]==len(ars["artifacts"])==6,"incomplete original artifacts")
    actual={a["id"]:a for a in ars["artifacts"]}
    require(len(actual)==6 and set(actual)=={a["id"] for a in REG["artifacts"]},"original artifact set differs")
    scans=[]
    for spec in REG["artifacts"]:
        a=actual[spec["id"]]
        require(a["name"]==spec["name"] and a["size_in_bytes"]==spec["bytes"]
            and a["digest"]=="sha256:"+spec["sha256"] and not a["expired"]
            and a["workflow_run"]["id"]==RUN and a["workflow_run"]["head_sha"]==SOURCE,"original artifact metadata differs")
        archive=OUT/"raw"/str(spec["id"])/"original.zip"
        require(free()>=RESERVE+spec["bytes"],"not_run: ZIP reserve")
        api("/repos/jyqj/codecortex/actions/artifacts/%d/zip"%spec["id"],archive)
        require(archive.stat().st_size==spec["bytes"] and sha(archive)==spec["sha256"],"original ZIP bytes differ")
        scanned=members(archive)
        save(OUT/"inventories"/("%d.json"%spec["id"]),scanned)
        scans.append((spec,archive,scanned))
    expanded=sum(sum(v["bytes"] for v in m.values()) for _,_,m in scans)
    save(OUT/"capacity.json",{"actual_free_bytes":free(),"expanded_bytes":expanded,"reserve_bytes":RESERVE,"source_and_output_allowance":128*1024**2})
    require(free()>=RESERVE+expanded+128*1024**2,"not_run: expansion reserve")
    for spec,archive,m in scans:
        expand(archive,archive.parent/"extracted",m)
    source=source_copy()
    require(controls()==before,"control files changed")
    for spec,archive,m in scans:
        require(archive.stat().st_size==spec["bytes"] and sha(archive)==spec["sha256"],"original ZIP changed")
        for name,row in m.items():
            p=archive.parent/"extracted"/name
            require(p.stat().st_size==row["bytes"] and sha(p)==row["sha256"],"original member changed")
    return {"status":"prepared_complete_original_bytes_not_semantic_acceptance","source":SOURCE,"run":RUN,
        "ZIP_count":6,"ZIP_bytes":sum(a["bytes"] for a in REG["artifacts"]),"expanded_bytes":expanded,
        "source_path":str(source),"raw_path":str(OUT/"raw"),"control_files":before,
        "no_product_or_Cargo_or_statistics_execution":True,"TODO_closed":0,"TODO_remaining":29}

if __name__=="__main__":
    result={"status":"incomplete_not_certified","source":SOURCE,"run":RUN}
    code=2
    try:
        result=run();code=0
    except BaseException as error:
        result.update(error_type=type(error).__name__,error=str(error) if isinstance(error,ValueError) else "bounded original transport/source preparation failure")
    finally:
        result["exit_code"]=code
        if CREATED: save(OUT/"preparation.json",result)
        print(json.dumps(result,sort_keys=True))
    raise SystemExit(code)
