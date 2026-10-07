"""Run PR134's byte-identical driver; 'before' selects its recovery score=1 oracle."""
import json, os, subprocess, tempfile
from pathlib import Path
repo = Path(__file__).resolve().parents[3]
output = Path(__file__).resolve().parent
env = dict(os.environ, PATH='/workspace/.cargo/bin:'+os.environ.get('PATH',''), CARGO_HOME='/workspace/.cargo', RUSTUP_HOME='/workspace/.rustup')
with (output/'build.jsonl').open('w') as log, (output/'build.log').open('w') as err:
    subprocess.run(['/workspace/.cargo/bin/cargo', 'build', '--locked', '-p', 'cc-eval', '--lib', '--message-format=json'], cwd=repo, env=env, stdout=log, stderr=err, check=True)
records = [json.loads(line) for line in (output/'build.jsonl').read_text().splitlines()]
rust = Path('/workspace/.rustup/toolchains/1.95.0-x86_64-unknown-linux-gnu/bin')
cmd = [str(rust/'rustc'), '--edition=2021', str(output/'native_driver.rs'), '-L', f'dependency={repo}/target/debug/deps', '-o', '/tmp/cpp-native-driver']
for name in ['cc_eval', 'cc_server', 'serde_json']:
    found = [r for r in records if r.get('reason') == 'compiler-artifact' and r['target']['name'] == name and 'lib' in r['target']['kind'] and (name != 'serde_json' or 'float_roundtrip' in r['features'])]
    assert len(found) == 1, (name, len(found))
    cmd += ['--extern', name+'='+next(f for f in found[0]['filenames'] if f.endswith('.rlib'))]
subprocess.run(cmd, check=True)
env = dict(os.environ, CODECORTEX_CACHE_DIR='/tmp/cpp-boundary-native-cache')
with tempfile.TemporaryDirectory(prefix='cpp-declarator-') as fixture, (output/'native.log').open('w') as log:
    subprocess.run(['/tmp/cpp-native-driver', fixture, str(output/'fixed'), 'before'], cwd=repo, env=env, stdout=log, stderr=subprocess.STDOUT, check=True)
