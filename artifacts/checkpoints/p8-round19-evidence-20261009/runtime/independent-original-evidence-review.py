import json,pathlib,hashlib,zipfile,zlib,stat,tarfile,re,datetime,collections,sys,traceback
N=pathlib.Path("/Users/jin/Desktop/codecortex-rust/artifacts/checkpoints/p8-round19-evidence-intake-20261009/runtime275e-author")
O=pathlib.Path("/Users/jin/Desktop/codecortex-rust/artifacts/checkpoints/p8-round19-evidence-intake-20261009/gates-lifecycle275e-author/runtime-independent-audit-01")
SOURCE="275e8799d4947d297329073eaa3ca675d3fd0777"
MANIFEST="593cf443fc914998dd2cb5a30c3caf180eb8d0d7a398a86e4624e3df2ca65e83"
OBSERVER="ca48939e0647dba2f12e9f26ab0bc88860219ddb304574b6386e5d47b8945064"
RUN=37890757030
PINS={
"delivery/manifest.json":"292ae8e3f920825f1eee52a9a711d11be5062785c8652293e438c54e2eef99ed",
"replay-output/offline-replay/receipt.json":"53d840315bec62f6a3e30f684a14d203cc359699cde87d7600112e46c40d1d5f",
"replay-output/wire-supplement/cache-wire-binding.json":"9a405c5d8b3f906aeace4358307e5a33202eab18a1999705047b6f6e69142453",
"delivery/safe-text-reports.tar.gz":"7542de53f05fbb3716309d2342b522b04329105e0050e8c0b2b12ac65bc868b9"}
EXPECTED={
"11600492680":(51068934,"d99ee2d1e69e0b6787c42a95d9e0277f0adcf0a72add01a7d2b995a427d2f8e7",4,1),
"11598875346":(16605523,"702548d29259a1cefbb444704ea0770df6d6489b4b15323d63633fb4b54904ef",1,1),
"11598815621":(16649026,"f6673ea6d3cba6047d8db028fcad8ef7724927f09425d35bbda5bfc0464f3cdb",8,7),
"11598690899":(16979017,"91266b26a0ca44d88c86947028c5c288b2afb024f2eec0e5c471f199e6545a7c",4,4),
"11598543621":(16412841,"dd27a0afbdc5382c616c56bc42fc78ebd085b15454bcd7f12bcfdebdcb320ec3",16,12),
"11597519732":(7831523,"2c16ee9e6e3ca7bb28844473b478b5bbe1b1f2596805713696f40cee854b7359",None,None)}
def require(ok,msg):
 if not ok: raise ValueError(msg)
def sha(b):return hashlib.sha256(b).hexdigest()
def read(p):
 p=pathlib.Path(p); require(p.is_file() and not p.is_symlink(),"not regular input "+str(p));return p.read_bytes()
def filehash(p):
 h=hashlib.sha256()
 with p.open("rb") as f:
  for c in iter(lambda:f.read(1024*1024),b""):h.update(c)
 return h.hexdigest()
def j(p):return json.loads(read(p))
def mark(p):
 s=p.lstat();require(stat.S_ISREG(s.st_mode) and not p.is_symlink(),"unsafe input")
 return [s.st_dev,s.st_ino,s.st_size,s.st_mtime_ns,s.st_nlink]
def json_bytes(v):return (json.dumps(v,ensure_ascii=False,sort_keys=True,indent=2)+"\n").encode()
def under(p,root):
 require(root in p.parents,"path escapes owned root");require(not any(x.is_symlink() for x in [p,*p.parents] if x==root or root in x.parents),"symlink path")
def paths(root):
 out={}
 for p in root.rglob("*"):
  require(not p.is_symlink(),"symlink expanded")
  if p.is_file():out[p.relative_to(root).as_posix()]=p
 return out
def tool_value(p):
 require("error" not in p and isinstance(p.get("result"),dict),"RPC error")
 r=p["result"];require(r.get("isError") is not True,"tool error")
 v=r["structuredContent"];return v.get("result",v)
