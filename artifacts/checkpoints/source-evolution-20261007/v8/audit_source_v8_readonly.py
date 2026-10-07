#!/usr/bin/env python3
"""Independent Git/blob/AST/YAML audit. Never imports a guard or executes tests."""
import ast
from collections import Counter
import hashlib
import json
from pathlib import Path
import re
import stat
import subprocess

import yaml

ROOT = Path('/workspace/scratch/71bc431c4e4f/codecortex-next')
FIXED = 'c03f498a3ef72c892b00afe49bbce3c014d6e843'
EVIDENCE = '6c094ef6'
OLD = '6d02'
OUT = Path('/workspace/scratch/71bc431c4e4f/reviewed-source-v8-review.json')


def git(*args, data=None):
    return subprocess.check_output(['git', *args], cwd=ROOT, input=data)


def blob(ref, path):
    return git('show', ref + ':' + path)


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def json_bytes(value):
    return (json.dumps(value, ensure_ascii=False, sort_keys=True, indent=2) + '\n').encode()


def inventory(ref, *paths):
    result = {}
    for entry in git('ls-tree', '-r', '-z', ref, '--', *paths).split(b'\0'):
        if entry:
            meta, path = entry.split(b'\t', 1)
            mode, kind, oid = meta.decode().split()
            assert kind == 'blob' and mode in ('100644', '100755'), (ref, path)
            result[path.decode()] = oid
    return result


def constants(raw):
    wanted = {'BASE', 'PRODUCT', 'REGISTRY_SHA256', 'VERSION', 'APPROVED', 'HISTORICAL_HELPERS'}
    return {n.targets[0].id: ast.literal_eval(n.value) for n in ast.parse(raw).body
            if isinstance(n, ast.Assign) and isinstance(n.targets[0], ast.Name)
            and n.targets[0].id in wanted}


guard_raw = blob(FIXED, 'scripts/verify_reviewed_source.py')
pins = constants(guard_raw)
base, product, approved = pins['BASE'], pins['PRODUCT'], pins['APPROVED']
assert pins['VERSION'] == 'p7-engineering-20261007-v8' and len(approved) == 6
old_pins = constants(blob(OLD, 'scripts/verify_reviewed_source.py'))
for name in ['validation_work', 'python_capture_revalidation']:
    assert approved[name] == old_pins['APPROVED'][name]
registry_raw = blob(FIXED, 'scripts/reviewed-source-registry.json')
registry = json.loads(registry_raw)
assert sha(registry_raw) == pins['REGISTRY_SHA256'] == 'd4085cd65fb4748d302255f6ee5ad40f27b8b4cfc79418c091e1d81f47466ffa'
assert registry['source_version'] == pins['VERSION'] and registry['base_source'] == base
assert registry['product_source'] == product and registry['scope'] == 'source_integrity_only'
assert registry['quality_and_100k'] == 'not_inherited'
assert set(registry['deltas']) == set(approved)

reviews = {}
for name, pin in approved.items():
    for field in ['source', 'review']:
        assert re.fullmatch('[0-9a-f]{40}', pin[field])
    raw = blob(pin['review'], pin['review_path'])
    review = json.loads(raw)
    assert review['source'] == pin['source'] and review['base'] == base and review['verdict'] == 'accepted_scoped'
    assert sha(raw) == registry['deltas'][name]['review_sha256']
    assert (ROOT / pin['review_path']).read_bytes() == raw == blob(FIXED, pin['review_path'])
    reviews[name] = review

head_before = git('rev-parse', 'HEAD').decode().strip()
refs = {base, product, FIXED, head_before}
for name, pin in approved.items():
    refs.update([pin['source'], pin['review']])
    review = reviews[name]
    refs.add(review.get('author_source') or review.get('author_combined_source') or review['author_local_source'])
inventories = {ref: inventory(ref, 'crates', 'Cargo.toml', 'Cargo.lock') for ref in refs}
objects = sorted({oid for entries in inventories.values() for oid in entries.values()})
raw = git('cat-file', '--batch', data=('\n'.join(objects) + '\n').encode())
offset, hashes = 0, {}
for wanted in objects:
    end = raw.index(b'\n', offset)
    oid, kind, length = raw[offset:end].decode().split()
    assert oid == wanted and kind == 'blob'
    offset = end + 1
    length = int(length)
    hashes[oid] = sha(raw[offset:offset + length])
    offset += length
    assert raw[offset:offset + 1] == b'\n'
    offset += 1
