#!/usr/bin/env python3
"""Verify explicitly reviewed deltas on the immutable v3 source inventory.

The pinned registry records individual source commits and before/after hashes.
The current checkout is compared with that reconstruction, never used to grant
itself approval. Historical source/quality evidence keeps its original scope.
"""
import argparse
import hashlib
import json
from pathlib import Path
import re
import subprocess
import sys

import verify_current_source_v3 as previous

ROOT = Path(__file__).resolve().parents[1]
VERSION = 'p7-capture-revalidation-20261007-v7'
BASE = '886f90a542a6174a037c79eebbb4f74848fb1f53'
REGISTRY = ROOT / 'scripts/reviewed-source-registry.json'
# These pins follow committed implementation and separately recorded review.
REGISTRY_SHA256 = '3503b15ddf72020d36a0f87a92e2a3fedd46979a70538aaa72214d7d6625268a'
PRODUCT = '3f613818b2ba5ab08b97009777b8bc4164afb330'
APPROVED = {'validation_work': {'source': 'a4090fe0b801a54a85231d5486ce3ad75c9cd410',
                     'review': 'd0d3a3d8c8dbca4ab33b17d651f73c8a46b8a467',
                     'review_path': 'artifacts/checkpoints/validation-work-metadata-review-20261007/review.json',
                     'paths': ['crates/cc-db/src/document_store.rs',
                               'crates/cc-db/src/index_db_retrieval.rs',
                               'crates/cc-db/src/symbol_identity_store.rs',
                               'crates/cc-eval/tests/p7_validation_work.rs',
                               'crates/cc-eval/tests/packing_validation_work.rs',
                               'crates/cc-search/src/evidence_hydrator.rs',
                               'crates/cc-search/src/selection/budget.rs']},
 'python_capture_revalidation': {'source': 'e0c1f3ddb6e666ce01cb5dcefb50cf572e735f8d',
                                 'review': '9e96795711c072c9aa2095a1fc4527bc28ba75b0',
                                 'review_path': 'artifacts/checkpoints/python-inventory-revalidation-20261007/review.json',
                                 'paths': ['crates/cc-index/src/project_model/python_inventory.rs',
                                           'crates/cc-index/src/project_model/python_inventory/revalidation.rs',
                                           'crates/cc-index/tests/python_inventory_revalidation.rs']}}


def require(condition, message):
    if not condition:
        raise AssertionError(message)


HISTORICAL_HELPERS = (
    'scripts/current-source-registry-v1.json',
    'scripts/current-source-registry-v2.json',
    'scripts/current-source-registry-v3.json',
    'scripts/verify_current_source.py',
    'scripts/verify_current_source_v2.py',
    'scripts/verify_current_source_v3.py',
    'scripts/p0_historical_corpus.py',
    'scripts/verify_fixed_e3_integration.py',
    'scripts/verify_packing_integration.py',
)


def verify_pins():
    refs = [BASE, PRODUCT]
    for pin in APPROVED.values():
        refs.extend([pin['source'], pin['review']])
    require(all(re.fullmatch(r'[0-9a-f]{40}', ref) for ref in refs),
            'source pins must be immutable full commit SHAs')
    require(bool(APPROVED), 'at least one reviewed delta is required')


def historical_blob(path):
    # Do not rely on the helper being checked to return its own expected bytes.
    return subprocess.check_output(['git', 'show', BASE + ':' + path], cwd=ROOT)


def ensure_base():
    verify_pins()
    if subprocess.run(['git', 'cat-file', '-e', BASE + '^{commit}'], cwd=ROOT,
                      stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL).returncode:
        subprocess.run(['git', 'fetch', '--no-tags', 'origin', BASE], cwd=ROOT, check=True)


def verify_historical_helpers(root=ROOT):
    ensure_base()
    for path in HISTORICAL_HELPERS:
        target = root / path
        require(not target.is_symlink() and target.read_bytes() == historical_blob(path),
                'historical verifier or registry changed: ' + path)


def identities():
    return {
        'schema_version': 4,
        'source_version': VERSION,
        'base_source': BASE,
        'previous_registry_sha256': previous.REGISTRY_SHA256,
        'product_source': PRODUCT,
        'scope': 'source_integrity_only',
        'quality_and_100k': 'not_inherited',
    }


