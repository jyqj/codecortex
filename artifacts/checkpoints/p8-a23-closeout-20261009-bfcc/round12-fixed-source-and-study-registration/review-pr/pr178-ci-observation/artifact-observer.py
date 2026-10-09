#!/usr/bin/env python3
import pathlib,subprocess,json,hashlib,datetime,time,zipfile,stat,re,shutil,traceback
BASE=pathlib.Path.cwd()
SOURCE="9f7b16f0758eb79f306cf44605b84550f02de441"
RUNS={37896539291:"runtime",37896539337:"lifecycle",37896539450:"platform",37896539353:"gates"}
ROOT=BASE/"raw-pr178-9f7b16f"
OBS=BASE/"review-pr/pr178-ci-observation"
GH="/opt/homebrew/bin/gh"
def now():return datetime.datetime.now(datetime.timezone.utc).isoformat()
def sha(path):
 h=hashlib.sha256()
 with path.open("rb") as f:
  for b in iter(lambda:f.read(1024*1024),b""):h.update(b)
 return h.hexdigest()
def write_json(path,value):
 with path.open("x",encoding="utf-8") as f:json.dump(value,f,indent=2);f.write("\n")
def emit(value):
 print(json.dumps({"at":now(),**value}),flush=True)
def native(argv,stdout,stderr,receipt,timeout):
 start=now(); timed_out=False
 with stdout.open("xb") as out,stderr.open("xb") as err:
  child=subprocess.Popen(argv,stdout=out,stderr=err)
  try: code=child.wait(timeout=timeout)
  except subprocess.TimeoutExpired:
   timed_out=True;child.kill();code=child.wait()
 row={"argv":argv,"started_at":start,"finished_at":now(),"exit_code":code,"timeout_terminated":timed_out,
 "stdout":{"path":str(stdout.relative_to(BASE)),"bytes":stdout.stat().st_size,"sha256":sha(stdout)},
 "stderr":{"path":str(stderr.relative_to(BASE)),"bytes":stderr.stat().st_size,"sha256":sha(stderr)}}
 write_json(receipt,row);return row
def api(endpoint,stem):
 row=native([GH,"api",endpoint,"--allow-escape-sequences"],stem.with_suffix(".stdout.json"),stem.with_suffix(".stderr"),stem.with_suffix(".command.json"),60)
 if row["exit_code"]!=0:raise RuntimeError("read-only API exit "+str(row["exit_code"])+" at "+str(stem))
 return json.loads(stem.with_suffix(".stdout.json").read_bytes())
def archive(artifact,run,run_snapshot):
 aid=artifact["id"];dest=ROOT/str(aid)
 if dest.exists():
  emit({"event":"artifact_existing_preserved_no_retry","artifact_id":aid,"path":str(dest.relative_to(BASE))})
  return
 dest.mkdir()
 write_json(dest/"github-metadata.json",artifact)
 write_json(dest/"run-metadata.json",run)
 write_json(dest/"provenance.json",{"source":SOURCE,"expected_run":run["id"],"run_attempt":run["run_attempt"],"run_snapshot":str(run_snapshot.relative_to(BASE)),"download_retry_allowed":False})
 try:
  assert artifact["workflow_run"]["id"]==run["id"] and artifact["workflow_run"]["head_sha"]==SOURCE
  assert run["head_sha"]==SOURCE and run["run_attempt"]==1
  assert not artifact["expired"] and artifact["name"].endswith(SOURCE)
  expected_size=artifact.get("size_in_bytes");expected_digest=artifact.get("digest")
  if expected_size is not None:assert shutil.disk_usage(dest).free>expected_size+1024**3
  row=native([GH,"api","repos/jyqj/codecortex/actions/artifacts/"+str(aid)+"/zip","--allow-escape-sequences"],
      dest/"original.zip",dest/"download.stderr",dest/"download-command.json",300)
  if row["exit_code"]!=0:raise RuntimeError("original ZIP transport exit "+str(row["exit_code"]))
  zip_size=(dest/"original.zip").stat().st_size;zip_sha=sha(dest/"original.zip")
  if expected_size is not None:assert zip_size==expected_size,("official_size_mismatch",zip_size,expected_size)
  if expected_digest is not None:assert expected_digest=="sha256:"+zip_sha,("official_digest_mismatch",zip_sha,expected_digest)
  members=[];seen=set();total=0
  with zipfile.ZipFile(dest/"original.zip") as z:
   for item in z.infolist():
    raw=item.filename
    assert "\x00" not in raw and "\\" not in raw,("invalid_member_name",raw)
    pp=pathlib.PurePosixPath(raw)
    assert not pp.is_absolute() and ".." not in pp.parts and not re.match(r"^[A-Za-z]:",raw),("escaped_member",raw)
    normalized=str(pp)
    assert normalized not in ("",".") and normalized not in seen,("duplicate_member",raw)
    seen.add(normalized);mode=(item.external_attr>>16)&0xffff
    assert not stat.S_ISLNK(mode),("symlink_member",raw)
    assert not mode or stat.S_IFMT(mode) in (0,stat.S_IFREG,stat.S_IFDIR),("special_member",raw)
    assert not item.flag_bits&1,("encrypted_member",raw)
    total+=item.file_size
    members.append({"name":raw,"normalized_name":normalized,"bytes":item.file_size,"compressed_bytes":item.compress_size,"crc32":"%08x"%item.CRC,"mode":mode,"is_directory":item.is_dir(),"sha256":None})
   assert shutil.disk_usage(dest).free>total+1024**3,("insufficient_extract_space",total)
   extract=dest/"extracted";extract.mkdir()
   for item,record in zip(z.infolist(),members):
    target=extract/pathlib.PurePosixPath(record["normalized_name"])
    assert target.resolve().is_relative_to(extract.resolve()) if hasattr(pathlib.Path,"is_relative_to") else str(target.resolve()).startswith(str(extract.resolve())+"/")
    if item.is_dir():target.mkdir(parents=True,exist_ok=True);continue
    target.parent.mkdir(parents=True,exist_ok=True)
    h=hashlib.sha256();count=0
    with z.open(item,"r") as src,target.open("xb") as out:
     for block in iter(lambda:src.read(1024*1024),b""):
      h.update(block);count+=len(block);out.write(block)
    assert count==item.file_size
    record["sha256"]=h.hexdigest()
    if record["mode"]&0o777:target.chmod(record["mode"]&0o777)
  write_json(dest/"zip-members.json",members)
  official_complete=expected_size is not None and expected_digest is not None
  receipt={"artifact_id":aid,"name":artifact["name"],"run_id":run["id"],"run_attempt":run["run_attempt"],"source":SOURCE,
   "zip_sha256":zip_sha,"zip_bytes":zip_size,"official_size":expected_size,"official_digest":expected_digest,
   "official_identity_complete":official_complete,"uncompressed_bytes":total,"members":len(members),
   "path":str(dest),"transport_status":"verified" if official_complete else "downloaded_official_identity_incomplete",
   "execution_acceptance":"not_inferred","all_member_crc_reads_completed":True,"all_regular_members_sha256_recorded":True,
   "download_exit_code":row["exit_code"]}
  write_json(dest/"transport-receipt.json",receipt)
  emit({"event":"artifact_download_terminal",**receipt})
 except Exception as e:
  failure={"artifact_id":aid,"at":now(),"transport_status":"failed_preserved_no_retry","exception":repr(e),"traceback":traceback.format_exc(),"execution_acceptance":"not_inferred"}
  write_json(dest/"transport-failure.json",failure);emit({"event":"artifact_download_failure",**failure})
