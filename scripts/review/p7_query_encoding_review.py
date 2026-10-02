#!/usr/bin/env python3
"""Compile an unregistered candidate without changing production registration.
Retains candidate bytes/tests, appends independent tests, filters execution to ours.
"""
import argparse, hashlib, pathlib, subprocess, tempfile
parser = argparse.ArgumentParser()
parser.add_argument("--features", choices=["semantic", "semantic-http"], default="semantic-http")
args = parser.parse_args()
root = pathlib.Path(__file__).resolve().parents[2]
source = root / 'crates/cc-server/src/semantic_query_encoding.rs'
expected = 'eda4df9f84943c5ec86f7d03f508c3fffedcc012'
original = subprocess.check_output(['git', 'show', f'{expected}:crates/cc-server/src/semantic_query_encoding.rs'], cwd=root)
assert source.read_bytes() == original, 'candidate changed; review must be rerun explicitly'
print('candidate', expected, 'file_sha256', hashlib.sha256(original).hexdigest(), flush=True)
harness = root / 'crates/cc-server/tests/p7_query_encoding_review_harness.rs'
assert not harness.exists()
with tempfile.TemporaryDirectory(prefix='p7-query-review-') as directory:
    candidate = pathlib.Path(directory) / 'candidate.rs'
    cases = root / 'scripts/review/p7_query_encoding_cases.rs'
    candidate.write_bytes(original + f'\n#[cfg(test)]\n#[path = "{cases}"]\nmod independent_review;\n'.encode())
    harness.write_text(f'#![cfg(feature = "semantic")]\n#[path = "{root}/crates/cc-server/src/service_factory.rs"]\nmod service_factory;\n#[path = "{candidate}"]\nmod candidate;\n')
    try:
        result = subprocess.run(['cargo', 'test', '-p', 'cc-server', '--features', args.features, '--locked', '--offline', '--test', harness.stem, 'independent_review', '--', '--nocapture'], cwd=root)
    finally:
        harness.unlink()
raise SystemExit(result.returncode)