assert offset == len(raw)
maps = {ref: {p: hashes[oid] for p, oid in sorted(items.items())} for ref, items in inventories.items()}
expected, changed, delta_results = dict(maps[base]), set(), {}
for name, pin in approved.items():
    paths = set(pin['paths'])
    actual = {p for p in set(maps[base]) | set(maps[pin['source']])
              if maps[base].get(p) != maps[pin['source']].get(p)}
    assert actual == paths and not changed.intersection(paths), name
    assert maps[pin['review']] == maps[pin['source']], name
    delta = registry['deltas'][name]
    assert {k: delta[k] for k in ('source', 'review', 'review_path')} == {k: pin[k] for k in ('source', 'review', 'review_path')}
    assert set(delta['paths']) == paths
    review = reviews[name]
    author = review.get('author_source') or review.get('author_combined_source') or review['author_local_source']
    for path in paths:
        assert delta['paths'][path]['before_sha256'] == maps[base].get(path)
        assert delta['paths'][path]['sha256'] == maps[pin['source']][path] == maps[author][path]
        expected[path] = maps[pin['source']][path]
        if path in review.get('copied_blobs', {}):
            assert review['copied_blobs'][path] == {'blob': inventories[author][path], 'sha256': maps[author][path]}
    if 'author_source_inputs' in review:
        assert len(maps[author]) == review['author_source_inputs']
        assert sha(json_bytes(maps[author])) == review['author_manifest_sha256']
        assert review['isolated_source_behavior_executed'] is False
        assert len(maps[pin['source']]) == review['isolated_source_inputs']
    if 'isolated_manifest_sha256' in review:
        assert sha(json_bytes(maps[pin['source']])) == review['isolated_manifest_sha256']
    changed.update(paths)
    delta_results[name] = {
        'source': pin['source'], 'review': pin['review'], 'review_path': pin['review_path'],
        'review_sha256': delta['review_sha256'], 'isolated_inputs': len(maps[pin['source']]),
        'author_source': author, 'author_inputs': len(maps[author]),
        'exact_changed_blobs_match_author_source': True,
        'paths': delta['paths'], 'independent_behavior_rerun_for_this_review': False,
    }
assert len(changed) == 23
assert expected == maps[product] == maps[FIXED] == maps[head_before] == registry['complete_inputs']
assert len(expected) == 776
recorded_map_path = 'artifacts/checkpoints/source-evolution-20261007/v8/source-manifest.json'
assert json.loads(blob(FIXED, recorded_map_path)) == expected
assert sha(blob(FIXED, recorded_map_path)) == sha(json_bytes(expected)) == 'baf2bd238706a0fd88b91c80357d1780415afd5b917ef56d4bf283f3932e8ee6'
tracked = {p.decode() for p in git('ls-files', '-z', '--', 'crates', 'Cargo.toml', 'Cargo.lock').split(b'\0') if p}
disk = {p.relative_to(ROOT).as_posix() for p in (ROOT / 'crates').rglob('*') if not p.is_dir() or p.is_symlink()}
assert tracked == set(expected) and disk == {p for p in expected if p.startswith('crates/')}
for path, digest in expected.items():
    assert stat.S_ISREG((ROOT / path).lstat().st_mode) and sha((ROOT / path).read_bytes()) == digest

historical = {}
for path in pins['HISTORICAL_HELPERS']:
    raw = blob(base, path)
    assert raw == blob(FIXED, path) == (ROOT / path).read_bytes()
    assert not (ROOT / path).is_symlink()
    historical[path] = sha(raw)
old_history = inventory(OLD, 'artifacts/checkpoints/source-evolution-20261007')
old_history = {p: oid for p, oid in old_history.items() if re.search(r'/v[4-7]/', p)}
fixed_history = inventory(FIXED, 'artifacts/checkpoints/source-evolution-20261007')
for path, oid in old_history.items():
    assert fixed_history[path] == oid
    original = blob(OLD, path)
    assert (ROOT / path).read_bytes() == original
    historical[path] = sha(original)
for name in ['validation_work', 'python_capture_revalidation']:
    path = approved[name]['review_path']
    assert blob(OLD, path) == blob(FIXED, path) == (ROOT / path).read_bytes()

