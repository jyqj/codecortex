#!/usr/bin/env python3
"""Fixed-ten original evidence transport and partial replay; no measurements."""
import argparse,base64,contextlib,hashlib,importlib.util,inspect,json,os,pathlib,platform,subprocess,sys,time,zipfile
P=pathlib.Path
PREFIX="artifacts/checkpoints/p8-round21-formal275e-four-shard-reception-20261009"
BRANCH="refs/heads/task/p8-formal275e-four-shard-reception-20261009"
ORIGINAL_PATH="artifacts/checkpoints/p8-round20-evidence-20261009/consumer/consumer.py"
ORIGINAL={"blob":"fbd6ca6c27adc7af855fc4e4ece25c95d6195213","bytes":38114,"sha256":"dd19c6c07ddbde0cf797828c2860d2df81bd0e144465f1afdd17930863e0fecc"}
WORKFLOW=".github/workflows/p8-formal275e-four-shard-reception.yml"
PINS=json.loads("{\"p8-scale-build-37896198208\":{\"id\":11600309282,\"bytes\":9218801,\"sha256\":\"9c13733e1bdb207301920f3dddb0bde4dcab278b31e4cdfd0dc8bc1f3c75ab67\"},\"p8-scale-shard-10000-0-37896198208\":{\"id\":11601336788,\"bytes\":853841,\"sha256\":\"33944d8b9c895b62064ce89dc5d99a476b78cbcc97c2f9a57ccf190571e20374\"},\"p8-scale-shard-1000-0-37896198208\":{\"id\":11601142045,\"bytes\":952777,\"sha256\":\"242eb240b06cccb6ffc6aa210a44a0985f969308661e7ad92b8451735c445171\"},\"p8-scale-capacity-5000-0-37896198208\":{\"id\":11601130609,\"bytes\":786,\"sha256\":\"568d505fe7da83f8df0679a2932405dbc5c042fe89d45fdd67066210097f45cf\"},\"p8-scale-capacity-100000-0-37896198208\":{\"id\":11601087740,\"bytes\":786,\"sha256\":\"7b830f3444bfa18a386775813730d9f8945f85a76ba30fae92eca8c448270311\"},\"p8-scale-shard-5000-0-37896198208\":{\"id\":11600946382,\"bytes\":583828,\"sha256\":\"7d994e6d922d477ee07c5bb7ea478f180d927b7c108a965bff675be54a52cbb2\"},\"p8-scale-capacity-50000-0-37896198208\":{\"id\":11600574581,\"bytes\":785,\"sha256\":\"abb0eebb51584c58f9ce243a229498acb7e5e4b51f554bb763b6e602e7f9ce24\"},\"p8-scale-capacity-10000-0-37896198208\":{\"id\":11600444517,\"bytes\":785,\"sha256\":\"4fd49d2682ec5279689c4dd47bdf353881e407b869b92d9978b6f5b9504ea66e\"},\"p8-scale-capacity-1000-0-37896198208\":{\"id\":11600229155,\"bytes\":783,\"sha256\":\"65c283361ecf9f6b1af395cd2fb51c2c14e8f5d74a2bd94110056ba314e623be\"},\"p8-scale-shard-50000-0-37896198208\":{\"id\":11603614090,\"bytes\":2964892,\"sha256\":\"5ec468722f22f6802e643bc74844f04337e83ad42bbcdee15fe63208ed200773\"}}")
SCALES=(1000,5000,10000,50000)
MAX_FRAME_FILE=4*1024**2

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
    for relative in (ORIGINAL_PATH,PREFIX+"/replay.py",PREFIX+"/plan.json",WORKFLOW):
        path=root/relative
        require(path.resolve(strict=True)==path and path.is_file() and not path.is_symlink(),"controller path differs")
        item=identity(path)
        require(path.read_bytes()==git(root,"show",head+":"+relative)
                and item["blob"]==git(root,"rev-parse",head+":"+relative).decode().strip(),"controller bytes differ")
        if relative==ORIGINAL_PATH: require(item==ORIGINAL,"original receiver changed")
        result[relative]=item
    return {"commit":head,"files":result}

def load_original(controller):
    require(identity(controller/ORIGINAL_PATH)==ORIGINAL,"original receiver identity differs")
    spec=importlib.util.spec_from_file_location("fixed_original_receiver",controller/ORIGINAL_PATH)
    module=importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    return module

def select_fixed(all_registered,original):
    by_name={entry["name"]:entry for entry in all_registered["artifacts"]}
    require(set(PINS)<=set(by_name),"one or more registered ten artifacts missing")
    selected=[]
    for name,pin in sorted(PINS.items()):
        entry=by_name[name]
        require((entry["artifact_id"],entry["bytes"],entry["sha256"])==
                (pin["id"],pin["bytes"],pin["sha256"]),"fixed-ten metadata changed")
        selected.append(entry)
    require(len(selected)==10 and sum(e["role"]=="build" for e in selected)==1
            and sum(e["role"]=="capacity" for e in selected)==5
            and sum(e["role"]=="shard" for e in selected)==4,"selected-ten roles differ")
    return selected

