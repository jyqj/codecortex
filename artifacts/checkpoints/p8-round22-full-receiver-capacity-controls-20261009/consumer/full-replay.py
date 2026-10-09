#!/usr/bin/env python3
"""Portable reception of the complete original 275e study; no measurements."""
import argparse,base64,contextlib,hashlib,importlib.util,inspect,json,os,pathlib,platform,subprocess,sys,time,zipfile
P=pathlib.Path
PREFIX="artifacts/checkpoints/p8-round22-formal275e-full-reception-20261009"
BRANCH="refs/heads/task/p8-formal275e-full-reception-20261009"
ORIGINAL_PATH="artifacts/checkpoints/p8-round20-evidence-20261009/consumer/consumer.py"
ORIGINAL={"blob":"fbd6ca6c27adc7af855fc4e4ece25c95d6195213","bytes":38114,"sha256":"dd19c6c07ddbde0cf797828c2860d2df81bd0e144465f1afdd17930863e0fecc"}
WORKFLOW=".github/workflows/p8-formal275e-full-reception.yml"
MAX_FRAME_FILE=4*1024**2
MAX_MATRIX_FRAME_FILE=16*1024**2
MAX_FRAME_TOTAL=24*1024**2
MAX_DECODER_LOG=64*1024**2
MATRIX_FRAME_NAME="replay/independent-matrix/matrix.json"
DERIVED_PATH=PREFIX+"/consumer-capacity.py"
DERIVED={"blob":"e873b4cedeb08dbeb44e594187307a7382e3932a","bytes":38134,"sha256":"678e163df14847203a0c71197afc58d617de7bacc437ed785c11a33dce25ee1c"}
FRAME_NAMES={
    "prepare":("source-before.json","source-after.json","stdout.log","stderr.log","receipt.json"),
    "replay":("source-before.json","source-after.json","capacity-results.json","aggregate-command.json",
              "aggregate-result.json","independent-matrix/matrix.json","remote-matrix-comparison.json",
              "stdout.log","stderr.log","receipt.json"),
}

def require(ok,message):
    if not ok: raise ValueError(message)

def sha(raw): return hashlib.sha256(raw).hexdigest()

def identity(path):
    raw=path.read_bytes()
    return {"blob":hashlib.sha1(b"blob "+str(len(raw)).encode()+b"\0"+raw).hexdigest(),
            "bytes":len(raw),"sha256":sha(raw)}

def git(root,*args):
    result=subprocess.run(["git","-C",str(root),*args],stdout=subprocess.PIPE,
                          stderr=subprocess.DEVNULL,timeout=20,check=False)
    require(result.returncode==0,"read-only Git check failed")
    return result.stdout

def controller_snapshot(root):
    head=os.environ["GITHUB_SHA"]
    require(git(root,"rev-parse","HEAD").decode().strip()==head,"controller checkout differs")
    result={}
    for relative in (ORIGINAL_PATH,DERIVED_PATH,PREFIX+"/replay.py",PREFIX+"/plan.json",WORKFLOW):
        path=root/relative
        require(path.resolve(strict=True)==path and path.is_file() and not path.is_symlink(),"controller path differs")
        item=identity(path)
        require(path.read_bytes()==git(root,"show",head+":"+relative)
                and item["blob"]==git(root,"rev-parse",head+":"+relative).decode().strip(),"controller bytes differ")
        if relative==ORIGINAL_PATH: require(item==ORIGINAL,"original receiver changed")
        if relative==DERIVED_PATH: require(item==DERIVED,"capacity receiver changed")
        result[relative]=item
    return {"commit":head,"files":result}

def load_original(controller):
    require(identity(controller/ORIGINAL_PATH)==ORIGINAL,"original receiver identity differs")
    require(identity(controller/DERIVED_PATH)==DERIVED,"capacity receiver identity differs")
    spec=importlib.util.spec_from_file_location("fixed_capacity_receiver",controller/DERIVED_PATH)
    module=importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

def emit_file(name,path,expected):
    size=path.stat().st_size
    require(size<=(MAX_MATRIX_FRAME_FILE if name==MATRIX_FRAME_NAME else MAX_FRAME_FILE),
            "safe report exceeds fixed frame bound")
    raw=path.read_bytes()
    require(len(raw)==size and {"bytes":size,"sha256":sha(raw)}==expected,"framed file changed")
    count=(size+1023)//1024
    common={"schema":"p8-fixed275e-full-reception-frame-v1","name":name}
    def output(value):
        line=json.dumps(dict(common,**value),sort_keys=True,separators=(",",":"))
        require(len(line.encode())<=2048,"safe frame exceeds2048")
        print(line,flush=True)
    output({"kind":"header","bytes":size,"sha256":sha(raw),"count":count,"raw_chunk_bytes":1024})
    for index in range(count):
        block=raw[index*1024:(index+1)*1024]
        output({"kind":"chunk","index":index,"count":count,"bytes":len(block),"sha256":sha(block),
                "base64":base64.b64encode(block).decode("ascii")})
    output({"kind":"end","bytes":size,"sha256":sha(raw),"count":count})

