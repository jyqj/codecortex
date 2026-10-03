"""Compile a receipted driver against cargo artifacts and run index only."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import tempfile

p = argparse.ArgumentParser()
p.add_argument('--build-json', type=Path, required=True)
p.add_argument('--output', type=Path, required=True)
p.add_argument('--mode', choices=['old', 'fixed'], required=True)
a = p.parse_args()
here = Path(__file__).resolve().parent
libs = {}
for line in a.build_json.read_text().splitlines():
    try:
        d = json.loads(line)
    except ValueError:
        continue
    if d.get('reason') == 'compiler-artifact':
        for f in d['filenames']:
            if f.endswith('.rlib'):
                libs[d['target']['name']] = Path(f)
command = ['rustc', '--edition=2021', str(here / 'probe.rs'), '-L', 'dependency=' + str(libs['cc_parsers'].parent)]
for name in ['cc_index', 'cc_parsers', 'cc_model', 'cc_db', 'serde_json']:
    command += ['--extern', name + '=' + str(libs[name])]
assert not a.output.exists(), 'refuse overwriting evidence'
a.output.mkdir()
binary = a.output / 'probe-local'
command += ['-o', str(binary)]
subprocess.run(command, check=True)
hash_file = lambda f: hashlib.sha256(f.read_bytes()).hexdigest()
cases = {
    'nested': ('case.go', 'package p\nfunc f() { a.B().C() }\n'),
    'same_leaf_three': ('case.go', 'package p\nfunc f() { a.B().B().B() }\n'),
}
if a.mode == 'fixed':
    cases.update({
        'arguments': ('case.go', 'package p\nfunc f() { a.B(C()).D(E()); F(G()) }\n'),
        'multiline_unicode': ('case.go', 'package p\nfunc f() { 对象.方法().\n 方法() }\n'),
        'generic_method': ('case.go', 'package p\nfunc (r R[T]) f() { Box[int]{}.Build().Build(); pkg.Make[int]().Next().Next(); r.Method() }\n'),
    })
cases['locked_gin_context'] = ('context.go', (here / 'public-source/context.go').read_text())
results = []
for label, (name, source) in cases.items():
    with tempfile.TemporaryDirectory(prefix='go-call-identity-index-') as t:
        root = Path(t)
        f = root / name
        f.write_text(source)
        r = subprocess.run([str(binary), str(f), str(root / 'project'), a.mode], capture_output=True, text=True, timeout=60)
        (a.output / (label + '.stderr')).write_text(r.stderr)
        assert r.returncode == 0, (label, r.stdout, r.stderr)
        value = json.loads(r.stdout)
        value.update(label=label, source_sha256=hash_file(f))
        results.append(value)
(a.output / 'results.json').write_text(json.dumps(results, indent=2) + '\n')
(a.output / 'build-receipt.json').write_text(json.dumps({
    'mode': a.mode, 'compile_command': command, 'probe_source_sha256': hash_file(here/'probe.rs'),
    'binary_sha256': hash_file(binary), 'artifact_sha256': {str(libs[n]): hash_file(libs[n]) for n in ['cc_index', 'cc_parsers', 'cc_model', 'cc_db', 'serde_json']},
    'index_calls': len(cases), 'search_calls': 0, 'provider_calls': 0,
}, indent=2) + '\n')
# Binary is intentionally local only, never added to the evidence directory in git.
print(json.dumps({'mode': a.mode, 'index_calls': len(cases), 'passed': len(results)}))
