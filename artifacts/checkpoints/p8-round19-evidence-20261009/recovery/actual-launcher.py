import pathlib,json,hashlib,subprocess,sys,ast,time,base64
P=pathlib.Path
root=P.cwd()/"artifacts/checkpoints/p8-round19-evidence-intake-20261009/platform275e-author"
e=root/"evidence";out=root/"recovery-review";source=e/"source";aid=11597639035
assert not out.exists() and root.resolve()==root
def digest(p):
 h=hashlib.sha256()
 with p.open("rb") as f:
  for b in iter(lambda:f.read(1024*1024),b""):h.update(b)
 return h.hexdigest()
def get(oid,size,sha):
 r=subprocess.run(["gh","api","/repos/jyqj/codecortex/git/blobs/"+oid],capture_output=True,timeout=60)
 assert r.returncode==0
 j=json.loads(r.stdout);b=base64.b64decode(j["content"])
 assert j["sha"]==oid and len(b)==size and hashlib.sha256(b).hexdigest()==sha
 assert hashlib.sha1(b"blob "+str(len(b)).encode()+b"\0"+b).hexdigest()==oid
 return b
adapter=get("a9f42ff0a794486679a6be441b282a14b3c63966",20970,"ae01b32d396eee3297136537693263fae5c08a176e102abf14d55e6fe548122f")
plan=get("267883c69b04189b2be8f28416440924fd9b9f3f",6292,"cbd4fa23d87307766b1eecdd5e7539bdaa2e15f3068d5ed1a2958920b0291c48")
review=get("00cb8b3e2f96f34fd89d370f0a449ce91efde0e7",6190,"76e1f659533f6540d78aa23b5b96f0f5943e15938332f5b5acacb2c638971f48")
old=(root/"recovery-auditor-original/audit_raw_G4_original.py").read_bytes()
assert hashlib.sha256(old).hexdigest()=="7433e7e84ef52df4525814144844f2edcfe52658f75cf448ac120a577aa7b213"
inverse=adapter.decode()
for c in reversed(json.loads(plan)["literal_changes"]):
 assert inverse.count(c["after"])==old.decode().count(c["before"])
 inverse=inverse.replace(c["after"],c["before"])
assert inverse.encode()==old
ast.parse(adapter);compile(adapter,"audit_raw_275e.py","exec")
for oid in ["275e8799d4947d297329073eaa3ca675d3fd0777","277f2490fad3fa30f2812b5547bad033867c9ea5"]:
 assert subprocess.run(["git","-C",str(source),"cat-file","-e",oid+"^{commit}"],capture_output=True).returncode==0
members=json.loads((e/"members"/(str(aid)+".json")).read_bytes())
assert "p8-full-recovery/full-recovery.json" in members and len(members)==499
assert {n.split("/")[0] for n in members}=={"p8-full-recovery","p8-recovery-preparation"}
assert {n for n in members if n.startswith("p8-recovery-preparation/")}=={"p8-recovery-preparation/controls.log","p8-recovery-preparation/fetch.log","p8-recovery-preparation/previous-fetch.log"}
archive=e/"zips"/(str(aid)+".zip")
assert archive.stat().st_size==121513685 and digest(archive)=="4c3035f0458e7c91088f9881ea9e5b3072b83dad9ae27a75939cc2afefbcd854"
raw=e/"recovery"/str(aid)
for n,r in members.items():
 p=raw/n;assert p.is_file() and not p.is_symlink() and p.stat().st_size==r["bytes"] and digest(p)==r["sha256"]
wrapper={n:dict(r,extracted_nonbinary=P(n).name not in ["codecortex","fault-test"]) for n,r in members.items()}
api=json.loads((e/"api/artifacts.json").read_bytes())
artifact=next(a for a in api["artifacts"] if a["id"]==aid)
assert artifact["digest"]=="sha256:4c3035f0458e7c91088f9881ea9e5b3072b83dad9ae27a75939cc2afefbcd854" and artifact["workflow_run"]["head_sha"]=="275e8799d4947d297329073eaa3ca675d3fd0777"
out.mkdir()
with (out/"preexecution-layout-addendum.json").open("xb") as f:f.write(get("7fd498a7a9dbd91901f65f71fc16fe94e8fc2419",3033,"a559a304cd5185f149e0689f4da5e3b80449c9413c816854faa867856b1ddaba"))
def save(n,v):
 with (out/n).open("x") as f:json.dump(v,f,sort_keys=True,indent=2);f.write("\n")
for n,b in [("audit_raw_275e.py",adapter),("adaptation-plan.json",plan),("independent-admission.json",review)]:
 with (out/n).open("xb") as f:f.write(b)
save("member-hashes.json",{"members":wrapper})
save("github-metadata.json",{"artifact":artifact})
argv=["/Users/jin/.local/bin/python3.14","-B",str(out/"audit_raw_275e.py")]
save("command.json",{"argv":argv,"cwd":str(root),"scope":"original offline recovery assertions; no product/Cargo/statistics"})
start=time.monotonic()
with (out/"stdout.log").open("xb") as so,(out/"stderr.log").open("xb") as se:
 done=subprocess.run(argv,cwd=root,stdout=so,stderr=se,timeout=240)
conserved=archive.stat().st_size==121513685 and digest(archive)==artifact["digest"].split(":")[1]
conserved=conserved and all((raw/n).stat().st_size==r["bytes"] and digest(raw/n)==r["sha256"] for n,r in members.items())
receipt={"exit_code":done.returncode,"wall_seconds":time.monotonic()-start,"all_original_recovery_bytes_unchanged":conserved,"members":len(members),"binary_members":sum(not r["extracted_nonbinary"] for r in wrapper.values()),"source":"275e8799d4947d297329073eaa3ca675d3fd0777","artifact_id":aid,"review_role":"author_reception","no_new_product_or_Cargo_or_statistics":True}
if (out/"author-review.json").exists():receipt["author_review_sha256"]=digest(out/"author-review.json");receipt["author_review_bytes"]=(out/"author-review.json").stat().st_size
save("execution-receipt.json",receipt)
print(json.dumps(receipt,sort_keys=True))
print((out/"stdout.log").read_text()[-3000:])
if done.returncode:print((out/"stderr.log").read_text()[-5000:])
assert conserved and done.returncode==0
