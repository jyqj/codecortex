import hashlib,io,json,pathlib,subprocess,sys,zipfile
from collections import Counter
SIZE=50994513;SHA="46a72baaecbd0e5ea3197d05e639f618cd3a570b9cc65f46fa51a5f0a4916286"
r=subprocess.run(["curl","--fail","--location","--silent","--show-error","--retry","0","--max-time","180",sys.argv[1]],stdout=subprocess.PIPE,stderr=subprocess.PIPE,timeout=190)
assert r.returncode==0
assert len(r.stdout)==SIZE and hashlib.sha256(r.stdout).hexdigest()==SHA
z=zipfile.ZipFile(io.BytesIO(r.stdout))
inv={}
for i in z.infolist():
 h=hashlib.sha256();n=0
 with z.open(i) as f:
  while b:=f.read(1024*1024):h.update(b);n+=len(b)
 assert n==i.file_size
 inv[i.filename]={"bytes":n,"sha256":h.hexdigest(),"crc32":format(i.CRC,"08x")}
assert len(inv)==2125
def canon(v):return json.dumps(v,sort_keys=True,separators=(",",":"),ensure_ascii=False)
def val(p):
 assert "error" not in p
 v=p["result"];assert v.get("isError") is not True
 s=v["structuredContent"];return s.get("result",s)
roles=Counter();reads=[];endpoint=[];rawkinds=Counter();operation_counts=Counter()
for line in z.open("p8-runtime/raw.jsonl"):
 row=json.loads(line);rawkinds[row["kind"]]+=1
 if row["kind"]=="endpoint_public":endpoint.append(row)
 if row["kind"]=="operation":
  operation_counts[row["operation"]]+=1
  if row["operation"]=="read":
   calls=[]
   for c in row["cache_probe"]["requests"]:
    roles[c["role"]]+=1
    calls.append({k:c[k] for k in ("name","arguments","started_ns","finished_ns","role")}|{"response_json":canon(c["response"])})
   reads.append({"id":row["id"],"call_started_ns":row["call_started_ns"],"finished_ns":row["finished_ns"],"calls":calls})
assert len(endpoint)==1 and len(reads)==2400
reads.sort(key=lambda r:r["call_started_ns"])
def transport(prefix):
 requests={};responses={};times={};wire={};kinds=Counter();eof=exit0=False
 for line in z.open("p8-runtime/"+prefix+"/rpc.jsonl"):
  e=json.loads(line);kind=e.get("kind") if e.get("event")=="terminal" else e.get("event");kinds[kind]+=1
  if kind=="request":
   p=e["payload"];assert p["id"] not in requests;requests[p["id"]]=p;times.setdefault(p["id"],{})["request"]=e["time_ns"]
  if kind=="response":
   p=e["payload"];assert p["id"] not in responses;responses[p["id"]]=canon(p);times.setdefault(p["id"],{})["response"]=e["time_ns"]
  if kind=="stdout_wire":
   b=e["text"].encode();assert len(b)==e["wire_bytes"] and hashlib.sha256(b).hexdigest()==e["wire_sha256"]
   p=json.loads(b)
   if "id" in p:assert p["id"] not in wire;wire[p["id"]]=canon(p)
  if kind=="process_exit":exit0=e.get("exit_code",e.get("code"))==0
  if kind in ("stdout_eof","eof"):eof=True
 assert eof and exit0 and set(requests)==set(responses)==set(wire) and responses==wire
 assert all(t["request"]<=t["response"] for t in times.values())
 return requests,responses,times,dict(kinds)
