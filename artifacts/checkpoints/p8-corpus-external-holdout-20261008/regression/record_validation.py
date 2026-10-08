#!/usr/bin/env python3
"""Record read-only validation of the frozen round4 PRODUCT commit."""
import datetime, hashlib, json, os, pathlib, subprocess, sys, time
ROOT=pathlib.Path("/workspace/scratch/031390cf22eb/codecortex-round4-validation")
OUT=pathlib.Path("/workspace/scratch/031390cf22eb/round4-final-validation")
BASE=ROOT.parent
EXPECTED_HEAD="615662bd0e651dc40a9d1d0d6757bc28646b900d"
EXPECTED_TREE="d36023d30673cd3eeb6b7fc146254b57daaf3679"
TOOLCHAIN=BASE/"toolchains/rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin"
ENV_OVERRIDES={"CARGO_HOME":str(BASE/"cargo-cache"),"RUSTUP_HOME":str(BASE/"toolchains/rustup"),"CARGO_TARGET_DIR":str(BASE/"target-scale"),"RUSTUP_TOOLCHAIN":"1.95.0","CARGO_TERM_COLOR":"never","CARGO_BUILD_JOBS":"2","CARGO_INCREMENTAL":"0","CARGO_PROFILE_DEV_DEBUG":"0","CARGO_PROFILE_TEST_DEBUG":"0","RUSTFLAGS":"-D warnings","PYTHONDONTWRITEBYTECODE":"1","PATH":str(TOOLCHAIN)+os.pathsep+str(BASE/"cargo-cache/bin")+os.pathsep+os.environ.get("PATH","")}
def digest(data): return hashlib.sha256(data).hexdigest()
def json_bytes(obj): return (json.dumps(obj,sort_keys=True,ensure_ascii=False,indent=2)+"\n").encode()
def write(path,obj): path.write_bytes(json_bytes(obj))
def git(*args): return subprocess.check_output(["git",*args],cwd=ROOT)
def inventory():
    head=git("rev-parse","HEAD").decode().strip()
    tree=git("rev-parse","HEAD^{tree}").decode().strip()
    status=git("status","--porcelain=v1","--untracked-files=all").decode()
    if (head,tree)!=(EXPECTED_HEAD,EXPECTED_TREE): raise RuntimeError("frozen PRODUCT identity changed")
    files=[]
    raw=git("ls-tree","-rz","HEAD","--","Cargo.toml","Cargo.lock","crates","scripts",".github/workflows","tests/source_integrity","CONTRIBUTING.md")
    for row in raw.split(b"\0"):
        if not row: continue
        meta,path_bytes=row.split(b"\t",1)
        mode,kind,oid=meta.decode().split()
        path=path_bytes.decode()
        if mode not in ("100644","100755") or kind!="blob": raise RuntimeError("unsupported input mode: "+path)
        local=ROOT/path
        if any(p.is_symlink() for p in [local,*list(local.parents)[:len(pathlib.PurePosixPath(path).parts)-1]]): raise RuntimeError("symlink input: "+path)
        data=local.read_bytes()
        blob=hashlib.sha1(b"blob "+str(len(data)).encode()+b"\0"+data).hexdigest()
        if blob!=oid: raise RuntimeError("input differs from PRODUCT Git blob: "+path)
        group="source" if path.startswith("crates/") or path in ("Cargo.toml","Cargo.lock") else "validation_and_policy"
        files.append({"path":path,"group":group,"mode":mode,"git_blob":oid,"bytes":len(data),"sha256":digest(data)})
    files.sort(key=lambda x:x["path"])
    counts={g:sum(x["group"]==g for x in files) for g in ("source","validation_and_policy")}
    return {"head":head,"tree":tree,"status":status,"counts":counts,"manifest_sha256":digest(json_bytes(files)),"files":files}
def tool_info(env):
    result={}
    for key,argv in {"rustc":[str(TOOLCHAIN/"rustc"),"-Vv"],"cargo":[str(TOOLCHAIN/"cargo"),"-Vv"],"python":[sys.executable,"--version"]}.items():
        p=subprocess.run(argv,env=env,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,text=True)
        result[key]={"argv":argv,"exit_code":p.returncode,"output":p.stdout}
    return result
def main():
    if len(sys.argv)<4 or sys.argv[2]!="--": raise SystemExit("usage: record_validation.py NAME -- COMMAND ...")
    name=sys.argv[1]
    if not name.replace("-","").replace("_","").isalnum(): raise SystemExit("invalid name")
    argv=sys.argv[3:]
    if argv[0]=="cargo": argv[0]=str(TOOLCHAIN/"cargo")
    before_path=OUT/(name+".before.json")
    after_path=OUT/(name+".after.json")
    log_path=OUT/(name+".log")
    receipt_path=OUT/(name+".json")
    if any(p.exists() for p in (before_path,after_path,log_path,receipt_path)): raise SystemExit("refusing to overwrite an existing validation receipt")
    env=dict(os.environ); env.update(ENV_OVERRIDES)
    before=inventory()
    if before["status"]: raise RuntimeError("validation worktree must start clean: "+before["status"])
    write(before_path,before)
    started=datetime.datetime.now(datetime.timezone.utc).isoformat()
    info=tool_info(env)
    rec={"schema_version":1,"name":name,"cwd":str(ROOT),"argv":argv,"environment_overrides":ENV_OVERRIDES,"toolchain":info,"started_at_utc":started,"status":"running","source_before":str(before_path),"source_before_sha256":digest(before_path.read_bytes()),"source_identity":{k:before[k] for k in ("head","tree","counts","manifest_sha256")},"log":str(log_path),"observer_sha256":digest(pathlib.Path(__file__).read_bytes())}
    write(receipt_path,rec)
    print(json.dumps({"name":name,"status":"running","argv":argv,"source":before["head"],"counts":before["counts"],"log":str(log_path)}),flush=True)
    start=time.monotonic()
    with log_path.open("wb") as log:
        child=subprocess.run(argv,cwd=ROOT,env=env,stdout=log,stderr=subprocess.STDOUT)
    elapsed=time.monotonic()-start
    after=inventory(); write(after_path,after)
    unchanged=before==after
    rec.update({"finished_at_utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"elapsed_seconds":round(elapsed,6),"exit_code":child.returncode,"source_after":str(after_path),"source_after_sha256":digest(after_path.read_bytes()),"source_unchanged":unchanged,"log_sha256":digest(log_path.read_bytes()),"log_bytes":log_path.stat().st_size,"status":"passed" if child.returncode==0 and unchanged else "failed"})
    write(receipt_path,rec)
    print(json.dumps({"name":name,"status":rec["status"],"exit_code":child.returncode,"elapsed_seconds":rec["elapsed_seconds"],"source_unchanged":unchanged,"receipt":str(receipt_path),"receipt_sha256":digest(receipt_path.read_bytes())}),flush=True)
    return 0 if rec["status"]=="passed" else 1
if __name__=="__main__": raise SystemExit(main())
