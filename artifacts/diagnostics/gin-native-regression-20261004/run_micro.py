"""Isolated official locked lib builds; link own driver without product changes."""
import json,os,subprocess,sys,hashlib
from pathlib import Path
arm=sys.argv[1]; variant=sys.argv[2] if len(sys.argv)>2 else "base"; suffix=arm if variant=="base" else arm+"-"+variant
base=Path('/workspace/scratch/gin-native-20261004')
repo=base/('base-src' if arm=='baseline' else 'cand-src')
out=Path('/workspace/codecortex/artifacts/diagnostics/gin-native-regression-20261004')/('micro-'+suffix)
out.mkdir(exist_ok=False)
tool=Path('/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin')
target=base/('target-'+arm); tmp=base/('tmp-'+arm);tmp.mkdir(exist_ok=True)
env=dict(os.environ,RUSTUP_HOME='/workspace/.rustup',CARGO_HOME='/workspace/.cargo',RUSTC=str(tool/'rustc'),RUSTDOC=str(tool/'rustdoc'),CARGO_TARGET_DIR=str(target),CARGO_INCREMENTAL='0',CARGO_PROFILE_DEV_DEBUG='0',CARGO_BUILD_JOBS='4',TMPDIR=str(tmp),CODECORTEX_CACHE_DIR=str(base/('cache-'+arm)))
env['PATH']=str(tool)+':'+env.get('PATH','')
cmd=[str(tool/'cargo'),'build','--locked','-p','cc-eval','--lib','--message-format=json']
with (out/'build.jsonl').open('w') as log,(out/'build.log').open('w') as err:
 subprocess.run(cmd,cwd=repo,env=env,stdout=log,stderr=err,check=True)
records=list(map(json.loads,(out/'build.jsonl').read_text().splitlines()))
link=[str(tool/'rustc'),'--edition=2021',str(Path(__file__).with_name('micro_driver.rs')),'-L','dependency='+str(target/'debug/deps'),'-o',str(base/('driver-'+arm))]
for name in ['cc_eval','cc_server','serde_json']:
 found=[r for r in records if r.get('reason')=='compiler-artifact' and r['target']['name']==name and 'lib' in r['target']['kind'] and (name!='serde_json' or 'float_roundtrip' in r['features'])]
 assert len(found)==1,(name,len(found));link+=['--extern',name+'='+next(f for f in found[0]['filenames'] if f.endswith('.rlib'))]
subprocess.run(link,env=env,check=True)
with (out/'run.log').open('w') as log:
 subprocess.run([str(base/('driver-'+arm)),str(base/('fixture-'+suffix)),str(out),variant],cwd=repo,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
(out/'receipt.json').write_text(json.dumps({'arm':arm,'source_sha':subprocess.check_output(['git','rev-parse','HEAD'],cwd=repo).decode().strip(),'build':cmd,'link':link,'binary_sha256':hashlib.sha256((base/('driver-'+arm)).read_bytes()).hexdigest(),'product_diff':subprocess.check_output(['git','diff','HEAD','--','crates','Cargo.toml','Cargo.lock'],cwd=repo).decode()},indent=2)+'\n')
print(arm,'micro complete')
