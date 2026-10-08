#!/usr/bin/env python3
"""Independent fixed-tree/pin verification; no guard import or source mutation."""
import argparse
import ast
import hashlib
import json
from pathlib import Path
import re
import subprocess

ROOT = Path('/dev/shm/codecortex-closeout-996727/codecortex-root')
BASE = '47d1939e43455ecd72ccc73e80825d971e3f33e7'
PRODUCT = '09291fdf4d968b0929d598cd5df6a3d1fbd3d6cc'
PREVIOUS = '9ebe1f619a298b9955d576d9d8250368259924d2'
GUARD = 'scripts/verify_reviewed_source_v13.py'
REGISTRY = 'scripts/reviewed-source-registry-v13.json'
REVIEW = 'artifacts/checkpoints/p7-closeout-20261008/independent-source-review.json'
FORMAL = 'artifacts/checkpoints/p7-closeout-20261008/independent-v13-guard/review.json'
EXCLUDE = {GUARD, REGISTRY, '.github/workflows/ci.yml'}


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT)


def blob(ref, path):
    return git('show', f'{ref}:{path}')


def inventory(ref, *roots):
    entries = []
    for line in git('ls-tree', '-r', '-z', ref, '--', *roots).split(b'\0'):
        if not line:
            continue
        metadata, path = line.decode().split('\t', 1)
        mode, kind, oid = metadata.split()
        assert kind == 'blob' and mode in {'100644', '100755'}, path
        entries.append((path, oid))
    raw = subprocess.check_output(['git', 'cat-file', '--batch'], cwd=ROOT,
                                  input=('\n'.join(oid for _, oid in entries) + '\n').encode())
    result, offset = {}, 0
    for path, oid in entries:
        end = raw.index(b'\n', offset)
        actual_oid, kind, length = raw[offset:end].decode().split()
        assert actual_oid == oid and kind == 'blob'
        offset = end + 1
        value = raw[offset:offset + int(length)]
        offset += int(length)
        assert raw[offset:offset + 1] == b'\n'
        offset += 1
        result[path] = sha(value)
    assert offset == len(raw)
    return result


