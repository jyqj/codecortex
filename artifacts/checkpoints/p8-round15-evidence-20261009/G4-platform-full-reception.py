# Read-only forensic receiver used for the exact original platform delivery.
# Supply its authorized download URL as argv[1]; requires the fixed G4 checkout path below.
# The executed URL credential is intentionally not archived. No predicates changed.
import os,sys,io,json,hashlib,zipfile,stat,subprocess
from pathlib import Path,PurePosixPath
SIZE=137027856
SHA="40aff225bb00a6c84a35c07505ce8602058bc322e3b32e0ffe44d5cee211de43"
REG=json.loads("{\"schema\":\"p8-G4-platform-offline-reception-v1\",\"source_commit\":\"260f596582f2d82b8d7c707b61a6b8b6a43b069f\",\"source_tree\":\"645404431ca15770d6e729785a3ada68b096c593\",\"source_manifest_sha256\":\"410e44c25ec85df2c5dfe43e46f32c7a04afccd31e9b79403dda051e82061d09\",\"observer_manifest_sha256\":\"f66cd4a5401eab4b5fc2283adf38775a2a7edb7347654a7aece6ce5a4c8b0935\",\"run_id\":37854847808,\"run_attempt\":1,\"reserve_bytes\":536870912,\"artifact_bytes\":68741273,\"expanded_bytes\":222194711,\"artifacts\":[{\"id\":11583244962,\"name\":\"p8-platform-cell-macos-stable-semantic-260f596582f2d82b8d7c707b61a6b8b6a43b069f\",\"bytes\":8605638,\"sha256\":\"eb620b7f62892976396156b4156cbcbf198568dd1f4604ab8f969adf3c26fe1c\",\"members\":19,\"expanded_bytes\":28542820,\"kind\":\"cell\",\"job_id\":113576447082,\"job_name\":\"cold macos / stable / semantic\",\"cell\":{\"platform\":\"macos\",\"toolchain\":\"stable\",\"package\":\"semantic\"}},{\"id\":11583772913,\"name\":\"p8-platform-cell-macos-1.95-semantic-260f596582f2d82b8d7c707b61a6b8b6a43b069f\",\"bytes\":8270335,\"sha256\":\"bf27ea1b70b050b9cd8f3580263deb8bf1212342eee38ffe21e45452982e6b9d\",\"members\":19,\"expanded_bytes\":25744120,\"kind\":\"cell\",\"job_id\":113576447234,\"job_name\":\"cold macos / 1.95 / semantic\",\"cell\":{\"platform\":\"macos\",\"toolchain\":\"1.95\",\"package\":\"semantic\"}},{\"id\":11584962547,\"name\":\"p8-platform-cell-macos-stable-default-260f596582f2d82b8d7c707b61a6b8b6a43b069f\",\"bytes\":8351960,\"sha256\":\"eeb9ea46fb1073d9008af5c3038b2b0a8d7186b2f04b11e0f618de3c129d72d2\",\"members\":19,\"expanded_bytes\":27941659,\"kind\":\"cell\",\"job_id\":113576447221,\"job_name\":\"cold macos / stable / default\",\"cell\":{\"platform\":\"macos\",\"toolchain\":\"stable\",\"package\":\"default\"}},{\"id\":11585688820,\"name\":\"p8-platform-cell-macos-1.95-default-260f596582f2d82b8d7c707b61a6b8b6a43b069f\",\"bytes\":8032137,\"sha256\":\"4ade802dec4d0d677c93a887d603ecfdde19706827491187bd8ea3db8b250689\",\"members\":19,\"expanded_bytes\":25254103,\"kind\":\"cell\",\"job_id\":113576447333,\"job_name\":\"cold macos / 1.95 / default\",\"cell\":{\"platform\":\"macos\",\"toolchain\":\"1.95\",\"package\":\"default\"}},{\"id\":11591060593,\"name\":\"p8-platform-cell-linux-1.95-default-260f596582f2d82b8d7c707b61a6b8b6a43b069f\",\"bytes\":8767683,\"sha256\":\"2994e919249b05d0efaa658e2385f1a982cf1c4524684c7600f61c8a2695b03d\",\"members\":19,\"expanded_bytes\":28449992,\"kind\":\"cell\",\"job_id\":113576447233,\"job_name\":\"cold linux / 1.95 / default\",\"cell\":{\"platform\":\"linux\",\"toolchain\":\"1.95\",\"package\":\"default\"}},{\"id\":11590640239,\"name\":\"p8-platform-cell-linux-stable-semantic-260f596582f2d82b8d7c707b61a6b8b6a43b069f\",\"bytes\":8968878,\"sha256\":\"0a376de04d71f0b78b7768dc3120db0df7cf32bbde871112a0591bbaa910c8b2\",\"members\":19,\"expanded_bytes\":28900165,\"kind\":\"cell\",\"job_id\":113576447250,\"job_name\":\"cold linux / stable / semantic\",\"cell\":{\"platform\":\"linux\",\"toolchain\":\"stable\",\"package\":\"semantic\"}},{\"id\":11591967823,\"name\":\"p8-platform-cell-linux-stable-default-260f596582f2d82b8d7c707b61a6b8b6a43b069f\",\"bytes\":8720306,\"sha256\":\"a57ac8d748b8cee62698028e4bba8637deb5dcee202b712c3c2e235729193842\",\"members\":19,\"expanded_bytes\":28336212,\"kind\":\"cell\",\"job_id\":113576447190,\"job_name\":\"cold linux / stable / default\",\"cell\":{\"platform\":\"linux\",\"toolchain\":\"stable\",\"package\":\"default\"}},{\"id\":11591803805,\"name\":\"p8-platform-cell-linux-1.95-semantic-260f596582f2d82b8d7c707b61a6b8b6a43b069f\",\"bytes\":9022932,\"sha256\":\"cb364e83ba9ee75a0fa95bf7d02de0252b9abbc07bbf43ce3f36708dbd927755\",\"members\":19,\"expanded_bytes\":29021757,\"kind\":\"cell\",\"job_id\":113576447383,\"job_name\":\"cold linux / 1.95 / semantic\",\"cell\":{\"platform\":\"linux\",\"toolchain\":\"1.95\",\"package\":\"semantic\"}},{\"id\":11591824150,\"name\":\"p8-platform-complete-260f596582f2d82b8d7c707b61a6b8b6a43b069f\",\"kind\":\"collector\",\"job_id\":113645013564,\"job_name\":\"all eight platform cells\",\"bytes\":1404,\"sha256\":\"bf34f33830d9c89ca00f2286f5e566ad18059acaa42cb37569878bb21876e261\",\"members\":1,\"expanded_bytes\":3883}],\"scope\":\"Nine original fixed G4 archives. Original collect-cells CLI only; no Cargo, native product, providers, rebuild, new benchmark, altered predicate, or TODO closure.\"}")
def need(ok,message):
 if not ok: raise ValueError(message)
