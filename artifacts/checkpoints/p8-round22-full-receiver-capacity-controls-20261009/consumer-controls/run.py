#!/usr/bin/env python3
"""Run only the fixed receiver AST/synthetic controls and preserve their bytes."""
import ast,base64,datetime,hashlib,json,os,pathlib,re,stat,subprocess,sys,traceback
PREFIX="artifacts/checkpoints/p8-round22-full-receiver-capacity-controls-20261009"
BRANCH="refs/heads/task/p8-full-receiver-capacity-controls-20261009"
WORKFLOW=".github/workflows/p8-full-receiver-capacity-controls.yml"
SELF=PREFIX+"/consumer-controls/run.py"
PLAN=PREFIX+"/consumer-controls/execution-plan.json"
PINS=json.loads("{\"consumer/consumer.py\":{\"blob\":\"e873b4cedeb08dbeb44e594187307a7382e3932a\",\"bytes\":38134,\"sha256\":\"678e163df14847203a0c71197afc58d617de7bacc437ed785c11a33dce25ee1c\"},\"consumer/operator.py\":{\"blob\":\"ca9f49761b80d1cc0cf95b9720667763f2a4eb0e\",\"bytes\":9745,\"sha256\":\"60a65b720fd94901fea51dd02f98e7f05d0fa1fdf41038ec79b4714d84a928cd\"},\"consumer/test_contract.py\":{\"blob\":\"b96ab232b30742897d3c9077dac20ad91b8080af\",\"bytes\":12326,\"sha256\":\"32edf97614cef5cc2e718172eabd22cc319fa2a039db3d5eb2086cd6e94353a0\"},\"consumer/p8_scale_matrix.py\":{\"blob\":\"b4ba4b57f8d23a16128ef82927e78799ae70f886\",\"bytes\":65092,\"sha256\":\"f169b0f26cf2a87d3e2548d734431c530cbf63d353ac054045ec8be80889f957\"},\"consumer/full-replay.py\":{\"blob\":\"cb82eba2c263b5830dcf17cca32b1e3ea60feaf0\",\"bytes\":14243,\"sha256\":\"6da29c67c0107937b92841f4a63f80e84a1e8b225f309480295aaa783c119b8f\"}}")
TEST_NAMES=json.loads("[\"test_301_core_and_failed_aggregate_job_not_rejected\",\"test_artifact_and_job_source_mismatch_rejected\",\"test_duplicate_extra_and_expired_artifacts_rejected\",\"test_exact_full_directory_relocation\",\"test_missing_original_is_reported_not_counted\",\"test_new_upload_not_added_to_selected_batch\",\"test_optional_remote_artifact_retained\",\"test_previously_received_original_identity_pinned\",\"test_relocation_rejects_missing_duplicate_extra_field\",\"test_remote_matrix_original_json_limit_capacity_boundaries\",\"test_selected_identity_change_refused\",\"test_wrong_source_or_attempt_or_created_time_rejected\"]")
EXPECTED_IDS=["__main__.ReceptionContractTests."+name for name in TEST_NAMES]
MAX_SMALL=256*1024
FRAME_RAW=1024

def require(ok,message):
    if not ok: raise ValueError(message)

def sha(raw):
    return hashlib.sha256(raw).hexdigest()

def git_oid(raw):
    return hashlib.sha1(b"blob "+str(len(raw)).encode()+b"\0"+raw).hexdigest()

def utc():
    return datetime.datetime.now(datetime.timezone.utc).isoformat()

def file_bytes(path,limit=MAX_SMALL):
    require(path.resolve(strict=True)==path and stat.S_ISREG(path.lstat().st_mode),"nonregular input")
    require(path.stat().st_size<=limit,"small evidence bound exceeded")
    raw=path.read_bytes()
    require(len(raw)<=limit,"small evidence changed beyond bound")
    return raw

def git(root,*args):
    completed=subprocess.run(["git","-C",str(root),*args],stdout=subprocess.PIPE,
                             stderr=subprocess.DEVNULL,timeout=15,check=False)
    require(completed.returncode==0,"read-only Git identity query failed")
    return completed.stdout

def snapshot(root,head):
    require(git(root,"rev-parse","HEAD").decode().strip()==head,"checkout HEAD differs")
    result={}
    paths=[PREFIX+"/"+name for name in PINS]+[SELF,PLAN,WORKFLOW]
    for path in paths:
        raw=file_bytes(root/path)
        original=git(root,"show",head+":"+path)
        require(raw==original,"working bytes differ from fixed checkout")
        oid=git(root,"rev-parse",head+":"+path).decode().strip()
        require(git_oid(raw)==oid,"Git blob identity differs")
        entry={"bytes":len(raw),"sha256":sha(raw),"blob":oid}
        suffix=path.removeprefix(PREFIX+"/")
        if suffix in PINS: require(entry==PINS[suffix],"registered source differs")
        result[path]=entry
    return result

