#!/usr/bin/env python3
"""Run only the authorized ordinary schema/rebuild tests and scoped lint."""
import hashlib
import json
import os
from pathlib import Path
import re
import subprocess
import time

HERE = Path(__file__).resolve().parent
ROOT = HERE.parents[2]
ENV = os.environ | {
    'CARGO_HOME': '/workspace/.cargo',
    'RUSTUP_HOME': '/workspace/.rustup',
    'RUSTUP_TOOLCHAIN': '1.95.0',
    'CARGO_BUILD_JOBS': '5',
}
ENV['PATH'] = '/workspace/.cargo/bin:' + ENV['PATH']
commands = [
    ('writer', ['cargo', 'test', '-p', 'cc-db', '--lib', 'direct_writer::tests::', '--locked', '--', '--nocapture'], 10),
    ('canonical', ['cargo', 'test', '-p', 'cc-db', '--lib', 'index_db::tests::direct_rebuild_canonical_schema_matches_normal_temp_path', '--locked', '--', '--exact', '--nocapture'], 1),
    ('errors', ['cargo', 'test', '-p', 'cc-db', '--lib', 'index_db::tests::direct_rebuild_callback_and_index_errors_leave_live_database_unchanged', '--locked', '--', '--exact', '--nocapture'], 1),
    ('normal-generation', ['cargo', 'test', '-p', 'cc-db', '--lib', 'index_db::tests::full_rebuild_advances_both_epochs_past_previous_values', '--locked', '--', '--exact', '--nocapture'], 1),
    ('fmt', ['cargo', 'fmt', '--all', '--', '--check'], None),
    ('clippy', ['cargo', 'clippy', '-p', 'cc-db', '--all-targets', '--locked', '--', '-D', 'warnings'], None),
]
results = []
for name, command, expected in commands:
    start = time.monotonic()
    result = subprocess.run(command, cwd=ROOT, env=ENV, stdout=subprocess.PIPE, stderr=subprocess.STDOUT, text=True)
    log = HERE / f'validation-{name}.log'
    log.write_text(result.stdout.rstrip() + '\n')
    summary = re.search(r'test result: ok\. (\d+) passed; (\d+) failed; (\d+) ignored;', result.stdout)
    passed = int(summary[1]) if summary else None
    ok = result.returncode == 0 and (expected is None or passed == expected)
    record = {'command': command, 'exit_code': result.returncode, 'seconds': round(time.monotonic() - start, 2),
              'log': log.name, 'expected_tests': expected, 'passed': passed, 'ok': ok}
    results.append(record)
    print(json.dumps(record), flush=True)
    (HERE / 'validation-receipt.json').write_text(json.dumps({
        'base': 'ee4c4fc0b41e298bf38b8269310053fa4b355c61',
        'production_sha256': hashlib.sha256((ROOT / 'crates/cc-db/src/direct_writer.rs').read_bytes()).hexdigest(),
        'test_sha256': hashlib.sha256((ROOT / 'crates/cc-db/src/index_db_tests.rs').read_bytes()).hexdigest(),
        'results': results,
    }, indent=2) + '\n')
    if not ok:
        raise SystemExit(1)
