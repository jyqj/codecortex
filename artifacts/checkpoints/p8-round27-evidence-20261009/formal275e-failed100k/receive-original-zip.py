import datetime,hashlib,json,subprocess,zipfile
from pathlib import Path,PurePosixPath
root=Path("/Users/jin/Desktop/codecortex-rust/artifacts/benchmarks/p8-pr184-native-20261009-root")
out=root/"formal275e-100k-failure-reception"
out.mkdir(exist_ok=False)
argv=["/opt/homebrew/bin/gh","api","--method","GET","/repos/jyqj/codecortex/actions/artifacts/11615980558/zip"]
zip_path=out/"artifact-11615980558.zip"
started=datetime.datetime.now(datetime.timezone.utc).isoformat()
with zip_path.open("xb") as f, (out/"download.stderr.log").open("xb") as err:
 p=subprocess.run(argv,stdout=f,stderr=err,timeout=180)
receipt={"schema":"p8-formal275e-failed100k-original-zip-reception-v1","source":"275e8799d4947d297329073eaa3ca675d3fd0777","run_id":37896198208,"attempt":1,"artifact_id":11615980558,"argv":argv,"exit_code":p.returncode,"started_at_utc":started,"completed_at_utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"read_only_reception":True,"product_executed":False}
with (out/"download-receipt.json").open("x") as f:f.write(json.dumps(receipt,indent=2)+"\n")
assert p.returncode==0,receipt
raw=zip_path.read_bytes()
receipt["zip"]={"bytes":len(raw),"sha256":hashlib.sha256(raw).hexdigest()}
assert receipt["zip"]=={"bytes":5210430,"sha256":"7738402529b8339bbb328fec0b02ba4ad834bb787038f008dbb6d12a17fec94d"},receipt
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
