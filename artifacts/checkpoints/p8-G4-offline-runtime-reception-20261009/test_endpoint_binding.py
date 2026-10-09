#!/usr/bin/env python3
import ast,copy,difflib,hashlib,json,sys
from pathlib import Path
h=Path(__file__).resolve().parent
old_inspect=(h/"originals/soak/inspect.py").read_text()
new_inspect=(h/"derived/soak/inspect.py").read_text()
def block(s):
 start=s.index("        transports={};cache_wire=None")
 end=s.index("        endpoint=next(",start)
 return s[start:end]
p={"old_wire":(h/"originals/soak/cache_wire.py").read_text(),"wire":(h/"derived/soak/cache_wire.py").read_text(),"old_inspect":old_inspect,"inspect":new_inspect,"old_block":block(old_inspect),"new_block":block(new_inspect)}

old=p["old_wire"];wire=p["wire"];inspect=p["inspect"]
assert wire.startswith(old);ast.parse(wire);ast.parse(inspect)
ns={};exec(compile(wire,"<derived cache_wire.py>","exec"),ns)
def fixture():
 q="p8_runtime_stable_signal";spec=[("before_status","status",{"aspect":"index"},{"v":"before"},10,20),("symbol","search",{"query":q,"mode":"symbol","top_k":5},{"v":"symbol"},21,30),("hybrid","search",{"query":q,"mode":"hybrid","top_k":1,"retrieval_strategy":"local"},{"v":"hybrid"},31,40),("after_status","status",{"aspect":"index"},{"v":"after"},41,50)]
 req={};res={};times={};calls=[]
 def response(i,value):return {"id":i,"result":{"structuredContent":{"result":copy.deepcopy(value)}}}
 for i,(role,name,args,value,start,finish) in enumerate(spec,1):
  req[i]={"id":i,"method":"tools/call","params":{"name":name,"arguments":copy.deepcopy(args)}}
  res[i]=response(i,value);times[i]={"request":1000+start+1,"response":1000+finish-1}
  calls.append({"role":role,"name":name,"arguments":copy.deepcopy(args),"response":copy.deepcopy(value),"started_ns":start,"finished_ns":finish})
 row={"kind":"operation","operation":"read","id":0,"call_started_ns":10,"finished_ns":60,"cache_probe":{"requests":calls}}
 build={"kind":"operation","operation":"build","id":1,"finished_ns":70}
 endpoint={"kind":"endpoint_public","incremental":{"v":"end-left"},"full":{"v":"end-right"}}
 args={"name":"search","arguments":{"query":q,"mode":"symbol","top_k":5}}
 req[9]={"id":9,"method":"tools/call","params":args};res[9]=response(9,endpoint["incremental"]);times[9]={"request":1200,"response":1202}
 fr={4:{"id":4,"method":"tools/call","params":copy.deepcopy(args)}};fs={4:response(4,endpoint["full"])};ft={4:{"request":1203,"response":1205}}
 return [[row,build],req,res,times,[row,build,endpoint],fr,fs,ft]
fn=ns["bind_compound_reads_with_endpoint"];x=fixture();o=fn(*x)
assert o["compound_reads"]==1 and o["bound_RPCs"]==4 and len(o["mapping"])==4 and o["endpoint_binding"]["incremental_rpc_id"]==9 and o["endpoint_binding"]["full_rpc_id"]==4
checks=["positive: all original four read RPCs and separate exact two endpoint RPCs bound"]
def negative(name,mutate):
 x=fixture();mutate(x)
 try:fn(*x)
 except ValueError as e:checks.append(name+": "+str(e))
 else:raise AssertionError("accepted "+name)
def extra(x):
 x[1][10]=copy.deepcopy(x[1][9]);x[1][10]["id"]=10;x[2][10]=copy.deepcopy(x[2][9]);x[3][10]={"request":1300,"response":1302}
negative("unexplained extra product search",extra)
negative("missing product endpoint",lambda x:[m.pop(9) for m in x[1:4]])
negative("wrong product endpoint arguments",lambda x:x[1][9]["params"]["arguments"].update(top_k=4))
negative("wrong product endpoint full response",lambda x:x[2][9]["result"]["structuredContent"]["result"].update(v="changed"))
negative("wrong full endpoint arguments",lambda x:x[5][4]["params"]["arguments"].update(top_k=4))
negative("wrong full endpoint full response",lambda x:x[6][4]["result"]["structuredContent"]["result"].update(v="changed"))
negative("missing full endpoint",lambda x:x[5].clear())
def extra_full(x):x[5][5]=copy.deepcopy(x[5][4])
negative("extra full search",extra_full)
negative("duplicate endpoint record",lambda x:x[4].append(copy.deepcopy(x[4][-1])))
negative("endpoint record before final operation",lambda x:x[4].insert(1,x[4].pop()))
negative("endpoint RPC overlaps bound reads",lambda x:x[3][9].update(request=1040,response=1042))
negative("endpoint after reads but before last build",lambda x:x[3][9].update(request=1060,response=1062))
negative("full endpoint precedes incremental response",lambda x:x[7][4].update(request=1201))
negative("original work search payload mismatch",lambda x:x[1][2]["params"]["arguments"].update(top_k=4))
negative("missing original status call retains four-RPC denominator",lambda x:x[0][0]["cache_probe"]["requests"].pop())
oldtree=ast.parse(old);newtree=ast.parse(wire)
for n in oldtree.body:
 if isinstance(n,ast.FunctionDef):
  other=next(q for q in newtree.body if isinstance(q,ast.FunctionDef) and q.name==n.name)
  assert ast.dump(n,include_attributes=False)==ast.dump(other,include_attributes=False)
assert "len(search_ids)==len(calls)==2*len(reads)" in wire and "len(used)==4*len(reads)" in wire
checks.append("all original cache_wire functions byte-prefix and AST unchanged, including 2x/4x gates")
oldinspect=p["old_inspect"]
inverse=inspect.replace("from cache_wire import transport, bind_compound_reads, bind_compound_reads_with_endpoint","from cache_wire import transport, bind_compound_reads").replace(p["new_block"],p["old_block"])
assert inverse==oldinspect
checks.append("inspect inverse replacement exactly restores every original byte")
diff="".join(difflib.unified_diff(old.splitlines(True),wire.splitlines(True),fromfile="a/originals/soak/cache_wire.py",tofile="b/derived/soak/cache_wire.py"))+"".join(difflib.unified_diff(oldinspect.splitlines(True),inspect.splitlines(True),fromfile="a/originals/soak/inspect.py",tofile="b/derived/soak/inspect.py"))
meta={k:{"bytes":len(p[k].encode()),"sha256":hashlib.sha256(p[k].encode()).hexdigest(),"git_blob":hashlib.sha1(("blob "+str(len(p[k].encode()))+"\0").encode()+p[k].encode()).hexdigest()} for k in ("wire","inspect")}
print(json.dumps({"status":"passed_scoped_18_controls","checks":checks,"files":meta,"patch":diff,"scope":"Synthetic pure-Python endpoint binding controls and inverse byte proof; no product/Cargo/statistics/receiver execution. Original failed receiver and all old originals retained.","author_control_failure_preserved":"First synthetic wrong-response test shared the same Python dict between wire and expected endpoint, so mutation changed both and was incorrectly accepted; fixed fixture isolation with deepcopy before this rerun. Production binder was not changed."}))

