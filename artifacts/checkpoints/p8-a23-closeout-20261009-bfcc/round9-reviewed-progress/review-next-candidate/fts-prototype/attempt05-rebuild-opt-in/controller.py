import pathlib,subprocess,json,hashlib,datetime,os,sys
base=pathlib.Path.cwd();src=base/"candidate-fts";out=base/"review-next-candidate/fts-prototype/attempt05-rebuild-opt-in";out.mkdir(parents=True,exist_ok=False);target=base/"fts-prototype-target"
paths=["crates/cc-db/src/index_db_snapshot_insert.rs","crates/cc-db/src/index_db_write_batch.rs","crates/cc-db/src/snapshot_write_txn.rs","crates/cc-db/src/snapshot_fts_tests.rs","crates/cc-index/src/indexer_phases/snapshot.rs"]
def inv():return [{"path":p,"bytes":(src/p).stat().st_size,"sha256":hashlib.sha256((src/p).read_bytes()).hexdigest()} for p in paths]
env=os.environ.copy();env.update({"SDKROOT":"/Library/Developer/CommandLineTools/SDKs/MacOSX15.4.sdk","CARGO_TARGET_DIR":str(target),"CARGO_BUILD_JOBS":"2","PATH":"/Users/jin/.cargo/bin:"+env.get("PATH","")})
commands=[
("rustfmt",["/Users/jin/.cargo/bin/rustup","run","1.95.0","rustfmt","--edition","2021"]+paths),
("public-api-regression",["/Users/jin/.cargo/bin/cargo","+1.95.0","test","--offline","--locked","-p","cc-db","--lib","index_db_snapshot_insert::snapshot_fts_tests::public_entry_preserves_partial_state_when_caller_commits_after_error","--","--exact","--test-threads=1","--nocapture"]),
("cc-db-lib",["/Users/jin/.cargo/bin/cargo","+1.95.0","test","--offline","--locked","-p","cc-db","--lib","--","--test-threads=1","--nocapture","--skip","index_db_snapshot_insert::snapshot_fts_tests::small_write_stage_ab_ba_diagnostic_keeps_complete_outputs"]),
("snapshot-leaf-batching",["/Users/jin/.cargo/bin/cargo","+1.95.0","test","--offline","--locked","-p","cc-index","--test","snapshot_leaf_batching","--","--test-threads=1","--nocapture"]),
("cc-db-clippy",["/Users/jin/.cargo/bin/cargo","+1.95.0","clippy","--offline","--locked","-p","cc-db","--lib","--tests","--","-D","warnings"]),
("cc-index-clippy",["/Users/jin/.cargo/bin/cargo","+1.95.0","clippy","--offline","--locked","-p","cc-index","--lib","--test","snapshot_leaf_batching","--","-D","warnings"])
]

results=[]
for name,cmd in commands:
 before=inv();r={"name":name,"command":cmd,"cwd":str(src),"started_at":datetime.datetime.now(datetime.timezone.utc).isoformat(),"source_before":before,"environment":{"SDKROOT":env["SDKROOT"],"CARGO_TARGET_DIR":str(target),"CARGO_BUILD_JOBS":"2"},"status":"running"}
 (out/(name+".json")).write_text(json.dumps(r,indent=2)+"\n")
 with (out/(name+".stdout")).open("wb") as so,(out/(name+".stderr")).open("wb") as se:
  p=subprocess.Popen(cmd,cwd=src,env=env,stdout=so,stderr=se);r["pid"]=p.pid;(out/(name+".json")).write_text(json.dumps(r,indent=2)+"\n");code=p.wait()
 r.update({"status":"completed","exit_code":code,"finished_at":datetime.datetime.now(datetime.timezone.utc).isoformat(),"source_after":inv()})
 for ext in ["stdout","stderr"]:
  b=(out/(name+"."+ext)).read_bytes();r[ext]={"bytes":len(b),"sha256":hashlib.sha256(b).hexdigest()}
 (out/(name+".json")).write_text(json.dumps(r,indent=2)+"\n");results.append(r)
 if name=="rustfmt" and code==0:
  for rel in paths:
   dest=out/"source"/rel;dest.parent.mkdir(parents=True,exist_ok=True);dest.write_bytes((src/rel).read_bytes())
  diff=subprocess.check_output(["git","diff","--full-index","--"],cwd=src)
  extra=subprocess.run(["git","diff","--no-index","--full-index","--","/dev/null","crates/cc-db/src/snapshot_fts_tests.rs"],cwd=src,stdout=subprocess.PIPE)
  assert extra.returncode==1
  (out/"source.diff").write_bytes(diff+extra.stdout)
  (out/"frozen-source.json").write_text(json.dumps({"head":subprocess.check_output(["git","rev-parse","HEAD"],cwd=src,text=True).strip(),"paths":inv(),"diff":{"path":"source.diff","bytes":len(diff+extra.stdout),"sha256":hashlib.sha256(diff+extra.stdout).hexdigest()},"skip_reason":"Only our optional 30-pair timing diagnostic is explicitly skipped: root prohibits repeating it. All pre-existing cc-db library controls run unchanged."},indent=2)+"\n")
 print(json.dumps({"name":name,"exit_code":code,"source_unchanged":r["source_before"]==r["source_after"]}),flush=True)
 if code!=0 or (name!="rustfmt" and r["source_before"]!=r["source_after"]):break
(out/"final.json").write_text(json.dumps({"results":results,"wrapper_scope":"Records real child status; wrapper0 never means all validation passed."},indent=2)+"\n")