def frame_inventory(original,review,mode):
    rows={}
    for name in FRAME_NAMES[mode]:
        path=review/name
        if not path.exists(): continue
        original.regular_tree_path(path,False)
        full=mode+"/"+name
        require(path.stat().st_size<=(MAX_MATRIX_FRAME_FILE if full==MATRIX_FRAME_NAME else MAX_FRAME_FILE),
                "safe report exceeds fixed frame bound")
        rows[full]=original.file_row(path)
    return rows

def previous_delivery(original,owned,controller_commit,run_id):
    review=owned/"prepare-review"
    path=review/"delivery-receipt.json"
    original.regular_tree_path(path,False)
    require(path.stat().st_size<=64*1024,"prepare delivery receipt exceeds bound")
    prior=original.json_load(path)
    files=frame_inventory(original,review,"prepare")
    require(set(files)=={"prepare/"+name for name in FRAME_NAMES["prepare"]},
            "prepare framed file population differs")
    require(prior["schema"]=="fixed275e-full-frame-delivery-v1" and prior["mode"]=="prepare"
            and prior["status"]=="passed" and prior["original_step_status"]=="passed"
            and prior["controller_commit"]==controller_commit and prior["reception_run_id"]==int(run_id)
            and prior["measurement_run"]==original.RUN and prior["source"]==original.SOURCE
            and prior["files"]==files and prior["emitted_files"]==list(files)
            and prior["cumulative_raw_bytes"]==sum(row["bytes"] for row in files.values())
            and prior["cumulative_raw_bytes"]<=MAX_FRAME_TOTAL
            and prior["limits"]=={"ordinary_file":MAX_FRAME_FILE,"matrix_file":MAX_MATRIX_FRAME_FILE,
                                 "total_raw":MAX_FRAME_TOTAL,"decoder_log":MAX_DECODER_LOG},
            "prepare delivery/bytes differ")
    return {"receipt":original.file_row(path),"files":files}

def deliver_frames(original,owned,review,mode,result,previous):
    delivery={"schema":"fixed275e-full-frame-delivery-v1","mode":mode,"status":"failed",
              "original_step_status":result["status"],"controller_commit":result["controller_commit"],
              "reception_run_id":result["reception_run_id"],"measurement_run":original.RUN,
              "source":original.SOURCE,"files":{},"emitted_files":[],
              "limits":{"ordinary_file":MAX_FRAME_FILE,"matrix_file":MAX_MATRIX_FRAME_FILE,
                        "total_raw":MAX_FRAME_TOTAL,"decoder_log":MAX_DECODER_LOG},
              "measurement_outcome_unchanged":True}
    okay=False
    try:
        prior_files={}
        if mode=="replay":
            current=previous_delivery(original,owned,result["controller_commit"],result["reception_run_id"])
            require(previous==current,"prepare delivery changed during replay")
            prior_files=current["files"]
            delivery["prepare_delivery_receipt"]=current["receipt"]
        files=frame_inventory(original,review,mode)
        if result["status"]=="passed":
            require(set(files)=={mode+"/"+name for name in FRAME_NAMES[mode]},"framed file population differs")
        delivery["files"]=dict(prior_files,**files)
        delivery["cumulative_raw_bytes"]=sum(row["bytes"] for row in delivery["files"].values())
        require(delivery["cumulative_raw_bytes"]<=MAX_FRAME_TOTAL,"cumulative raw frame budget exceeded")
        for name,row in files.items():
            emit_file(name,review/name.split("/",1)[1],row)
            delivery["emitted_files"].append(name)
        require(frame_inventory(original,review,mode)==files,"framed files changed during delivery")
        if mode=="replay":
            require(previous_delivery(original,owned,result["controller_commit"],result["reception_run_id"])==previous,
                    "prepare delivery changed during output")
        delivery["status"]="passed"
        okay=True
    except BaseException as error:
        delivery["error"]=original.safe_error(error)
    delivery["completed_utc"]=original.stamp()
    original.save_new(review/"delivery-receipt.json",delivery)
    print(json.dumps({"schema":"fixed275e-full-frame-delivery-result-v1","mode":mode,"status":delivery["status"],
                      "original_step_status":result["status"],"emitted_files":len(delivery["emitted_files"]),
                      "cumulative_raw_bytes":delivery.get("cumulative_raw_bytes"),
                      "receipt_sha256":original.digest(review/"delivery-receipt.json"),
                      "measurement_outcome_unchanged":True},sort_keys=True),flush=True)
    return okay

