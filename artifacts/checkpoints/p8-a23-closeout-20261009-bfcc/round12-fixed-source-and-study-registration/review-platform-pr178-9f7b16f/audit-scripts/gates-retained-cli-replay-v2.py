import sys
sys.dont_write_bytecode=True
import pathlib,json,hashlib,subprocess,time,tarfile,shutil
base=pathlib.Path.cwd()
root=base/"review-platform-pr178-9f7b16f"/"gates-retained-cli"
original=base/"raw-pr178-9f7b16f"/"11600548850"/"extracted"
source=(base/"candidate-combined").resolve()
sys.path.insert(0,str(source/"scripts"))
from p7_build_identity import source_snapshot
from p8_cold_build import digest
source_before=source_snapshot(source)
assert source_before["source_commit"]=="9f7b16f0758eb79f306cf44605b84550f02de441"
report=json.loads((original/"report.json").read_text())
assert report["source"]==source_before
assert digest(root/"cc-eval")==digest(original/"cc-eval")==report["cli"]["executable_sha256"]=="e21fa0863c18ea447cf060384fc15ffe502ecb24f9a2627e2248c0373b037e9e"
original_before={p.relative_to(original).as_posix():digest(p) for p in original.rglob("*") if p.is_file()}
image="ubuntu@sha256:f610ab94648195aa356059f5b41d6085c9d4d903c072430cdd1af7bdb646106b"
inspect=json.loads(subprocess.check_output(["/usr/local/bin/docker","image","inspect",image],text=True))[0]
assert inspect["Architecture"]=="amd64" and inspect["Os"]=="linux"
assert inspect["Descriptor"]["digest"]==image.split("@")[1]
(root/"ubuntu-amd64-image-inspect.json").write_text(json.dumps(inspect,indent=2)+"\n")
arm=json.loads(subprocess.check_output(["/usr/local/bin/docker","image","inspect","ubuntu:24.04"],text=True))[0]
assert arm["Architecture"]=="arm64" and arm["Descriptor"]["digest"]=="sha256:534baea6a22c03a63003dbc8dbe78fe34bc0d7e595d9a9dc9834884ff530eb55"
cmd=["/usr/local/bin/docker","run","--rm","--pull=never","--network","none","--read-only","--platform","linux/amd64","--entrypoint","/usr/bin/getconf",image,"GNU_LIBC_VERSION"]
libc=subprocess.run(cmd,capture_output=True,timeout=10)
assert libc.returncode==0 and libc.stdout.strip()==b"glibc 2.39"
(root/"compatible-glibc-receipt.json").write_text(json.dumps({"command":cmd,"exit_code":libc.returncode,"stdout":libc.stdout.decode(),"stderr":libc.stderr.decode()},indent=2)+"\n")
outputs=root/"outputs-v2"
outputs.mkdir()
working=root/"inputs-copy-v2"
working.mkdir()
for index in (0,2,3,4,5):
    rel=pathlib.Path(f"case-{index:02}")/"retained-fixtures"
    (working/rel).parent.mkdir(parents=True,exist_ok=True)
    shutil.copytree(original/rel,working/rel)
working_before={p.relative_to(working).as_posix():digest(p) for p in working.rglob("*") if p.is_file()}
assert all(original_before[n]==h for n,h in working_before.items())
assert sum(p.stat().st_size for p in working.rglob("*") if p.is_file())==209265
fixtures={}
for index in (0,2,3,4,5):
    matches=list((original/f"case-{index:02}"/"retained-fixtures").iterdir())
    assert len(matches)==1
    fixtures[index]=matches[0]
def inside(path):
    return "/working/"+path.relative_to(original).as_posix()
