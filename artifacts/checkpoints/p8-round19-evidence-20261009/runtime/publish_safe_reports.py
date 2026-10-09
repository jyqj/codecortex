import base64,gzip,hashlib,io,json,pathlib,re,subprocess,tarfile
R=pathlib.Path("artifacts/checkpoints/p8-round19-evidence-intake-20261009/runtime275e-author").resolve()
D=R/"delivery"
if D.exists():raise ValueError("delivery already exists")
D.mkdir()
def sha(b):return hashlib.sha256(b).hexdigest()
def filehash(p):
 h=hashlib.sha256()
 with p.open("rb") as f:
  for x in iter(lambda:f.read(1048576),b""):h.update(x)
 return h.hexdigest()
def save(name,value):
 b=(json.dumps(value,sort_keys=True,indent=2)+chr(10)).encode()
 with (D/name).open("xb") as f:f.write(b)
 return b
safe=re.compile(rb"(?:[?&](?:sig|signature|x-amz-signature|x-goog-signature|access_token|token)=|github_pat_[A-Za-z0-9_]+|gh[pousr]_[A-Za-z0-9_]{20,})",re.I)
paths=[R/"materialization.json"]
for name in ("preparation.json","capacity.json","registration.json","source-copy.json"):
 paths.append(R/"evidence"/name)
for sub in ("api","inventories"):paths += sorted((R/"evidence"/sub).glob("*.json"))
for sub in ("launch","replay-output"):
 paths += [p for p in sorted((R/sub).rglob("*")) if p.is_file() and p.suffix in (".json",".stdout",".stderr",".log")]
if len(paths)!=len(set(paths)):raise ValueError("duplicate selected path")
inventory={}
for p in paths:
 b=p.read_bytes();b.decode("utf-8")
 if safe.search(b):raise ValueError("private transport field in selected output")
 inventory[p.relative_to(R).as_posix()]={"bytes":len(b),"sha256":sha(b)}
receipt=json.loads((R/"replay-output/offline-replay/receipt.json").read_bytes())
launch=json.loads((R/"launch/receipt.json").read_bytes())
if receipt["status"]!="accepted_scoped_original_275e_runtime_six_domains" or receipt["exit_code"]!=0 or not receipt["all_originals_unchanged"]:raise ValueError("semantic receipt")
if launch["original_consumer_exit_code"]!=0 or launch["receipt_sha256"]!=inventory["replay-output/offline-replay/receipt.json"]["sha256"]:raise ValueError("outer receipt binding")
reg=json.loads((R/"registration.json").read_bytes())
zips=[]
for a in reg["artifacts"]:
 p=R/"evidence/raw"/str(a["id"])/"original.zip"
 if p.stat().st_size!=a["bytes"] or filehash(p)!=a["sha256"]:raise ValueError("original ZIP no longer equal")
 zips.append({"artifact_id":a["id"],"bytes":a["bytes"],"sha256":a["sha256"],"local_path":str(p)})
peers=[]
for p in sorted((R/"replay-output/review-runtime").glob("*.json")):
 q=json.loads(p.read_bytes())
 if q["errors"] or q["source_sha"]!=reg["source"] or q["run_id"]!=reg["run"]:raise ValueError("peer result identity/error")
 keys=("artifact_id","review_status","profile","configured_concurrency","outcomes","raw_kind_counts","actual_concurrency","independent_call_interval_maximum","independent_call_interval_read_build_overlap","observed_work_ns","branch_switches","catalog_compactions","parity_tables","requests","seeds","cells","execution_elapsed_ns")
 peers.append({"report_path":str(p.relative_to(R)),"sha256":filehash(p),"summary":{k:q[k] for k in keys if k in q}})
