#!/usr/bin/env python3
"""Build only; never run a benchmark. Receipt matches p7_release_resource_preparation.py."""
import os,pathlib,subprocess,json,hashlib,shutil
root=pathlib.Path(__file__).resolve().parents[3]
out=pathlib.Path(__file__).resolve().parent
expected='11f5b76273a16b520e6b6e01ad32a0a6743173fb'
assert subprocess.check_output(['git','diff',expected,'--','crates','Cargo.lock','Cargo.toml'],cwd=root)==b''
tc='/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin'
env=os.environ.copy();env.update(PATH=tc+':'+env['PATH'],CARGO_HOME='/workspace/.cargo',CARGO_TARGET_DIR='/workspace/codecortex/target',RUSTC=tc+'/rustc',RUSTDOC=tc+'/rustdoc')
command=[tc+'/cargo','build','-p','cc-server','--bin','codecortex','--no-default-features','--features','semantic-http','--release','--locked','--offline','--message-format=json']
with (out/'release-build.log').open('w') as f: status=subprocess.run(command,cwd=root,env=env,stdout=f,stderr=subprocess.STDOUT).returncode
artifacts=[]
for line in (out/'release-build.log').read_text().splitlines():
 try:a=json.loads(line)
 except ValueError:continue
 if a.get('reason')=='compiler-artifact' and a.get('target',{}).get('name')=='codecortex' and a.get('executable'):artifacts.append(a)
receipt={'build_exit_code':status,'guard_stop':None,'profile':'release','command':command,'source_sha':expected,'head_at_build':subprocess.check_output(['git','rev-parse','HEAD'],cwd=root,text=True).strip(),'benchmark_status':'not_run; awaits parent acceptance','protocol_script_sha256':hashlib.sha256((root/'scripts/p7_release_resource_preparation.py').read_bytes()).hexdigest(),'environment':{k:env[k] for k in ['CARGO_HOME','CARGO_TARGET_DIR','RUSTC','RUSTDOC']}}
if status==0:
 assert len(artifacts)==1
 a=artifacts[0];assert sorted(a['features'])==['semantic','semantic-http'];assert a['profile']['opt_level']=='3' and not a['profile']['debug_assertions']
 snapshot=pathlib.Path('/workspace/startup-candidate-binaries/release/codecortex');snapshot.parent.mkdir(parents=True,exist_ok=True);shutil.copy2(a['executable'],snapshot)
 receipt.update(compiler_artifact=a,binary=str(snapshot),binary_sha256=hashlib.sha256(snapshot.read_bytes()).hexdigest())
(out/'release-build-receipt.json').write_text(json.dumps(receipt,indent=2)+'\n');print(json.dumps(receipt),flush=True)
raise SystemExit(status)