def function_asts(raw):
    return {n.name: ast.dump(n, include_attributes=False) for n in ast.parse(raw).body
            if isinstance(n, (ast.FunctionDef, ast.AsyncFunctionDef))}

before_functions = function_asts(blob(OLD, 'scripts/verify_reviewed_source.py'))
after_functions = function_asts(guard_raw)
function_changes = sorted(k for k in set(before_functions) | set(after_functions)
                          if before_functions.get(k) != after_functions.get(k))
assert function_changes == ['expected_ci', 'verify_ci']
ci_raw = blob(FIXED, '.github/workflows/ci.yml')
old_ci = blob(OLD, '.github/workflows/ci.yml').decode()
shared = 'CODECORTEX_BENCH_BINARY="$PWD/target/debug/codecortex"'
private = 'CODECORTEX_BENCH_BINARY="$RUNNER_TEMP/p7-017-default/codecortex"'
old_selector = 'verify_reviewed_source.py --source-version ' + old_pins['VERSION']
new_selector = 'verify_reviewed_source.py --source-version ' + pins['VERSION']
assert old_ci.count(old_selector) == 1 and old_ci.count(shared) == 19
assert old_ci.replace(old_selector, new_selector).replace(shared, private).encode() == ci_raw
assert ci_raw.decode().count(private) == 20

engineering_path = '.github/workflows/p7-engineering.yml'
engineering_raw = blob(FIXED, engineering_path)
workflow = yaml.load(engineering_raw, Loader=yaml.BaseLoader)
job = workflow['jobs']['engineering']
assert workflow['on'] == {'pull_request': '', 'push': {'branches': ['main']}}
assert workflow['permissions'] == {'contents': 'read'}
steps = {row.get('name', row.get('uses')): row for row in job['steps']}
source_step = steps['Record exact input source']
assert source_step['id'] == 'source'
assert 'assert not target.exists()' in source_step['run']
assert 'CARGO_TARGET_DIR={target}\\nCARGO_BUILD_BUILD_DIR={target}' in source_step['run']
assert 'source_snapshot(pathlib.Path.cwd())' in source_step['run']
execution_steps = {name: row for name, row in steps.items() if re.search(r'cargo test|python3 -B -m unittest', row.get('run', ''))}
for name, row in execution_steps.items():
    assert row['if'] == "${{ !cancelled() && steps.source.outcome == 'success' }}", name
    assert 'continue-on-error' not in row and 'set -o pipefail' in row['run']
assert '--test-threads' not in engineering_raw.decode() and 'RUST_TEST_THREADS' not in engineering_raw.decode()
assert workflow['env']['CARGO_BUILD_JOBS'] == '2'
assert workflow['env']['RUSTFLAGS'] == '-D warnings'
assert steps['dtolnay/rust-toolchain@stable']['with']['toolchain'] == '1.95.0'
matrix = steps['Original deadline, cache and public retrieval matrix']['run']
assert '--no-fail-fast' in matrix and '--features semantic-http' in matrix
matrix_targets = re.findall(r'--test\s+(\w+)', matrix)
receipt_path = 'artifacts/checkpoints/p7-deadline-wiring-20261007/p7-013-receipt.json'
prior013 = json.loads(blob(FIXED, receipt_path))
prior_cases = [case for row in prior013['primary_matrix'] for case in row['test_cases']]
assert len(prior_cases) == len({x['identity'] for x in prior_cases}) == 58
target_counts = Counter(x['identity'].split('::')[0].removeprefix('cc-server/tests/').removesuffix('.rs')
                        for x in prior_cases if x['identity'].startswith('cc-server/tests/'))
assert set(matrix_targets) == set(target_counts) and len(matrix_targets) == 11 and sum(target_counts.values()) == 40
original_test_hashes = {}
for target in matrix_targets:
    path = 'crates/cc-server/tests/' + target + '.rs'
    raw = blob(prior013['source'], path)
    assert raw == blob(base, path) == blob(FIXED, path) == (ROOT / path).read_bytes()
    original_test_hashes[path] = sha(raw)
for path in ['crates/cc-server/src/semantic_query_encoding.rs', 'crates/cc-search/src/execution.rs',
             'artifacts/checkpoints/capability-snapshot-optimization-20261003/current-status-under-test.rs',
             'artifacts/checkpoints/capability-snapshot-optimization-20261003/v2-status-checks.rs']:
    raw = blob(prior013['source'], path)
    assert raw == blob(base, path) == blob(FIXED, path) == (ROOT / path).read_bytes()
    original_test_hashes[path] = sha(raw)