def read_transport(p):
 req={};res={};wire={};times={};events=collections.Counter();eof=exit0=False
 for line in p.open():
  e=json.loads(line);k=e.get("kind") if e.get("event")=="terminal" else e.get("event");events[k]+=1
  if k in ("request","response"):
   v=e["payload"];target=req if k=="request" else res
   require(v["id"] not in target,"duplicate RPC ID");target[v["id"]]=v
   times.setdefault(v["id"],{})[k]=e["time_ns"]
  if k=="stdout_wire":
   b=e["text"].encode();require(len(b)==e["wire_bytes"] and sha(b)==e["wire_sha256"],"wire bytes mismatch");v=json.loads(b)
   if "id" in v:require(v["id"] not in wire,"duplicate wire ID");wire[v["id"]]=v
  if k=="process_exit":exit0=e.get("exit_code",e.get("code"))==0
  if k in ("stdout_eof","eof"):eof=True
 require(eof and exit0 and set(req)==set(res)==set(wire) and res==wire,"incomplete transport")
 require(all(t["request"]<=t["response"] for t in times.values()),"transport time")
 return req,res,times,dict(events)
O.mkdir(exist_ok=False)
report={"schema":"independent-original-runtime-custody-review-v1","status":"rejected_or_partial","source":SOURCE,"run":RUN,"reviewer":"/root/pr_audit","started_at":datetime.datetime.now(datetime.timezone.utc).isoformat(),"scope":"Read-only independent original ZIP/member/expanded custody, existing consumer execution and exact output checks; no validator, statistics binary, product or Cargo reexecuted.","TODO_closed":0,"TODO_remaining":29,"errors":[]}
tracked={}
try:
 for rel,pin in PINS.items():
  p=N/rel;require(filehash(p)==pin,"pinned body "+rel);tracked[p]=mark(p)
 manifest=j(N/"delivery/manifest.json");receipt=j(N/"replay-output/offline-replay/receipt.json")
 require(receipt["status"]=="accepted_scoped_original_275e_runtime_six_domains" and receipt["exit_code"]==0 and receipt["all_originals_unchanged"],"receipt verdict")
 require(receipt["source"]==SOURCE and receipt["run"]==RUN and receipt["source_input_count"]==1089 and receipt["observer_count"]==9,"receipt source")
 require(manifest["file_count"]==len(manifest["files"])==106,"text count")
 text_bytes=0
 for rel,m in manifest["files"].items():
  p=N/rel;under(p,N);b=read(p);text=b.decode("utf-8")
  require(len(b)==m["bytes"] and sha(b)==m["sha256"],"text manifest "+rel)
  require(not re.search(r"https?://[^\s\"<>]*[?&](?:X-Amz-Signature|X-Goog-Signature|sig|access_token)=",text,re.I),"private signed URL "+rel)
  tracked[p]=mark(p);text_bytes+=len(b)
 require(text_bytes==manifest["text_bytes"]==5883340,"text byte total")
 with tarfile.open(N/"delivery/safe-text-reports.tar.gz","r:gz") as t:
  items=t.getmembers();require(len(items)==106 and {m.name for m in items}==set(manifest["files"]),"safe tar population")
  for m in items:
   require(m.isfile(),"safe tar nonfile");b=t.extractfile(m).read();expected=manifest["files"][m.name]
   require(len(b)==expected["bytes"] and sha(b)==expected["sha256"],"safe tar member")
 report["text_delivery"]={"files":106,"bytes":text_bytes,"manifest_sha256":PINS["delivery/manifest.json"],"tar_all_members_equal":True,"signed_URI_scan":"none matched URL signature/access-token query keys"}
 report["archives"]=[]
 require(set(receipt["six_original_ZIP_inventories"])==set(EXPECTED),"six receipt archives")
 for aid,(size,digest,c,peak) in EXPECTED.items():
  z=N/"evidence/raw"/aid/"original.zip";expanded=z.parent/"extracted";inv=j(N/"evidence/inventories"/(aid+".json"));ri=receipt["six_original_ZIP_inventories"][aid]
  require(z.stat().st_size==size and filehash(z)==digest,"whole ZIP "+aid);tracked[z]=mark(z)
  require(ri["members"]==inv and ri["archive"]["bytes"]==size and ri["archive"]["sha256"]==digest and ri["archive"]["id"]==int(aid),"inventory receipt "+aid)
  actual=paths(expanded);require(set(actual)==set(inv),"expanded closure "+aid)
  total=0
  with zipfile.ZipFile(z) as archive:
   members=archive.infolist();require(len(members)==len(inv) and len({m.filename for m in members})==len(members),"ZIP member population")
   for m in members:
    pp=pathlib.PurePosixPath(m.filename);require(not pp.is_absolute() and ".." not in pp.parts and not m.is_dir(),"ZIP path")
    h=hashlib.sha256();crc=0;n=0
    with archive.open(m) as stream:
     for b in iter(lambda:stream.read(1024*1024),b""):h.update(b);crc=zlib.crc32(b,crc);n+=len(b)
    v=inv[m.filename];require(n==m.file_size==v["bytes"] and h.hexdigest()==v["sha256"] and ("%08x"%(crc&0xffffffff))==v["crc32"] and crc&0xffffffff==m.CRC,"ZIP CRC/SHA "+m.filename)
    p=actual[m.filename];require(p.stat().st_size==n and filehash(p)==h.hexdigest(),"expanded SHA "+m.filename);tracked[p]=mark(p);total+=n
  report["archives"].append({"artifact_id":int(aid),"bytes":size,"sha256":digest,"members":len(inv),"expanded_bytes":total,"all_member_CRC_SHA_and_exact_expanded_closure":True})
 source=N/"evidence/source";before=j(N/"replay-output/offline-replay/source-before.json");after=j(N/"replay-output/offline-replay/source-after.json")
 require(before==after and before["source_commit"]==SOURCE and before["input_count"]==len(before["inputs"])==1089 and before["manifest_sha256"]==MANIFEST,"source snapshots")
 require(sha(json_bytes(before["inputs"]))==MANIFEST,"source map digest")
 for rel,digest in before["inputs"].items():
  p=source/rel;require(filehash(p)==digest,"source body "+rel);tracked[p]=mark(p)
 ob=j(N/"replay-output/offline-replay/observer-before.json")
 require(ob==j(N/"replay-output/offline-replay/observer-after.json") and ob["source_commit"]==SOURCE and len(ob["files"])==9 and ob["manifest_sha256"]==OBSERVER,"observer snapshots")
 for rel,m in ob["files"].items():
  b=read(source/rel);require(len(b)==m["bytes"] and sha(b)==m["sha256"] and hashlib.sha1(("blob "+str(len(b))+"\0").encode()+b).hexdigest()==m["git_blob"],"observer source "+rel)
 report["source_binding"]={"input_count":1089,"manifest_sha256":MANIFEST,"source_before_after_equal":True,"all_current_input_bytes_match":True,"observer_count":9,"observer_manifest_sha256":OBSERVER,"all_observer_bytes_and_git_blobs_match":True}
 controls=j(N/"launch/control-before.json")
 require(controls==j(N/"launch/control-after.json"),"control after")
 for rel,m in controls.items():
  b=read(N/rel);require(len(b)==m["bytes"] and sha(b)==m["sha256"],"control bytes");tracked[N/rel]=mark(N/rel)
 launch=j(N/"launch/receipt.json");container_before=j(N/"launch/container-before.stdout")[0];container_after=j(N/"launch/container-after.stdout")[0]
 require(launch["exit_code"]==launch["original_consumer_exit_code"]==0 and launch["source"]==SOURCE and launch["run"]==RUN,"launch receipt")
 require(container_before["Id"]==container_after["Id"]==launch["container_id"] and container_after["State"]["ExitCode"]==0,"actual container exit")
 hc=container_before["HostConfig"]
 require(hc["NetworkMode"]=="none" and hc["ReadonlyRootfs"] and not hc["Privileged"],"sandbox actual")
 mounts=container_before["Mounts"];require([x["Destination"] for x in mounts if x["RW"]]==["/work"],"only owned output writable")
 for target in ["/receiver","/work/source","/work/raw","/work/inventories"]:require(any(x["Destination"]==target and not x["RW"] for x in mounts),"readonly mount "+target)
 require(container_before["Config"]["Image"]==launch["image"]=="python@sha256:3d361d7fea344d55ac7a0f51ed7faa99213808ccc96fc9b9170adb95b6570b96","image pin")
 report["existing_launch"]={"container_id":launch["container_id"],"exit_code":0,"original_consumer_exit_code":0,"image":launch["image"],"network":"none","rootfs_readonly":True,"only_writable_bind":"/work","input_binds_readonly":True,"all_eight_control_files_unchanged":True,"executions_by_this_auditor":0}
 report["domains"]=[];report["statistics_pairs"]=[]
 for aid,(_,_,c,peak) in EXPECTED.items():
  backfill=c is None;soak=aid=="11600492680"
  path=N/"replay-output/review-runtime"/(("backfill-"+aid+"-review.json") if backfill else ("runtime-"+aid+"-review-v2.json"))
  r=j(path);require(r["errors"]==[] and r["review_status"]=="raw_and_receipt_review_passed" and r["source_sha"]==SOURCE and r["run_id"]==RUN and r["source_input_count"]==1089 and r["observer_count"]==9,"domain verdict")
  if backfill:
   require(r["requests"]==768 and len(r["cells"])==24 and all(x["n"]==32 for x in r["cells"]),"backfill population")
   require({(x["seed"],x["phase"],x["concurrency"]) for x in r["cells"]}=={(s,p,k) for s in [7,19,43] for p in ["quiet","held"] for k in [1,4,8,16]},"backfill cell keys")
   require(all(x["max_provider_active"]==4 and x["new_provider_calls"]==25 and x["old_held_input_publications"]==0 and x["final_queue"]["pending"]==x["final_queue"]["claimed"]==0 for x in r["seeds"]),"backfill final facts")
   report["domains"].append({"profile":"backfill","artifact_id":int(aid),"report_sha256":filehash(path),"requests":768,"cells":24,"requests_per_cell":32,"seeds":[7,19,43],"scope":"fake-provider; no confidence interval or new tail/SLA claim"})
   continue
  root=N/"evidence/raw"/aid/"extracted/p8-runtime";original=j(root/"report.json")
  require(r["outcomes"]==({"read":2400,"build":1201} if soak else {"read":600,"build":300}),"runtime denominator")
  require(r["configured_concurrency"]==c and r["actual_concurrency"]["maximum"]==peak,"runtime concurrency")
  require(len(r["parity_tables"])==15 and all(x["equal"] for x in r["parity_tables"]),"15-table equality")
  require(r["original_verify_cli"]["exit_code"]==0 and json.loads(r["original_verify_cli"]["stdout"])["status"]=="sealed_artifacts_verified","original verify success")
  if soak:
   cache=r["soak_cache_replay"];require(cache["passed"] and cache["errors"]==[] and cache["validated_reads"]==2400 and cache==original["cache_reuse"],"cache summary")
   require(cache["observed"]=={"hit":1400,"miss":1000,"invalidations":999},"cache counts")
   require(r["observed_work_ns"]>=3600*10**9 and r["branch_switches"]==200 and r["catalog_compactions"]==25,"hour workload")
   new=N/"replay-output/wire-supplement/statistics.json";cmd=j(new.parent/"statistics-command.json");result=j(new.parent/"statistics-execution.json")
  else:
   new=N/"replay-output/offline-replay"/("statistics-"+aid+".json");cmd=j(new.with_suffix(".command.json"));result=j(new.with_suffix(".result.json"))
  require(result["exit_code"]==0,"original stats exit")
  require(read(new)==read(root/"statistics.json")==read(root/"statistics-replay.json"),"three actual stats outputs equal")
  argv=cmd["argv"];require(argv[argv.index("--plan")+1].endswith("/raw/"+aid+"/extracted/p8-runtime/plan.json") and argv[argv.index("--raw")+1].endswith("/raw/"+aid+"/extracted/p8-runtime/raw.jsonl"),"original stats inputs")
  report["statistics_pairs"].append({"artifact_id":int(aid),"sha256":filehash(new),"bytes":new.stat().st_size,"actual_recorded_exit_code":0,"both_original_outputs_byte_equal":True,"original_plan_raw_arguments":True})
  report["domains"].append({"artifact_id":int(aid),"profile":"soak" if soak else "mixed","report_sha256":filehash(path),"outcomes":r["outcomes"],"configured_concurrency":c,"actual_maximum":peak,"parity_tables":15,"branches":r["branch_switches"],"compactions":r["catalog_compactions"],"original_verify_exit_code":0,"observed_work_ns":r["observed_work_ns"]})
 # Independently check the supplied mapping against exact original transport bytes.
 root=N/"evidence/raw/11600492680/extracted/p8-runtime"
 records=[json.loads(l) for l in (root/"raw.jsonl").open()];rows=[x for x in records if x.get("kind")=="operation"];reads={x["id"]:x for x in rows if x["operation"]=="read"}
 require(len(rows)==3601 and len(reads)==2400 and len({x["id"] for x in rows})==3601,"hour raw population")
 require(all(x["status"]=="success" for x in rows),"hour raw outcomes")
 rq,rs,tm,kinds=read_transport(root/"product/rpc.jsonl");fq,fs,ft,fk=read_transport(root/"full-product/rpc.jsonl")
 mapping=j(N/"replay-output/wire-supplement/cache-wire-binding.json");low,high=mapping["common_monotonic_origin_interval_ns"];require(low<=high,"origin interval")
 seen=set();pairs=set();bounds=[];roles=collections.Counter()
 for m in mapping["mapping"]:
  oid,role,i=m["operation_id"],m["role"],m["rpc_id"];require(i not in seen and (oid,role) not in pairs,"binding duplicate");seen.add(i);pairs.add((oid,role));roles[role]+=1
  calls=[x for x in reads[oid]["cache_probe"]["requests"] if x["role"]==role];require(len(calls)==1,"role unique");call=calls[0]
  require(call["status"]=="success" and rq[i]["method"]=="tools/call" and rq[i]["params"]=={"name":call["name"],"arguments":call["arguments"]},"bound parameters")
  require(tool_value(rs[i])==call["response"],"bound full response")
  lo=tm[i]["response"]-call["finished_ns"];hi=tm[i]["request"]-call["started_ns"];require(lo<=low<=high<=hi,"bound common clock")
  bounds.append((lo,hi))
 require(len(seen)==mapping["bound_RPCs"]==9600 and mapping["compound_reads"]==2400 and roles==mapping["role_counts"]=={k:2400 for k in ["before_status","symbol","hybrid","after_status"]},"9600 denominator")
 require(max(b[0] for b in bounds)==low and min(b[1] for b in bounds)==high,"exact shared origin")
 endpoints=[(i,x) for i,x in enumerate(records) if x.get("kind")=="endpoint_public"];require(len(endpoints)==1,"endpoint population")
 position,end=endpoints[0];ep=mapping["endpoint_binding"];ii,fi=ep["incremental_rpc_id"],ep["full_rpc_id"]
 require(position==ep["endpoint_public_record_index"] and max(i for i,x in enumerate(records) if x.get("kind")=="operation")<position,"endpoint order")
 expected={"name":"search","arguments":{"query":"p8_runtime_stable_signal","mode":"symbol","top_k":5}}
 require(ep["exact_parameters"]==rq[ii]["params"]==fq[fi]["params"]==expected and tool_value(rs[ii])==end["incremental"] and tool_value(fs[fi])==end["full"],"endpoint full value")
 searches={i for i,x in rq.items() if x.get("method")=="tools/call" and x.get("params",{}).get("name")=="search"}
 require(searches-{ii}=={m["rpc_id"] for m in mapping["mapping"] if m["role"] in ("symbol","hybrid")} and len(searches)==4801,"all search IDs explained")
 require(len([i for i,x in fq.items() if x.get("params",{}).get("name")=="search"])==1,"full endpoint unique")
 latest=max(tm[i]["response"] for i in seen);last_finished=max(x["finished_ns"] for x in rows)
 require(tm[ii]["request"]>max(latest,high+last_finished) and ft[fi]["request"]>=tm[ii]["response"],"endpoint after work")
 require(ep["latest_bound_read_response_ns"]==latest and ep["latest_raw_operation_finished_ns"]==last_finished,"endpoint receipt times")
 for prefix,i,t in [("incremental",ii,tm),("full",fi,ft)]:
  require(ep[prefix+"_request_ns"]==t[i]["request"] and ep[prefix+"_response_ns"]==t[i]["response"],"endpoint time binding")
 status={i for i,x in rq.items() if x.get("params",{}).get("name")=="status"};hour=j(N/"replay-output/wire-supplement/inspection.json")
 require(len(status-seen)==mapping["unbound_status_RPCs"]==3595 and hour["sampler_status_responses"]==3594 and hour["endpoint_status_rpc_id"] in status-seen,"sampler vs endpoint status population")
 require(len(rq)==14400 and len(fq)==4 and hour["cache_wire_sha256"]==PINS["replay-output/wire-supplement/cache-wire-binding.json"],"transport population and map digest")
 report["hour_wire"]={"compound_reads":2400,"bound_RPCs":9600,"role_counts":dict(roles),"complete_product_RPCs":14400,"complete_full_control_RPCs":4,"all_stdout_wire_bytes_crc_not_applicable_sha_verified":True,"all_request_params_full_response_values_equal":True,"common_clock_interval_ns":[low,high],"endpoint_searches_separately_proven":2,"sampler_status_responses":3594,"separate_endpoint_status":1,"no_contiguous_ID_assumption":True,"map_sha256":PINS["replay-output/wire-supplement/cache-wire-binding.json"],"cache":{"hit":1400,"miss":1000,"invalidations":999},"actual_hour_maximum":1}
 for p,metadata in tracked.items():require(mark(p)==metadata,"input metadata changed during review "+str(p))
 for rel,pin in PINS.items():require(filehash(N/rel)==pin,"pinned input final digest")
 report["input_preservation"]={"read_only":True,"regular_file_metadata_before_after_checked":len(tracked),"pinned_full_receipt_manifest_map_tar_rehashed":True,"no_source_or_original_writes":True}
 report["limitations"]=["No new product, Cargo, validator or statistics execution. Existing recorded original consumer results inspected and byte-bound.","Hour configured concurrency 4 had actual maximum 1; four mixed configured 1/4/8/16 had actual maxima 1/4/7/12.","No original N150 scale acceptance, TODO closure, source relabelling, cross-source performance comparison or live-provider certification."]
 report["status"]="accepted_scoped_original_runtime_six_domains"
except BaseException as e:
 report["errors"].append({"type":type(e).__name__,"message":str(e)})
 report["traceback"]=traceback.format_exc()
finally:
 report["finished_at"]=datetime.datetime.now(datetime.timezone.utc).isoformat()
 b=json_bytes(report);(O/"review.json").write_bytes(b)
 print(json.dumps({"status":report["status"],"errors":report["errors"],"report_path":str(O/"review.json"),"bytes":len(b),"sha256":sha(b),"archives":report.get("archives"),"text_delivery":report.get("text_delivery"),"domains":report.get("domains"),"hour_wire":report.get("hour_wire")}))
if report["status"]!="accepted_scoped_original_runtime_six_domains":sys.exit(1)
