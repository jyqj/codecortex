import sys
sys.dont_write_bytecode=True
import pathlib,json,hashlib,shutil,os,subprocess,time,zipfile,re,datetime
base=pathlib.Path.cwd()
source=(base/"source").resolve()
review=base/"review-platform"
sys.path.insert(0,str(source/"scripts"))
from p8_cold_build import source_identity,digest
from p7_build_identity import source_snapshot
commit="a23bb72d3c954f385b99fe81ce9189885c208557"
source_before=source_snapshot(source)
assert source_before["source_commit"]==commit
identity,_=source_identity(source,commit)
artifact=base/"raw"/"11592498371"
meta=json.loads((artifact/"github-metadata.json").read_text())
sha="28a2590190645f2e923926097092d6008e3a8c7014dc78b3b832fb6608712700"
assert meta["id"]==11592498371 and meta["size_in_bytes"]==1504 and meta["digest"]=="sha256:"+sha
assert meta["workflow_run"]["head_sha"]==commit and meta["workflow_run"]["id"]==37871838952
assert (artifact/"original.zip").stat().st_size==1504 and digest(artifact/"original.zip")==sha
with zipfile.ZipFile(artifact/"original.zip") as z:
    assert z.namelist()==["matrix.json"]
    assert z.testzip() is None
    assert z.read("matrix.json")==(artifact/"extracted/matrix.json").read_bytes()
original=json.loads((artifact/"extracted/matrix.json").read_text())
assert original["status"]=="passed" and original["exit_code"]==0 and original["counts"]=={"passed":8,"failed":0,"not_run":0}
assert original["expected_commit"]==commit and original["runner_sha256"]==digest(source/"scripts/p8_cold_build.py")
assert dict(original["source"],source_root=identity["source_root"])==identity
assert original["source"]["source_root"]=="/home/runner/work/codecortex/codecortex"
assert original["input_directory"]=="/home/runner/work/_temp/p8-platform-cells"
rows=[
("linux-1.95-default",113631480900,11591733636,11591593757),
("linux-1.95-semantic",113631480999,11592442010,11592596644),
("linux-stable-default",113631481002,11592750594,11592169160),
("linux-stable-semantic",113631481011,11592990763,11592403011),
("macos-1.95-default",113631481350,11591229630,11591134950),
("macos-1.95-semantic",113631481010,11591108235,11591671795),
("macos-stable-default",113631480988,11591159319,11591239060),
("macos-stable-semantic",113631481070,11592465349,11591809454)]
stage=review/"collector-input-copies"
assert not stage.exists()
free=shutil.disk_usage(review).free
copybytes=sum(p.stat().st_size for _,_,aid,_ in rows for p in (base/"raw"/str(aid)/"extracted").rglob("*") if p.is_file())
assert free>copybytes+4*1024**3
stage.mkdir()
input_hashes={}
bindings=[]
for label,job,aid,raw_id in rows:
    home=base/"raw"/str(aid)/"extracted"
    manifest={p.relative_to(home).as_posix():digest(p) for p in home.rglob("*") if p.is_file()}
    assert len(manifest)==19 and not any(p.is_symlink() for p in home.rglob("*"))
    input_hashes[str(aid)]=manifest
    shutil.copytree(home,stage/label)
    assert {p.relative_to(stage/label).as_posix():digest(p) for p in (stage/label).rglob("*") if p.is_file()}==manifest
    record=json.loads((home/"receipt.json").read_text())
    log=(review/"github-job-logs"/(str(job)+".log")).read_text()
    assert "Ran 59 tests in " in log and re.search(r"Z OK\s*(?:\n|$)",log)
    assert '"passed": 1, "failed": 0, "not_run": 7' in log
    assert commit in log and digest(base/"raw"/str(aid)/"original.zip") in log
    bindings.append({"cell":record["cell"],"job_id":job,"cell_artifact_id":aid,"raw_artifact_id":raw_id,"original_control_count":59,"original_controls_status":"OK","cell_cli_exit_code":2,"selected_export_exit_code":0,"profile":record["profile"],"host":record["toolchain"]["host"],"rustc_release":record["toolchain"]["rustc_release"],"binary_sha256":record["binary_sha256"],"binary_bytes":record["binary_bytes"],"wall_seconds":record["wall_seconds"],"bundle_sha256":manifest["bundle.json"],"receipt_sha256":manifest["receipt.json"],"job_log_sha256":digest(review/"github-job-logs"/(str(job)+".log"))})