def saved_full_snapshot(directory,original):
    return {"run":original.json_load(directory/"run.json"),
            "jobs":original.saved_pages(directory,"jobs"),
            "artifacts":original.saved_pages(directory,"artifacts")}

def audit_selection(transport,original):
    before=original.register(saved_full_snapshot(transport/"metadata/before",original))
    after=original.register(saved_full_snapshot(transport/"metadata/after",original))
    require(before==original.json_load(transport/"full-registration-before.json")
            and after==original.json_load(transport/"full-registration-after.json"),"full API registration differs")
    original.selected_entries_unchanged(before,after)
    selected=select_fixed(before,original)
    require(selected==select_fixed(after,original),"selected original identities changed")
    selection=original.json_load(transport/"fixed-selection.json")
    expected={"schema":"fixed-ten-original-selection-v1","run":original.RUN,"source":original.SOURCE,
              "selected":selected,"selected_count":10,
              "full_before_artifact_count":len(before["artifacts"]),
              "unselected_before":[e["name"] for e in before["artifacts"] if e["name"] not in PINS],
              "scope":"This selection is not a complete301 registration or an aggregate input."}
    require(selection==expected,"fixed selection declaration changed")
    return before,after,selected

def prepare_fixed10(transport,source,matrix,original):
    (transport/"metadata").mkdir()
    before=original.register(original.metadata_snapshot(transport/"metadata/before"))
    selected=select_fixed(before,original)
    original.save_new(transport/"full-registration-before.json",before)
    original.save_new(transport/"fixed-selection.json",{
        "schema":"fixed-ten-original-selection-v1","run":original.RUN,"source":original.SOURCE,
        "selected":selected,"selected_count":10,"full_before_artifact_count":len(before["artifacts"]),
        "unselected_before":[e["name"] for e in before["artifacts"] if e["name"] not in PINS],
        "scope":"This selection is not a complete301 registration or an aggregate input."})
    originals,expanded=transport/"originals",transport/"expanded"
    originals.mkdir();expanded.mkdir()
    import shutil
    need=sum(e["bytes"] for e in selected)
    require(shutil.disk_usage(transport).free>=need+original.HEADROOM+original.OUTPUT_ALLOWANCE,
            "selected ZIPs would consume unchanged reserve")
    proofs=[]
    for entry in selected:
        archive=originals/(str(entry["artifact_id"])+".zip")
        transfer=original.gh_to_file("repos/jyqj/codecortex/actions/artifacts/"+str(entry["artifact_id"])+"/zip",
                                     archive,1800,entry["bytes"])
        require(original.file_row(archive)=={"bytes":entry["bytes"],"sha256":entry["sha256"]},"original ZIP differs")
        with zipfile.ZipFile(archive) as package:
            require(sum(i.file_size for i in package.infolist())<=original.MAX_EXPANDED_PER_ZIP,"ZIP expansion bound")
        members=original.zip_inventory(archive,entry)
        count=sum(v["bytes"] for v in members.values())
        require(count<=original.MAX_EXPANDED_PER_ZIP,"ZIP expansion bound")
        proof={"entry":entry,"transfer":transfer,"members":members,"expanded_bytes":count}
        original.save_new(originals/(str(entry["artifact_id"])+".inventory.json"),proof)
        proofs.append(proof)
    require(shutil.disk_usage(transport).free>=sum(p["expanded_bytes"] for p in proofs)+original.HEADROOM+original.OUTPUT_ALLOWANCE,
            "expansions would consume unchanged reserve")
    for proof in proofs:
        entry=proof["entry"]
        require(shutil.disk_usage(transport).free>=proof["expanded_bytes"]+original.HEADROOM+original.OUTPUT_ALLOWANCE,
                "next expansion would consume reserve")
        target=original.expand(originals/(str(entry["artifact_id"])+".zip"),expanded/entry["name"],proof["members"],matrix)
        if entry["role"]=="build": (target/"p8-scale").chmod(0o755)
        require(matrix.inventory(target)==proof["members"],"expansion changed original bytes")
    after=original.register(original.metadata_snapshot(transport/"metadata/after"))
    original.save_new(transport/"full-registration-after.json",after)
    new_names=original.selected_entries_unchanged(before,after)
    require(select_fixed(after,original)==selected,"fixed selection changed after transfer")
    # A narrow envelope is used only by the immutable byte/closure helper, not register or aggregate.
    original.original_bytes(transport,{"artifacts":selected},matrix,full_crc=False)
    audit_selection(transport,original)
    return {"status":"transport_complete_fixed10_not_measurement_qualification",
            "selected_artifacts":10,"zip_bytes":need,"expanded_bytes":sum(p["expanded_bytes"] for p in proofs),
            "full_before_artifacts":len(before["artifacts"]),"full_after_artifacts":len(after["artifacts"]),
            "unselected_after":[e["name"] for e in after["artifacts"] if e["name"] not in PINS],
            "new_artifacts_after_selection_not_imported":new_names,
            "complete_full150_aggregate_executed":False}

