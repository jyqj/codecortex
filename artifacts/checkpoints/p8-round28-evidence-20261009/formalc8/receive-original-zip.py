import datetime,hashlib,json,subprocess,zipfile
from pathlib import Path,PurePosixPath
root=Path("/Users/jin/Desktop/codecortex-rust/artifacts/benchmarks/p8-pr184-native-20261009-root")
out=root/"formalc8-100k-failure-reception"
out.mkdir(exist_ok=False)
argv=["/opt/homebrew/bin/gh","api","--method","GET","/repos/jyqj/codecortex/actions/artifacts/11617588539/zip"]
zip_path=out/"artifact-11617588539.zip"
started=datetime.datetime.now(datetime.timezone.utc).isoformat()
with zip_path.open("xb") as f, (out/"download.stderr.log").open("xb") as err:
 p=subprocess.run(argv,stdout=f,stderr=err,timeout=180)
receipt={"schema":"p8-formalc8-failed100k-original-zip-reception-v1","source":"c8be5afaac568ffd40ef86d3795423c3b73c9f39","run_id":37902429727,"attempt":1,"artifact_id":11617588539,"argv":argv,"exit_code":p.returncode,"started_at_utc":started,"completed_at_utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"read_only_reception":True,"product_executed":False}
with (out/"download-receipt.json").open("x") as f:f.write(json.dumps(receipt,indent=2)+"\n")
assert p.returncode==0,receipt
raw=zip_path.read_bytes()
receipt["zip"]={"bytes":len(raw),"sha256":hashlib.sha256(raw).hexdigest()}
assert receipt["zip"]=={"bytes":5212266,"sha256":"f758ca519a6b5ed90bfe1040944ad32b1daad8248e44b8ea4c7d4840d4a43236"},receipt
receipt["members"]=[]
with zipfile.ZipFile(zip_path) as z:
 infos=z.infolist()
 assert len(infos)==12 and len({i.filename for i in infos})==12
 for item in infos:
  name=PurePosixPath(item.filename)
  assert not name.is_absolute() and ".." not in name.parts and not item.is_dir()
  raw=z.read(item)
  receipt["members"].append({"path":item.filename,"bytes":len(raw),"sha256":hashlib.sha256(raw).hexdigest(),"crc32":"%08x"%item.CRC,"crc_validated":True})
  dest=out/"members"/name
  dest.parent.mkdir(parents=True,exist_ok=True)
  with dest.open("xb") as f:f.write(raw)
receipt["status"]="zip_and_all_12_members_received_crc_verified_measurement_failed_preserved"
with (out/"reception-manifest.json").open("x") as f:f.write(json.dumps(receipt,indent=2)+"\n")
print(json.dumps(receipt))
