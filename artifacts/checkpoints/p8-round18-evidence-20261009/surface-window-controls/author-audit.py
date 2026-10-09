import ast,collections,datetime,hashlib,json,pathlib,re,sys,textwrap,zipfile,zlib
P=pathlib.Path("artifacts/checkpoints/p8-round17-evidence-intake-20261009/surface-window-controls-author")
OUT=P/"review"
OUT.mkdir(exist_ok=False)
(OUT/"verify_reception.py").write_bytes(pathlib.Path(__file__).read_bytes())
def sha(b): return hashlib.sha256(b).hexdigest()
def canonical(v): return (json.dumps(v,ensure_ascii=False,sort_keys=True,indent=2)+"\n").encode()
def nodup(pairs):
 d={}
 for k,v in pairs:
  if k in d: raise ValueError("duplicate JSON key")
  d[k]=v
 return d
def parse(b): return json.loads(b,object_pairs_hook=nodup)
checks=[]
def check(ok,label):
 if not ok: raise ValueError(label)
 checks.append(label)
report={"schema":"p8-surface-window-controls-author-reception-v1","role":"author evidence reception; independent approval separate","status":"incomplete_not_certified","new_product_or_Cargo_or_statistics_execution":False,"TODO_closed":0,"TODO_remaining":29,"started_at":datetime.datetime.now(datetime.timezone.utc).isoformat()}
try:
 wf=(P/"fixed-workflow.yml").read_bytes();check(sha(wf)=="f27250c1c66238aa2b60b7ff53c013611198e8653227851c76311f2efa56ed0a","exact published workflow")
 py=wf.decode().split("python3 - <<'PY'\n",1)[1].split("\n          PY",1)[0]
 tree=ast.parse(textwrap.dedent(py))
 constants={}
 for node in tree.body:
  if isinstance(node,ast.Assign) and len(node.targets)==1 and isinstance(node.targets[0],ast.Name) and node.targets[0].id in {"commands","expected_tests","expected_observers","expected_product_manifest"}:
   constants[node.targets[0].id]=ast.literal_eval(node.value)
 func=next(n for n in tree.body if isinstance(n,ast.FunctionDef) and n.name=="validate_test_stdout")
 scope={"re":re}
 exec(compile(ast.Module(body=[func],type_ignores=[]),"<original frozen workflow validator>","exec"),scope)
 commands=constants["commands"];expected=constants["expected_tests"]
 check(len(commands)==6 and sorted(expected)==[3,4,5] and [len(expected[i]) for i in (3,4,5)]==[6,3,1] and len(set(sum(expected.values(),[])))==10,"exact registered six commands/ten methods")
 run=parse((P/"surface_window_final_run.json").read_bytes());jobs=parse((P/"surface_window_final_jobs.json").read_bytes());arts=parse((P/"surface_window_final_artifacts.json").read_bytes())
 head="275e8799d4947d297329073eaa3ca675d3fd0777"
 check(run["id"]==37890734575 and run["head_sha"]==head and run["run_attempt"]==1 and run["event"]=="push" and run["status"]=="completed" and run["conclusion"]=="success","exact first push run")
 check(jobs["total_count"]==len(jobs["jobs"])==1 and jobs["jobs"][0]["id"]==113690845958 and jobs["jobs"][0]["conclusion"]=="success","complete single job")
 check(arts["total_count"]==len(arts["artifacts"])==1,"complete unique original artifact page")
 artifact=arts["artifacts"][0]
 check(artifact["id"]==11597533828 and artifact["size_in_bytes"]==114785 and artifact["digest"]=="sha256:ca3a5283df03b7c4e307b0a33588b505a903eabb13e8e5a70fe2c34e7d39aab4" and artifact["expired"] is False,"original API artifact identity")
 archive=(P/"original-11597533828.zip").read_bytes()
 check(len(archive)==114785 and sha(archive)==artifact["digest"].split(":")[1],"complete original ZIP actual SHA")
 with zipfile.ZipFile(P/"original-11597533828.zip") as z:
  infos=z.infolist();check(len(infos)==len({i.filename for i in infos})==21,"complete 21-member ZIP denominator")
  raw={i.filename:z.read(i) for i in infos}
  inventory={i.filename:{"bytes":len(raw[i.filename]),"sha256":sha(raw[i.filename]),"crc32":"%08x"%(zlib.crc32(raw[i.filename])&0xffffffff)} for i in infos}
  for i in infos: check(len(raw[i.filename])==i.file_size and inventory[i.filename]["crc32"]=="%08x"%i.CRC,"actual complete member CRC/size "+i.filename)
  receipt=parse(raw["receipt.json"]);plan=parse(raw["plan.json"]);progress=parse(raw["progress.json"])
  check(set(receipt["files"])==set(raw)-{"receipt.json"},"receipt exact two-way complete member inventory")
  for n,row in receipt["files"].items(): check(row=={k:inventory[n][k] for k in ("bytes","sha256")},"receipt complete file bytes/SHA "+n)
  check(receipt["expected_source"]==head and receipt["status"]=="controls_completed" and receipt["exit_code"]==0 and receipt["source_unchanged"] is True and receipt["observers_unchanged"] is True and receipt["not_run_command_indices"]==[],"complete actual control receipt")
  check(receipt["commands"]==commands and receipt["expected_tests"]=={str(k):v for k,v in expected.items()} and receipt["expected_test_count"]==10,"receipt unchanged declared population")
  check(plan["commands"]==commands and plan["expected_tests"]==receipt["expected_tests"] and plan["expected_source"]==head and plan["results"]==[] and plan["status"]=="running" and plan["cwd"]==receipt["cwd"],"original pre-execution plan identity")
  check(progress["results"]==receipt["results"] and progress["commands"]==commands,"preserved actual sequential progress")
  before=parse(raw["source-before.json"]);after=parse(raw["source-after.json"])
  check(raw["source-before.json"]==raw["source-after.json"] and before==after and before["source_commit"]==head and before["input_count"]==len(before["inputs"])==1089 and sha(canonical(before["inputs"]))==before["manifest_sha256"]==constants["expected_product_manifest"]=="593cf443fc914998dd2cb5a30c3caf180eb8d0d7a398a86e4624e3df2ca65e83","all 1089 inputs canonical manifest before/after")
  ob=parse(raw["observer-before.json"]);check(raw["observer-before.json"]==raw["observer-after.json"] and set(ob)==set(constants["expected_observers"]) and all(ob[n]["sha256"]==h for n,h in constants["expected_observers"].items()),"all three fixed source-bound observers before/after")
  check(receipt["cwd"]=="/home/runner/work/codecortex/codecortex","actual runner checkout cwd")
  env=receipt["environment"]
  check(env=={"CARGO_BUILD_JOBS":"2","CARGO_INCREMENTAL":"0","CARGO_PROFILE_DEV_DEBUG":"0","CARGO_PROFILE_TEST_DEBUG":"0","CARGO_TARGET_DIR":"/home/runner/work/_temp/surface-window-controls-cargo","CODECORTEX_BENCH_OBSERVATIONS":"/home/runner/work/_temp/surface-window-controls/observations","RUSTC_WORKSPACE_WRAPPER":"","RUSTC_WRAPPER":"","RUSTFLAGS":""},"exact private target and registered control environment")
  check(len(receipt["results"])==6,"all original command results present")
  actual_methods=[];command_results=[]
  for index,result in enumerate(receipt["results"]):
   check(result["argv"]==commands[index] and result["stdout"]=="%02d.stdout"%index and result["stderr"]=="%02d.stderr"%index and result["exit_code"]==0 and result["accepted"] is True and type(result["elapsed_ns"]) is int and result["elapsed_ns"]>=0,"original command argv/stdout/stderr/exit "+str(index))
   population=None
   if index in expected:
    population=scope["validate_test_stdout"](raw[result["stdout"]].decode(),expected[index])
    check(population==result["test_population"],"actual original strict libtest validator "+str(index))
    actual_methods+=population["actual_names"]
   command_results.append(dict(index=index,argv=commands[index],cwd=receipt["cwd"],exit_code=result["exit_code"],elapsed_ns=result["elapsed_ns"],stdout=inventory[result["stdout"]],stderr=inventory[result["stderr"]],population=population))
  check(len(actual_methods)==len(set(actual_methods))==10,"ten unique actual methods no ignored/measured/empty selector")
  gate=parse(raw["00.stdout"])
  check(gate["status"]=="passed" and gate["source_version"]=="p8-completion-source-20261009-v15" and gate["previous_source_version"]==gate["executed_previous_proof"]=="p8-oracle-compat-source-20261008-v14" and gate["base_source"]=="7354db236c9d9850a75f31672697ae9eab44565e" and gate["product_source"]=="a402e88d460179afb12e3e2dc066d62cf168c690" and gate["review_source"]=="f66daf6088f6f6a8d990d736ee0c1213032ca175" and gate["complete_inputs"]==1089,"actual original v15 and frozen previous proof")
  observations={n:parse(b) for n,b in raw.items() if n.startswith("observations/")}
  check(set(observations)=={"observations/p8-prepare-dependency-window-work.json","observations/p8-prepare-surface-window-work.json"},"complete original SQL observations retained")
  observation_summary={}
  for n,rows in observations.items():
   check(isinstance(rows,list) and bool(rows),"nonempty original observation population "+n)
   groups=collections.Counter();totals={"original_two_lookup_work":collections.Counter(),"retained_single_lookup_work":collections.Counter()}
   for row in rows:
    check(set(row["original_two_lookup_work"])==set(row["retained_single_lookup_work"])=={"statements","rows","sorts","fullscan_steps","vm_steps"} and all(type(v) is int and v>=0 for group in totals for v in row[group].values()),"actual SQL counter domains "+n)
    for group in totals:totals[group].update(row[group])
    groups[(row["original_two_lookup_work"]["statements"],row["retained_single_lookup_work"]["statements"])]+=1
   observation_summary[n]={"original_bytes":inventory[n],"rows":len(rows),"statement_pair_populations":[{"original":a,"retained":b,"cases":c} for (a,b),c in sorted(groups.items())],"total_work":{k:dict(v) for k,v in totals.items()},"scope":"Actual SQL counters of these bounded correctness fixtures only. No large-scale or wall-time speedup conclusion."}
  check((P/"original-11597533828.zip").read_bytes()==archive and (P/"fixed-workflow.yml").read_bytes()==wf,"preserved input original bytes unchanged")
  report.update(status="accepted_scoped_original_controls_delivery",source=head,source_tree=before["source_tree"],manifest_sha256=before["manifest_sha256"],input_count=1089,run_id=37890734575,job_id=113690845958,attempt=1,artifact=artifact,archive_members=inventory,member_count=21,total_member_bytes=sum(v["bytes"] for v in inventory.values()),commands=command_results,actual_methods=actual_methods,passed=10,failed=0,ignored=0,measured=0,source_unchanged=True,observers_unchanged=True,v15_original_result=gate,original_SQL_observations=observation_summary,scope="Exact first controls execution and complete original artifact received. Original v15/fmt/Clippy plus six surface, three dependency and one stale-commit methods passed. This intake executes only Python evidence inspection. No 1k/10k/100k study, N150 completion or TODO closure is claimed.")
except Exception as error:
 report.update(error_type=type(error).__name__,safe_error=str(error) if isinstance(error,ValueError) else "typed author intake error")
 raise
finally:
 report["completed_checks"]=len(checks);report["finished_at"]=datetime.datetime.now(datetime.timezone.utc).isoformat()
 (OUT/"review.json").write_bytes(canonical(report));(OUT/"check-labels.json").write_bytes(canonical(checks))
 print(json.dumps({"status":report["status"],"bytes":len(canonical(report)),"sha256":sha(canonical(report)),"script_sha256":sha((OUT/"verify_reception.py").read_bytes()),"completed_checks":len(checks),"report_path":str(OUT/"review.json")},sort_keys=True))