def safe_error(error):
    return {"type":type(error).__name__,"locations":[
        {"file":pathlib.Path(frame.filename).name,"function":frame.name,"line":frame.lineno}
        for frame in traceback.extract_tb(error.__traceback__)]}

def dump_new(path,value):
    raw=(json.dumps(value,sort_keys=True,indent=2)+"\n").encode()
    require(len(raw)<=MAX_SMALL,"receipt exceeds fixed small bound")
    with path.open("xb") as stream: stream.write(raw)
    return raw

def emit_frame(name,raw):
    require(len(raw)<=MAX_SMALL,"framed evidence exceeds fixed bound")
    count=(len(raw)+FRAME_RAW-1)//FRAME_RAW
    common={"schema":"p8-original-receiver-controls-frame-v1","name":name}
    def line(value):
        text=json.dumps(dict(common,**value),sort_keys=True,separators=(",",":"))
        require(len(text.encode())<=2048,"frame line exceeds fixed bound")
        print(text,flush=True)
    line({"kind":"header","bytes":len(raw),"sha256":sha(raw),"count":count,"raw_chunk_bytes":FRAME_RAW})
    for index in range(count):
        block=raw[index*FRAME_RAW:(index+1)*FRAME_RAW]
        line({"kind":"chunk","index":index,"count":count,"bytes":len(block),
              "sha256":sha(block),"base64":base64.b64encode(block).decode("ascii")})
    line({"kind":"end","bytes":len(raw),"sha256":sha(raw),"count":count})

def validate_report(report):
    require(report["status"]=="passed_static_and_synthetic_only","original controls did not pass")
    require(report["actual_tests"]==12 and report["failed"]==0 and report["errors"]==0
            and report["skipped"]==0,"original test population differs")
    require(report["methods"]==EXPECTED_IDS,"original method names differ")
    require(report["receiver_executed"] is False and report["operator_executed"] is False,
            "receiver operation claimed")
    for label,path in (("controller","consumer/consumer.py"),("operator","consumer/operator.py"),
                       ("matrix","consumer/p8_scale_matrix.py")):
        require(report["modules"][label]=={
            "bytes":PINS[path]["bytes"],"sha256":PINS[path]["sha256"],
            "AST_and_compile":"passed","executed_as_operation":False},"AST source result differs")
    actual=[]
    for line in report["stdout_stderr"].splitlines():
        match=re.fullmatch(r"(test_[A-Za-z0-9_]+) \((__main__\.ReceptionContractTests\.test_[A-Za-z0-9_]+)\) \.\.\. ok",line)
        if match:
            require(match.group(2)=="__main__.ReceptionContractTests."+match.group(1),"test line differs")
            actual.append(match.group(2))
    require(actual==EXPECTED_IDS,"actual per-method output differs")

