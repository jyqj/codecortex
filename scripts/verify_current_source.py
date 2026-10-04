#!/usr/bin/env python3
"""Verify python-resource-20261004-v1 source integrity only.

Reconstruct every crate/Cargo/lock input from pinned historical packing,
reviewed bounded imports, their mechanical lint transformation, and the approved
Python/resource imports and R1 patch. Independently compare that union to fixed
product 50a493, then compare the complete tracked and disk inventory and bytes.
The registry is immutable: future reviewed development needs a new explicit
version and pins, never a regenerated digest of the current checkout.
Historical packing/e3 results and old failure evidence remain historical;
quality, formal DEV and 100k certification are separate and not inherited.
"""
import hashlib
import json
from pathlib import Path
import subprocess

ROOT = Path(__file__).resolve().parent.parent
REGISTRY = ROOT / 'scripts/current-source-registry-v1.json'
REGISTRY_SHA256 = 'fdab5efa046cce7ffc4a9e900a908f781f9c90c282a9eaabf790ad689e44ed60'
PRODUCT = '50a4933e48ef20b16401ac8f75c386a660aa8e5c'
PACKING = '37dd042eaa1209a86e0cafdcd92ae77e036e76f5'
BOUNDED = 'e4a8df4cbc6dfae29af8cb0eac9ee83fbba4d696'
BASE = '84d5d57978cfaa3a0dd7f63e39d7c7288b614d95'
B = 'artifacts/checkpoints/bounded-integration-20261004/'
P = 'artifacts/checkpoints/python-resource-integration-20261004/'
# Expand short historical manifest references to immutable object identities.
REVIEW_REFS = {
    'bc0d0a25': 'bc0d0a25dba2a60fd80b7d1674961b6e12285af9',
    '56fd54e2': '56fd54e26f3f9c388fdcf0a15543b43dd5f26590',
}


def sha(raw):
    return hashlib.sha256(raw).hexdigest()


def git(*args):
    return subprocess.check_output(['git', *args], cwd=ROOT)


def blob(ref, path):
    return git('show', f'{ref}:{path}')


