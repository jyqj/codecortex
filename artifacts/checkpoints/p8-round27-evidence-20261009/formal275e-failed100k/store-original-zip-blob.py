import base64,datetime,hashlib,json,subprocess
from pathlib import Path
root=Path("/Users/jin/Desktop/codecortex-rust/artifacts/benchmarks/p8-pr184-native-20261009-root")
folder=root/"formal275e-100k-failure-reception"
p=folder/"artifact-11615980558.zip";raw=p.read_bytes()
assert len(raw)==5210430 and hashlib.sha256(raw).hexdigest()=="7738402529b8339bbb328fec0b02ba4ad834bb787038f008dbb6d12a17fec94d"
expected_git=hashlib.sha1(b"blob "+str(len(raw)).encode()+b"\\0".replace(b"\\0",bytes([0]))+raw).hexdigest()
request=json.dumps({"encoding":"base64","content":base64.b64encode(raw).decode("ascii")}).encode()
req=folder/"zip-github-blob-request.json"
with req.open("xb") as f:f.write(request)
argv=["/opt/homebrew/bin/gh","api","--method","POST","/repos/jyqj/codecortex/git/blobs","--input",str(req)]
started=datetime.datetime.now(datetime.timezone.utc).isoformat()
p=subprocess.run(argv,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=180)
receipt={"schema":"p8-formal275e-failed100k-original-zip-Git-blob-storage-v1","argv":argv,"exit_code":p.returncode,"started_at_utc":started,"completed_at_utc":datetime.datetime.now(datetime.timezone.utc).isoformat(),"stdout":p.stdout.decode(),"stderr":p.stderr.decode(),"payload":{"bytes":len(raw),"sha256":hashlib.sha256(raw).hexdigest(),"git_blob_sha":expected_git},"request":{"encoding":"base64","bytes":len(request),"sha256":hashlib.sha256(request).hexdigest()},"ref_changed":False,"product_executed":False}
with (folder/"zip-github-blob-receipt.json").open("x") as f:f.write(json.dumps(receipt,indent=2)+"\n")
assert p.returncode==0,receipt
response=json.loads(p.stdout);assert response["sha"]==expected_git,receipt
report=folder/"timing-observation.json";data=report.read_bytes();receipt["timing_observation"]={"bytes":len(data),"sha256":hashlib.sha256(data).hexdigest()}
print(json.dumps(receipt))
