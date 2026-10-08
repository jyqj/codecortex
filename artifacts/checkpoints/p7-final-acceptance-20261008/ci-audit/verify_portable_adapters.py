#!/usr/bin/env python3
"""Verify the exact portable adaptation before replay; this is not product acceptance."""
import ast
import hashlib
import json
import os
from pathlib import Path
import subprocess

ROOT = Path(os.environ['P7_AUDIT_SOURCE_ROOT']).resolve()
HERE = Path(__file__).resolve().parent
OUT = Path(os.environ['P7_AUDIT_OUTPUT']).resolve()
MANIFEST_SHA256 = '1cd5c7d90d8606321ffe459cc5a3a28ef6fde57af951c15876cd8a8c2aa125ca'

def sha(data):
    return hashlib.sha256(data).hexdigest()

raw = (HERE / 'portable-adaptation.json').read_bytes()
assert sha(raw) == MANIFEST_SHA256
manifest = json.loads(raw)
assert manifest['fixed_candidate'] == 'ffdc6f0f97db78cc25a6c026904e7c2adde05d14'
assert subprocess.check_output(['git', 'rev-parse', manifest['fixed_candidate'] + '^{tree}'],
                               cwd=ROOT).decode().strip() == manifest['expected_complete_tree']
checked = []
for row in manifest['scripts']:
    old = subprocess.check_output(['git', 'show', manifest['fixed_candidate'] + ':' + row['old_path']], cwd=ROOT)
    assert sha(old) == row['old_sha256']
    assert hashlib.sha1(b'blob ' + str(len(old)).encode() + b'\0' + old).hexdigest() == row['old_blob']
    rebuilt = old.decode('utf-8')
    for replacement in row['replacements']:
        assert rebuilt.count(replacement['from']) == replacement['occurrences']
        rebuilt = rebuilt.replace(replacement['from'], replacement['to'])
    current = (HERE / row['new_file']).read_bytes()
    assert current == rebuilt.encode('utf-8') and sha(current) == row['new_sha256']
    old_ast = ast.parse(old)
    new_ast = ast.parse(current)
    old_functions = [ast.dump(node, include_attributes=False) for node in old_ast.body
                     if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))]
    new_functions = [ast.dump(node, include_attributes=False) for node in new_ast.body
                     if isinstance(node, (ast.FunctionDef, ast.AsyncFunctionDef))]
    assert old_functions == new_functions
    assert sum(isinstance(node, ast.Assert) for node in ast.walk(old_ast)) == row['old_assert_statements']
    assert sum(isinstance(node, ast.Assert) for node in ast.walk(new_ast)) == row['new_assert_statements']
    compile(current, row['new_file'], 'exec')
    checked.append({'file': row['new_file'], 'sha256': sha(current),
                    'exact_whitelisted_adaptation': True, 'function_asts_unchanged': True,
                    'original_assert_statements': row['old_assert_statements'],
                    'adapted_assert_statements': row['new_assert_statements']})
for row in manifest['raw_logs']:
    content = (HERE / row['file']).read_bytes()
    assert len(content) == row['utf8_bytes'] and sha(content) == row['sha256']
    assert content.startswith(b'\xef\xbb\xbf')
OUT.mkdir(parents=True, exist_ok=True)
result = {'schema_version': 1, 'status': 'portable_adapter_bytes_verified_not_product_acceptance',
          'fixed_candidate': manifest['fixed_candidate'], 'adaptation_manifest_sha256': MANIFEST_SHA256,
          'scripts': checked, 'raw_logs': manifest['raw_logs'],
          'artifact_replay_remains_required': True}
target = OUT / 'portable_adapter_verification.json'
target.write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')
print(json.dumps(result, indent=2, sort_keys=True))
