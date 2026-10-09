#!/usr/bin/env python3
"""Run a frozen offline original-slice validator; only a newly created container is cleaned."""
import base64,hashlib,json,os,pathlib,shutil,subprocess,time
P=pathlib.Path
project=P("/Users/jin/Desktop/codecortex-rust")
root=project/"artifacts/checkpoints/p8-round19-scale-intake-20261009-root"
source=project/"artifacts/checkpoints/p8-round19-evidence-intake-20261009/platform275e-author/evidence/source"
build=root/"original-build-validation/build-copy"
slices=root/"original-slices-001"
owned=root/"first-slices-validation-001"
assert all(p.resolve(strict=True)==p for p in (project,root,source,build,slices))
assert not owned.exists() and shutil.disk_usage(root).free>=512*1024**2+64*1024**2
owned.mkdir()
control=owned/"validate_original_slices.py"
oid="4a66aa4145f9e9ae29567fed811248b153276713"
wanted="81f16b18296497bc321401ade8d9628d4086b854d3def34581e5c1cebc089fcb"
with (owned/"control-git-response.json").open("xb") as stream:
    cp=subprocess.run(["gh","api","repos/jyqj/codecortex/git/blobs/"+oid],cwd=project,stdout=stream,stderr=subprocess.DEVNULL,timeout=30)
assert cp.returncode==0
packet=json.loads((owned/"control-git-response.json").read_bytes())
body=base64.b64decode(packet["content"])
assert packet["sha"]==oid and packet["encoding"]=="base64" and len(body)==10972
assert hashlib.sha256(body).hexdigest()==wanted
assert hashlib.sha1(b"blob "+str(len(body)).encode()+b"\0"+body).hexdigest()==oid
with control.open("xb") as stream: stream.write(body)
alternate=(source/".git/objects/info/alternates").read_text()
assert len(alternate.splitlines())==1
objects=P(alternate.strip())
assert objects==project/".git/objects" and objects.resolve(strict=True)==objects
output=owned/"output";output.mkdir()
docker="/usr/local/bin/docker"
image="python@sha256:3d361d7fea344d55ac7a0f51ed7faa99213808ccc96fc9b9170adb95b6570b96"
record={"schema":"p8-275e-first-original-slices-launch-v1","controller_blob":oid,"controller_sha256":wanted,
        "started_utc":time.strftime("%Y-%m-%dT%H:%M:%SZ",time.gmtime()),"only_original_ELF_hash_commands":True,
        "new_measurement_or_Cargo":False,"TODO_closed":0,"TODO_remaining":29}
cid=None
def invoke(label,argv,timeout=30):
    with (owned/(label+".stdout")).open("xb") as out,(owned/(label+".stderr")).open("xb") as err:
        done=subprocess.run(argv,cwd=project,stdin=subprocess.DEVNULL,stdout=out,stderr=err,timeout=timeout)
    record[label]={"argv":argv,"exit_code":done.returncode}
    assert done.returncode==0,label+" failed; complete outputs retained"
    return (owned/(label+".stdout")).read_bytes()
try:
    inspected=json.loads(invoke("image-inspect",[docker,"image","inspect","--platform","linux/amd64",image]))
    assert len(inspected)==1 and inspected[0]["Id"]=="sha256:3d361d7fea344d55ac7a0f51ed7faa99213808ccc96fc9b9170adb95b6570b96"
    assert inspected[0]["Architecture"]=="amd64" and inspected[0]["Os"]=="linux"
    argv=[docker,"create","--name=p8-275e-first-slices-root-20261009-001","--pull=never","--network=none","--read-only",
          "--platform=linux/amd64","--user",str(os.getuid())+":"+str(os.getgid()),
          "--cap-drop=ALL","--security-opt=no-new-privileges","--pids-limit=128","--memory=2g","--cpus=1",
          "--env=PYTHONDONTWRITEBYTECODE=1","--env=GIT_OPTIONAL_LOCKS=0"]
    for path in (source,objects,build,slices):
        argv+=["--mount","type=bind,src="+str(path)+",dst="+str(path)+",readonly"]
    argv+=["--mount","type=bind,src="+str(control)+",dst=/validate_original_slices.py,readonly",
           "--mount","type=bind,src="+str(output)+",dst=/output",image,
           "python3","-B","/validate_original_slices.py",str(source),str(build),str(slices)]
    cid=invoke("create",argv).decode().strip()
    assert len(cid)==64 and all(c in "0123456789abcdef" for c in cid)
    invoke("run",[docker,"start","--attach",cid],180)
    status=json.loads(invoke("inspect",[docker,"inspect",cid]))[0]
    assert status["State"]["Status"]=="exited" and status["State"]["ExitCode"]==0
    assert status["HostConfig"]["NetworkMode"]=="none" and status["HostConfig"]["ReadonlyRootfs"] is True
    mounts={m["Destination"]:m for m in status["Mounts"]}
    assert set(mounts)=={str(p) for p in (source,objects,build,slices)}|{"/validate_original_slices.py","/output"}
    assert all(m["RW"] is (path=="/output") for path,m in mounts.items())
    accepted=json.loads((output/"inspection.json").read_bytes())
    assert accepted["status"]=="accepted_scoped_partial_originals" and accepted["validated_shards"]==3 and accepted["validated_capacity"]==5 and accepted["validated_samples"]==32
    assert accepted["complete_N150_accepted"] is False and accepted["new_measurement_or_Cargo"] is False
    assert hashlib.sha256(control.read_bytes()).hexdigest()==wanted
    record.update(status="passed",container_id=cid,original_shards=3,original_capacity=5,original_samples=32,
                  only_output_mount_writable=True,inspection_sha256=hashlib.sha256((output/"inspection.json").read_bytes()).hexdigest())
except BaseException as error:
    record.update(status="failed",exception_type=type(error).__name__,error=str(error))
    raise
finally:
    if cid is not None:
        assert len(cid)==64 and all(c in "0123456789abcdef" for c in cid)
        cleanup=subprocess.run([docker,"rm","-f",cid],cwd=project,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=20)
        (owned/"cleanup.stdout").write_bytes(cleanup.stdout)
        (owned/"cleanup.stderr").write_bytes(cleanup.stderr)
        record["owned_container_cleanup_exit_code"]=cleanup.returncode
    record["finished_utc"]=time.strftime("%Y-%m-%dT%H:%M:%SZ",time.gmtime())
    with (owned/"launch.json").open("x") as stream: json.dump(record,stream,sort_keys=True,indent=2);stream.write("\n")
    print(json.dumps(record))