def main():
    out=None
    created=False
    record={"schema":"p8-original-receiver-controls-execution-v1","started_utc":utc(),
            "status":"failed","inputs":PINS,"expected_methods":EXPECTED_IDS,
            "scope":"Original 11 synthetic metadata/path controls plus one real temporary JSON capacity-boundary method; wrapper AST/compile only",
            "receiver_operation_executed":False,"operator_operation_executed":False,
            "real_ZIP_or_ELF_or_Cargo_or_product_or_scale_workload":False}
    success=False
    before=None
    try:
        require(not sys.flags.optimize,"optimized Python forbidden")
        run_id=os.environ["GITHUB_RUN_ID"]
        attempt=os.environ["GITHUB_RUN_ATTEMPT"]
        head=os.environ["GITHUB_SHA"]
        require(re.fullmatch(r"[0-9]+",run_id) is not None and attempt=="1","first attempt required")
        require(re.fullmatch(r"[0-9a-f]{40}",head) is not None,"invalid actual HEAD")
        root=pathlib.Path(os.environ["GITHUB_WORKSPACE"]).resolve(strict=True)
        temp=pathlib.Path(os.environ["RUNNER_TEMP"]).resolve(strict=True)
        out=temp/("p8-full-receiver-capacity-controls-"+run_id)
        require(not out.exists(),"fresh evidence directory already exists")
        out.mkdir(mode=0o700)
        created=True
        record["identity"]={"repository":os.environ["GITHUB_REPOSITORY"],"head":head,
                            "ref":os.environ["GITHUB_REF"],"event":os.environ["GITHUB_EVENT_NAME"],
                            "run_id":int(run_id),"run_attempt":1,"job":os.environ["GITHUB_JOB"]}
        require(record["identity"]["repository"]=="jyqj/codecortex"
                and record["identity"]["event"]=="push" and record["identity"]["ref"]==BRANCH,
                "unexpected execution identity")
        require(pathlib.Path(__file__).resolve()==root/SELF,"unexpected runner location")
        before=snapshot(root,head)
        dump_new(out/"source-before.json",before)
        wrapper_raw=file_bytes(root/PREFIX/"consumer/full-replay.py")
        wrapper_ast=ast.parse(wrapper_raw,filename="full-replay.py")
        compile(wrapper_ast,"full-replay.py","exec")
        record["wrapper_AST_and_compile"]={"bytes":len(wrapper_raw),"sha256":sha(wrapper_raw),
                                           "AST_and_compile":"passed","executed_as_operation":False}
        matrix_text=file_bytes(root/PREFIX/"consumer/p8_scale_matrix.py").decode()
        matrix_ast=ast.parse(matrix_text,filename="p8_scale_matrix.py")
        selected={"require","unique_object","decode","read_json","exact_equal"}
        nodes=[node for node in matrix_ast.body if isinstance(node,ast.FunctionDef) and node.name in selected]
        require(len(nodes)==5 and {node.name for node in nodes}==selected,"original matrix function population differs")
        record["original_matrix_function_source_identity"]={}
        for node in nodes:
            span=ast.get_source_segment(matrix_text,node).encode()
            record["original_matrix_function_source_identity"][node.name]={"bytes":len(span),"sha256":sha(span)}
        command=[sys.executable,"-B",str(root/PREFIX/"consumer/test_contract.py"),
                 "--controller",str(root/PREFIX/"consumer/consumer.py"),
                 "--operator",str(root/PREFIX/"consumer/operator.py"),
                 "--matrix",str(root/PREFIX/"consumer/p8_scale_matrix.py"),
                 "--output",str(out/"controls-report.json")]
        record["command"]={"argv":command,"cwd":str(out),"timeout_seconds":90,
                           "environment":{"PATH":"/usr/bin:/bin","LC_ALL":"C.UTF-8",
                                          "PYTHONDONTWRITEBYTECODE":"1","PYTHONHASHSEED":"0"}}
        with (out/"stdout.log").open("xb") as stdout,(out/"stderr.log").open("xb") as stderr:
            result=subprocess.run(command,cwd=out,env=record["command"]["environment"],
                                  stdout=stdout,stderr=stderr,timeout=90,check=False)
        record["exit_code"]=result.returncode
        require(result.returncode==0,"original controls exited nonzero")
        raw=file_bytes(out/"controls-report.json")
        report=json.loads(raw)
        validate_report(report)
        record["original_report"]={"bytes":len(raw),"sha256":sha(raw),"actual_tests":12,
                                   "failed":0,"errors":0,"skipped":0,"methods":report["methods"]}
        success=True
    except BaseException as error:
        record["error"]=safe_error(error)
    finally:
        if created:
            if before is not None:
                try:
                    after=snapshot(root,head)
                    dump_new(out/"source-after.json",after)
                    require(after==before,"fixed input bytes changed")
                    record["source_before_after_equal"]=True
                except BaseException as error:
                    record["source_after_error"]=safe_error(error)
                    success=False
            record["status"]="passed_AST_and_12_synthetic_only" if success else "failed"
            record["finished_utc"]=utc()
            inventory=[]
            for name in ("controls-report.json","stdout.log","stderr.log","source-before.json","source-after.json"):
                path=out/name
                if path.exists():
                    raw=file_bytes(path)
                    inventory.append({"name":name,"bytes":len(raw),"sha256":sha(raw)})
            record["inventory"]=inventory
            raw_receipt=dump_new(out/"execution-receipt.json",record)
            for name in ("controls-report.json","stdout.log","stderr.log","source-before.json","source-after.json"):
                path=out/name
                if path.exists(): emit_frame(name,file_bytes(path))
            emit_frame("execution-receipt.json",raw_receipt)
        else:
            print(json.dumps({"schema":record["schema"],"status":"failed_before_output","error":record.get("error")},sort_keys=True))
    return 0 if success else 2

if __name__=="__main__":
    raise SystemExit(main())