collector_log=(review/"github-job-logs"/"113646688067.log").read_text()
assert commit in collector_log and sha in collector_log
assert 'Total of 8 artifact(s) downloaded' in collector_log
assert '{"status": "passed", "exit_code": 0, "counts": {"passed": 8, "failed": 0, "not_run": 0}}' in collector_log
output=review/"collector-replay-output"
assert not output.exists()
command=[sys.executable,str(source/"scripts/p8_cold_build.py"),"--collect-cells",str(stage),"--expected-commit",commit,"--output-dir",str(output)]
env=dict(os.environ,PYTHONDONTWRITEBYTECODE="1")
t=time.monotonic()
proc=subprocess.run(command,cwd=source,env=env,capture_output=True,timeout=60)
elapsed=time.monotonic()-t
(review/"collector-replay.stdout.log").write_bytes(proc.stdout)
(review/"collector-replay.stderr.log").write_bytes(proc.stderr)
assert proc.returncode==0,{"code":proc.returncode,"stderr":proc.stderr.decode(),"stdout":proc.stdout.decode()}
replayed=json.loads((output/"matrix.json").read_text())
assert replayed["status"]=="passed" and replayed["exit_code"]==0 and replayed["counts"]=={"passed":8,"failed":0,"not_run":0}
normalized=json.loads(json.dumps(original))
normalized["input_directory"]=str(stage.absolute())
normalized["source"]["source_root"]=str(source)
assert normalized==replayed
for label,_,aid,_ in rows:
    home=base/"raw"/str(aid)/"extracted"
    assert input_hashes[str(aid)]=={p.relative_to(home).as_posix():digest(p) for p in home.rglob("*") if p.is_file()}
    assert input_hashes[str(aid)]=={p.relative_to(stage/label).as_posix():digest(p) for p in (stage/label).rglob("*") if p.is_file()}
assert digest(artifact/"original.zip")==sha
assert source_snapshot(source)==source_before
report={"schema_version":1,"status":"accepted_all_8_original_cells_and_original_collector_replay","reviewed_at_utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"source_commit":commit,"source_tree":identity["source_tree"],"source_input_count":identity["input_count"],"source_manifest_sha256":identity["manifest_sha256"],"product_manifest_sha256":source_before["manifest_sha256"],"run_id":37871838952,"run_attempt":1,"collector_job_id":113646688067,"collector_job_status":"completed","collector_job_conclusion":"success","collector_artifact_id":11592498371,"collector_zip_bytes":1504,"collector_zip_sha256":sha,"collector_original_matrix_sha256":digest(artifact/"extracted/matrix.json"),"collector_runner_sha256":digest(source/"scripts/p8_cold_build.py"),"counts":replayed["counts"],"replay":{"command":command,"cwd":str(source),"environment_override":{"PYTHONDONTWRITEBYTECODE":"1"},"exit_code":proc.returncode,"elapsed_seconds":elapsed,"stdout_file":"collector-replay.stdout.log","stdout_sha256":digest(review/"collector-replay.stdout.log"),"stderr_file":"collector-replay.stderr.log","stderr_sha256":digest(review/"collector-replay.stderr.log"),"output_file":"collector-replay-output/matrix.json","output_sha256":digest(output/"matrix.json"),"original_receipt_equivalent":True,"transport_normalizations":[{"field":"input_directory","original":original["input_directory"],"replay":replayed["input_directory"]},{"field":"source.source_root","original":original["source"]["source_root"],"replay":replayed["source"]["source_root"]}]},"cells":bindings,"copied_input_bytes":copybytes,"free_bytes_before_copy":free,"copied_original_files":152,"original_cells_and_source_unchanged":True,"copied_input_manifests":input_hashes,"scope":"Original eight native fresh-target release build cells and native stdio verified by unchanged exact-source portable validator and final original collector; replay does not rebuild, execute foreign binaries or measure performance.","todo_status_change":False}
p=review/"platform-eight-cell-collector-audit.json"
assert not p.exists();p.write_text(json.dumps(report,indent=2)+"\n")
print(json.dumps({"status":report["status"],"counts":report["counts"],"collector_zip_sha256":sha,"original_matrix_sha256":report["collector_original_matrix_sha256"],"replay_exit_code":proc.returncode,"equivalent":True,"report_sha256":digest(p),"report_bytes":p.stat().st_size,"cells":[{"cell":b["cell"],"rustc_release":b["rustc_release"],"host":b["host"],"original_controls":b["original_control_count"]} for b in bindings]}))
