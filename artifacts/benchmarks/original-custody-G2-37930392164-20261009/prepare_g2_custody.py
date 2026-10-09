#!/usr/bin/env python3
"""Prepare exact original ZIP custody; no ZIP rewriting, extraction or measurement."""
import hashlib,json,collections,datetime
from pathlib import Path
ROOT=Path("/workspace/scratch/a217aaae3bde")
BASE=ROOT/"original-LG2-runtime"
OUT=BASE/"original-custody-final"
PREFIX="artifacts/benchmarks/original-custody-G2-37930392164-20261009"
HEAD="4d18dcdb34b5d7a277566b57801cc882d8b1eb61"
LIMIT=2*1024*1024
def sha(b):return hashlib.sha256(b).hexdigest()
def oid(b):return hashlib.sha1(b"blob "+str(len(b)).encode()+b"\0"+b).hexdigest()
def write(rel,x):
 p=OUT/rel;p.parent.mkdir(parents=True,exist_ok=True)
 with p.open("x") as f:json.dump(x,f,sort_keys=True,indent=2);f.write("\n")
 return p
helper=(OUT/"restore_original_zip.py").read_bytes()
assert len(helper)==4339 and sha(helper)=="bbc91e5734b33050b02216d79afb5d4a58197a360ac5c997860a20c04c3ce632" and oid(helper)=="b4972957fa531e3ad64fe131a075700f8e3e0ff2"
prior=BASE/"original-custody-preparation/custody-preparation-manifest.json"
assert sha(prior.read_bytes())=="dcf42d5a9110eb738a85adaf0e3dff21316fe97f05434c963f8801cb2c0e8ab1"
old=json.loads(prior.read_text())
artifacts=[];unique={};all_chunks=[];small=[]
for a in old["artifacts"]:
 aid=a["artifact_id"];original=BASE/"artifacts"/str(aid)/(str(aid)+".zip")
 metadata=ROOT/a["official_metadata_path"];mb=metadata.read_bytes();meta=json.loads(mb)
 assert meta["workflow_run"]["head_sha"]==HEAD and meta["workflow_run"]["id"]==37930392164
 md=OUT/"metadata"/(str(aid)+".json");md.parent.mkdir(exist_ok=True)
 with md.open("xb") as f:f.write(mb)
 small.append(md)
 before=original.stat();whole=hashlib.sha256();offset=0;chunks=[]
 with original.open("rb") as f:
  for index,raw in enumerate(iter(lambda:f.read(LIMIT),b"")):
   digest=sha(raw);git=oid(raw);path="chunks/"+git+".bin"
   row={"path":path,"offset":offset,"bytes":len(raw),"sha256":digest,"git_blob":git};chunks.append(row);all_chunks.append(row)
   u={"path":path,"bytes":len(raw),"sha256":digest,"git_blob":git,"source_zip":str(original),"source_offset":offset}
   if git in unique:assert {k:unique[git][k] for k in ["path","bytes","sha256","git_blob"]}=={k:u[k] for k in ["path","bytes","sha256","git_blob"]}
   else:unique[git]=u
   offset+=len(raw);whole.update(raw)
 assert original.stat()==before and offset==meta["size_in_bytes"] and whole.hexdigest()==meta["digest"].removeprefix("sha256:")
 manifest={"schema":"codecortex-original-actions-zip-custody-v1","artifact_id":aid,"artifact_name":meta["name"],"workflow_run_id":37930392164,"workflow_attempt":1,"head_sha":HEAD,"original_zip_bytes":offset,"original_zip_sha256":whole.hexdigest(),"original_metadata":"metadata/"+md.name,"original_metadata_sha256":sha(mb),"official_artifact_url":meta["url"],"actions_expires_at":meta["expires_at"],"chunks":chunks,"complete_original_zip":True,"raw_or_binary_members_filtered":False,"compression_or_zip_bytes_changed":False}
 mp=write("manifests/"+str(aid)+".json",manifest);small.append(mp)
 artifacts.append({"artifact_id":aid,"name":meta["name"],"bytes":offset,"sha256":whole.hexdigest(),"manifest":"manifests/"+str(aid)+".json","manifest_sha256":sha(mp.read_bytes()),"chunk_count":len(chunks),"scope":"Original admission/control records" if aid==11617781499 else "Complete original mixed profile ZIP including original build, all raw, binaries, databases and seals"})
