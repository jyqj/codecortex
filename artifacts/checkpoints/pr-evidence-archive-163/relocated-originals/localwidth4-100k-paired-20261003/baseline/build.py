#!/usr/bin/env python3
import hashlib,json,os,pathlib,subprocess,time,signal,shutil
ROOT=pathlib.Path('/workspace/localwidth4-baseline')
OUT=pathlib.Path(__file__).resolve().parent
SHA="513a98c9a94b15ec77153df41af26fa3c8c0b5e8"
assert subprocess.check_output(["git","--git-dir",str(pathlib.Path("/workspace/codecortex/.git")),"rev-parse",SHA],cwd=ROOT,text=True).strip()==SHA
assert all(hashlib.sha256((ROOT/p).read_bytes()).hexdigest()==h for p,h in json.loads((OUT/"source-files.json").read_text()).items())
runtime=OUT.parent/"runtime/baseline/build"
(runtime/"tmp").mkdir(parents=True,exist_ok=True)
env=dict(os.environ,CARGO_HOME=str(runtime/"cargo"),RUSTC="/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/rustc",TMPDIR=str(runtime/"tmp"),CARGO_TARGET_DIR=str(runtime/"target"),CARGO_BUILD_JOBS="4")
env["PATH"]="/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin"+":"+env["PATH"]
command=["/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin/cargo","build","--release","-p","cc-server","--bin","codecortex","--no-default-features","--features","semantic-http","--locked","--message-format=json-render-diagnostics"]
receipt={"production_sha":"513a98c9a94b15ec77153df41af26fa3c8c0b5e8","source_sha":SHA,"source_tree":subprocess.check_output(["git","--git-dir",str(pathlib.Path("/workspace/codecortex/.git")),"rev-parse",SHA+"^{tree}"],cwd=ROOT,text=True).strip(),"command":command,"profile":"release","target_dir":str(runtime/"target"),"environment":{k:env[k] for k in ["CARGO_HOME","TMPDIR","CARGO_TARGET_DIR","CARGO_BUILD_JOBS"]},"timeout_seconds":900,"guard_memory_bytes":12*1024**3,"guard_disk_reserve_bytes":4*1024**3,"sample_interval_seconds":0.5,"guard_stop":None}
for tool in ["rustc","cargo"]:receipt[tool]=subprocess.check_output([tool,"-Vv"],cwd=ROOT,env=env,text=True)
start=time.monotonic()
with (OUT/"cargo-build.jsonl").open("w") as stdout,(OUT/"build-stderr.log").open("w") as stderr,(OUT/"build-resources.jsonl").open("w") as samples:
    proc=subprocess.Popen(command,cwd=ROOT,env=env,stdout=stdout,stderr=stderr,start_new_session=True)
    while proc.poll() is None:
        current=int(pathlib.Path("/sys/fs/cgroup/memory.current").read_text());free=shutil.disk_usage(OUT).free
        samples.write(json.dumps({"wall_seconds":time.monotonic()-start,"pid":proc.pid,"cgroup_memory_current":current,"disk_free_bytes":free})+"\n");samples.flush()
        reason="timeout_900s" if time.monotonic()-start>900 else "memory_12GiB" if current>12*1024**3 else "disk_below_4GiB" if free<4*1024**3 else None
        if reason:
            receipt["guard_stop"]=reason;os.killpg(proc.pid,signal.SIGTERM)
            try:proc.wait(timeout=15)
            except subprocess.TimeoutExpired:raise RuntimeError("controlled build termination incomplete; no kill")
            break
        time.sleep(.5)
    receipt["build_exit_code"]=proc.wait()
receipt["wall_seconds"]=time.monotonic()-start
if receipt["build_exit_code"]==0:
    artifacts=[m for line in (OUT/"cargo-build.jsonl").read_text().splitlines() if (m:=json.loads(line)).get("reason")=="compiler-artifact" and m.get("target",{}).get("name")=="codecortex" and m.get("executable")]
    assert len(artifacts)==1
    artifact=artifacts[0];assert sorted(artifact["features"])==["semantic","semantic-http"]
    assert artifact["profile"]["opt_level"]=="3" and not artifact["profile"]["debug_assertions"]
    receipt["compiler_artifact"]=artifact
    receipt["binary_sha256"]=hashlib.sha256(pathlib.Path(artifact["executable"]).read_bytes()).hexdigest()
(OUT/"build-receipt.json").write_text(json.dumps(receipt,indent=2)+"\n")
print(json.dumps(receipt))
raise SystemExit(receipt["build_exit_code"])
