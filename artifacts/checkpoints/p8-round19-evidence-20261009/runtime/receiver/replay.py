#!/usr/bin/env python3
"""Original275e offline runtime reception. Only retained statistics ELF may run."""
import collections,hashlib,json,os,pathlib,re,subprocess,sys,time
if sys.flags.optimize or sys.platform!="linux":
    raise SystemExit("unoptimized Linux Python is required for retained statistics replay")
sys.dont_write_bytecode=True
P=pathlib.Path
HERE=P(__file__).resolve().parent
REG=json.loads((HERE/"registration.json").read_text())
ROOT=P(sys.argv[1]).resolve(strict=True)
SOURCE=ROOT/"source"
OUT=ROOT/"offline-replay"
G="275e8799d4947d297329073eaa3ca675d3fd0777"
CREATED=False
ENV={k:v for k,v in os.environ.items() if k not in ("GH_TOKEN","GITHUB_TOKEN")}
ENV["PYTHONDONTWRITEBYTECODE"]="1"
sys.path.insert(0,str(SOURCE/"scripts"))
from p7_build_identity import source_snapshot
from p8_runtime_build import observer_snapshot,verify_output,command_for,TARGETS

def require(ok,message):
    if not ok: raise ValueError(message)

def sha(path):
    with P(path).open("rb") as stream:
        return hashlib.file_digest(stream,"sha256").hexdigest()

def read(path):
    return json.loads(P(path).read_bytes())

def write(path,value):
    path.parent.mkdir(parents=True,exist_ok=True)
    with path.open("x") as stream:
        json.dump(value,stream,sort_keys=True,indent=2)
        stream.write("\n")

def invoke(label,argv,cwd=ROOT):
    write(OUT/(label+".command.json"),{"argv":argv,"cwd":str(cwd),"scope":"offline original-record consumer"})
    start=time.monotonic()
    with (OUT/(label+".stdout")).open("xb") as so,(OUT/(label+".stderr")).open("xb") as se:
        done=subprocess.run(argv,cwd=cwd,env=ENV,stdout=so,stderr=se,timeout=900)
    result={"exit_code":done.returncode,"wall_seconds":time.monotonic()-start}
    write(OUT/(label+".result.json"),result)
    require(done.returncode==0,"original consumer failed: "+label)
    return result

def originals():
    result={}
    for spec in REG["artifacts"]:
        aid=str(spec["id"]);home=ROOT/"raw"/aid
        archive=home/"original.zip"
        require(archive.stat().st_size==spec["bytes"] and sha(archive)==spec["sha256"],"original archive changed")
        expected=read(ROOT/"inventories"/(aid+".json"))
        found={p.relative_to(home/"extracted").as_posix():p for p in (home/"extracted").rglob("*") if p.is_file()}
        require(set(found)==set(expected) and not any(p.is_symlink() for p in (home/"extracted").rglob("*")),"original extracted inventory differs")
        for name,p in found.items():
            require(p.stat().st_size==expected[name]["bytes"] and sha(p)==expected[name]["sha256"],"original extracted bytes changed")
        result[aid]={"archive":spec,"members":expected}
    return result

def controls():
    found={}
    for name,pin in REG["control_files"].items():
        p=HERE/name
        require(p.is_file() and not p.is_symlink() and p.stat().st_size==pin["bytes"] and sha(p)==pin["sha256"],"fixed receiver differs")
        found[name]=pin
    return found

def compiler_record(record):
    before=record["toolchain_before"]
    require(before==record["toolchain_after"] and set(before)=={"cargo","rustc"},"original compiler records differ")
    if "toolchain_final" in record:
        require(record["toolchain_final"]==before,"original final compiler record differs")
    for name,row in before.items():
        require(P(row["invocation"]).is_absolute() and P(row["executable"]).is_absolute()
            and row["command"]==[row["invocation"],"--version","--verbose"]
            and re.fullmatch("[0-9a-f]{64}",row["executable_sha256"])
            and isinstance(row["version"],str) and bool(row["version"]),"recorded compiler identity incomplete")
    require(record["compiler_environment"]=={"RUSTC":before["rustc"]["invocation"],"RUSTC_WRAPPER":"","RUSTC_WORKSPACE_WRAPPER":""}
        and record["target_initially_absent"] is True and record["build_dir"]==record["target_dir"],
        "original private target/compiler selection differs")
    # Portable receipt review, not same-job verification of destroyed original compiler paths.
    return before

