import json,sys,hashlib,subprocess,os,datetime,time
from pathlib import Path
base=Path("/Users/jin/Desktop/codecortex-rust/artifacts/benchmarks/p8-pr184-native-20261009-root")
work=base/"worktree"
root=base/"descendant-stage-probe-G-20261009"
source=sys.stdin.buffer.read()
assert len(source)==1770 and hashlib.sha256(source).hexdigest()=="def9e5ad771be2079eb9340a10ddf2788b3fd3600019e21f7a738f7f621e7bb0"
def state():
    ids=subprocess.check_output(["/usr/bin/git","rev-parse","HEAD","HEAD^{tree}"],cwd=work,text=True).splitlines()
    status=subprocess.check_output(["/usr/bin/git","status","--porcelain=v1","--untracked-files=normal"],cwd=work,text=True)
    assert ids==["b356043c2c0c970d000e5f5e32f1dff69f88454f","5af23abb6f53a182c18cbff228b87adab1bcb213"] and not status
    return {"head":ids[0],"tree":ids[1],"status":status}
def digest(p):
    b=p.read_bytes();return {"bytes":len(b),"sha256":hashlib.sha256(b).hexdigest()}
before=state()
root.mkdir()
(root/"probe.rs").write_bytes(source)
rlib=base/"cargo-target/debug/deps/libcc_eval-81fa4882470a4bc4.rlib"
env=dict(os.environ);env["SDKROOT"]="/Library/Developer/CommandLineTools/SDKs/MacOSX26.5.sdk"
env["PATH"]="/Users/jin/.cargo/bin:"+env.get("PATH","")
manifest={"schema":"p8-descendant-one-shot-phase-diagnostic-v1","source_before":before,"probe_source":digest(root/"probe.rs"),"cc_eval_rlib":{"path":str(rlib),**digest(rlib)},"sdkroot":env["SDKROOT"],"commands":[],"formal_measurement":False}
def save():
    (root/"manifest.json").write_text(json.dumps(manifest,indent=2)+"\n")
for name,argv,timeout in [
 ("compile",["/Users/jin/.cargo/bin/rustc","+1.95.0","--edition=2021","--crate-name","p8_descendant_stage_probe","-D","warnings",str(root/"probe.rs"),"--extern","cc_eval="+str(rlib),"-L","dependency="+str(base/"cargo-target/debug/deps"),"-o",str(root/"probe")],180),
 ("probe",[str(root/"probe"),str(root/"one-shot")],10)]:
    entry={"name":name,"argv":argv,"started_at_utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"timeout_seconds":timeout}
    t=time.monotonic()
    with (root/(name+".stdout.log")).open("xb") as out,(root/(name+".stderr.log")).open("xb") as err:
        try:
            p=subprocess.run(argv,cwd=work,env=env,stdout=out,stderr=err,timeout=timeout);entry["exit_code"]=p.returncode;entry["timed_out"]=False
        except subprocess.TimeoutExpired:
            entry["exit_code"]=None;entry["timed_out"]=True
    entry.update({"elapsed_seconds":time.monotonic()-t,"stdout":digest(root/(name+".stdout.log")),"stderr":digest(root/(name+".stderr.log"))})
    manifest["commands"].append(entry);save()
    print(json.dumps(entry),flush=True)
    if name=="compile" and entry["exit_code"]!=0:
        manifest["source_after"]=state();save();sys.exit(1)
if (root/"probe").exists():manifest["probe_binary"]=digest(root/"probe")
manifest["source_after"]=state()
manifest["files"]=[{"path":str(p.relative_to(root)),**digest(p)} for p in sorted(root.rglob("*")) if p.is_file() and p.name not in ("probe","manifest.json")]
report=root/"one-shot/run/report.json"
if report.exists():manifest["observed_report"]=json.loads(report.read_text())
manifest["completed_at_utc"]=datetime.datetime.now(datetime.timezone.utc).isoformat();save()
print(json.dumps({"manifest":str(root/"manifest.json"),"observed_report":manifest.get("observed_report"),"files":manifest["files"]}))
