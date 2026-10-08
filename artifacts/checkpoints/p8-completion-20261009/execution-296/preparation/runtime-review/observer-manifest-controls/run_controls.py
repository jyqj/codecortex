"""Adversarial controls execute the exact helper manifest-gate AST.

This is a narrow manifest-boundary control, not full artifact acceptance. Original
receipts are read-only seeds; their mutations are retained as explicit controls.
No product, observer receipt, ZIP, or historical review is rewritten.
"""
import ast
import copy
import hashlib
import json
from pathlib import Path
import subprocess
import sys

BASE = Path(__file__).resolve().parent
REVIEW = BASE.parent
FROZEN = Path('/dev/shm/a217aaae3bde/codecortex-round5-frozen')
HEAD = '599a7050e7d52b5b7b93975c419138e175b3f754'
assert subprocess.check_output(['git', 'rev-parse', 'HEAD'], cwd=FROZEN, text=True).strip() == HEAD
sys.dont_write_bytecode = True
sys.path.insert(0, str(FROZEN / 'scripts'))
import p8_runtime_build as owner

def sha(path):
    return hashlib.sha256(path.read_bytes()).hexdigest()

assert Path(owner.__file__).resolve() == FROZEN / 'scripts/p8_runtime_build.py'
owner_git = subprocess.check_output(['git', 'show', HEAD + ':scripts/p8_runtime_build.py'], cwd=FROZEN)
assert Path(owner.__file__).read_bytes() == owner_git
assert len(owner.OBSERVER_FILES) == len(set(owner.OBSERVER_FILES)) == 9
result = {'scope': __doc__, 'seed_head': HEAD, 'expected_observer_files': list(owner.OBSERVER_FILES),
          'expected_inventory_owner_sha256': sha(Path(owner.__file__)), 'helpers': []}

for kind, receipt_path, helper_name in [
    ('runtime', Path('/dev/shm/a217aaae3bde/ci-artifacts/11575574524/extracted/p8-build/build-receipt.json'),
     'review_runtime_new_observer_fixed_source.py'),
    ('backfill', Path('/dev/shm/a217aaae3bde/ci-artifacts/11576813300/extracted/receipt.json'),
     'review_backfill_new_observer_fixed_source.py'),
]:
    helper = REVIEW / helper_name
    source = helper.read_text()
    tree = ast.parse(source, filename=str(helper))
    assertions = [x for x in ast.walk(tree) if isinstance(x, ast.Assert)]
    anchors = [x for x in assertions if 'observer_before' in ast.unparse(x.test)
               and 'observer_after' in ast.unparse(x.test)]
    added = [x for x in assertions if isinstance(x.msg, ast.Constant)
             and x.msg.value in (kind + ' observer manifest HEAD differs',
                                kind + ' observer manifest inventory differs')]
    assert len(anchors) == 1 and len(added) == 2
    # Execute production assertions, rather than duplicating their predicates.
    old_gate = compile(ast.fix_missing_locations(ast.Module(body=anchors, type_ignores=[])), str(helper), 'exec')
    new_gate = compile(ast.fix_missing_locations(ast.Module(body=anchors + added, type_ignores=[])), str(helper), 'exec')
    (BASE / (kind + '-executed-gate.py')).write_text('\n'.join(ast.unparse(x) for x in anchors + added) + '\n')
    original = json.loads(receipt_path.read_text())
    assert set(original['observer_before']['files']) == set(owner.OBSERVER_FILES)
    assert original['observer_before']['source_commit'] == HEAD
    entry = {'kind': kind, 'helper': str(helper), 'helper_sha256': sha(helper),
             'seed_receipt': str(receipt_path), 'seed_receipt_sha256': sha(receipt_path), 'cases': []}
    for case in ('clean', 'reordered_keys', 'missing_member', 'unknown_member_same_count', 'wrong_observer_head'):
        receipt = copy.deepcopy(original)
        manifest = receipt['observer_before']
        if case == 'reordered_keys':
            manifest['files'] = dict(reversed(list(manifest['files'].items())))
        elif case == 'missing_member':
            manifest['files'].pop('scripts/p8_runtime.py')
        elif case == 'unknown_member_same_count':
            manifest['files']['scripts/not-in-runtime-observer-set.py'] = manifest['files'].pop('scripts/p8_runtime.py')
        elif case == 'wrong_observer_head':
            manifest['source_commit'] = '0' * 40
        # Keep old before/after/final equality self-consistent: it must not be the
        # reason an adversarial manifest is rejected by the new boundary.
        receipt['observer_after'] = copy.deepcopy(manifest)
        if kind == 'backfill':
            receipt['observer_final'] = copy.deepcopy(manifest)
        control_path = BASE / (kind + '-' + case + '.json')
        control_path.write_text(json.dumps({'protocol_control_only': True, 'observer_manifest': manifest}, indent=2, sort_keys=True) + '\n')
        namespace = {'head': HEAD, 'owner': owner, 'b': owner, 'build': receipt, 'r': receipt,
                     'plan': {'source': receipt['source_before']}}
        observations = {}
        for label, gate in [('old_manifest_gate', old_gate), ('new_manifest_gate', new_gate)]:
            try:
                exec(gate, namespace)
                observations[label] = {'accepted': True}
            except AssertionError as error:
                observations[label] = {'accepted': False, 'error': str(error)}
        expected = case in ('clean', 'reordered_keys')
        assert observations['new_manifest_gate']['accepted'] is expected, (kind, case, observations)
        if not expected:
            message = kind + ' observer manifest ' + ('HEAD differs' if case == 'wrong_observer_head' else 'inventory differs')
            if kind == 'backfill' and case == 'missing_member':
                message = ''  # Existing len==9 rejects first; its assertion remains unchanged.
            assert observations['new_manifest_gate']['error'] == message
        if case != 'missing_member' or kind == 'runtime':
            assert observations['old_manifest_gate']['accepted'] is True
        else:
            assert observations['old_manifest_gate']['accepted'] is False
        entry['cases'].append({'case': case, 'input': str(control_path), 'input_sha256': sha(control_path), **observations})
    assert sha(receipt_path) == entry['seed_receipt_sha256']
    result['helpers'].append(entry)

result.update(status='passed', checks=10, positive_controls=4, adversarial_rejections=6,
              limitation='Only the actual manifest-boundary AST was executed; this does not claim full synthetic-artifact acceptance or a new-source workload run.',
              control_script_sha256=sha(Path(__file__)))
output = BASE / 'control-results.json'
output.write_text(json.dumps(result, indent=2, sort_keys=True) + '\n')
print(json.dumps({'status': 'passed', 'checks': 10, 'positive_controls': 4, 'adversarial_rejections': 6,
                  'output': str(output), 'sha256': sha(output)}, indent=2))
