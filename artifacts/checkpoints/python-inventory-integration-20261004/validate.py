#!/usr/bin/env python3
"""Scoped tests; strict all-target lint/build only compile the remaining targets."""
import argparse
import hashlib
import json
import os
from pathlib import Path
import subprocess
import time

ROOT = Path(__file__).resolve().parents[3]
OUT = Path(__file__).resolve().parent
ENV = dict(os.environ, PATH='/workspace/.cargo/bin:' + os.environ['PATH'],
           RUSTUP_HOME='/workspace/.rustup', CARGO_HOME='/workspace/.cargo',
           PYTHONDONTWRITEBYTECODE='1', RUSTFLAGS='-D warnings')
CARGO = ['cargo', '+1.95.0']
COMMANDS = [
    ('toolchain', ['rustc', '+1.95.0', '-vV']),
    ('fmt', CARGO + ['fmt', '--all', '--', '--check']),
    ('lint', CARGO + ['clippy', '--locked', '--workspace', '--all-targets', '--', '-D', 'warnings']),
    ('build', CARGO + ['build', '--locked', '--workspace', '--all-targets']),
    ('index', CARGO + ['test', '--locked', '-p', 'cc-index', '--test', 'python_inventory_capture', '--test', 'python_inventory_resource_integration', '--test', 'python_identity_resource_integration']),
    ('drift', CARGO + ['test', '--locked', '-p', 'cc-index', '--lib', 'python_inventory_observed_midcapture_mutations_refuse']),
    ('provenance', CARGO + ['test', '--locked', '-p', 'cc-index', '--lib', 'python_provenance']),
    ('model', CARGO + ['test', '--locked', '-p', 'cc-model', '--test', 'declaration_identity_v1', '--test', 'declaration_identity_independent_review', '--test', 'declaration_identity_resources', '--test', 'declaration_resource_independent', '--test', 'provenance_compatibility']),
    ('parser', CARGO + ['test', '--locked', '-p', 'cc-parsers', '--test', 'python_identity_adapter', '--test', 'python_identity_independent', '--test', 'python_identity_r1_independent_delta', '--test', 'python_identity_trivia_r1']),
    ('architecture', ['python3', 'scripts/check_source_architecture.py']),
    ('module-architecture', ['python3', 'scripts/check_module_architecture.py']),
    ('source-controls', ['python3', '-m', 'unittest', 'discover', '-s', 'tests/source_integrity', '-v']),
    ('source-v2', ['python3', 'scripts/verify_current_source_v2.py', '--source-version', 'python-inventory-20261004-v2']),
    ('historical-guards', ['python3', 'scripts/verify_fixed_e3_integration.py']),
    ('historical-p0-controls', ['python3', '-m', 'unittest', 'discover', '-s', 'tests/historical_corpus', '-v']),
    ('historical-p0', ['python3', 'scripts/p0_historical_corpus.py', '--validator', 'target/debug/cc-eval']),
    ('plan', ['python3', 'scripts/code_index_plan.py']),
    ('diff', ['git', 'diff', '--check']),
]


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--start-at', choices=[name for name, _ in COMMANDS])
    args = parser.parse_args()
    start_index = next((i for i, (name, _) in enumerate(COMMANDS) if name == args.start_at), 0)
    records = [] if not args.start_at else json.loads((OUT / 'checks.json').read_text())['records'][:start_index]
    assert all(record['exit'] == 0 for record in records)
    for name, argv in COMMANDS[start_index:]:
        start = time.monotonic()
        log = OUT / (name + '.log')
        with log.open('wb') as stream:
            result = subprocess.run(argv, cwd=ROOT, env=ENV, stdout=stream, stderr=subprocess.STDOUT)
        raw = log.read_bytes()
        if raw:
            log.write_bytes(raw.rstrip(b'\n') + b'\n')
        record = dict(name=name, argv=argv, exit=result.returncode,
                      seconds=round(time.monotonic() - start, 3),
                      log_sha256=hashlib.sha256(log.read_bytes()).hexdigest())
        records.append(record)
        (OUT / 'checks.json').write_text(json.dumps({
            'fixed_product': '780502322816e4fd1d61d8f77a98d7d6b635d9fb',
            'rustflags': ENV['RUSTFLAGS'], 'records': records,
            'scope': 'bounded_capture_admission_and_source_integrity',
            'formal_DEV_100k_runtime_private42_GCWALfaults': 'not_run',
        }, indent=2) + '\n')
        print(json.dumps(record), flush=True)
        if result.returncode:
            raise SystemExit(result.returncode)


if __name__ == '__main__':
    main()
