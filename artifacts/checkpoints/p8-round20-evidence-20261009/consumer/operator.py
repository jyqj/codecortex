#!/usr/bin/env python3
"""DRAFT operator: one explicit fresh275e batch or one networkless offline replay."""
import argparse,base64,hashlib,json,os,pathlib,re,shutil,subprocess,sys,time
P=pathlib.Path
if sys.flags.optimize: raise SystemExit("optimized Python forbidden")
PROJECT=P("/Users/jin/Desktop/codecortex-rust")
ROOT=PROJECT/"artifacts/checkpoints/p8-round20-formal275e-full-consumer"
SOURCE=PROJECT/"artifacts/checkpoints/p8-round19-evidence-intake-20261009/platform275e-author/evidence/source"
IMAGE="python@sha256:3d361d7fea344d55ac7a0f51ed7faa99213808ccc96fc9b9170adb95b6570b96"
OID="fbd6ca6c27adc7af855fc4e4ece25c95d6195213"
SHA="dd19c6c07ddbde0cf797828c2860d2df81bd0e144465f1afdd17930863e0fecc"
SIZE=38114
def require(value,message):
    if not value: raise ValueError(message)
def safe_error(error):
    frames=[]
    tb=error.__traceback__
    while tb:
        frames.append({"file":P(tb.tb_frame.f_code.co_filename).name,"function":tb.tb_frame.f_code.co_name,"line":tb.tb_lineno})
        tb=tb.tb_next
    return {"type":type(error).__name__,"frames":frames}
def file_row(path):
    data=path.read_bytes()
    return {"bytes":len(data),"sha256":hashlib.sha256(data).hexdigest()}
