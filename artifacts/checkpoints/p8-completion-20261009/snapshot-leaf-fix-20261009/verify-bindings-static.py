import sys,json,hashlib,ast,difflib,termios,yaml
t=termios.tcgetattr(0);t[3]&=~termios.ECHO;termios.tcsetattr(0,termios.TCSANOW,t);print("READY",flush=True)
chunks=[]
for line in sys.stdin:
 x=json.loads(line)
 if x["op"]=="data":chunks.append(x["data"])
 elif x["op"]=="end":break
p=json.loads("".join(chunks));f=p["files"];pre=p["prefix"];old=p["oldPy"]
old_branch="task/p8-snapshot-leaf-engineering-20261009";new_branch=p["branch"]
old_manifest="d006974afc0fa4752f3ebcc5ac441c651bf18870ca6af8ae1217e135bed2730d"
old_path="artifacts/checkpoints/p8-completion-20261009/integrated-runtime-controls/test-expectations.json"
new_path=pre+"/python-controls/test-expectations.json";old_hash="11300b2e9dc47d5a8f1fa39e9a5da0d5b27d90cd34a31108f4340b084a96afad"
def reverse_py(s):return s.replace(new_branch,old_branch).replace(new_path,old_path).replace(p["manifest_hash"],old_hash)
pc=f[pre+"/python-controls/controller.py"];pw=f[".github/workflows/p8-integrated-runtime-controls.yml"];rw=f[".github/workflows/p8-snapshot-leaf-engineering.yml"]
assert reverse_py(pc)==old["controller.py"]
assert reverse_py(pw)==old["p8-integrated-runtime-controls.yml"]
assert rw.replace(new_branch,old_branch).replace(p["manifest"],old_manifest)==p["oldRust"]
ast.parse(pc)
ys=yaml.load(pw,Loader=yaml.BaseLoader);yr=yaml.load(rw,Loader=yaml.BaseLoader)
for y in (ys,yr):
 assert y["on"]=={"push":{"branches":[new_branch]}}
 assert y["permissions"]=={"contents":"read"}
pyjob=ys["jobs"]["python_controls"]
assert pyjob["timeout-minutes"]=="20" and pyjob["runs-on"]=="ubuntu-24.04" and pyjob["if"]=="github.run_attempt == 1"
run=next(s["run"] for s in pyjob["steps"] if "P8_CONTROLLER" in s.get("run",""))
source=run.split("\n",1)[1].rsplit("\nP8_CONTROLLER",1)[0];tree=ast.parse(source)
embedded=[ast.literal_eval(n.value) for n in tree.body if isinstance(n,ast.Assign) and any(isinstance(t,ast.Name) and t.id=="payload" for t in n.targets)][0]
assert embedded==pc
u=pyjob["steps"][-1];assert u["if"]=="always()" and u["with"]["include-hidden-files"]=="true"
oldy=yaml.load(old["p8-integrated-runtime-controls.yml"],Loader=yaml.BaseLoader)
assert u["with"]["path"]==oldy["jobs"]["python_controls"]["steps"][-1]["with"]["path"]
manifest=json.loads(f[new_path]);oldm=json.loads(old["test-expectations.json"])
changed=sorted(k for k in manifest if manifest[k]!=oldm[k]);assert changed==["branch","source_manifest_sha256"]
assert manifest["expected_methods"]==50 and manifest["expected_explicit_subtests"]==24 and len(manifest["fixed_observer_files"])==9
assert hashlib.sha256(f[new_path].encode()).hexdigest()==p["manifest_hash"]
inline=[]
for job in yr["jobs"].values():
 for step in job.get("steps",[]):
  run=step.get("run","")
  lines=run.splitlines()
  for i,line in enumerate(lines):
   if "<<'PY'" in line:
    end=lines.index("PY",i+1);body="\n".join(lines[i+1:end])+"\n";ast.parse(body);inline.append(body)
control=next(b for b in inline if "expected_tests =" in b)
vals={}
for n in ast.parse(control).body:
 if isinstance(n,ast.Assign) and len(n.targets)==1 and isinstance(n.targets[0],ast.Name) and n.targets[0].id in ("commands","expected_tests","expected_product_manifest"):
  vals[n.targets[0].id]=ast.literal_eval(n.value)
assert vals["commands"]==p["oldPlan"]["commands"]
assert {str(k):v for k,v in vals["expected_tests"].items()}==p["oldPlan"]["expected_tests"]
assert sum(map(len,vals["expected_tests"].values()))==53 and len(vals["commands"])==18
assert vals["expected_product_manifest"]==p["manifest"]
before_lines=p["oldTest"].splitlines(True); before_lines[148]=before_lines[148].replace("rusqlite::Result<_>","rusqlite::Result<Vec<String>>"); assert p["newTest"]=="".join(before_lines)
lines=[(i+1,a,b) for i,(a,b) in enumerate(zip(p["oldTest"].splitlines(),p["newTest"].splitlines())) if a!=b];assert len(lines)==1 and lines[0][0]==149
plan=json.loads(f[pre+"/rust-controls/engineering-plan.json"])
same_fields=["commands","expected_tests","expected_unique_test_count","command_count","canonical_stream_budget_bytes","capacity_profile","control_build_timeout_minutes","diagnostic_order","diagnostic_repetition","diagnostic_scales","dirty_budget","fanout_dirty_budget","fanout_resume_iterations","job_timeout_minutes","native_deadline_ms","observer_sha256","oracle_scratch_budget_bytes","raw_budget_bytes","repetitions","resume_iterations","seed","shard_count","wrapper_minutes","test_population_admission"]
assert all(plan[k]==p["oldPlan"][k] for k in same_fields)
hashes={k:{"bytes":len(v.encode()),"sha256":hashlib.sha256(v.encode()).hexdigest(),"git_blob":hashlib.sha1(b"blob "+str(len(v.encode())).encode()+b"\0"+v.encode()).hexdigest()} for k,v in f.items()}
diffs={
 "python-controller": "".join(difflib.unified_diff(old["controller.py"].splitlines(True),pc.splitlines(True),fromfile="G1/controller.py",tofile="fix/controller.py")),
 "python-workflow":"".join(difflib.unified_diff(old["p8-integrated-runtime-controls.yml"].splitlines(True),pw.splitlines(True),fromfile="G1/python.yml",tofile="fix/python.yml")),
 "rust-workflow":"".join(difflib.unified_diff(p["oldRust"].splitlines(True),rw.splitlines(True),fromfile="G1/rust.yml",tofile="fix/rust.yml")),
}
print(json.dumps({"status":"static_checks_passed","tests_executed":False,"Cargo_executed":False,"source_input_count":1089,"historical_delta":41,"manifest":p["manifest"],"python_manifest_changed_keys":changed,"python_controller_exact_inverse":True,"python_workflow_exact_inverse":True,"python_embedded_controller_equal":True,"upload_two_paths_and_hidden_true_unchanged":True,"rust_workflow_exact_inverse":True,"rust_inline_python_blocks_parsed":len(inline),"unchanged_Rust_commands":18,"unchanged_Rust_tests":53,"unchanged_Python_methods":50,"unchanged_Python_subtests":24,"unchanged_Python_observers":9,"source_delta":lines,"plan_unchanged_fields":same_fields,"file_hashes":hashes,"diffs":diffs},ensure_ascii=False),flush=True)

