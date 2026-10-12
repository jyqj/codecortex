#!/usr/bin/env python3
"""Read-only independent exact Git-object review of the final v15 binding."""
import ast
import copy
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path('/workspace/scratch/2509e1756d27/codecortex')
OUT = Path('/workspace/scratch/2509e1756d27/pr-audit-20261012')
P = '0692ed27c4f86a254484e76155fe07a55ab2e09c'
R = '72723404303125d4dca201b27139cdc4089dfff9'
G = 'f631350e07841164c5d27f23769483ef71aadea4'
TREE = '3c33da90b80be319b97604544bca2fa49c59e85c'
REVIEW_PATH = 'artifacts/checkpoints/p8-pr203-admission-20261012/independent-source-review.json'
REVIEW_SHA = '9550b3b77dd421432100864703d8c19096d34d86e9d59c1318fdd22c9f70dea8'
REGISTRY_SHA = 'e4c82d029c32f7635ff40914818f52fbbc0b59deefb2c23bff546f5372c80faa'
GUARD = 'scripts/verify_reviewed_source_v15.py'
REGISTRY = 'scripts/reviewed-source-registry-v15.json'
EXCLUSIONS = {GUARD, REGISTRY, '.github/workflows/ci.yml'}
VROOTS = ('scripts', '.github/workflows', 'tests/source_integrity')
commands = []


def git(*args):
    commands.append(['git', *args])
    return subprocess.check_output(['git', *args], cwd=ROOT)


def blob(ref, path):
    return git('show', f'{ref}:{path}')


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def inventory(ref, roots):
    result = {}
    for row in git('ls-tree', '-r', '-z', ref, '--', *roots).split(b'\0'):
        if row:
            header, path = row.decode().split('\t', 1)
            mode, kind, oid = header.split()
            assert kind == 'blob' and mode in {'100644', '100755'}
            result[path] = dict(mode=mode, oid=oid)
    return result


def validation(ref):
    return {p: row for p, row in inventory(ref, VROOTS).items()
            if p not in EXCLUSIONS
            and not ('__pycache__' in Path(p).parts and p.endswith('.pyc'))}


def changes(a, b):
    return set(git('diff', '--name-only', a, b).decode().splitlines())


def commit_header(ref):
    header = git('cat-file', '-p', ref).decode().split('\n\n', 1)[0].splitlines()
    return {'tree': next(l[5:] for l in header if l.startswith('tree ')),
            'parents': [l[7:] for l in header if l.startswith('parent ')]}


def normalized_guard(raw):
    pins = {}
    tree = ast.parse(raw)
    for node in tree.body:
        if isinstance(node, ast.Assign) and len(node.targets) == 1 and isinstance(node.targets[0], ast.Name):
            name = node.targets[0].id
            if name in {'PRODUCT', 'REVIEW', 'REVIEW_PATH', 'REGISTRY_SHA256'}:
                pins[name] = ast.literal_eval(node.value)
                node.value = ast.Constant(value='<IDENTITY-PIN>')
    assert set(pins) == {'PRODUCT', 'REVIEW', 'REVIEW_PATH', 'REGISTRY_SHA256'}
    return ast.dump(tree, include_attributes=False), pins