def main():
    parser=argparse.ArgumentParser()
    parser.add_argument("mode",choices=("prepare","replay"))
    parser.add_argument("--batch",required=True)
    args=parser.parse_args()
    require(re.fullmatch(r"batch-[0-9]{3}",args.batch),"invalid explicit batch ID")
    require(PROJECT.resolve(strict=True)==PROJECT and SOURCE.resolve(strict=True)==SOURCE,"noncanonical fixed input")
    require(ROOT.parent.resolve(strict=True)==ROOT.parent,"noncanonical owned parent")
    if args.mode=="prepare" and not ROOT.exists(): ROOT.mkdir()
    require(ROOT.resolve(strict=True)==ROOT,"noncanonical owned root")
    batch=ROOT/args.batch
    if args.mode=="prepare":
        require(not batch.exists(),"existing batch cannot be retried or overwritten")
        batch.mkdir()
    require(batch.resolve(strict=True)==batch,"noncanonical existing batch")
    own=batch/(args.mode+"-operator")
    require(not own.exists(),"this explicit operation already has retained output")
    own.mkdir()
    control=batch/"consumer.py"
    evidence=batch/"transport"
    record={"schema":"p8-275e-original-full150-operator-v1","mode":args.mode,"batch":args.batch,
            "controller_blob":OID,"controller_sha256":SHA,"started_utc":time.strftime("%Y-%m-%dT%H:%M:%SZ",time.gmtime()),
            "new_measurements":0,"TODO_closed":0,"TODO_remaining":29}
    cid=None
    okay=False
    def invoke(label,argv,timeout=30):
        row={"argv":argv,"cwd":str(PROJECT),"timeout_seconds":timeout,"status":"started"}
        record[label]=row
        with (own/(label+".stdout")).open("xb") as stdout,(own/(label+".stderr")).open("xb") as stderr:
            try:
                done=subprocess.run(argv,cwd=PROJECT,stdin=subprocess.DEVNULL,stdout=stdout,stderr=stderr,timeout=timeout)
            except BaseException as error:
                row.update(status="interrupted_or_failed",error=safe_error(error))
                raise
        row.update(status="finished",exit_code=done.returncode)
        require(done.returncode==0,label+" failed; original output retained")
        return own/(label+".stdout")
    try:
        require(shutil.disk_usage(batch).free>=512*1024**2+128*1024**2,"receiver reserve unavailable")
        if args.mode=="prepare":
            response=invoke("controller-git-get",["gh","api","repos/jyqj/codecortex/git/blobs/"+OID],60)
            packet=json.loads(response.read_bytes())
            body=base64.b64decode(packet["content"])
            require(packet["sha"]==OID and packet["encoding"]=="base64" and len(body)==SIZE,"fixed controller identity")
            require(hashlib.sha256(body).hexdigest()==SHA and
                    hashlib.sha1(b"blob "+str(len(body)).encode()+b"\0"+body).hexdigest()==OID,"controller full bytes differ")
            with control.open("xb") as stream: stream.write(body)
            require(not evidence.exists(),"prepare transport must be new")
            invoke("prepare",["/Users/jin/.local/bin/python3.14","-B",str(control),"prepare","--evidence-root",str(evidence)],7200)
            accepted=json.loads((evidence/"review/receipt.json").read_bytes())
            require(accepted["status"]=="passed" and accepted["mode"]=="prepare" and
                    accepted["result"]["status"]=="transport_complete_not_measurement_qualified","transport did not complete")
            record["complete_core_301"]=accepted["result"]["complete_core_301"]
            record["measurement_qualification"]="not_run"
        else:
            require(file_row(control)=={"bytes":SIZE,"sha256":SHA},"retained controller changed")
            require(evidence.resolve(strict=True)==evidence,"noncanonical retained transport")
            original=json.loads((evidence/"transport-complete.json").read_bytes())
            require(original["status"]=="transport_complete_full_core" and original["complete_core_301"] is True,
                    "incomplete original core; do not launch partial aggregate")
            alternate=(SOURCE/".git/objects/info/alternates").read_text()
            require(len(alternate.splitlines())==1,"different source object layout")
            objects=P(alternate.strip())
            require(objects==PROJECT/".git/objects" and objects.resolve(strict=True)==objects,"different source Git objects")
            output=batch/"replay-output"
            require(not output.exists(),"existing offline result cannot be overwritten")
            output.mkdir()
            docker="/usr/local/bin/docker"
            inspected=json.loads(invoke("image-inspect",[docker,"image","inspect","--platform","linux/amd64",IMAGE]).read_bytes())
            require(len(inspected)==1 and inspected[0]["Id"]==IMAGE.split("@")[1] and
                    inspected[0]["Architecture"]=="amd64" and inspected[0]["Os"]=="linux","existing image differs")
            argv=[docker,"create","--name=p8-275e-full150-"+args.batch,"--pull=never","--network=none","--read-only",
                  "--platform=linux/amd64","--user",str(os.getuid())+":"+str(os.getgid()),
                  "--cap-drop=ALL","--security-opt=no-new-privileges","--pids-limit=128","--memory=2g","--cpus=1",
                  "--env=PYTHONDONTWRITEBYTECODE=1","--env=GIT_OPTIONAL_LOCKS=0"]
            for path in (SOURCE,objects,evidence):
                argv+=["--mount","type=bind,src="+str(path)+",dst="+str(path)+",readonly"]
            argv+=["--mount","type=bind,src="+str(control)+",dst=/consumer.py,readonly",
                   "--mount","type=bind,src="+str(output)+",dst=/output",IMAGE,
                   "python3","-B","/consumer.py","replay","--evidence-root",str(evidence),"--output","/output"]
            created=invoke("create",argv).read_text().strip()
            require(re.fullmatch(r"[0-9a-f]{64}",created),"invalid ID from owned create")
            cid=created
            invoke("replay",[docker,"start","--attach",cid],1800)
            status=json.loads(invoke("inspect",[docker,"inspect",cid]).read_bytes())[0]
            require(status["State"]["Status"]=="exited" and status["State"]["ExitCode"]==0 and
                    status["HostConfig"]["NetworkMode"]=="none" and status["HostConfig"]["ReadonlyRootfs"] is True,
                    "offline container state differs")
            mounts={m["Destination"]:m for m in status["Mounts"]}
            expected={str(p):str(p) for p in (SOURCE,objects,evidence)}
            expected.update({"/consumer.py":str(control),"/output":str(output)})
            require(set(mounts)==set(expected) and all(mounts[p]["Source"]==src and
                    mounts[p]["RW"] is (p=="/output") for p,src in expected.items()),"offline mounts differ")
            accepted=json.loads((output/"receipt.json").read_bytes())
            require(accepted["status"]=="passed" and accepted["mode"]=="replay" and
                    accepted["result"]["status"]=="accepted_original_275e_full_measurement_coverage" and
                    accepted["result"]["primary_shards"]==150 and accepted["result"]["capacity_receipts"]==150 and
                    accepted["result"]["samples"]==1500 and accepted["result"]["groups"]==50,"original aggregate was not accepted")
            record.update(complete_core_301=True,original_full_aggregate_passed=True,
                          only_original_ELF_hash_commands=True,only_output_mount_writable=True)
        require(file_row(control)=={"bytes":SIZE,"sha256":SHA},"fixed controller changed")
        record["receipt_sha256"]=hashlib.sha256((evidence/"review/receipt.json" if args.mode=="prepare" else output/"receipt.json").read_bytes()).hexdigest()
        okay=True
    except BaseException as error:
        record["error"]=safe_error(error)
    finally:
        if cid is not None:
            try:
                cleanup=invoke("cleanup-owned",[docker,"rm","-f",cid],20)
                record["owned_container_cleanup_succeeded"]=True
            except BaseException as error:
                okay=False
                record["cleanup_error"]=safe_error(error)
        record["status"]="passed" if okay else "failed_or_incomplete"
        record["finished_utc"]=time.strftime("%Y-%m-%dT%H:%M:%SZ",time.gmtime())
        with (own/"receipt.json").open("x") as stream: json.dump(record,stream,sort_keys=True,indent=2);stream.write("\n")
        print(json.dumps({"mode":args.mode,"status":record["status"],"batch":args.batch,
                          "receipt":str(own/"receipt.json"),"TODO_closed":0,"TODO_remaining":29}))
    return 0 if okay else 1
if __name__=="__main__":
    raise SystemExit(main())