def runtime_origin(spec,snapshot,observer):
    aid=spec["id"];base=ROOT/"raw"/str(aid)/"extracted";b=base/"p8-build";m=base/"p8-runtime"
    record,plan,report=read(b/"build-receipt.json"),read(m/"plan.json"),read(m/"report.json")
    require(record["source_before"]==record["source_after"]==snapshot
        and record["observer_before"]==record["observer_after"]==observer,"exact source/observer")
    compiler=compiler_record(record)
    require(record["build_command"]==command_for(P(record["target_dir"]))
        and record["build_exit_code"]==0 and record["status"]=="passed","original runtime Cargo command failed")
    require(plan["build_identity"]==report["final_build_verification"]
        and plan["build_identity"]["toolchain"]==compiler,"original full build before/after")
    require(sha(b/"product-build.jsonl")==record["cargo_log_sha256"]
        and sha(b/"product-build.stderr")==record["stderr_sha256"],"raw Cargo logs differ")
    messages=[json.loads(line) for line in (b/"product-build.jsonl").open() if line.strip()]
    require(set(record["artifacts"])==set(TARGETS),"three release roles required")
    for name,stored in record["artifacts"].items():
        a=stored["cargo_artifact"];package,relative,_,_=TARGETS[name]
        producer=REG["original_producer_checkout"]+"/crates/"+package+"/"
        selected=[x for x in messages if x.get("reason")=="compiler-artifact" and x.get("target",{}).get("name")==name and x.get("target",{}).get("kind")==["bin"]]
        require(selected==[a] and a["manifest_path"]==producer+"Cargo.toml"
            and a["target"]["src_path"]==producer+relative
            and a["executable"]==record["target_dir"]+"/release/"+name
            and a["executable"]==stored["copy_source"]["path"],"unique exact Cargo producer/copy path")
        require(a["fresh"] is False and a["features"]==[] and a["profile"]["opt_level"]=="3"
            and a["profile"]["test"] is False and a["profile"]["debug_assertions"] is False,"original release profile")
        require(sha(b/name)==stored["binary_sha256"]==stored["copy_source"]["sha256"]
            and (b/name).stat().st_size==stored["binary_bytes"]==stored["copy_source"]["bytes"],"retained executable copy differs")
    if spec["profile"]=="mixed":
        executable=OUT/("statistics-"+str(aid))
        with executable.open("xb") as target: target.write((b/"p8-runtime-statistics").read_bytes())
        executable.chmod(0o555)
        require(sha(executable)==record["statistics_sha256"],"retained statistics copied bytes")
        output=OUT/("statistics-"+str(aid)+".json")
        invoke("statistics-"+str(aid),[str(executable),"--plan",str(m/"plan.json"),"--raw",str(m/"raw.jsonl"),"--output",str(output)])
        require(output.read_bytes()==(m/"statistics.json").read_bytes()==(m/"statistics-replay.json").read_bytes(),"retained statistics replay differs")
        require(sha(executable)==record["statistics_sha256"],"statistics copy changed")
    verify_output(b);verify_output(m)
    return {"artifact_id":aid,"portable_origin":"passed","mixed_statistics_replay":spec["profile"]=="mixed",
        "hour_statistics_replay_owner":"hour-wire-supplement" if spec["profile"]=="soak" else None}

def backfill_origin(spec,snapshot,observer):
    base=ROOT/"raw"/str(spec["id"])/"extracted";r=read(base/"receipt.json")
    require(r["source_before"]==r["source_after"]==r["source_final"]==snapshot
        and r["observer_before"]==r["observer_after"]==r["observer_final"]==observer,"backfill exact source/observer")
    compiler_record(r)
    target=r["target_dir"]
    expected=["cargo","test","--release","--locked","--offline","-p","cc-eval","--no-default-features","--features","semantic","--test","p7_worker_contention","--no-run","--message-format=json-render-diagnostics","--target-dir",target]
    require(r["build_command"]==expected and sha(base/"build.jsonl")==r["cargo_log_sha256"]
        and sha(base/"build.stderr")==r["stderr_sha256"],"backfill original build command/logs")
    a=r["cargo_artifact"];exe=P(a["executable"]);producer=REG["original_producer_checkout"]+"/crates/cc-eval/"
    require(a["manifest_path"]==producer+"Cargo.toml"
        and a["target"]["src_path"]==producer+"tests/p7_worker_contention.rs"
        and a["target"]["kind"]==["test"] and exe.is_absolute()
        and exe.parent==P(target)/"release/deps" and re.fullmatch(r"p7_worker_contention-[0-9a-f]+",exe.name)
        and str(exe)==r["copy_source"]["path"],"backfill exact Cargo producer/copy path")
    require((base/"p7_worker_contention").stat().st_size==r["copy_source"]["bytes"]
        and sha(base/"p7_worker_contention")==r["copy_source"]["sha256"]==r["executable_sha256"]==r["executable_sha256_after"],
        "backfill retained binary copy")
    invoke("backfill-original-verify",[sys.executable,"-B",str(SOURCE/"scripts/p8_backfill.py"),"verify","--output",str(base)])
    verify_output(base)
    return {"artifact_id":spec["id"],"portable_origin":"passed","worker_or_Cargo_reexecuted":False}