def run():
    assert commit_header(G) == {'tree': TREE, 'parents': [R]}
    assert commit_header(R)['parents'] == [P]
    assert changes(R, G) == {GUARD, REGISTRY}
    review_changes = changes(P, R)
    assert review_changes and all(p.startswith('artifacts/checkpoints/p8-pr203-admission-20261012/') for p in review_changes)
    old_guard, new_guard = blob(R, GUARD), blob(G, GUARD)
    old_ast, old_pins = normalized_guard(old_guard)
    new_ast, new_pins = normalized_guard(new_guard)
    assert old_ast == new_ast
    expected_pins = {'PRODUCT': P, 'REVIEW': R, 'REVIEW_PATH': REVIEW_PATH, 'REGISTRY_SHA256': REGISTRY_SHA}
    assert new_pins == expected_pins
    reconstructed_guard = old_guard.decode()
    for name, value in expected_pins.items():
        pattern = rf'^{name} = "[^"]+"$'
        reconstructed_guard, count = re.subn(pattern, f'{name} = "{value}"', reconstructed_guard, flags=re.MULTILINE)
        assert count == 1
    assert reconstructed_guard.encode() == new_guard
    raw_registry = blob(G, REGISTRY)
    assert sha(raw_registry) == REGISTRY_SHA
    actual_registry = json.loads(raw_registry)
    expected_registry = copy.deepcopy(json.loads(blob(P, REGISTRY)))
    expected_registry.update(product_source=P, review_source=R, review_sha256=REVIEW_SHA)
    expected_registry['validation_inputs']['.github/workflows/p8-task-profile.yml'] = '2dfcab748378de13e2bc5ffef3a35bb08ff4448921496c1119d8b4c4e552c344'
    assert actual_registry == expected_registry
    review_raw = blob(R, REVIEW_PATH)
    assert sha(review_raw) == REVIEW_SHA
    assert review_raw == (OUT / 'round2-source-review.json').read_bytes()
    assert blob(G, REVIEW_PATH) == review_raw
    review = json.loads(review_raw)
    assert review['source'] == P and review['base'] == actual_registry['base_source']
    assert review['scope'] == actual_registry['scope'] == 'independently_reviewed_source_and_validation_inputs'
    assert review['verdict'] == 'accepted_scoped' and review['unresolved_blockers'] == []
    assert review['paths'] == actual_registry['delta']
    assert review['complete_inputs'] == actual_registry['complete_inputs']
    assert review['validation_inputs'] == actual_registry['validation_inputs']
    product = inventory(G, ('crates', 'Cargo.toml', 'Cargo.lock'))
    assert product == inventory(P, ('crates', 'Cargo.toml', 'Cargo.lock')) and len(product) == 1096
    inputs = validation(G)
    assert inputs == validation(P) and len(inputs) == 157
    assert set(product) == set(actual_registry['complete_inputs'])
    assert set(inputs) == set(actual_registry['validation_inputs'])
    assert len(actual_registry['delta']) == 80
    ci = blob(G, '.github/workflows/ci.yml')
    assert ci == blob(P, '.github/workflows/ci.yml')
    assert ci.count(b'verify_reviewed_source_v15.py --source-version p8-completion-source-20261009-v15') == 1
    for path in review['guard_integrity']['frozen_inputs_equal_original_base']:
        assert blob(G, path) == blob(review['base'], path)
    assert sha(blob(G, 'scripts/current-source-registry-v1.json')) == 'fdab5efa046cce7ffc4a9e900a908f781f9c90c282a9eaabf790ad689e44ed60'
    record = {'verdict': 'accepted_exact_binding', 'reviewer': '/root/pr_audit', 'source': G, 'source_tree': TREE,
              'source_parents': [R], 'product': P, 'review': R, 'review_parent': P,
              'review_path': REVIEW_PATH, 'review_bytes': len(review_raw), 'review_sha256': REVIEW_SHA,
              'registry_sha256': REGISTRY_SHA, 'guard_sha256': sha(new_guard),
              'R_evidence_paths_count': len(review_changes), 'G_changed_paths': sorted(changes(R, G)),
              'guard_delta': {'constants_only': expected_pins, 'AST_equal_after_only_four_pin_normalization': True,
                              'raw_bytes_equal_after_only_four_assignment_replacements': True,
                              'BASE_VERSION_FROZEN_algorithms_and_CI_selector_unchanged': True},
              'registry_delta': {'product_source': P, 'review_source': R, 'review_sha256': REVIEW_SHA,
                                 'sole_validation_leaf': '.github/workflows/p8-task-profile.yml',
                                 'all_other_fields_exactly_unchanged': True},
              'inventory_counts': {'product': len(product), 'original_BASE_delta': len(actual_registry['delta']), 'validation': len(inputs)},
              'full_product_and_validation_Git_mode_blob_maps_equal_reviewed_P': True,
              'canonical_review_raw_bytes_equal_independent_original': True,
              'historical_v1_registry_exact_unchanged': True,
              'original_verifier_execution': 'not_executed_by_this_reviewer; parent executing separately on exactG',
              'CI': 'new-head runtime checks pending; original797e failure retained',
              'merge_approval': False, 'remote_mutations': False, 'original_TODO_completion_credit': 0,
              'commands': commands}
    (OUT / 'finalG-review.json').write_text(json.dumps(record, indent=2, sort_keys=True) + '\n')
    print(json.dumps({'status': record['verdict'], 'source': G, 'review_sha256': REVIEW_SHA,
                      'registry_sha256': REGISTRY_SHA, 'counts': record['inventory_counts'],
                      'review_bytes': len(review_raw), 'finalG_review_sha256': sha((OUT / 'finalG-review.json').read_bytes()),
                      'original_TODO_completion_credit': 0}, sort_keys=True))


if __name__ == '__main__':
    run()
