#!/usr/bin/env python3
"""Focused validation only; no broad, runtime, corpus or excluded tests."""
import hashlib, json, os, pathlib, subprocess, time
root = pathlib.Path(__file__).resolve().parents[3]
out = pathlib.Path(__file__).resolve().parent
env = dict(os.environ, PATH='/workspace/.cargo/bin:' + os.environ['PATH'],
           RUSTUP_HOME='/workspace/.rustup', CARGO_HOME='/workspace/.cargo')
commands = [
    ('toolchain', ['rustc', '+1.95.0', '--version']),
    ('index', ['cargo', '+1.95.0', 'test', '--locked', '-p', 'cc-index', '--test', 'python_inventory_capture', '--test', 'python_identity_resource_integration']),
    ('drift', ['cargo', '+1.95.0', 'test', '--locked', '-p', 'cc-index', '--lib', 'python_inventory_observed_midcapture_mutations_refuse']),
    ('provenance', ['cargo', '+1.95.0', 'test', '--locked', '-p', 'cc-index', '--lib', 'python_provenance']),
    ('model', ['cargo', '+1.95.0', 'test', '--locked', '-p', 'cc-model', '--test', 'declaration_identity_v1', '--test', 'declaration_identity_independent_review', '--test', 'declaration_identity_resources', '--test', 'declaration_resource_independent']),
    ('parser', ['cargo', '+1.95.0', 'test', '--locked', '-p', 'cc-parsers', '--test', 'python_identity_adapter', '--test', 'python_identity_independent', '--test', 'python_identity_r1_independent_delta', '--test', 'python_identity_trivia_r1']),
    ('lint', ['cargo', '+1.95.0', 'clippy', '--locked', '-p', 'cc-index', '--lib', '--test', 'python_inventory_capture', '--', '-D', 'warnings']),
    ('format', ['cargo', '+1.95.0', 'fmt', '--all', '--', '--check']),
    ('diff', ['git', 'diff', '--check']),
]
records = []
for name, argv in commands:
    start = time.monotonic()
    with (out / (name + '.log')).open('wb') as log:
        result = subprocess.run(argv, cwd=root, env=env, stdout=log, stderr=subprocess.STDOUT)
    path = out / (name + '.log')
    data = path.read_bytes()
    if data: path.write_bytes(data.rstrip(b'\n') + b'\n')
    record = dict(name=name, argv=argv, exit=result.returncode, seconds=round(time.monotonic()-start, 3))
    records.append(record)
    print(json.dumps(record), flush=True)
    if result.returncode: break
paths = ['Cargo.lock', 'crates/cc-index/src/project_model/mod.rs',
         'crates/cc-index/src/project_model/python_inventory.rs',
         'crates/cc-index/src/project_model/python_inventory/native.rs',
         'crates/cc-index/src/project_model/python_inventory/tests.rs',
         'crates/cc-index/tests/python_inventory_capture.rs']
(out / 'validation.json').write_text(json.dumps(dict(base='ba6197771da5425166fb0386d46c74f4a05e8273',
    records=records, sha256={p:hashlib.sha256((root/p).read_bytes()).hexdigest() for p in paths}), indent=2)+'\n')
raise SystemExit(records[-1]['exit'])