def digest(raw):return hashlib.sha256(raw).hexdigest()
mem=Path("/sys/fs/cgroup")
current=int((mem/"memory.current").read_text()); maximum=int((mem/"memory.max").read_text())
stats=dict(x.split() for x in (mem/"memory.stat").read_text().splitlines())
available=maximum-current+int(stats["inactive_file"])+int(stats["slab_reclaimable"])
required=512*1024**2+SIZE+64*1024**2
need(available>=required,"insufficient reclaimable memory reserve before download")
memory=dict(current=current,maximum=maximum,inactive_file=int(stats["inactive_file"]),slab_reclaimable=int(stats["slab_reclaimable"]),estimated_available=available,required=required)
class MemoryViewFile:
 def __init__(self,data):self.data=memoryview(data);self.position=0
 def tell(self):return self.position
 def seek(self,offset,whence=0):
  if whence==0:position=offset
  elif whence==1:position=self.position+offset
  elif whence==2:position=len(self.data)+offset
  else:raise ValueError("bad whence")
  if position<0:raise ValueError("negative seek")
  self.position=position
  return position
 def read(self,size=-1):
  end=len(self.data) if size is None or size<0 else min(len(self.data),self.position+size)
  result=bytes(self.data[self.position:end]);self.position=end
  return result
 def seekable(self):return True
raw=bytearray(SIZE)
process=subprocess.Popen(["curl","--fail","--location","--silent","--show-error","--retry","0","--connect-timeout","30","--max-time","240","--max-filesize",str(SIZE),sys.argv[1]],stdout=subprocess.PIPE,stderr=subprocess.PIPE)
position=0
while position<SIZE:
 n=process.stdout.readinto(memoryview(raw)[position:min(SIZE,position+1024*1024)])
 if not n:break
 position+=n