units = steps['Original query encoding and absolute deadline controls']['run']
assert 'semantic_query_encoding::tests::' in units and 'execution::tests::' in units
assert 'python3 -B -m unittest discover -s tests/evidence_lock -v' in steps['Benchmark input lock controls']['run']
lock_tests = blob(FIXED, 'tests/evidence_lock/test_benchmark_input_lock.py')
assert lock_tests == blob('1bb5ea9c6d66e9109fdaf8766606337eedec8219', 'tests/evidence_lock/test_benchmark_input_lock.py')
assert sum(isinstance(n, ast.FunctionDef) and n.name.startswith('test_') for n in ast.walk(ast.parse(lock_tests))) == 15
assert steps['Verify final input identity']['if'] == 'always()'
assert steps['Preserve executed commands and raw output']['if'] == 'always()'
assert "assert json.loads((out / 'source-before.json').read_text()) == current" in steps['Verify final input identity']['run']

checked_files = {path: sha(blob(FIXED, path)) for path in [
    'scripts/verify_reviewed_source.py', 'scripts/reviewed-source-registry.json',
    'tests/source_integrity/test_reviewed_source.py', '.github/workflows/ci.yml', engineering_path,
    'scripts/p7_build_identity.py', 'scripts/p7_stdio_build_receipt.py',
    'scripts/lock_benchmark_inputs.py', 'tests/evidence_lock/test_benchmark_input_lock.py',
]}
for path, digest in checked_files.items():
    assert sha((ROOT / path).read_bytes()) == digest
verification_path = 'artifacts/checkpoints/source-evolution-20261007/v8/verification.json'
verification_raw = blob(EVIDENCE, verification_path)
assert (ROOT / verification_path).read_bytes() == verification_raw
verification = json.loads(verification_raw)
assert verification['source_commit'] == FIXED and verification['all_passed']
execution_evidence = []
for row in verification['executions']:
    assert row['exit_code'] == 0 and row['source_commit'] == FIXED
    for log in row['logs']:
        path = str(Path(verification_path).parent / log['path'])
        raw = blob(EVIDENCE, path)
        assert len(raw) == log['bytes'] and sha(raw) == log['sha256']
        assert (ROOT / path).read_bytes() == raw
    execution_evidence.append(row)
stdout = json.loads(blob(EVIDENCE, str(Path(verification_path).parent / 'source-cli.stdout.log')))
assert stdout['source_version'] == pins['VERSION'] and stdout['product_source'] == product
assert stdout['status'] == 'passed' and stdout['complete_inputs'] == 776
control_log = blob(EVIDENCE, str(Path(verification_path).parent / 'negative-controls.stderr.log')).decode()
methods = re.findall(r'^(test_\w+) \(test_reviewed_source\.ReviewedSourceTests\.\1\) \.\.\. ok$', control_log, re.M)
control_ast = ast.parse(blob(FIXED, 'tests/source_integrity/test_reviewed_source.py'))
control_names = {n.name for n in ast.walk(control_ast) if isinstance(n, ast.FunctionDef) and n.name.startswith('test_')}
assert set(methods) == control_names and len(methods) == 14
assert 'Ran 14 tests' in control_log and control_log.rstrip().endswith('OK')
head_after = git('rev-parse', 'HEAD').decode().strip()
for path, digest in checked_files.items():
    assert sha((ROOT / path).read_bytes()) == digest

