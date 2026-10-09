#!/usr/bin/env python3
"""Receive exact PR173 platform artifacts and replay its unchanged cold collector.
Transport and private source preparation only; no Cargo, builds or product execution.
"""
import hashlib,json,os,pathlib,shutil,stat,subprocess,sys,time,zipfile
if sys.flags.optimize:
    raise SystemExit("optimized Python is forbidden")
sys.dont_write_bytecode=True
P=pathlib.Path
HERE=P(__file__).resolve().parent
REG=json.loads((HERE/"registration.json").read_text())
ROOT=P.cwd().resolve()
OWNED=ROOT/"artifacts/checkpoints/p8-round19-evidence-intake-20261009/platform275e-author"
OUT=OWNED/"evidence"
RESERVE=512*1024**2
SOURCE="275e8799d4947d297329073eaa3ca675d3fd0777"
RUN=37890757129
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

def collect(source):
    sys.path.insert(0,str(source/"scripts"))
    import p8_cold_build as cold
    before,manifest=cold.source_identity(source,SOURCE)
    observers=cold.observer_snapshot(source)
    require(len(manifest)==1089,"original cold manifest population differs")
    require(set(observers["inputs"])==set(REG["cold_observers"]),"cold observer population differs")
    for name,pin in REG["cold_observers"].items():
        require(observers["inputs"][name]==pin,"cold observer differs")
    save(OUT/"source-before.json",before)
    save(OUT/"source-inputs.json",manifest)
    save(OUT/"observer-before.json",observers)
    argv=[sys.executable,"-B",str(source/"scripts/p8_cold_build.py"),
          "--source-root",str(source),"--collect-cells",str(OUT/"cells"),
          "--expected-commit",SOURCE,"--output-dir",str(OUT/"replay")]
    save(OUT/"collector-command.json",{"argv":argv,"cwd":str(source),"scope":"unchanged original offline collector"})
    started=time.monotonic()
    with (OUT/"collector.stdout").open("xb") as so,(OUT/"collector.stderr").open("xb") as se:
        result=subprocess.run(argv,cwd=source,stdout=so,stderr=se,timeout=900,
            env={**{k:v for k,v in os.environ.items() if k not in ("GH_TOKEN","GITHUB_TOKEN")},"PYTHONDONTWRITEBYTECODE":"1"})
    save(OUT/"collector-result.json",{"exit_code":result.returncode,"wall_seconds":time.monotonic()-started})
    require(result.returncode==0,"original cold collector rejected")
    replay=json.loads((OUT/"replay/matrix.json").read_text())
    collector=next(x for x in REG["artifacts"] if x["kind"]=="collector")
    original=json.loads((OUT/"collector"/str(collector["id"])/"matrix.json").read_text())
    normalized=json.loads(json.dumps(original))
    recorded_root=cold.absolute_recorded_path(original["source"]["source_root"],"original collector source")
    recorded_cells=cold.absolute_recorded_path(original["input_directory"],"original collector input")
    require(recorded_root.name=="codecortex" and recorded_root.parent.name=="codecortex"
        and recorded_cells==recorded_root.parent.parent/"_temp/p8-platform-cells",
        "original collector path does not match its fixed workflow")
    require(original["expected_commit"]==SOURCE and original["runner_sha256"]==sha(source/"scripts/p8_cold_build.py")
        and type(original["exit_code"]) is int and original["exit_code"]==0,
        "original collector wrapper identity differs")
    normalized["source"]["source_root"]=str(source)
    normalized["input_directory"]=str(OUT/"cells")
    require(cold.json_bytes(replay)==cold.json_bytes(normalized) and replay["counts"]=={"passed":8,"failed":0,"not_run":0},
        "original matrix differs beyond the two registered path mappings")
    after,after_manifest=cold.source_identity(source,SOURCE)
    after_observers=cold.observer_snapshot(source)
    require(before==after and manifest==after_manifest and observers==after_observers,"source/observer changed")
    save(OUT/"source-after.json",after)
    save(OUT/"observer-after.json",after_observers)
    save(OUT/"collector-comparison.json",{"all_fields_equal_except_registered_paths":True,"counts":replay["counts"],
        "path_mapping":{"source.source_root":{"original":str(recorded_root),"replay":str(source)},
        "input_directory":{"original":str(recorded_cells),"replay":str(OUT/"cells")}}})
    return replay

