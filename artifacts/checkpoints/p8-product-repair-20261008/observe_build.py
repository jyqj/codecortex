#!/usr/bin/env python3
import hashlib,json,os,pathlib,shutil,subprocess,time
ROOT=pathlib.Path.cwd()
OUT=ROOT/"artifacts/checkpoints/p8-product-repair-20261008/build-observation"
OUT.mkdir(parents=True,exist_ok=True)
SOURCE="ea6cc7e02ba5f0649f93dd03176fb36cfa797743"
subprocess.run(["git","diff","--exit-code",SOURCE,"--","crates","Cargo.toml","Cargo.lock"],check=True,stdout=subprocess.PIPE)
def digest(b):
    return {"bytes":len(b),"sha256":hashlib.sha256(b).hexdigest()}
inputs=[]
for line in subprocess.check_output(["git","ls-tree","-r",SOURCE,"--","crates","Cargo.toml","Cargo.lock"]).decode().splitlines():
    info,p=line.split("\t",1)
    mode,kind,oid=info.split()
    if kind!="blob":
        raise RuntimeError("Unexpected source kind "+p)
    b=(ROOT/p).read_bytes()
    if hashlib.sha1(("blob "+str(len(b))+"\0").encode()+b).hexdigest()!=oid:
        raise RuntimeError("Source bytes differ "+p)
    inputs.append({"path":p,"mode":mode,"git_blob":oid,**digest(b)})
inputs_raw=(json.dumps(inputs,ensure_ascii=False,indent=2)+"\n").encode()
(OUT/"source-inputs.json").write_bytes(inputs_raw)
receipt={"schema_version":1,"phase":"in_progress","product_source_commit":SOURCE,"execution_head":subprocess.check_output(["git","rev-parse","HEAD"]).decode().strip(),"source_inputs":digest(inputs_raw),"source_count":len(inputs),"source_guard":"Not repinned or waived. This isolated utility validates this candidate; required main/source-chain acceptance is pending.","build_profile":"release/default features","commands":[],"binaries":{}}
def save():
    (OUT/"receipt.json").write_text(json.dumps(receipt,ensure_ascii=False,indent=2)+"\n")
def run(name,args,timeout=1200,extra_env=None):
    save()
    env=os.environ.copy()
    if extra_env:
        env.update(extra_env)
    started=time.monotonic()
    r=subprocess.run(args,stdout=subprocess.PIPE,stderr=subprocess.STDOUT,timeout=timeout,env=env)
    log=OUT/(name+".log")
    log.write_bytes(r.stdout)
    text=r.stdout.decode("utf-8","replace")
    row={"name":name,"argv":args,"returncode":r.returncode,"duration_s":time.monotonic()-started,"log":log.name,**digest(r.stdout),"summaries":[line for line in text.splitlines() if line.startswith("test result:") or line.startswith("error") or line.startswith("warning:") or "panicked at" in line]}
    receipt["commands"].append(row)
    print("P8_BUILD_COMMAND "+json.dumps(row,separators=(",",":")),flush=True)
    save()
    if r.returncode:
        receipt["phase"]="failed_actual_command"
        save()
        print(text[-20000:],flush=True)
        raise SystemExit(r.returncode)
save()
run("format-check",["cargo","fmt","--all","--","--check"],120)
run("new-text-admission",["cargo","test","-p","cc-index","--test","explicit_text_admission","--","--nocapture"])
run("scanner-regression",["cargo","test","-p","cc-index","--lib","scanner::"])
run("new-notify-admission",["cargo","test","-p","cc-server","--lib","explicit_text_admission_tests"])
run("watcher-regression",["cargo","test","-p","cc-server","--lib","watcher::"])
run("eval-library",["cargo","test","-p","cc-eval","--lib"])
run("eval-adapter-regression",["cargo","test","-p","cc-eval","--test","benchmark_adapters"])
run("release-build",["cargo","build","--release","--workspace","--bins"],1800)
for name in ["codecortex","cc-eval"]:
    src=ROOT/"target/release"/name
    if not src.is_file():
        raise RuntimeError("Expected binary missing "+name)
    dest=OUT/"binaries"/name
    dest.parent.mkdir(parents=True,exist_ok=True)
    shutil.copy2(src,dest)
    receipt["binaries"][name]={"path":str(dest.relative_to(OUT)),**digest(dest.read_bytes())}
save()
run("real-mcp-readiness",["cargo","test","-p","cc-eval","--test","p8_mcp_readiness","--","--ignored","--nocapture"],300,{"CODECORTEX_BENCH_BINARY":str(ROOT/"target/release/codecortex")})
receipt["phase"]="candidate_targeted_tests_passed_main_source_approval_pending"
save()
print("P8_BUILD_RECEIPT "+json.dumps(receipt,separators=(",",":")),flush=True)