def replay_fixed4(transport,source,review,matrix,original):
    require(platform.system()=="Linux" and platform.machine()=="x86_64","original ELF requires Linux amd64")
    require(not any(os.environ.get(k) for k in ("GH_TOKEN","GITHUB_TOKEN","ACTIONS_RUNTIME_TOKEN")),"replay received token")
    before_reg,after_reg,selected=audit_selection(transport,original)
    proofs=original.original_bytes(transport,{"artifacts":selected},matrix,full_crc=True)
    directories={e["name"]:transport/"expanded"/e["name"] for e in selected}
    build=directories["p8-scale-build-"+str(original.RUN)]
    built,binary=matrix.validate_build(build,root=source)
    require(matrix.source_snapshot(source)==matrix.read_json(build/"source-before.json"),"full1089 source differs")
    build_sha=matrix.file_sha256(build/"build.json")
    capacities=[]
    for entry in selected:
        if entry["role"]=="capacity":
            result=original.capacity_review(directories[entry["name"]],entry["scale"],
                original.OBSERVERS["scripts/p8_runner_capacity.py"],matrix,entry["github_job"])
            result.update(artifact_id=entry["artifact_id"],job_id=entry["job_id"],index=entry["index"])
            capacities.append(result)
            original.save_new(review/("capacity-"+str(entry["scale"])+".json"),result)
    require(len(capacities)==5,"capacity population differs")
    shards=[]
    for scale in SCALES:
        entry=next(e for e in selected if e["role"]=="shard" and e["scale"]==scale and e["index"]==0)
        result=matrix.validate_shard(directories[entry["name"]],built,binary,build_sha)
        expected=matrix.registered_plan(scale,0,30,30,12648430,18000000,matrix.CAPACITY_PROFILE)
        require(matrix.exact_equal(result["plan"],expected),"original plan differs")
        require(len(result["measurements"])==(14 if scale==1000 else 9),"original stage population differs")
        original.save_new(review/("validated-"+str(scale)+".json"),result)
        shards.append({"artifact_id":entry["artifact_id"],"job_id":entry["job_id"],"scale":scale,"index":0,"validation":result})
    require(len(shards)==4 and sum(len(s["validation"]["measurements"]) for s in shards)==41,"partial population differs")
    original.original_bytes(transport,{"artifacts":selected},matrix,full_crc=False)
    inspection={"schema":"fixed275e-four-original-shards-inspection-v1","status":"passed_partial_originals",
        "source":original.SOURCE,"run":original.RUN,"attempt":1,"build":built,"build_receipt_sha256":build_sha,
        "zip_proofs":proofs,"capacities":capacities,"shards":shards,
        "full_before_artifact_count":len(before_reg["artifacts"]),"full_after_artifact_count":len(after_reg["artifacts"]),
        "unselected_after":[e["name"] for e in after_reg["artifacts"] if e["name"] not in PINS],
        "accepted_original_shards":4,"required_shards":150,"original_samples":41,
        "full150_aggregate_executed":False,"native_scope":"Original matrix.native_digest --hash-file only",
        "network_scope":"credential-free validation; no network operations requested; host network not physically disabled",
        "TODO_closed":0,"TODO_remaining":29}
    original.save_new(review/"inspection.json",inspection)
    return {"status":"passed_partial_originals","original_shards":4,"required_shards":150,"samples":41,
            "capacity_receipts":5,"inspection_sha256":original.digest(review/"inspection.json"),
            "aggregate_executed":False,"new_measurements":0}

def emit_file(name,path):
    size=path.stat().st_size
    require(size<=MAX_FRAME_FILE,"safe report exceeds fixed frame bound")
    raw=path.read_bytes()
    require(len(raw)==size,"framed file changed")
    count=(size+1023)//1024
    common={"schema":"p8-fixed275e-four-shard-reception-frame-v1","name":name}
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
    owned=P(os.environ["RUNNER_TEMP"]).resolve(strict=True)/("p8-formal275e-four-shard-reception-"+run_id)
    created=False
    review=None
    before=None
    input_before=None
    success=False
    result={"schema":"fixed275e-four-shard-reception-step-v1","mode":args.mode,"status":"failed",
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
            for name in ("prepare","register","source_identity","capacity_review","zip_inventory","expand","original_bytes")}
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
            input_before=original.inventory(transport)
        with (review/"stdout.log").open("x") as stdout,(review/"stderr.log").open("x") as stderr:
            with contextlib.redirect_stdout(stdout),contextlib.redirect_stderr(stderr):
                result["result"]=prepare_fixed10(transport,source,matrix,original) if args.mode=="prepare" else replay_fixed4(transport,source,review,matrix,original)
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
            for name in ("source-before.json","source-after.json","inspection.json","receipt.json"):
                path=review/name
                if path.exists(): emit_file(args.mode+"/"+name,path)
        else:
            print(json.dumps({"schema":result["schema"],"mode":args.mode,"status":"failed_before_owned_review",
                              "error":result.get("error")},sort_keys=True))
    return 0 if success else 2

if __name__=="__main__":
    raise SystemExit(main())
