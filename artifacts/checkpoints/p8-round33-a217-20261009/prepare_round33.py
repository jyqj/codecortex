#!/usr/bin/env python3
"""Package frozen R33 public original records, never original ZIPs or measurements."""
import gzip,hashlib,io,json,os,re,tarfile
from pathlib import Path
ROOT=Path("/workspace/scratch/a217aaae3bde")
OUT=ROOT/"round33-public-archive"
INPUTS=[
("runtime",ROOT/"original-LG2-runtime/current-three-public-files-manifest-1421.json","b702ae12e978a352e8099c66937cfc8bf7cb5fce0f65464043d99f8bbd6c311a"),
("pr-audit",ROOT/"original-C3ff-platform-gates/round33-public-files-manifest.json","5e3b3c4bdacd0ab12c83a310c7b10f85f35441941e4676b1da5b13af140fe952"),
("scale",ROOT/"scale-pr180-C-review/recovery/round33-public-manifest.json","5bea5d58b1e1815625e4de091abd4d3c7686aecb9aa4c9161a828d6ef71f0614")]
ROOT_NAMES=["C-scale-build-original-recovery-receipt.json","M6-PR180-posted-description.txt","M6-PR180-publication-receipt.json","M6-original-validation-local-receipt-loss-notice-v1.json","PR178-closure-readback-receipt.json","round33-summary.json"]
def sha(b):return hashlib.sha256(b).hexdigest()
def oid(b):return hashlib.sha1(b"blob "+str(len(b)).encode()+b"\0"+b).hexdigest()
def write_json(name,obj):
 p=OUT/name
 with p.open("x") as f:json.dump(obj,f,ensure_ascii=False,sort_keys=True,indent=2);f.write("\n")
 return p
selected=[]
def add(source,target,expected=None):
 source=source.resolve(strict=True)
 assert source.is_file() and source.is_relative_to(ROOT)
 assert not target.startswith("/") and ".." not in Path(target).parts
 b=source.read_bytes();mode=source.stat().st_mode&0o777
 if expected:
  assert len(b)==expected["bytes"] and sha(b)==expected["sha256"] and oid(b)==expected["git_blob"]
  assert mode==int(expected["mode"],8)
 assert not re.search(rb'file_[A-Za-z0-9]{12,}|library://|[?&](?:sig|X-Amz-Signature|se|sv)=',b,re.I),(str(source),"private transport content")
 selected.append({"source":str(source.relative_to(ROOT)),"target":target,"bytes":len(b),"sha256":sha(b),"git_blob":oid(b),"mode":format(mode,"04o")})
input_records=[]
for group,path,digest in INPUTS:
 b=path.read_bytes();assert sha(b)==digest
 inv=json.loads(b)
 input_records.append({"group":group,"path":str(path.relative_to(ROOT)),"bytes":len(b),"sha256":sha(b),"git_blob":oid(b),"file_count":inv["file_count"]})
 for row in inv["files"]:
  source=Path(row["source"])
  if not source.is_absolute():source=ROOT/source
  target=("runtime/"+row["target"]) if group=="runtime" else row["target"]
  add(source,target,row)
 add(path,"inputs/"+group+"-public-manifest.json")
for name in ROOT_NAMES:add(ROOT/"round33-root-operations"/name,"root/"+name)
selected.sort(key=lambda r:r["target"])
assert len(selected)==124 and len({x["target"] for x in selected})==len(selected)
tar_bytes=io.BytesIO()
with tarfile.open(fileobj=tar_bytes,mode="w",format=tarfile.USTAR_FORMAT) as tf:
 for row in selected:
  data=(ROOT/row["source"]).read_bytes()
  assert sha(data)==row["sha256"]
  info=tarfile.TarInfo(row["target"]);info.size=len(data);info.mode=int(row["mode"],8);info.uid=info.gid=0;info.uname=info.gname="";info.mtime=0
  tf.addfile(info,io.BytesIO(data))
raw=tar_bytes.getvalue();gz=io.BytesIO()
with gzip.GzipFile(fileobj=gz,mode="wb",filename="",mtime=0,compresslevel=9) as f:f.write(raw)
archive=OUT/"round33-public-evidence.tar.gz"
with archive.open("xb") as f:f.write(gz.getvalue())
with tarfile.open(archive,"r:gz") as tf:
 members=tf.getmembers();assert [m.name for m in members]==[x["target"] for x in selected]
 for m,row in zip(members,selected):
  assert m.isreg() and not m.pax_headers and m.uid==m.gid==m.mtime==0 and m.uname==m.gname==""
  assert m.mode==int(row["mode"],8)
  b=tf.extractfile(m).read();source=(ROOT/row["source"]).read_bytes()
  assert b==source and len(b)==row["bytes"] and sha(b)==row["sha256"] and oid(b)==row["git_blob"]
