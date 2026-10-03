"""Build/link own driver using official lock; no cc-eval or gold/scorer edits."""
import json, os, subprocess, sys, tempfile
from pathlib import Path
repo, output, mode = Path(sys.argv[1]), Path(sys.argv[2]), sys.argv[3]
owned = Path('/workspace/scratch/python-kind')
rust = owned / 'rust/bin'
env = dict(os.environ, CARGO_HOME=str(owned/'cargo'), RUSTC=str(rust/'rustc'), RUSTDOC=str(rust/'rustdoc'), CARGO_TARGET_DIR=str(owned/'target'), CODECORTEX_CACHE_DIR=str(owned/f'cache-{mode}'), CARGO_BUILD_JOBS='5')
output.mkdir(parents=True, exist_ok=True)
with (output/'build.jsonl').open('w') as log, (output/'build.log').open('w') as err:
    subprocess.run([str(rust/'cargo'),'build','--locked','-p','cc-eval','--lib','--message-format=json'], cwd=repo, env=env, stdout=log, stderr=err, check=True)
records = [json.loads(line) for line in (output/'build.jsonl').read_text().splitlines()]
cmd = [str(rust/'rustc'),'--edition=2021', str(Path(__file__).with_name('native_driver.rs')), '-L',f'dependency={owned}/target/debug/deps','-o',str(owned/f'driver-{mode}')]
for name in ['cc_eval','cc_server','serde_json']:
    found = [r for r in records if r.get('reason')=='compiler-artifact' and r['target']['name']==name and 'lib' in r['target']['kind'] and (name!='serde_json' or 'float_roundtrip' in r['features'])]
    assert len(found)==1, (name,len(found))
    cmd += ['--extern', name+'='+next(f for f in found[0]['filenames'] if f.endswith('.rlib'))]
subprocess.run(cmd, env=env, check=True)
with tempfile.TemporaryDirectory(prefix=f'fixture-{mode}-',dir=owned) as fixture, (output/'native.log').open('w') as log:
    subprocess.run([str(owned/f'driver-{mode}'),fixture,str(output),mode],cwd=repo,env=env,stdout=log,stderr=subprocess.STDOUT,check=True)
