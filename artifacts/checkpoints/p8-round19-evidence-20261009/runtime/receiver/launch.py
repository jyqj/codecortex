#!/usr/bin/env python3
"""Launch one offline original-record reception in an already present pinned image."""
import hashlib,json,os,pathlib,subprocess,sys,time
P=pathlib.Path
if sys.flags.optimize:
    raise SystemExit("optimized Python is forbidden")
ROOT=P.cwd().resolve()
OWNED=ROOT/"artifacts/checkpoints/p8-round19-evidence-intake-20261009/runtime275e-author"
HERE=P(__file__).resolve().parent
EVIDENCE=OWNED/"evidence"
LOG=OWNED/"launch"
OUTPUT=OWNED/"replay-output"
REG=json.loads((HERE/"registration.json").read_bytes())
DOCKER="/usr/local/bin/docker"
IMAGE="python@sha256:3d361d7fea344d55ac7a0f51ed7faa99213808ccc96fc9b9170adb95b6570b96"
NAME="p8-275e-runtime-original-replay-20261009-01"
CREATED=False
CID=None

def require(ok,message):
    if not ok:raise ValueError(message)

def sha(path):
    h=hashlib.sha256()
    with P(path).open("rb") as f:
        for chunk in iter(lambda:f.read(1024*1024),b""):h.update(chunk)
    return h.hexdigest()

def save(path,value):
    with path.open("x") as f:
        json.dump(value,f,sort_keys=True,indent=2);f.write("\n")

def capture(label,argv):
    save(LOG/(label+".command.json"),{"argv":argv})
    r=subprocess.run(argv,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=120)
    (LOG/(label+".stdout")).write_bytes(r.stdout)
    (LOG/(label+".stderr")).write_bytes(r.stderr)
    save(LOG/(label+".result.json"),{"exit_code":r.returncode})
    require(r.returncode==0,"owned offline Docker operation failed: "+label)
    return r.stdout

def control_inventory():
    result={}
    for name,pin in REG["control_files"].items():
        p=OWNED/name
        require(p.is_file() and not p.is_symlink() and p.stat().st_size==pin["bytes"]
            and sha(p)==pin["sha256"],"reviewed control changed")
        result[name]=pin
    for name in ("registration.json","launch.py"):
        p=OWNED/name
        require(p.is_file() and not p.is_symlink(),"control not regular")
        result[name]={"bytes":p.stat().st_size,"sha256":sha(p)}
    return result

def object_directories():
    start=(EVIDENCE/"source/.git/objects").resolve(strict=True)
    todo=[start];seen=set()
    while todo:
        current=todo.pop()
        if current in seen:continue
        require(current.is_dir() and current.resolve(strict=True)==current,"noncanonical object storage")
        seen.add(current)
        alternate=current/"info/alternates"
        if alternate.exists():
            for line in alternate.read_text().splitlines():
                require(bool(line) and not line.startswith('"'),"unsupported quoted object alternate")
                p=P(line)
                if not p.is_absolute():p=current/p
                todo.append(p.resolve(strict=True))
    return sorted(seen,key=str)

def inspect(label):
    return json.loads(capture(label,[DOCKER,"container","inspect",CID]))[0]