def normalized(raw):
    result, count = re.subn(rb'^(PRODUCT|REVIEW|REGISTRY_SHA256) = .*$',
                           rb'\1 = "<FIXED_AFTER_REVIEW>"', raw, flags=re.MULTILINE)
    assert count == 3
    return result


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument('--installed', action='store_true')
    parser.add_argument('--output', type=Path, required=True)
    args = parser.parse_args()
    raw = (ROOT / GUARD).read_bytes()
    tree = ast.parse(raw)
    lines = raw.splitlines(keepends=True)
    functions = {f.name: sha(b''.join(lines[f.lineno - 1:f.end_lineno]))
                 for f in tree.body if isinstance(f, ast.FunctionDef)}
    formal = json.loads(blob(PREVIOUS, FORMAL))['guard']
    assert len(functions) == 14 and functions == formal['top_level_function_body_sha256']
    assert sha(normalized(raw)) == formal['three_pin_normalized_sha256']
    assert normalized(raw) == normalized(blob(PREVIOUS, GUARD))
    pins = {n.targets[0].id: ast.literal_eval(n.value) for n in tree.body
            if isinstance(n, ast.Assign) and isinstance(n.targets[0], ast.Name)
            and n.targets[0].id in {'BASE', 'PRODUCT', 'REVIEW', 'REGISTRY_SHA256'}}
    source = inventory(PRODUCT, 'crates', 'Cargo.toml', 'Cargo.lock')
    base = inventory(BASE, 'crates', 'Cargo.toml', 'Cargo.lock')
    assert set(base) <= set(source)
    delta = {p: {'before_sha256': base.get(p), 'sha256': digest}
             for p, digest in source.items() if base.get(p) != digest}
    validation = {p: h for p, h in inventory(PRODUCT, 'scripts', '.github/workflows', 'tests/source_integrity').items()
                  if p not in EXCLUDE and not ('__pycache__' in Path(p).parts and p.endswith('.pyc'))}
    previous_validation = {p: h for p, h in inventory(PREVIOUS, 'scripts', '.github/workflows', 'tests/source_integrity').items()
                           if p not in EXCLUDE and not ('__pycache__' in Path(p).parts and p.endswith('.pyc'))}
    assert validation == previous_validation
    assert len(source) == 795 and len(delta) == 35 and len(validation) == 99
    manifest_raw = Path('/dev/shm/codecortex-closeout-996727/validation/reclaim-new-source-inventory.json').read_bytes()
    manifest = json.loads(manifest_raw)
    assert manifest['source'] == PRODUCT and manifest['base'] == BASE
    assert manifest['tree'] == git('rev-parse', PRODUCT + '^{tree}').decode().strip()
    assert manifest['complete_inputs'] == source and manifest['delta'] == delta and manifest['validation_inputs'] == validation
    for p, h in source.items() | validation.items():
        assert not (ROOT / p).is_symlink() and sha((ROOT / p).read_bytes()) == h, p
    result = {'schema_version': 1, 'reviewer': '/root/p7_wiring',
              'status': 'static_complete_manifest_verified_pins_pending',
              'root_head_at_observation': git('rev-parse', 'HEAD').decode().strip(),
              'product': PRODUCT, 'product_tree': manifest['tree'], 'base': BASE,
              'independently_hashed_source_inputs': len(source), 'source_delta': len(delta),
              'independently_hashed_validation_inputs': len(validation),
              'all_99_validation_inputs_equal_previous_fixed_ci_head': PREVIOUS,
              'manifest_sha256': sha(manifest_raw), 'observed_guard_pins': pins,
              'guard_sha256': sha(raw), 'guard_three_pin_normalized_sha256': sha(normalized(raw)),
              'all_14_exact_function_source_sha256': functions,
              'guard_ast_without_pin_values_unchanged': True,
              'runtime_and_current_ci_execution': 'not_inferred_from_this_static_verification'}
    if args.installed:
        assert pins['BASE'] == BASE and pins['PRODUCT'] == PRODUCT
        registry_raw = (ROOT / REGISTRY).read_bytes()
        assert sha(registry_raw) == pins['REGISTRY_SHA256']
        registry = json.loads(registry_raw)
        assert registry['base_source'] == BASE and registry['product_source'] == PRODUCT
        assert registry['review_source'] == pins['REVIEW']
        assert registry['complete_inputs'] == source and registry['delta'] == delta and registry['validation_inputs'] == validation
        review_raw = blob(pins['REVIEW'], REVIEW)
        assert not (ROOT / REVIEW).is_symlink()
        assert review_raw == (ROOT / REVIEW).read_bytes() and sha(review_raw) == registry['review_sha256']
        review = json.loads(review_raw)
        assert review['source'] == PRODUCT and review['base'] == BASE and review['verdict'] == 'accepted_scoped'
        assert review['source_tree'] == manifest['tree'], 'aggregate review source_tree is stale'
        assert review['paths'] == delta and review['validation_inputs'] == validation
        assert review['independent_reviewers'] and review['unresolved_blockers'] == []
        references = review['review_records'] + review['offline_evidence_records'] + [
            review['new_opportunistic_reclaim_independent_review'], review['prior_accepted_source_review']]
        for record in references:
            assert sha(blob(pins['REVIEW'], record['path'])) == record['sha256'], record['path']
        reclaim = json.loads(blob(pins['REVIEW'], review['new_opportunistic_reclaim_independent_review']['path']))
        assert reclaim['reviewer'] == 'pr_audit' and reclaim['author'] == 'root' and reclaim['non_author_source_review']
        assert reclaim['verdict'] == 'accepted_scoped'
        assert reclaim['local_source_tree'] == manifest['tree'] == git('rev-parse', reclaim['local_source_commit'] + '^{tree}').decode().strip()
        assert reclaim['remote_product_binding']['commit'] == PRODUCT
        prior_source = inventory(PREVIOUS, 'crates', 'Cargo.toml', 'Cargo.lock')
        changed = {p for p, digest in source.items() if prior_source.get(p) != digest}
        assert changed == {r['path'] for r in reclaim['exact_source_delta']} and len(changed) == 4
        for record in reclaim['exact_source_delta']:
            assert record['sha256'] == source[record['path']]
            assert record['before_sha256'] == prior_source.get(record['path'])
        result.update(status='installed_pins_and_complete_manifest_statically_verified',
                      fixed_review=pins['REVIEW'], registry_sha256=sha(registry_raw), review_sha256=sha(review_raw),
                      aggregate_source_tree=review['source_tree'], referenced_review_records_verified=len(references),
                      reclaim_non_author_review_sha256=review['new_opportunistic_reclaim_independent_review']['sha256'],
                      all_four_reclaim_delta_hashes_independently_match=True)
    args.output.write_text(json.dumps(result, sort_keys=True, indent=2) + '\n')
    print(json.dumps({'receipt': str(args.output), 'sha256': sha(args.output.read_bytes()),
                      'status': result['status'], 'source': len(source), 'validation': len(validation), 'functions': len(functions)}))


if __name__ == '__main__':
    main()
