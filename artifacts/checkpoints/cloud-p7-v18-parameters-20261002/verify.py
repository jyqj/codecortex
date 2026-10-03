#!/usr/bin/env python3
"""Run only the new V18 contract suite, with separate canonical JSON traces.
Profiles run sequentially because Cargo's product binary path is shared.
No provider endpoint beyond each test's synthetic 127.0.0.1 listener is used.
"""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess

HERE = Path(__file__).resolve().parent
REPO = HERE.parents[2]
BASE = '7481731429118f45b6144518e8e9cfad3e02d7b4'
SOURCE = '30eb750dd4172be67e20ab61bf2ba74f5cfd0a8b'

def sha(p):
    return hashlib.sha256(p.read_bytes()).hexdigest()

def write(p, value):
    p.write_text(json.dumps(value, indent=2, sort_keys=True) + '\n')

root = HERE / 'matrix'
root.mkdir()  # Immutable new-run authoring; never replace prior evidence.
env = dict(os.environ, CARGO_HOME='/workspace/.cargo', RUSTUP_HOME='/workspace/.rustup',
           PATH='/workspace/.cargo/bin:' + os.environ['PATH'], CARGO_BUILD_JOBS='5', CARGO_INCREMENTAL='0')
results = []
for profile, features, count, repeats in [('default', None, 4, 5), ('semantic', 'semantic', 5, 5),
                                           ('semantic-http', 'semantic-http', 6, 10)]:
    profile_root = root / profile
    profile_root.mkdir()
    for n in range(1, repeats + 1):
        run = profile_root / f'{n:02}'
        run.mkdir()
        raw = run / 'raw'
        command = ['cargo', 'test', '-p', 'cc-server', '--test', 'p7_v18_parameter_contract',
                   '--locked', '--offline']
        if features:
            command += ['--features', features]
        command += ['--', '--nocapture']
        with (run / 'test.log').open('wb') as out:
            result = subprocess.run(command, cwd=REPO, env=dict(env, P7_V18_EVIDENCE_DIR=str(raw)),
                                    stdout=out, stderr=subprocess.STDOUT)
        log = (run / 'test.log').read_text()
        if result.returncode or f'{count} passed; 0 failed; 0 ignored' not in log:
            raise RuntimeError(f'{profile}/{n} failed ({result.returncode}); raw retained')
        records = [json.loads(p.read_text()) for p in sorted(raw.glob('*.json'))]
        assert sum(r['kind'] == 'schema' for r in records) == 2
        assert sum(r['kind'] == 'sanitize_error' for r in records) == 10
        assert sum(r['kind'] == 'schema_error' for r in records) == 12
        loopback = [r['data'] for r in records if r['kind'] == 'loopback']
        if features == 'semantic-http':
            assert len(loopback) == 1 and loopback[0]['embedding_http_calls'] > 0
            assert loopback[0]['query_http_calls'] == 0 and loopback[0]['real_credentials'] is False
        else:
            assert not loopback
        binary = REPO / 'target/debug/codecortex'
        runners = re.findall(r'Running tests/p7_v18_parameter_contract\.rs \(([^)]+)\)', log)
        assert len(runners) == 1
        receipt = {
            'profile': profile, 'repetition': n, 'baseline_sha': BASE, 'source_sha': SOURCE,
            'command': command, 'exit_code': result.returncode, 'passed': count, 'failed': 0, 'ignored': 0,
            'features': features or 'default', 'build_profile': 'test/dev', 'jobs': 5, 'incremental': False,
            'product_binary_sha256': sha(binary), 'test_binary_sha256': sha(REPO / runners[0]),
            'source_file_sha256': sha(REPO / 'crates/cc-server/tests/p7_v18_parameter_contract.rs'),
            'raw_record_count': len(records), 'loopback': loopback,
            'artifact_sha256': {str(p.relative_to(run)): sha(p) for p in sorted(run.rglob('*')) if p.is_file()},
            'status': 'parameter/status propagation only; nonempty semantic query remains not_proven',
        }
        write(run / 'receipt.json', receipt)
        results.append(receipt)
    print(profile, repeats, 'runs passed', flush=True)
write(root / 'summary.json', {'baseline_sha': BASE, 'source_sha': SOURCE, 'test_runs': len(results),
                             'test_executions': sum(r['passed'] for r in results),
                             'failed': 0, 'ignored': 0, 'runs': results,
                             'nonempty_semantic_query': 'not_proven: correct cold-cache default; new separately opted-in query encoding feature awaiting wiring/acceptance',
                             'formal_P7_014': 'pending dependencies and independent review',
                             'real_provider_calls': 0, 'D1_D2': 'unchanged'})
