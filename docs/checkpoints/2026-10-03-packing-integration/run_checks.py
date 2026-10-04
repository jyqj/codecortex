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
OUT = HERE / 'checks'
OUT.mkdir(exist_ok=False)
env = dict(os.environ, PATH='/workspace/.cargo/bin:' + os.environ['PATH'],
           CARGO_HOME='/workspace/.cargo', RUSTUP_HOME='/workspace/.rustup',
           CARGO_TARGET_DIR='/workspace/target-packing-integration',
           PYTHONDONTWRITEBYTECODE='1')
cargo = ['cargo', 'test', '--locked', '-j', '2']
checks = [
    ('schema', cargo + ['-p', 'cc-db', '--test', 'ci_schema_guard_contract']),
    ('cost-priority-packing', cargo + ['-p', 'cc-eval', '--test', 'p1d_cost', '--test', 'p5e_priority_pressure', '--test', 'packing_scope_v1', '--', '--nocapture']),
    ('db13', cargo + ['-p', 'cc-index', '-p', 'cc-server', '--test', 'qname_db_independent_review', '--', '--nocapture']),
    ('parser3', cargo + ['-p', 'cc-parsers', '--test', 'independent_cpp_final_declarators']),
    ('leaf', cargo + ['-p', 'cc-index', '--test', 'independent_qname_repair_guard', 'cpp_rejected_kind_hint_keeps_actual_function_name_not_return_type', '--', '--exact']),
    ('python-owner', cargo + ['-p', 'cc-index', '--test', 'independent_qname_repair_guard', 'decorator_owners_and_canonical_class_method_are_not_duplicated', '--', '--exact']),
    ('python-proof', cargo + ['-p', 'cc-server', '--test', 'independent_qname_repair_public']),
    ('fmt', ['cargo', 'fmt', '--all', '--', '--check']),
    ('clippy', ['cargo', 'clippy', '--locked', '-j', '2', '--workspace', '--all-targets', '--', '-D', 'warnings']),
    ('module', ['python3', 'scripts/check_module_architecture.py']),
    ('source', ['python3', 'scripts/check_source_architecture.py']),
    ('guard', ['python3', '-m', 'unittest', 'discover', '-s', 'scripts/tests', '-v']),
    ('plan', ['python3', 'scripts/code_index_plan.py']),
    ('pygo-registration', ['python3', 'artifacts/checkpoints/public-dev-declaration-kind-admission-20261003/test_selector.py']),
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
    (HERE / 'checks.json').write_text(json.dumps(results, indent=2)+'\n')
    print(json.dumps(row), flush=True)
raise SystemExit(any(r['exit_code'] for r in results))