container={"filename":archive.name,"bytes":archive.stat().st_size,"sha256":sha(archive.read_bytes()),"git_blob":oid(archive.read_bytes()),"tar_bytes":len(raw),"tar_sha256":sha(raw),"format":"sorted regular USTAR; fixed zero ownership/timestamps; original modes; gzip level9 mtime0 empty filename"}
write_json("round33-public-evidence-inventory.json",{"schema":"round33-public-evidence-inventory-v1","cutoff_utc":"2026-10-09T14:21:51Z","parent_commit":"76369a0306026ebd90239b700f3ba16138a24c5c","container":container,"files":selected,"file_count":len(selected),"total_uncompressed_payload_bytes":sum(x["bytes"] for x in selected),"actual_reopen_all_source_member_bytes":True,"formal_task_completion":False,"remaining":29})
write_json("round33-public-bundle-inputs.json",{"schema":"round33-public-bundle-inputs-v1","inputs":input_records,"root_files":ROOT_NAMES,"packager":{"path":str(Path(__file__).relative_to(ROOT)),"sha256":sha(Path(__file__).read_bytes())},"excluded":["Private transfer captures/file identifiers/signed links","Original ZIPs, binaries, databases, large raw payloads","R34 C16 acceptance and post-cutoff CI observations","Missing original stage-scope clarification not reconstructed"],"fixed_prior_links":{"R31_helpers_commit":"8a8cc951d6c198d0ea28c01eaaaaf12df903ab77","C8_complete_original_custody_commit":"92b5624b821da81bbf3bf4c1eb05d1d3c44d7c1c","C_original_custody_commit":"1d56747eafcef6c22ceaec949fabe30a3362862c","M6_source_commit":"b2a17adc188338c48b95d01761836916e5d0fcdb"},"native_measurement_executed":False})
readme="""# 第33轮原始记录保全
截至 2026-10-09 14:21:51 UTC，原任务仍 192 项、163 done、29 remaining，本轮关闭 0 项。
本容器保留三个固定清单和 root 的六份原始管理记录，所有成员按原字节、原模式保存；清单、路径和 SHA/Git blob 身份见 inventory。打包与重新开包逐成员核对不构成新的原件负载执行。
G2 在该截止点已接受 C1/C4/C8 三份各900请求原件；C16 未纳入本轮接受记录。其后看到的原C16完成时间以及第34轮验收，不能回填本轮事实快照。旧C scale 仍4/150片、41/1500样本；M6当时只有已观察的排队CI状态，没有本轮新的native通过。
本地记录缺失和恢复边界原样保留：C8 新离线复验复现此前报告长度与SHA，但旧执行收据本地字节未恢复，不伪造旧时间；M6原guard实际观察结果与缺失本地收据分开。唯一旧005/006/008澄清原文未恢复，不能以新文字冒充其原哈希。
R31冻结工具、M6源码准入及两份完整原ZIP永久保管分支只按 inputs 中固定commit引用，不在此重复上传。此包不包含私有下载引用或原ZIP，不代表全规模通过、发布批准或任务关闭。
"""
with (OUT/"README.md").open("x") as f:f.write(readme)
files=[]
for name in ["round33-public-evidence.tar.gz","round33-public-evidence-inventory.json","round33-public-bundle-inputs.json","README.md","prepare_round33.py"]:
 p=OUT/name;b=p.read_bytes();files.append({"source":str(p.relative_to(ROOT)),"repository_target":"artifacts/checkpoints/p8-round33-a217-20261009/"+name,"bytes":len(b),"sha256":sha(b),"git_blob":oid(b),"mode":"100644","type":"blob"})
plan=write_json("git-upload-plan.json",{"schema":"round33-git-upload-plan-v1","expected_branch":"task/p8-round25-receipts-a217-20261009","expected_parent":"76369a0306026ebd90239b700f3ba16138a24c5c","files":files,"source_and_tasks_changed":False,"ref_mutation_authorized_only_after_parent_confirmation":True})
print(json.dumps({"members":len(selected),"payload_bytes":sum(x["bytes"] for x in selected),"container":container,"upload_files":len(files),"plan":str(plan),"plan_sha256":sha(plan.read_bytes())}))