extra=process.stdout.read(1);stderr=process.stderr.read()
try:code=process.wait(timeout=250)
except subprocess.TimeoutExpired:raise ValueError("fixed original artifact transfer timeout") from None
if not (code==0 and not stderr and position==SIZE and not extra):
 print(json.dumps(dict(phase="transport_failed",curl_exit=code,received_bytes=position,stderr_bytes=len(stderr),stderr_sha256=digest(stderr),extra_byte=bool(extra))),flush=True)
 raise ValueError("bounded single-buffer original artifact transfer failed")
need(digest(raw)==SHA,"outer original ZIP differs")
archive=zipfile.ZipFile(MemoryViewFile(raw))
infos=archive.infolist()
need(len({i.filename for i in infos})==len(infos),"outer duplicate paths")
members={}
for info in infos:
 path=PurePosixPath(info.filename)
 need(not info.is_dir() and not path.is_absolute() and ".." not in path.parts and path.as_posix()==info.filename and "\\" not in info.filename and stat.S_IFMT(info.external_attr>>16) in (0,stat.S_IFREG) and not info.flag_bits&1,"outer unsafe nonregular member")
 h=hashlib.sha256();n=0
 with archive.open(info) as stream:
  while block:=stream.read(1024*1024):h.update(block);n+=len(block)
 need(n==info.file_size,"outer expanded size")
 members[info.filename]=dict(bytes=n,sha256=h.hexdigest(),crc32="%08x"%info.CRC)
def read(name):return archive.read(name)
def value(name):return json.loads(read(name))
receipt=value("receipt.json"); inventory=value("file-inventory.json")
need(set(members)-{"receipt.json","file-inventory.json"}==set(inventory),"outer full inventory population")
for name,row in inventory.items():
 need(row=={k:members[name][k] for k in ("bytes","sha256")},"outer full inventory value")
need(digest(read("file-inventory.json"))==receipt["file_inventory_sha256"],"file inventory digest")
need(receipt["status"]=="accepted_scoped_original_G4_platform_replay" and receipt["exit_code"]==0 and receipt["controller_commit"]=="faf8893ba1f8c7a875b94b648e0a7d2f520599c8" and receipt["source_commit"]==REG["source_commit"] and receipt["original_run"]==REG["run_id"],"receipt source and result")
need(receipt["counts"]==dict(passed=8,failed=0,not_run=0) and receipt["original_artifact_ids"]==[s["id"] for s in REG["artifacts"]] and receipt["product_or_Cargo_execution"] is False,"receipt original full population")
need(value("registration.json")==REG,"retained registration")
need(read("source-before.json")==read("source-after.json") and read("observer-before.json")==read("observer-after.json"),"before-after original source")
source=value("source-before.json"); observer=value("observer-before.json")
need(source["source_commit"]==REG["source_commit"] and source["source_tree"]==REG["source_tree"] and source["input_count"]==1087 and source["manifest_sha256"]==REG["source_manifest_sha256"],"source snapshot")
need(observer["manifest_sha256"]==REG["observer_manifest_sha256"] and len(observer["inputs"])==4,"observer snapshot")
sys.path.insert(0,"/workspace/scratch/2eaa00d0f93a/p8-G4-exact-source/scripts")
import p8_cold_build as cold
local_source,local_inputs=cold.source_identity(Path("/workspace/scratch/2eaa00d0f93a/p8-G4-exact-source"),REG["source_commit"])
need(value("source-inputs.json")==local_inputs and local_source["manifest_sha256"]==REG["source_manifest_sha256"],"all1087 original input hashes")
need(cold.observer_snapshot(Path("/workspace/scratch/2eaa00d0f93a/p8-G4-exact-source"))["manifest_sha256"]==REG["observer_manifest_sha256"],"all4 original loaded observer hashes")
originals=[]
for spec in REG["artifacts"]:
 zipname="zips/%d.zip"%spec["id"]
 original=read(zipname)
 need(len(original)==spec["bytes"] and digest(original)==spec["sha256"],"retained original ZIP identity")
 inner=zipfile.ZipFile(io.BytesIO(original));innerinfos=inner.infolist()
 need(len(innerinfos)==len({i.filename for i in innerinfos})==spec["members"] and sum(i.file_size for i in innerinfos)==spec["expanded_bytes"],"original member population")
 prefix=("cells/" if spec["kind"]=="cell" else "original-collector/")+str(spec["id"])+"/"
 expected_names=set()
 original_members={}
 for info in innerinfos:
  name=prefix+info.filename;expected_names.add(name)
  h=hashlib.sha256();n=0
  with inner.open(info) as stream:
   while block:=stream.read(1024*1024):h.update(block);n+=len(block)
  need(n==info.file_size and members[name]["bytes"]==n and members[name]["sha256"]==h.hexdigest(),"retained original versus extracted byte identity")
  original_members[info.filename]=dict(bytes=n,sha256=h.hexdigest(),crc32="%08x"%info.CRC,extracted=True)
 need(expected_names=={name for name in members if name.startswith(prefix)},"exact original expanded population")
 need(value("members/%d.json"%spec["id"])==original_members,"per-original inventory complete")
 if spec["kind"]=="cell":
  bundle=value(prefix+"bundle.json")
  need(bundle["cell"]==spec["cell"],"registered original cell")
 else:need(set(original_members)=={"matrix.json"},"original collector sole member")
 originals.append(dict(artifact_id=spec["id"],kind=spec["kind"],bytes=spec["bytes"],sha256=spec["sha256"],members=original_members,expanded_prefix=prefix))
 inner.close()