policy=fixtures[2]/"policy.json"
cases=[
("quality_failed",fixtures[2]/"base",fixtures[2]/"quality",policy,1,"failed","quality-comparison.json"),
("latency_failed",fixtures[2]/"base",fixtures[2]/"latency",policy,1,"failed","latency-comparison.json"),
("insufficient_samples",fixtures[3]/"base",fixtures[3]/"candidate",fixtures[3]/"policy.json",1,"inconclusive","comparison.json"),
("zero_latency_denominator",fixtures[0]/"base",fixtures[0]/"candidate",policy,1,"inconclusive",None),
("raw_lock_drift",fixtures[4]/"base",fixtures[4]/"candidate",fixtures[4]/"policy.json",2,"invalid_measurement","comparison.json"),
("bad_policy",fixtures[5]/"missing-run",fixtures[5]/"missing-run",fixtures[5]/"policy.json",2,"invalid_measurement","comparison.json")]
results=[]
lastcmd=None
try:
    for name,b,c,p,expected_exit,expected_status,oldreport in cases:
        output=outputs/(name+".json")
        args=["compare","--baseline",inside(b),"--candidate",inside(c),"--gate",inside(p),"--output","/output/"+output.name]
        command=["/usr/local/bin/docker","run","--rm","--pull=never","--network","none","--read-only","--platform","linux/amd64","--mount",f"type=bind,source={root/'cc-eval'},target=/usr/local/bin/cc-eval,readonly","--mount",f"type=bind,source={original},target=/original,readonly","--mount",f"type=bind,source={working},target=/working","--mount",f"type=bind,source={outputs},target=/output","--entrypoint","/usr/local/bin/cc-eval",image,*args]
        start=time.monotonic()
        observed=subprocess.run(command,capture_output=True,timeout=20)
        (root/("v2-"+name+".stdout.log")).write_bytes(observed.stdout)
        (root/("v2-"+name+".stderr.log")).write_bytes(observed.stderr)
        data=json.loads(output.read_text()) if output.exists() else None
        row={"case":name,"command":command,"cli_argv":["cc-eval",*args],"expected_exit_code":expected_exit,"actual_exit_code":observed.returncode,"expected_status":expected_status,"actual_status":data.get("status") if data else None,"actual_report":data,"stdout_sha256":hashlib.sha256(observed.stdout).hexdigest(),"stderr_sha256":hashlib.sha256(observed.stderr).hexdigest(),"output_sha256":digest(output) if output.exists() else None,"wall_seconds_for_audit_only":time.monotonic()-start}
        results.append(row)
        assert observed.returncode==expected_exit and data and data["status"]==expected_status and data["exit_code"]==expected_exit,(name,row,observed.stderr.decode(errors="replace"))
        if name=="quality_failed":
            assert data["p95_ratio"]==1.0 and "Top-1 regression" in data["reasons"]
        elif name=="latency_failed":
            assert data["p95_ratio"]==10.0 and "p95 regression" in data["reasons"]
        elif name=="insufficient_samples":
            assert data["baseline_samples"]==data["candidate_samples"]==2 and "insufficient latency samples" in data["inconclusive_reasons"]
        elif name=="zero_latency_denominator":
            assert data["baseline_samples"]==data["candidate_samples"]==30 and data["p95_ratio"] is None
        elif name=="raw_lock_drift":
            assert data["reasons"]==["invalid benchmark input: raw response drift"]
        elif name=="bad_policy":
            assert any("JSON:" in x for x in data["reasons"])
        if oldreport:
            prior=json.loads((b.parent/oldreport).read_text())
            if expected_status=="invalid_measurement":
                assert data["reasons"]==prior["reasons"] and data["status"]==prior["status"] and data["exit_code"]==prior["exit_code"]
            else:
                assert data==prior
        lastcmd=command
    output=outputs/"bad_policy.json"
    before=output.read_bytes()
    start=time.monotonic()
    repeated=subprocess.run(lastcmd,capture_output=True,timeout=20)
    (root/"v2-output_overwrite_refusal.stdout.log").write_bytes(repeated.stdout)
    (root/"v2-output_overwrite_refusal.stderr.log").write_bytes(repeated.stderr)
    results.append({"case":"output_overwrite_refusal","command":lastcmd,"expected_exit_code":2,"actual_exit_code":repeated.returncode,"output_before_sha256":hashlib.sha256(before).hexdigest(),"output_after_sha256":digest(output),"existing_report_unchanged":output.read_bytes()==before,"stdout_sha256":hashlib.sha256(repeated.stdout).hexdigest(),"stderr_sha256":hashlib.sha256(repeated.stderr).hexdigest(),"stderr":repeated.stderr.decode(),"wall_seconds_for_audit_only":time.monotonic()-start})
    assert repeated.returncode==2 and output.read_bytes()==before
    assert {p.relative_to(original).as_posix():digest(p) for p in original.rglob("*") if p.is_file()}==original_before
    assert source_snapshot(source)==source_before and digest(root/"cc-eval")==report["cli"]["executable_sha256"]
    assert {p.relative_to(working).as_posix():digest(p) for p in working.rglob("*") if p.is_file()}==working_before
    final={"schema_version":1,"status":"accepted_all_7_original_retained_CLI_replays","source_commit":source_before["source_commit"],"artifact_id":11600548850,"artifact_zip_sha256":"db7136a640ce4a8861a1b72777968d0694e64c9b7c7125a0ce0224212da23016","retained_cli_sha256":report["cli"]["executable_sha256"],"retained_cli_bytes":(root/"cc-eval").stat().st_size,"image_reference":image,"image_id":inspect["Id"],"image_architecture":inspect["Architecture"],"image_manifest_digest":inspect["Descriptor"]["digest"],"original_arm64_tag_unchanged":True,"execution_context":"Docker Desktop on macOS arm64, linux/amd64 compatibility execution; deterministic fixture comparison only, not native platform/performance measurement","network":"none","original_artifact_mount":"read-only","original_artifact_files_unchanged":len(original_before),"working_fixture_files_before_after_equal":len(working_before),"working_fixture_bytes":209265,"working_fixture_manifest":working_before,"attempt_identity":{"number":1,"method_version":"retained-CLI-v2","note":"First G9f deterministic retained ELF comparison audit; no prior a23 execution or failure is relabeled as G9f evidence."},"cases":results,"zero_plan_control":"separately accepted original nonignored zero_measurement_gate_is_invalid Rust test; not relabeled as a CLI process","todo_status_change":False}
except BaseException as error:
    final={"schema_version":1,"status":"replay_failed","error":str(error),"cases":results,"todo_status_change":False}
finally:
    (root/"replay-report-v2.json").write_text(json.dumps(final,indent=2)+"\n")
print(json.dumps({"status":final["status"],"error":final.get("error"),"image":image,"retained_cli_sha256":report["cli"]["executable_sha256"],"cases":[{"case":x["case"],"expected_exit_code":x["expected_exit_code"],"actual_exit_code":x["actual_exit_code"],"status":x.get("actual_status"),"output_sha256":x.get("output_sha256",x.get("output_after_sha256"))} for x in results]},indent=2))
if final["status"]!="accepted_all_7_original_retained_CLI_replays":
    raise SystemExit(1)