report = {
    'schema_version': 1, 'reviewer': '/root/todo_audit',
    'verdict': 'accepted_scoped_source_integrity_and_ci_static', 'findings': [],
    'scope': 'Independent read-only fixed Git/blob/AST/workflow and retained raw-log review. No guard, unittest, Rust or product execution by this review.',
    'source_version': pins['VERSION'], 'fixed_review_commit': FIXED,
    'observed_head_before': head_before, 'observed_head_after': head_after,
    'base': base, 'product': product, 'product_tree': git('rev-parse', product + '^{tree}').decode().strip(),
    'registry_sha256': pins['REGISTRY_SHA256'], 'complete_inputs': 776,
    'complete_manifest_sha256': sha(json_bytes(expected)), 'reviewed_delta_count': 6,
    'disjoint_changed_paths': 23, 'delta_review_results': delta_results,
    'equal_source_maps': ['base plus six fixed deltas', 'registry.complete_inputs', 'fixed product', 'c03f498 integration', 'observed current committed crate/Cargo inventory', 'current tracked/disk bytes', 'v8/source-manifest.json'],
    'checked_file_sha256': checked_files, 'historical_file_sha256': historical,
    'historical_scope': 'Nine immutable helper/registry files versus 886 base; all previously present v4-v7 source-evolution files versus 6d02; original A/B pins and review records unchanged. No rerun of prior 40/100k/quality gates.',
    'guard_function_ast_changes_from_v7': function_changes,
    'guard_logic_review': 'The reconstruction, immutable identities, history checks, exact before/after digests, review blob checks, six-way overlap rejection, full product map comparison and live tree verification are unchanged. New executable CI logic accepts exactly the nineteen reviewed product-path migrations; its control adds migration reversal rejection.',
    'ci_static': {
        'legacy_selector_changes': 1, 'legacy_default_path_migrations': 19,
        'legacy_default_references_after': 20, 'other_legacy_changes_from_v7': 0,
        'private_product_builder': 'Existing original default-build command creates an exclusive output and absent private Cargo target/build directory, verifies source before/after and the actual package/feature artifact, and copies a hash-bound product snapshot.',
        'engineering_fresh_target': True, 'engineering_build_dir_matches_target': True,
        'engineering_toolchain': '1.95.0', 'cargo_jobs': 2,
        'explicit_test_thread_override': False, 'matrix_no_fail_fast': True,
        'independent_execution_steps': len(execution_steps),
        'independent_steps_use_not_cancelled_and_successful_source_condition': True,
        'pipefail_and_no_continue_on_error': True, 'final_source_and_upload_use_always': True,
        'original_p7_013_http_targets_and_prior_function_counts': dict(sorted(target_counts.items())),
        'original_p7_013_encoding_functions': 13, 'original_p7_013_execution_functions': 5,
        'original_p7_013_distinct_functions_in_selected_scope': 58,
        'original_test_or_literal_sha256': original_test_hashes,
        'p8_fixed_stdlib_functions': 15,
        'new_fixture_targets': ['p7_dense_artifact_coverage', 'p7_gc_retention_config', 'gc_unlink_accounting', 'p7_worker_contention', 'p7_strategy_ablation'],
    },
    'retained_execution_owner': '/root',
    'retained_execution_evidence_commit': git('rev-parse', EVIDENCE).decode().strip(),
    'retained_verification_receipt_sha256': sha(verification_raw),
    'retained_source_cli_and_control_execution': execution_evidence,
    'retained_unittest_function_count': 14,
    'retained_unittest_count_note': 'Fourteen methods include one matching-product positive control; subtests and repeated checks are not additional functions.',
    'review_executions': {'independent_git_blob_hash_audit': True, 'read_only_ast_yaml_analysis': True, 'source_guard_reruns': 0, 'unittest_reruns': 0, 'rust_or_product_invocations': 0, 'new_benchmark_runs': 0},
    'limits': [
        'This accepts fixed source admission and static CI scope only. The combined 776-input product has not acquired behavior acceptance from its isolated source pins or prior differently composed author executions.',
        'The GitHub workflows were not executed or certified by this review; the future integrated CI head and actual uploaded logs must be reviewed separately before claiming a pass.',
        'Original P7-013 primary 57-pass/1-fail normal-schedule evidence remains open. Its one isolated two-function diagnostic is not a replacement or proof of an environment cause.',
        'Independent steps remain enabled after another step fails, but several commands/targets grouped inside a step retain their normal fail-fast behavior. A compile failure or job cancellation/timeout can still prevent remaining execution; zero skipped cases are not promised.',
        'The 35-minute workflow timeout and cold-build completion remain runtime constraints, not verified performance guarantees.',
        'The source inventory is the declared crate/Cargo scope. Fixed extra Python/workflow inputs and original literal status modules are separately hashed here; no hermetic build, whole repository execution attestation, live provider, quality, 100k, G7 or release approval is inherited.',
    ],
}
OUT.write_bytes(json_bytes(report))
print(json.dumps({'report': str(OUT), 'sha256': sha(OUT.read_bytes()), 'verdict': report['verdict'], 'source_inputs': 776, 'changed_paths': 23, 'historical_files': len(historical), 'reviewed_test_methods_from_original_log': len(methods), 'reviewer_test_reruns': 0}))