def ensure_refs(refs):
    missing = [ref for ref in sorted(set(refs)) if subprocess.run(
        ['git', 'cat-file', '-e', ref + '^{commit}'], cwd=ROOT,
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode]
    if missing:
        subprocess.run(['git', 'fetch', '--no-tags', 'origin', *missing], cwd=ROOT, check=True)


def inputs(ref):
    return set(git('ls-tree', '-r', '--name-only', ref, '--',
                   'crates', 'Cargo.toml', 'Cargo.lock').decode().splitlines())


def scoped(path):
    return path.startswith('crates/') or path in ('Cargo.toml', 'Cargo.lock')


def load_records(registry_path=REGISTRY, root=ROOT):
    raw = registry_path.read_bytes()
    assert sha(raw) == REGISTRY_SHA256, 'stale or altered current-source registry'
    registry = json.loads(raw)
    assert registry['schema_version'] == 1 and registry['approved_product'] == PRODUCT
    ensure_refs([PRODUCT, PACKING, BOUNDED, BASE] +
                [row['commit'] for row in registry['records'].values()])
    records = {}
    for path, row in registry['records'].items():
        original = blob(row['commit'], path)
        assert sha(original) == row['sha256'], f'declared record SHA differs: {path}'
        assert (root / path).read_bytes() == original, f'approved record changed: {path}'
        records[path] = original
    return records


def approved_union(records, root=ROOT):
    """Reconstruct from accepted sources, then independently check the fixed product."""
    packing = json.loads(records['docs/checkpoints/2026-10-03-packing-integration/source-manifest.json'])
    bounded = json.loads(records[B + 'source-guard.json'])
    transform = json.loads(records[B + 'test-only-lint-fix/transformation.json'])
    python = json.loads(records[P + 'source-guard.json'])
    migration = json.loads(records[P + 'migration.json'])
    binding = json.loads(records[P + 'binding.json'])
    assert bounded['base'] == PACKING and bounded['product'] == BOUNDED
    assert python['integration_base'] == BASE and python['production_anchor'] == BOUNDED
    assert migration == python['migration'] and migration['current_product_commit'] == PRODUCT
    assert binding['product_sha'] == PRODUCT and binding['integration_base'] == BASE
    ensure_refs([r['source_commit'] for r in packing['crate_inputs']] +
                [r['source'] for r in bounded['source_files'].values()] +
                [REVIEW_REFS.get(r, r) for r in bounded['review_imports'].values()] +
                [r['source'] for r in python['imports']] +
                [r['integrated'] for r in python['imports']])
    expected = {}
    for row in packing['crate_inputs']:
        path = row['path']
        raw = blob(row['source_commit'], path)
        if row.get('adaptation') == 'four_cloned_ref_to_slice_refs':
            assert sha(raw) == row['original_sha256']
            assert (root / 'docs/checkpoints/2026-10-03-packing-integration/original-tests/qname_db_independent_review.rs').read_bytes() == raw
            head, tail = raw.split(b'&[original.clone()]', 1)
            raw = head + b'&[original.clone()]' + tail.replace(
                b'&[original.clone()]', b'std::slice::from_ref(&original)').replace(
                b'&[repeated.clone()]', b'std::slice::from_ref(&repeated)')
        assert sha(raw) == row['sha256'], f'packing declared SHA differs: {path}'
        assert raw == blob(PACKING, path), f'historical packing provenance differs: {path}'
        assert path not in expected, f'duplicate packing input: {path}'
        expected[path] = raw
    assert set(expected) == inputs(PACKING), 'historical packing inventory differs'
    for path, row in bounded['source_files'].items():
        raw = blob(row['source'], path)
        assert sha(raw) == row['sha256'], f'bounded declared SHA differs: {path}'
        assert raw == blob(BOUNDED, path)
        expected[path] = raw
    # Historical review sources were cherry-picked, not necessarily ancestors.
    for path, ref in bounded['review_imports'].items():
        raw = blob(REVIEW_REFS.get(ref, ref), path)
        assert raw == blob(BOUNDED, path)
        if scoped(path):
            expected[path] = raw
    compatibility = 'crates/cc-model/tests/provenance_compatibility.rs'
    expected[compatibility] = blob(REVIEW_REFS['bc0d0a25'], 'crates/cc-index/tests/provenance_review_support/prototype_compatibility.rs')
    assert set(expected) == inputs(BOUNDED), 'bounded source union inventory differs'
    assert all(raw == blob(BOUNDED, path) for path, raw in expected.items()), 'bounded source union differs'
    path = transform['path']
    original = expected[path]
    assert sha(original) == transform['original_sha256']
    assert (root / transform['original_evidence']).read_bytes() == original
    old = (transform['old_expression'] + '\n').encode()
    new = (transform['new_expression'] + '\n').encode()
    assert original.count(old) == transform['occurrences'] == 1
    expected[path] = original.replace(old, new)
    assert sha(expected[path]) == transform['transformed_sha256']
    assert set(expected) == inputs(BASE)
    assert all(raw == blob(BASE, path) for path, raw in expected.items()), 'bounded lint transformation differs'
    for entry in python['imports']:
        paths = git('diff-tree', '--no-commit-id', '--name-only', '-r', entry['source']).decode().splitlines()
        assert paths == entry['paths'], 'Python import inventory differs'
        for path in paths:
            raw = blob(entry['source'], path)
            assert raw == blob(entry['integrated'], path), f'import provenance differs: {path}'
            if scoped(path):
                expected[path] = raw
    path = migration['path']
    original = expected[path]
    assert original == blob(migration['historical_source_commit'], path)
    assert original == blob(migration['historical_integrated_commit'], path)
    assert sha(original) == migration['original_sha256']
    assert (root / (P + 'python_identity_independent.rs.original')).read_bytes() == original
    # Fix serialization: Git chooses a longer default abbreviation in larger
    # object databases, including Actions' fetched/shallow provenance objects.
    patch = git('diff', '--no-ext-diff', '--no-textconv', '--no-color',
                '--abbrev=7', '--src-prefix=a/', '--dst-prefix=b/',
                '--unified=3', '--diff-algorithm=myers',
                migration['historical_integrated_commit'], PRODUCT, '--', path)
    assert patch == records[P + 'r1-migration.patch'], 'R1 test transformation differs'
    expected[path] = blob(PRODUCT, path)
    assert sha(expected[path]) == migration['current_sha256']
    path = 'crates/cc-index/tests/python_identity_resource_integration.rs'
    expected[path] = blob(PRODUCT, path)
    assert sha(expected[path]) == binding['new_test_sha256']
    assert sha(expected['Cargo.lock']) == binding['cargo_lock_sha256']
    assert set(expected) == inputs(PRODUCT), 'approved product/source union inventory differs'
    assert all(raw == blob(PRODUCT, path) for path, raw in expected.items()), 'approved product/source union bytes differ'
    return expected


def verify_tree(root, expected, tracked):
    assert set(tracked) == set(expected), 'current tracked input inventory differs'
    disk = {p.relative_to(root).as_posix() for p in (root / 'crates').rglob('*')
            if p.is_file() or p.is_symlink()}
    assert disk == {p for p in expected if p.startswith('crates/')}, 'current disk input inventory differs'
    for path, raw in expected.items():
        target = root / path
        assert not target.is_symlink(), f'symlink input: {path}'
        assert target.read_bytes() == raw, f'current input bytes differ: {path}'


def main():
    expected = approved_union(load_records())
    tracked = git('ls-files', '--', 'crates', 'Cargo.toml', 'Cargo.lock').decode().splitlines()
    verify_tree(ROOT, expected, tracked)
    print(json.dumps({'status': 'passed', 'source_version': 'python-resource-20261004-v1',
                      'approved_product': PRODUCT, 'complete_inputs': len(expected),
                      'scope': 'source_integrity_only', 'quality_and_100k': 'not_inherited'}))


if __name__ == '__main__':
    main()