def check_identity(registry):
    verify_pins()
    for key, value in identities().items():
        require(registry.get(key) == value, 'wrong source identity: ' + key)
    require(set(registry.get('deltas', {})) == set(APPROVED),
            'reviewed delta inventory differs')


def load_registry(path=REGISTRY):
    raw = path.read_bytes()
    require(hashlib.sha256(raw).hexdigest() == REGISTRY_SHA256,
            'stale or altered reviewed registry')
    registry = json.loads(raw)
    check_identity(registry)
    return registry


def approved_union(registry, root=ROOT):
    check_identity(registry)
    verify_historical_helpers(root)
    git = previous.v2.v1
    git.ensure_refs([BASE, PRODUCT] + [p['source'] for p in APPROVED.values()]
                    + [p['review'] for p in APPROVED.values()])
    expected = previous.approved_union(previous.load_registry(), root=root)
    require(set(expected) == git.inputs(BASE), 'v3 base inventory differs')
    require(all(raw == git.blob(BASE, path) for path, raw in expected.items()),
            'v3 base bytes differ')
    changed = set()
    for name, pin in APPROVED.items():
        delta = registry['deltas'][name]
        for key in ['source', 'review', 'review_path']:
            require(delta.get(key) == pin[key], 'wrong delta pin: ' + name)
        paths = set(pin['paths'])
        require(not changed.intersection(paths), 'overlapping reviewed deltas')
        changed.update(paths)
        require(previous.v2.changed_paths(BASE, pin['source']) == paths,
                'source delta inventory differs: ' + name)
        require(set(delta.get('paths', {})) == paths,
                'registry delta inventory differs: ' + name)
        review_raw = git.blob(pin['review'], pin['review_path'])
        require(git.sha(review_raw) == delta['review_sha256'],
                'review digest differs: ' + name)
        review_path = root / pin['review_path']
        require(not review_path.is_symlink() and review_path.read_bytes() == review_raw,
                'review record changed: ' + name)
        review = json.loads(review_raw)
        require(review['source'] == pin['source'] and review['base'] == BASE
                and review['verdict'] == 'accepted_scoped',
                'review does not accept fixed delta: ' + name)
        for path in sorted(paths):
            row = delta['paths'][path]
            before = expected.get(path)
            require((git.sha(before) if before is not None else None)
                    == row['before_sha256'], 'delta base SHA differs: ' + path)
            raw = git.blob(pin['source'], path)
            require(git.sha(raw) == row['sha256'], 'delta source SHA differs: ' + path)
            expected[path] = raw
    require({p: git.sha(raw) for p, raw in expected.items()} == registry['complete_inputs'],
            'complete source manifest differs')
    require(set(expected) == git.inputs(PRODUCT), 'product source inventory differs')
    require(all(raw == git.blob(PRODUCT, path) for path, raw in expected.items()),
            'product source bytes differ')
    return expected


def expected_ci():
    ensure_base()
    original = historical_blob('.github/workflows/ci.yml').decode()
    old = 'verify_current_source_v3.py --source-version ' + previous.VERSION
    new = 'verify_reviewed_source.py --source-version ' + VERSION
    require(original.count(old) == 1, 'historical CI selector differs')
    return original.replace(old, new).replace(
        'explicitly selected v3 accepted owner-context union',
        'explicitly selected reviewed source union')


def verify_ci(root=ROOT):
    require((root / '.github/workflows/ci.yml').read_text() == expected_ci(),
            'CI differs beyond the explicit source selector')


def main():
    # The inherited historical verifiers use assertions; never run them with
    # optimization, which would silently disable their integrity checks.
    if sys.flags.optimize:
        raise SystemExit('source verification requires Python without -O')
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-version', required=True, choices=[VERSION])
    parser.parse_args()
    expected = approved_union(load_registry())
    tracked = previous.v2.v1.git('ls-files', '--', 'crates', 'Cargo.toml', 'Cargo.lock')
    previous.v2.v1.verify_tree(ROOT, expected, tracked.decode().splitlines())
    verify_ci()
    print(json.dumps(dict(identities(), status='passed', complete_inputs=len(expected),
                          reviewed_deltas=list(APPROVED))))


if __name__ == '__main__':
    main()