requests,responses,times,kinds=transport("product")
search_ids=sorted([i for i,p in requests.items() if p.get("method")=="tools/call" and p.get("params",{}).get("name")=="search"],key=lambda i:times[i]["request"])
calls=[(row,c) for row in reads for c in row["calls"] if c["name"]=="search"]
full_req,full_res,full_times,full_kinds=transport("full-product")
full_search=[i for i,p in full_req.items() if p.get("method")=="tools/call" and p.get("params",{}).get("name")=="search"]
assert len(search_ids)==len(calls)+1==4801 and len(full_search)==1
last=search_ids[-1];right=full_search[0];args={"name":"search","arguments":{"query":"StableProbe","mode":"symbol","top_k":5}}
# Use the exact first raw symbol request as the registered endpoint query shape.
args={"name":"search","arguments":calls[0][1]["arguments"]}
assert requests[last]["params"]==args and full_req[right]["params"]==args
assert val(json.loads(responses[last]))==endpoint[0]["incremental"]
assert val(json.loads(full_res[right]))==endpoint[0]["full"]
lo=None;hi=None;mapping=[];used=set()
for (row,c),i in zip(calls,search_ids[:-1],strict=True):
 assert requests[i]["params"]=={"name":c["name"],"arguments":c["arguments"]}
 assert val(json.loads(responses[i]))==json.loads(c["response_json"])
 low=times[i]["response"]-c["finished_ns"];high=times[i]["request"]-c["started_ns"];assert low<=high
 lo=low if lo is None else max(lo,low);hi=high if hi is None else min(hi,high);assert lo<=hi
 used.add(i);mapping.append({"operation_id":row["id"],"role":c["role"],"rpc_id":i})
assert times[last]["request"]>max(times[i]["response"] for i in used)
status_ids=[i for i,p in requests.items() if p.get("method")=="tools/call" and p.get("params",{}).get("name")=="status"]
status_by_response={}
for i in status_ids:status_by_response.setdefault(canon(val(json.loads(responses[i]))),[]).append(i)
for row in reads:
 for c in row["calls"]:
  if c["name"]!="status":continue
  candidates=[]
  for i in status_by_response.get(c["response_json"],[]):
   if i in used or requests[i]["params"]!={"name":c["name"],"arguments":c["arguments"]}:continue
   low=times[i]["response"]-c["finished_ns"];high=times[i]["request"]-c["started_ns"]
   if low<=high and max(lo,low)<=min(hi,high):candidates.append((i,low,high))
  assert len(candidates)==1,(row["id"],c["role"],len(candidates))
  i,low,high=candidates[0];lo=max(lo,low);hi=min(hi,high);used.add(i);mapping.append({"operation_id":row["id"],"role":c["role"],"rpc_id":i})
assert len(used)==9600 and last not in used
assert times[last]["request"]>max(times[i]["response"] for i in used)
row={"status":"diagnostic_exact_endpoint_partition_and_9600_call_mapping","artifact_id":11590202308,"zip_bytes":SIZE,"zip_sha256":SHA,"members":len(inv),"expanded_bytes":sum(x["bytes"] for x in inv.values()),"all_members_CRC_SHA_verified":True,"source":"260f596582f2d82b8d7c707b61a6b8b6a43b069f","raw_operations":dict(operation_counts),"read_roles":dict(roles),"read_operations":len(reads),"raw_search_calls":len(calls),"wire_product_search_RPCs":len(search_ids),"extra_product_endpoint_search_RPC":{"id":last,"request":requests[last],"times":times[last],"response":val(json.loads(responses[last]))},"full_endpoint_search_RPC":{"id":right,"request":full_req[right],"times":full_times[right],"response":val(json.loads(full_res[right]))},"endpoint_public":endpoint[0],"search_and_status_bound_RPCs":len(used),"mapping":mapping,"common_monotonic_origin_interval_ns":[lo,hi],"unbound_status_RPCs":len(set(status_ids)-used),"transport":{"product":kinds,"full-product":full_kinds},"selected_input_hashes":{n:inv[n] for n in ("p8-runtime/raw.jsonl","p8-runtime/product/rpc.jsonl","p8-runtime/full-product/rpc.jsonl")},"scope":"Pure-RAM diagnostic of exact original raw with typed response equality and original clock constraints. This does not amend or rerun the failed receiver, and is not final acceptance of a new receiver."}
print(json.dumps(row))