del original
root=source["source_root"]
cmd=value("commands/original-cold-collector/command.json"); result=value("commands/original-cold-collector/result.json")
argv=cmd["argv"]
need(Path(argv[0]).name in ("python3","python3.12","python") and argv[1:]==["-B",root+"/scripts/p8_cold_build.py","--source-root",root,"--collect-cells","/home/runner/work/_temp/p8-G4-offline-platform/cells","--expected-commit",REG["source_commit"],"--output-dir","/home/runner/work/_temp/p8-G4-offline-platform/replay"],"actual original collector argv")
need(cmd["cwd"]==root and cmd["source_commit"]==REG["source_commit"] and cmd["scope"]=="offline replay only" and result["exit_code"]==0,"actual original collector exit")
collectors=[s for s in REG["artifacts"] if s["kind"]=="collector"]
original_matrix=value("original-collector/%d/matrix.json"%collectors[0]["id"]); replay=value("replay/matrix.json")
mapped=json.loads(json.dumps(original_matrix));mapped["source"]["source_root"]=root
need(replay==mapped and replay["counts"]==dict(passed=8,failed=0,not_run=0),"full original collector matrix except source-root")
comparison=value("collector-comparison.json")
need(comparison["exact_except_source_root"] is True and comparison["original_sha256"]==digest(read("original-collector/%d/matrix.json"%collectors[0]["id"])) and comparison["replay_sha256"]==digest(read("replay/matrix.json")),"retained complete collector comparison")
need(digest(raw)==SHA,"outer original retained unchanged")
report=dict(schema_version=1,reviewer="/root",status="accepted_scoped_original_G4_platform_full_receiver_delivery",
 source_commit=REG["source_commit"],source_tree=REG["source_tree"],original_run=REG["run_id"],controller_commit=receipt["controller_commit"],receiver_run=37877975618,receiver_job=113650819937,
 artifact_id=11593143051,outer_zip=dict(bytes=SIZE,sha256=SHA,members=len(members),expanded_bytes=sum(x["bytes"] for x in members.values())),
 receipt=receipt,all_delivered_member_CRC_size_SHA_verified=True,full_inventory_bidirectional=True,missing=0,extra=0,
 original_artifact_population=originals,source=source,observer=observer,all1087_input_hashes_independently_matched_existing_exact_G4=True,actual_original_collector_command=cmd,actual_original_collector_result=result,
 original_matrix=original_matrix,replay_matrix=replay,exact_full_matrix_except_source_root=True,
 outer_inventory=members,memory_preflight=memory,receiver_completed_source_before_after_equal=True,
 scope=["Actual original Path-dependent collector executed remotely on all8 complete original cell archives and returned0 with8passed/0failed/0not_run.","Root independently received the full original outer ZIP, every original nested ZIP and extracted byte, complete retained inventories, original source and collector command/result.","No local collector/native/product/Cargo execution occurred. Only read-only original source identity and pure-memory ZIP/hash/JSON verification.","Original G4 identity1087/cold observer4 is preserved; it is not a currentG2/PR169 or a23/PR167 execution. Original TODO dependencies and release readiness remain separate.","This report references retained original Actions ZIPs; it is not itself a standalone replay bundle."],transport_preparation=dict(prior_attempt="Rejected insufficient memory before any transfer",single_fixed_bytearray=True,no_full_ZIP_copy_for_ZipFile=True,same_512MiB_reserve=True),local_files_written=0,new_TODO_closed=0,TODO_remaining=29)
print(json.dumps(report,sort_keys=True),flush=True)