manifest={"schema":"p8-275e-original-runtime-complete-safe-text-delivery-v1","source":reg["source"],"run":reg["run"],"status":"actual_original_offline_consumers_passed_author_reception","files":inventory,"file_count":len(inventory),"text_bytes":sum(x["bytes"]for x in inventory.values()),"original_ZIPs_preserved":zips,"original_ZIP_bytes":sum(x["bytes"]for x in zips),"original_byte_inventory_and_seal_in_full_receipt":True,"original_consumer_receipt_sha256":launch["receipt_sha256"],"peer_reports":peers,"scope":"Six original275e domains, original plan/raw/statistics only. Not a new measurement or N150 acceptance. Original ZIPs and copied binaries remain at native paths and Actions, deliberately absent from this text-only archive.","note":"First model-facing whole receipt plus file-list output exceeded display budget and was truncated; no inference was made from that view. This publication reads complete native bytes, preserves exact originals and verifies full Git GET bodies.","TODO_closed":0,"TODO_remaining":29}
save("manifest.json",manifest)
archive=D/"safe-text-reports.tar.gz"
with archive.open("xb") as af:
 with gzip.GzipFile(filename="",fileobj=af,mode="wb",mtime=0) as gz:
  with tarfile.open(fileobj=gz,mode="w") as t:
   for name in sorted(inventory):
    body=(R/name).read_bytes()
    if sha(body)!=inventory[name]["sha256"]:raise ValueError("changed report before archive")
    info=tarfile.TarInfo(name);info.size=len(body);info.mode=0o644;info.mtime=0;t.addfile(info,io.BytesIO(body))
with tarfile.open(archive,"r:gz") as t:
 if {m.name for m in t.getmembers()}!=set(inventory):raise ValueError("archive inventory")
 for m in t.getmembers():
  b=t.extractfile(m).read()
  if len(b)!=inventory[m.name]["bytes"]or sha(b)!=inventory[m.name]["sha256"]:raise ValueError("archive member")
def publish(p):
 body=p.read_bytes()
 packet=json.dumps({"encoding":"base64","content":base64.b64encode(body).decode()}).encode()
 c=subprocess.run(["/opt/homebrew/bin/gh","api","--method","POST","repos/jyqj/codecortex/git/blobs","--input","-"],input=packet,stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=120)
 if c.returncode:raise ValueError("Git blob publication failed")
 value=json.loads(c.stdout);oid=value["sha"]
 expected=hashlib.sha1(("blob "+str(len(body))).encode()+bytes([0])+body).hexdigest()
 if oid!=expected:raise ValueError("Git blob create identity")
 c=subprocess.run(["/opt/homebrew/bin/gh","api","repos/jyqj/codecortex/git/blobs/"+oid],stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=120)
 if c.returncode:raise ValueError("Git full readback failed")
 v=json.loads(c.stdout);got=base64.b64decode(v["content"])
 if v["sha"]!=oid or v["size"]!=len(body) or got!=body:raise ValueError("Git full readback mismatch")
 return {"local_path":str(p),"blob":oid,"bytes":len(body),"sha256":sha(body),"full_native_Git_GET_byte_equal":True}
published=[]
for p in (R/"replay-output/offline-replay/receipt.json",R/"replay-output/wire-supplement/cache-wire-binding.json",D/"manifest.json",archive):
 published.append(publish(p))
final={"schema":"p8-275e-runtime-original-delivery-publication","status":"all_original_receivers_passed_full_native_delivery_readback","source":reg["source"],"run":reg["run"],"published_unreferenced_blobs":published,"text_files":len(inventory),"text_bytes":manifest["text_bytes"],"original_ZIPs":len(zips),"original_ZIP_bytes":manifest["original_ZIP_bytes"],"new_refs_commits_or_measurement":False,"TODO_closed":0,"TODO_remaining":29}
save("publication.json",final)
summary=publish(D/"publication.json")
print(json.dumps({"publication":summary,"payloads":published,"text_files":len(inventory),"text_bytes":manifest["text_bytes"],"original_ZIPs":6,"original_ZIP_bytes":manifest["original_ZIP_bytes"]},sort_keys=True))
