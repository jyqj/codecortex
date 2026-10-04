#!/usr/bin/env python3
"""Bounded integration checks, preserving every log; no broad runtime execution."""
import json
import os
from pathlib import Path
import re
import subprocess
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
OUT = HERE / 'final-checks'
OUT.mkdir(exist_ok=False)
env = dict(os.environ, PATH='/workspace/.cargo/bin:' + os.environ['PATH'],
           CARGO_HOME='/workspace/.cargo', RUSTUP_HOME='/workspace/.rustup',
           CARGO_TARGET_DIR='/workspace/target-packing-integration',
           PYTHONDONTWRITEBYTECODE='1')
cargo = ['cargo', 'test', '--locked', '-j', '2']
checks = [
    ('db-index-after-clippy', cargo + ['-p', 'cc-index', '--test', 'qname_db_independent_review', '--', '--nocapture']),
    ('independent-packing-boundary', cargo + ['-p', 'cc-eval', '--test', 'diag_p5e_fix_review_boundary_20261003', '--', '--nocapture']),
    ('independent-packing-stage', cargo + ['-p', 'cc-eval', '--test', 'diag_p5e_fix_review_stage_20261003', 'diag_p5e_stage_receipts_same_microfixture', '--', '--exact', '--nocapture']),
    ('fmt', ['cargo', 'fmt', '--all', '--', '--check']),
    ('clippy', ['cargo', 'clippy', '--locked', '-j', '2', '--workspace', '--all-targets', '--', '-D', 'warnings']),
    ('identity', ['python3', 'scripts/verify_fixed_e3_integration.py']),
    ('plan', ['python3', 'scripts/code_index_plan.py']),
    ('diff', ['git', 'diff', '--check']),
]
results = []
for name, argv in checks:
    start = time.monotonic()
    log = OUT / (name + '.log')
    with log.open('w') as f:
        f.write(json.dumps(argv) + '\n')
        f.flush()
        result = subprocess.run(argv, cwd=ROOT, env=env, stdout=f, stderr=subprocess.STDOUT)
    raw = log.read_text()
    counts = [tuple(map(int, m)) for m in re.findall(r'test result: \w+\. (\d+) passed; (\d+) failed; (\d+) ignored', raw)]
    row = dict(name=name, argv=argv, exit_code=result.returncode,
               seconds=round(time.monotonic()-start, 3), rust_counts=counts)
    results.append(row)
    (HERE / 'final-checks.json').write_text(json.dumps(results, indent=2)+'\n')
    print(json.dumps(row), flush=True)
raise SystemExit(any(r['exit_code'] for r in results))