def run():
    global CREATED,CID
    require(HERE==OWNED and OWNED.resolve()==OWNED,"wrong owned runtime root")
    require(not LOG.exists() and not OUTPUT.exists(),"launch or output already exists")
    prep=json.loads((EVIDENCE/"preparation.json").read_bytes())
    require(prep["status"]=="prepared_complete_original_bytes_not_semantic_acceptance" and prep["exit_code"]==0
        and prep["source"]==REG["source"] and prep["run"]==REG["run"],"original preparation unavailable")
    before=control_inventory()
    LOG.mkdir();OUTPUT.mkdir();CREATED=True
    save(LOG/"control-before.json",before)
    info=json.loads(capture("image-inspect",[DOCKER,"image","inspect","--platform","linux/amd64",IMAGE]))[0]
    require(info["Id"]=="sha256:3d361d7fea344d55ac7a0f51ed7faa99213808ccc96fc9b9170adb95b6570b96"
        and info["Architecture"]=="amd64" and info["Os"]=="linux","existing exact Linux Python image differs")
    args=[DOCKER,"create","--pull=never","--platform=linux/amd64","--name",NAME,
        "--hostname","p8-original-runtime-receiver","--network=none","--read-only",
        "--cap-drop=ALL","--security-opt=no-new-privileges","--pids-limit=128","--memory=2g","--cpus=2",
        "--tmpfs","/tmp:rw,noexec,nosuid,size=64m",
        "--env","PYTHONDONTWRITEBYTECODE=1","--env","GIT_OPTIONAL_LOCKS=0",
        "--env","GIT_CONFIG_GLOBAL=/dev/null","--env","GIT_CONFIG_NOSYSTEM=1",
        "--env","GIT_CONFIG_COUNT=1","--env","GIT_CONFIG_KEY_0=safe.directory",
        "--env","GIT_CONFIG_VALUE_0=/work/source","--workdir","/work"]
    mounts=[(OUTPUT,"/work",False),(OWNED,"/receiver",True),
        (EVIDENCE/"source","/work/source",True),(EVIDENCE/"raw","/work/raw",True),
        (EVIDENCE/"inventories","/work/inventories",True)]
    # Source's own object store is already in the source mount; mount only external alternate stores.
    for p in object_directories():
        if p!=(EVIDENCE/"source/.git/objects").resolve():
            mounts.append((p,str(p),True))
    for source,target,readonly in mounts:
        args+=["--mount","type=bind,src="+str(source)+",dst="+target+(",readonly" if readonly else "")]
    args += [IMAGE,"python3","-B","/receiver/replay.py","/work"]
    CID=capture("create",args).decode().strip()
    require(bool(CID) and all(c in "0123456789abcdef" for c in CID),"container identity malformed")
    before_container=inspect("container-before")
    require(before_container["HostConfig"]["NetworkMode"]=="none"
        and before_container["HostConfig"]["ReadonlyRootfs"] is True,"container isolation differs")
    binds=[m for m in before_container["Mounts"] if m["Type"]=="bind"]
    require(len(binds)==len(mounts) and all(m["RW"]==(m["Destination"]=="/work") for m in binds),
        "only new replay output may be writable")
    require({(m["Source"],m["Destination"],not m["RW"]) for m in binds}
        =={(str(s),d,r) for s,d,r in mounts},"actual mounted original inputs differ")
    argv=[DOCKER,"start","--attach",CID];save(LOG/"start.command.json",{"argv":argv})
    started=time.monotonic()
    with (LOG/"run.stdout").open("xb") as so,(LOG/"run.stderr").open("xb") as se:
        done=subprocess.run(argv,stdout=so,stderr=se,timeout=1800)
    after_container=inspect("container-after")
    require(done.returncode==0 and after_container["State"]["ExitCode"]==0
        and after_container["State"]["Running"] is False and not after_container["State"]["OOMKilled"],
        "original offline consumer failed")
    result=json.loads((OUTPUT/"offline-replay/receipt.json").read_bytes())
    require(result["status"]=="accepted_scoped_original_275e_runtime_six_domains"
        and result["exit_code"]==0,"original consumer semantic receipt failed")
    require(control_inventory()==before,"control inputs changed")
    save(LOG/"control-after.json",before)
    return {"status":"accepted_scoped_original_runtime_offline_reception","source":REG["source"],"run":REG["run"],
        "container_id":CID,"image":IMAGE,"original_consumer_exit_code":done.returncode,
        "wall_seconds":time.monotonic()-started,"receipt_sha256":sha(OUTPUT/"offline-replay/receipt.json"),
        "input_mounts_readonly":True,"only_output_writable":"/work",
        "native_execution":"five retained original statistics replays only",
        "new_product_or_Cargo_or_measurement":False,"TODO_closed":0,"TODO_remaining":29}

if __name__=="__main__":
    result={"status":"incomplete_not_certified","source":REG["source"],"run":REG["run"]};code=2
    try:
        result=run();code=0
    except BaseException as error:
        result.update(error_type=type(error).__name__,error=str(error) if isinstance(error,ValueError) else "bounded offline launch failure")
        if CID is not None and CREATED:
            try:
                state=inspect("container-error")
                if state["State"]["Running"]:
                    capture("stop-owned-container",[DOCKER,"stop","--time","10",CID])
                final=inspect("container-error-final")
                result["owned_container_stopped"]=final["State"]["Running"] is False
            except BaseException:
                result["owned_container_stopped"]=False
    finally:
        result["exit_code"]=code
        if CREATED:save(LOG/"receipt.json",result)
        print(json.dumps(result,sort_keys=True))
    raise SystemExit(code)