assert len(artifacts)==5
catalog={"schema":"g2-five-original-actions-zip-custody-v1","prepared_at":datetime.datetime.now(datetime.timezone.utc).isoformat(),"source_parent":HEAD,"source_tree":"383716945d2d5f2404578d726afb317e4490c181","workflow_run":37930392164,"workflow_event":"push","workflow_attempt":1,"repository_prefix":PREFIX,"official_complete_artifact_count":5,"artifacts":artifacts,"original_zip_total_bytes":sum(a["bytes"] for a in artifacts),"chunk_size_limit":LIMIT,"ordered_chunk_references":len(all_chunks),"unique_chunk_count":len(unique),"unique_chunk_bytes":sum(c["bytes"] for c in unique.values()),"deduplication":"Only identical complete chunk bytes share one Git object/path. Every original ZIP is restored unchanged; none of its members are removed.","build_scope":"The four mixed ZIPs each contain the complete original 19-member p8-lock-build tree with all three binaries, Cargo event records, ten observer sources, receipt and seal. No separate build artifact exists in the complete official five-artifact collection.","controls_recovery":"The 7220-byte original controls ZIP was recovered again by root at 2026-10-09 14:38 UTC after loss of the prior local copy. This preserves the same immutable artifact bytes; it is not called a first or unique download and no controls were rerun.","prior_missing_snapshot":{"path":"original-LG2-runtime/original-custody-preparation/custody-preparation-manifest.json","bytes":26520,"sha256":"dcf42d5a9110eb738a85adaf0e3dff21316fe97f05434c963f8801cb2c0e8ab1","preserved_unchanged":True},"restore_helper":{"path":"restore_original_zip.py","source_commit":"1d56747eafcef6c22ceaec949fabe30a3362862c","source_path":"artifacts/benchmarks/original-custody-C3ff-20261009/restore_original_zip.py","git_blob":oid(helper),"sha256":sha(helper),"bytes":len(helper),"algorithm_changed":False},"small_evidence_fixed_references":{"frozen_intake_helpers":{"commit":"8a8cc951d6c198d0ea28c01eaaaaf12df903ab77","path":"artifacts/checkpoints/p8-round31-a217-20261009/round31-public-evidence.tar.gz","member_prefix":"runtime/LG2-intake/"},"admission_log_controls_review":{"commit":"76369a0306026ebd90239b700f3ba16138a24c5c","scope":"R32 original admission/control records; not a substitute for the original controls ZIP"},"three_original_intakes":{"commit":"64dbafa6897f44699f31570e4bda06938f624daa","path":"artifacts/checkpoints/p8-round33-a217-20261009/round33-public-evidence.tar.gz","member_prefix":"runtime/"},"fourth_intake_R34":{"publication":"pending separate R34 preservation","review_sha256":"fa567fccead00e3f94f1a6525262917fb5c1ceeae690bde276ce512015c9bd69","execution_sha256":"5a6276baa81b0fefcf9ad275d7ad9ff2b407c0518f8110f43e9ad3afa73d1458"}},"publication_status":"candidate preparation; no branch created","native_reexecuted":False,"task_complete":False,"done":163,"remaining":29}
cp=write("catalog.json",catalog);small.extend([cp,OUT/"restore_original_zip.py"])
readme="""# G2 原始工件永久保全候选
本目录逐字节保留 GitHub Actions run 37930392164、attempt 1、固定 G2 4d18dcdb34b5d7a277566b57801cc882d8b1eb61 的完整五个原 ZIP；四份 mixed 原件与 controls 原件分别保持原 SHA。所有 ELF、数据库、raw、Cargo/observer 输入与封存成员都在原 ZIP 字节内，没有筛除或重新压缩。
每个 manifest 按原顺序引用不超过2 MiB的块。相同块可以复用同一 Git 对象，不能把不同原件合并成一个测量结果。四个 mixed ZIP 各自含完整 build；不另造重复 build ZIP。
在完整检出的本目录中，恢复单个原 ZIP：
```sh
python3 restore_original_zip.py manifests/11623110311.json --output /absolute/new/11623110311.zip
```
原冻结恢复器逐块和整件检查字节数、SHA256及Git blob身份，拒绝覆盖已有输出。输出目录须已存在。该操作只重建原 ZIP，不解压、不执行原件、不开新测量。
controls 原ZIP因原本地副本丢失，于14:38 UTC由root恢复性再次取得；旧缺件快照原样保留。R31/R32/R33已公开的小审查记录只按catalog固定commit引用，R34 C16验收记录另行保存。此目录不声称tasks完成、全规模通过或发布批准。尚未创建保全分支时，只是候选。
"""
rp=OUT/"README.md"
with rp.open("x") as f:f.write(readme)
small.extend([rp,Path(__file__)])
entries=[]
for p in small:
 raw=p.read_bytes();entries.append({"source":str(p),"path":str(p.relative_to(OUT)),"bytes":len(raw),"sha256":sha(raw),"git_blob":oid(raw),"mode":"100644","type":"blob"})
for row in sorted(unique.values(),key=lambda x:x["path"]):entries.append(dict(row,mode="100644",type="blob",virtual_original_slice=True))
plan=write("private-upload-plan.json",{"schema":"g2-original-zip-upload-plan-v1","branch":"evidence/p8-originals-g2-a217-20261009","source_parent":HEAD,"repository_prefix":PREFIX,"files":entries,"leaf_count":len(entries),"unique_upload_bytes":sum(x["bytes"] for x in entries),"private_capture_or_signed_url_included":False,"original_zip_total_bytes":catalog["original_zip_total_bytes"],"chunk_count":len(unique),"source_or_task_mutation":False})
print(json.dumps({"catalog_bytes":cp.stat().st_size,"catalog_sha256":sha(cp.read_bytes()),"plan_bytes":plan.stat().st_size,"plan_sha256":sha(plan.read_bytes()),"leaves":len(entries),"unique_chunks":len(unique),"unique_chunk_bytes":catalog["unique_chunk_bytes"],"all_original_zip_bytes":catalog["original_zip_total_bytes"],"total_unique_upload_bytes":sum(x["bytes"] for x in entries)}))