def main():
    require(not sys.flags.optimize,"optimized Python forbidden")
    parser=argparse.ArgumentParser()
    parser.add_argument("mode",choices=("prepare","replay"))
    args=parser.parse_args()
    workspace=P(os.environ["GITHUB_WORKSPACE"]).resolve(strict=True)
    controller,source=workspace/"controller",workspace/"source"
    run_id=os.environ["GITHUB_RUN_ID"]
    require(run_id.isdecimal() and os.environ["GITHUB_RUN_ATTEMPT"]=="1","first actual attempt required")
    require(os.environ["GITHUB_REPOSITORY"]=="jyqj/codecortex" and os.environ["GITHUB_EVENT_NAME"]=="push"
            and os.environ["GITHUB_REF"]==BRANCH,"wrong reception execution")
    require(P(__file__).resolve()==controller/PREFIX/"replay.py","wrong wrapper path")
    original=load_original(controller)
    owned=P(os.environ["RUNNER_TEMP"]).resolve(strict=True)/("p8-formal275e-full-reception-"+run_id)
    created=False
    review=None
    before=None
    input_before=None
    previous=None
    success=False
    result={"schema":"fixed275e-full-reception-step-v1","mode":args.mode,"status":"failed",
            "source":original.SOURCE,"source_tree":original.TREE,"measurement_run":original.RUN,
            "controller_commit":os.environ["GITHUB_SHA"],"reception_run_id":int(run_id),"reception_attempt":1,
            "started_utc":original.stamp(),"new_measurements":0,"TODO_closed":0,"TODO_remaining":29}
    try:
        original.regular_tree_path(controller);original.regular_tree_path(source)
        if args.mode=="prepare":
            require(not owned.exists(),"fresh reception root already exists")
            owned.mkdir()
        else:
            original.regular_tree_path(owned)
        review=owned/(args.mode+"-review")
        require(not review.exists(),"review directory already exists")
        review.mkdir()
        created=True
        before={"controller":controller_snapshot(controller)}
        sys.dont_write_bytecode=True
        sys.path.insert(0,str(source/"scripts"))
        import p8_scale_matrix as matrix
        require(P(matrix.__file__).resolve()==source/"scripts/p8_scale_matrix.py","wrong imported original matrix")
        before["source"]=original.source_identity(source,matrix)
        original.save_new(review/"source-before.json",before)
        result["original_function_sha256"]={name:sha(inspect.getsource(getattr(matrix,name)).encode())
            for name in ("validate_build","validate_shard","inspect_raw","native_digest","combine","aggregate")}
        result["immutable_receiver_function_sha256"]={name:sha(inspect.getsource(getattr(original,name)).encode())
            for name in ("prepare","replay","register","saved_registration","source_identity","capacity_review","zip_inventory","expand","original_bytes","compare_remote_matrix")}
        transport=owned/"transport"
        if args.mode=="prepare":
            require(os.environ.get("GH_TOKEN"),"download token missing")
            require(not transport.exists(),"transport directory already exists")
            transport.mkdir()
        else:
            original.regular_tree_path(transport)
            prior=original.json_load(owned/"prepare-review/receipt.json")
            require(prior["status"]=="passed" and prior["controller_commit"]==os.environ["GITHUB_SHA"]
                    and prior["measurement_run"]==original.RUN,"prepare did not pass under same receiver")
            previous=previous_delivery(original,owned,os.environ["GITHUB_SHA"],run_id)
            result["prepare_frame_delivery"]=previous
            input_before=original.inventory(transport)
        with (review/"stdout.log").open("x") as stdout,(review/"stderr.log").open("x") as stderr:
            with contextlib.redirect_stdout(stdout),contextlib.redirect_stderr(stderr):
                result["result"]=original.prepare(transport,matrix) if args.mode=="prepare" else original.replay(transport,source,matrix,review)
        success=True
    except BaseException as error:
        result["error"]=original.safe_error(error)
    finally:
        if created:
            try:
                after={"controller":controller_snapshot(controller),"source":original.source_identity(source,matrix)}
                original.save_new(review/"source-after.json",after)
                require(before==after,"full source/observer/controller changed")
                if input_before is not None: require(original.inventory(transport)==input_before,"original transport inputs changed")
                result["source_and_original_inputs_unchanged"]=True
            except BaseException as error:
                result["source_after_error"]=original.safe_error(error)
                success=False
            result["status"]="passed" if success else "failed"
            result["completed_utc"]=original.stamp()
            result["output_inventory_before_receipt"]=original.inventory(review)
            original.save_new(review/"receipt.json",result)
            delivered=deliver_frames(original,owned,review,args.mode,result,previous)
            success=success and delivered
        else:
            print(json.dumps({"schema":result["schema"],"mode":args.mode,"status":"failed_before_owned_review",
                              "error":result.get("error")},sort_keys=True))
    return 0 if success else 2

if __name__=="__main__":
    raise SystemExit(main())