def run():
    global CREATED
    require(HERE==OWNED and HERE.resolve()==HERE,"controller must reside in its single owned evidence directory")
    require(REG["source"]==SOURCE and REG["run"]==RUN and len(REG["artifacts"])==18,"registration identity differs")
    require(not OUT.exists() and OUT.parent.resolve()==OUT.parent,"owned receiver exists or parent is not canonical")
    require(free()>=RESERVE+sum(x["bytes"] for x in REG["artifacts"])+64*1024**2,"not_run: insufficient original ZIP capacity")
    OUT.mkdir(parents=True)
    CREATED=True
    save(OUT/"registration.json",REG)
    run_record=json.loads(api("/repos/jyqj/codecortex/actions/runs/%d"%RUN,OUT/"api/run.json").read_text())
    require(run_record["head_sha"]==SOURCE and run_record["run_attempt"]==1 and run_record["conclusion"]=="success","original run identity/state differs")
    artifact_record=json.loads(api("/repos/jyqj/codecortex/actions/runs/%d/artifacts?per_page=100&page=1"%RUN,OUT/"api/artifacts.json").read_text())
    require(artifact_record["total_count"]==18 and len(artifact_record["artifacts"])==18,"incomplete artifact page")
    actual={x["id"]:x for x in artifact_record["artifacts"]}
    require(len(actual)==18 and set(actual)=={x["id"] for x in REG["artifacts"]},"artifact identity population differs")
    pending=[]
    for spec in REG["artifacts"]:
        item=actual[spec["id"]]
        require(item["name"]==spec["name"] and item["size_in_bytes"]==spec["bytes"]
           and item["digest"]=="sha256:"+spec["sha256"] and not item["expired"]
           and item["workflow_run"]["id"]==RUN and item["workflow_run"]["head_sha"]==SOURCE,"artifact metadata differs")
        archive=OUT/"zips"/("%d.zip"%spec["id"])
        require(free()>=RESERVE+spec["bytes"],"not_run: insufficient ZIP reserve")
        api("/repos/jyqj/codecortex/actions/artifacts/%d/zip"%spec["id"],archive)
        require(archive.stat().st_size==spec["bytes"] and sha(archive)==spec["sha256"],"original ZIP digest differs")
        scanned=members(archive)
        save(OUT/"members"/("%d.json"%spec["id"]),scanned)
        pending.append((spec,archive,scanned))
    expanded=sum(sum(x["bytes"] for x in entries.values()) for _,_,entries in pending)
    save(OUT/"expansion-capacity.json",{"free_bytes":free(),"required_expansion_bytes":expanded,"extra_source_and_report_allowance":64*1024**2,"reserve_bytes":RESERVE})
    require(free()>=RESERVE+expanded+64*1024**2,"not_run: insufficient complete expansion capacity")
    for spec,archive,entries in pending:
        kind={"cell":"cells","raw":"raw","collector":"collector","recovery":"recovery"}[spec["kind"]]
        dest=expand(archive,OUT/kind/str(spec["id"]),entries)
        if spec["kind"]=="collector":
            require(set(entries)=={"matrix.json"},"collector ZIP member changed")
        if spec["kind"]=="cell":
            require(json.loads((dest/"bundle.json").read_text())["cell"]==spec["cell"],"cell registration differs")
    source=source_copy()
    replay=collect(source)
    for spec,archive,entries in pending:
        require(sha(archive)==spec["sha256"] and archive.stat().st_size==spec["bytes"],"original ZIP changed")
        kind={"cell":"cells","raw":"raw","collector":"collector","recovery":"recovery"}[spec["kind"]]
        for name,row in entries.items():
            p=OUT/kind/str(spec["id"])/name
            require(p.stat().st_size==row["bytes"] and sha(p)==row["sha256"],"original member changed")
    return {"status":"accepted_scoped_original_275e_cold_platform_and_complete_transport",
        "source":SOURCE,"run":RUN,"review_role":"author_reception","counts":replay["counts"],"original_zip_count":18,
        "original_zip_bytes":sum(x["bytes"] for x in REG["artifacts"]),"all_member_bytes":expanded,
        "recovery_scope":"Original ZIP and all member bytes verified only; semantic recovery/rollback predicates require the separate original audit.",
        "raw_scope":"All eight additional raw ZIPs fully retained and CRC/SHA verified; no replacement for portable cell validation.",
        "product_or_Cargo_execution":False,"TODO_closed":0,"TODO_remaining":29}

if __name__=="__main__":
    started=time.monotonic()
    receipt={"status":"incomplete_not_certified","source":SOURCE,"run":RUN}
    exit_code=2
    try:
        receipt=run();exit_code=0
    except BaseException as error:
        receipt["error_type"]=type(error).__name__
        receipt["error"]=str(error) if isinstance(error,ValueError) else "bounded native receipt or transport failure"
    finally:
        receipt.update(exit_code=exit_code,wall_seconds=time.monotonic()-started)
        if CREATED:
            save(OUT/"receipt.json",receipt)
        print(json.dumps(receipt,sort_keys=True))
    raise SystemExit(exit_code)