ROOT.mkdir(exist_ok=True)
watch=OBS/("observer-"+datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S%fZ"));watch.mkdir()
write_json(watch/"configuration.json",{"source":SOURCE,"runs":RUNS,"poll_seconds":180,"max_seconds":14400,"no_primary_execution":True,"no_remote_mutation":True,"single_download_attempt_per_artifact_id":True})
emit({"event":"observer_started","path":str(watch.relative_to(BASE)),"source":SOURCE,"runs":RUNS})
started=time.monotonic();terminal={}
while time.monotonic()-started<14400:
 cycle=watch/datetime.datetime.now(datetime.timezone.utc).strftime("%Y%m%dT%H%M%S%fZ");cycle.mkdir()
 for rid,name in RUNS.items():
  try:
   run=api("repos/jyqj/codecortex/actions/runs/"+str(rid),cycle/(name+"-run"))
   jobs=api("repos/jyqj/codecortex/actions/runs/"+str(rid)+"/jobs?per_page=100",cycle/(name+"-jobs"))
   arts=api("repos/jyqj/codecortex/actions/runs/"+str(rid)+"/artifacts?per_page=100",cycle/(name+"-artifacts"))
   assert run["head_sha"]==SOURCE and run["run_attempt"]==1
   assert jobs["total_count"]<=100 and arts["total_count"]<=100
   emit({"event":"workflow_observation","run_id":rid,"name":name,"status":run["status"],"conclusion":run["conclusion"],
         "jobs":[{"id":j["id"],"name":j["name"],"status":j["status"],"conclusion":j["conclusion"]} for j in jobs["jobs"]],
         "artifact_ids":[a["id"] for a in arts["artifacts"]]})
   for artifact in arts["artifacts"]:
    if not (ROOT/str(artifact["id"])).exists():archive(artifact,run,cycle/(name+"-run.stdout.json"))
   terminal[rid]=run["status"]=="completed"
  except Exception as e:
   write_json(cycle/(name+"-observation-failure.json"),{"at":now(),"exception":repr(e),"traceback":traceback.format_exc()})
   emit({"event":"observation_error","run_id":rid,"error":repr(e)})
 if len(terminal)==4 and all(terminal.values()):
  write_json(watch/"terminal.json",{"at":now(),"all_four_runs_terminal":True,"execution_acceptance":"separate_raw_review_required"})
  emit({"event":"all_four_runs_terminal","source":SOURCE});break
 time.sleep(180)
else:
 write_json(watch/"observation-window-ended.json",{"at":now(),"all_four_runs_terminal":False,"terminal":terminal})
 emit({"event":"observation_window_ended","terminal":terminal})