def run():
    global CREATED
    require(REG["source"]==G and REG["run"]==37890757030 and REG["attempt"]==1,"registration differs")
    require(not OUT.exists() and not (ROOT/"review-runtime").exists() and not (ROOT/"wire-supplement").exists(),"replay output already exists")
    OUT.mkdir();CREATED=True
    control_before=controls();inputs=originals()
    snapshot=source_snapshot(SOURCE);observer=observer_snapshot(SOURCE)
    require(snapshot["source_commit"]==G and snapshot["source_tree"]==REG["source_tree"]
        and snapshot["input_count"]==1089 and snapshot["manifest_sha256"]==REG["product_manifest"],"original275e production source")
    require(observer["source_commit"]==G and observer["files"]==REG["runtime_observers"]
        and observer["manifest_sha256"]==REG["observer_manifest"],"original nine observers")
    write(OUT/"source-before.json",snapshot);write(OUT/"observer-before.json",observer)
    origins=[]
    for spec in REG["artifacts"]:
        aid=spec["id"]
        if spec["profile"]=="backfill":
            invoke("backfill-checker",[sys.executable,"-B",str(HERE/"backfill_checker.py")])
            review=read(ROOT/"review-runtime"/("backfill-"+str(aid)+"-review.json"))
            require(review["review_status"]=="raw_and_receipt_review_passed" and review["errors"]==[],"backfill checker result")
            origins.append(backfill_origin(spec,snapshot,observer))
        else:
            invoke("runtime-checker-"+str(aid),[sys.executable,"-B",str(HERE/"runtime_checker.py"),str(aid),spec["sha256"],spec["profile"],str(spec["concurrency"])])
            review=read(ROOT/"review-runtime"/("runtime-"+str(aid)+"-review-v2.json"))
            require(review["review_status"]=="raw_and_receipt_review_passed" and review["errors"]==[],"runtime checker result")
            origins.append(runtime_origin(spec,snapshot,observer))
    invoke("hour-wire-supplement",[sys.executable,"-B",str(HERE/"hour_supplement.py"),str(ROOT),str(SOURCE)])
    wire=read(ROOT/"wire-supplement/inspection.json")
    require(wire["status"]=="accepted_scoped_275e_soak_supplement"
        and wire["compound_reads"]==2400 and wire["bound_RPCs"]==9600
        and wire["statistics_replay_byte_identical"] is True,"hour supplement result")
    require(originals()==inputs and controls()==control_before,"inputs or observers changed during replay")
    require(source_snapshot(SOURCE)==snapshot and observer_snapshot(SOURCE)==observer,"source/observer after all replay")
    write(OUT/"source-after.json",snapshot);write(OUT/"observer-after.json",observer)
    return {"status":"accepted_scoped_original_275e_runtime_six_domains","source":G,"run":37890757030,
        "origins":origins,"source_input_count":1089,"observer_count":9,"six_original_ZIP_inventories":inputs,
        "hour_supplement":wire,"all_originals_unchanged":True,
        "only_native_execution":"five retained p8-runtime-statistics binaries replay original plan/raw; no product/oracle/worker/Cargo",
        "scope":"Original source-specific receipt/raw/wire/resource/parity/statistics facts, not new primary measurement or performance comparison.",
        "TODO_closed":0,"TODO_remaining":29}

if __name__=="__main__":
    result={"status":"incomplete_not_certified","source":G,"run":37890757030};code=2
    try:
        result=run();code=0
    except BaseException as error:
        result.update(error_type=type(error).__name__,error=str(error) if isinstance(error,ValueError) else "bounded offline receiver failure")
    finally:
        result["exit_code"]=code
        if CREATED:write(OUT/"receipt.json",result)
        print(json.dumps({k:result.get(k) for k in ("status","source","run","exit_code","error_type","error")},sort_keys=True))
    raise SystemExit(code)
