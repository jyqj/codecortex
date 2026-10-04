#!/usr/bin/env python3
"""Reconstruct approved v1 + exact reviewed capture delta + fixed fresh fixtures.

This explicitly selected registry checks source integrity only. It does not
inherit historical packing/e3/P0 quality, runtime publication or scale claims.
The v1 registry and its independent approved50a493 reconstruction stay intact.
No HEAD-derived manifest or implicit latest-version admission is permitted.
"""
import argparse
import json
from pathlib import Path

import verify_current_source as v1

ROOT = Path(__file__).resolve().parents[1]
VERSION = 'python-inventory-20261004-v2'
REGISTRY = ROOT / 'scripts/current-source-registry-v2.json'
REGISTRY_SHA256 = '6bce6b8c8b3a7925bf03dd1559113012d4b9fc650d4d667106ae8c4aab731172'
BASE = '50a4933e48ef20b16401ac8f75c386a660aa8e5c'
ORIGINAL = '8d2b312c066f0514fb5cee555767955bf34708d0'
CAPTURE = 'e3b932ed4b1e197022c0902fd4c11af3e87ae87e'
REVIEW = 'a17f05d674e82bc54aac6a664b2b69a87b276b36'
DELTA_REVIEW = '1fe4dc5f44f53e215e3b6cc99c756bd9b62b0a18'
PRODUCT = '780502322816e4fd1d61d8f77a98d7d6b635d9fb'
INTEGRATION_BASE = '2d297de0e5c7776c358ab49ab2072659ac03c8fc'
MODULE = 'crates/cc-index/src/project_model/mod.rs'
CAPTURE_PATHS = {
    MODULE,
    'crates/cc-index/src/project_model/python_inventory.rs',
    'crates/cc-index/src/project_model/python_inventory/native.rs',
    'crates/cc-index/src/project_model/python_inventory/tests.rs',
    'crates/cc-index/tests/python_inventory_capture.rs',
}
FIXTURE = 'crates/cc-index/tests/python_inventory_resource_integration.rs'


def load_registry(path=REGISTRY):
    raw = path.read_bytes()
    assert v1.sha(raw) == REGISTRY_SHA256, 'stale or altered v2 registry'
    registry = json.loads(raw)
    assert registry['schema_version'] == 2 and registry['source_version'] == VERSION
    for key, value in {
        'approved_base': BASE, 'original_capture_source': ORIGINAL,
        'approved_capture_source': CAPTURE, 'original_review': REVIEW,
        'delta_review': DELTA_REVIEW, 'fixed_product': PRODUCT,
        'integration_base': INTEGRATION_BASE,
        'scope': 'source_integrity_only', 'quality_and_100k': 'not_inherited',
    }.items():
        assert registry[key] == value, 'wrong v2 identity: ' + key
    v1.ensure_refs([BASE, ORIGINAL, CAPTURE, REVIEW, DELTA_REVIEW, PRODUCT,
                    INTEGRATION_BASE])
    return registry


def changed_paths(before, after):
    return set(v1.git('diff', '--name-only', before, after, '--',
                      'crates', 'Cargo.toml', 'Cargo.lock').decode().splitlines())


def approved_union(registry, root=ROOT):
    # Actual reconstruction from the original approved sources, transformations,
    # review pins and manifests. BASE is independently compared by the v1 guard.
    expected = v1.approved_union(v1.load_records(root=root), root=root)
    assert changed_paths(BASE, ORIGINAL) == CAPTURE_PATHS, 'original capture delta inventory'
    assert changed_paths(BASE, CAPTURE) == CAPTURE_PATHS, 'final capture delta inventory'
    assert changed_paths(ORIGINAL, CAPTURE) == {
        'crates/cc-index/src/project_model/python_inventory.rs',
        'crates/cc-index/tests/python_inventory_capture.rs',
    }, 'P2 delta inventory differs'
    assert set(registry['capture_delta']) == CAPTURE_PATHS
    for path, row in registry['capture_delta'].items():
        before = expected.get(path)
        assert (v1.sha(before) if before is not None else None) == row['before_sha256']
        original = v1.blob(ORIGINAL, path)
        assert v1.sha(original) == row['original_sha256'], 'original capture SHA: ' + path
        raw = v1.blob(CAPTURE, path)
        assert v1.sha(raw) == row['sha256'], 'capture SHA: ' + path
        if path == MODULE:
            # The sole modification to an existing production file is this export.
            assert raw == before.replace(b'mod python;\n', b'mod python;\npub mod python_inventory;\n')
        expected[path] = raw
    assert set(expected) == v1.inputs(CAPTURE), 'capture union inventory differs'
    assert all(raw == v1.blob(CAPTURE, path) for path, raw in expected.items()), 'capture union bytes differ'
    assert set(registry['integration_fixtures']) == {FIXTURE}
    assert changed_paths(CAPTURE, PRODUCT) == {FIXTURE}, 'fixed integration fixture delta'
    for path, row in registry['integration_fixtures'].items():
        assert row['commit'] == PRODUCT
        raw = v1.blob(row['commit'], path)
        assert v1.sha(raw) == row['sha256'], 'fixture SHA: ' + path
        expected[path] = raw
    assert {p: v1.sha(b) for p, b in expected.items()} == registry['complete_inputs'], 'complete manifest differs'
    assert set(expected) == v1.inputs(PRODUCT), 'fixed product inventory differs'
    assert all(raw == v1.blob(PRODUCT, path) for path, raw in expected.items()), 'fixed product bytes differ'
    # Preserve full finding/red history and accepted delta evidence on disk.
    for path, row in registry['records'].items():
        raw = v1.blob(row['commit'], path)
        assert v1.sha(raw) == row['sha256'], 'review/contract SHA: ' + path
        assert not (root / path).is_symlink()
        assert (root / path).read_bytes() == raw, 'review/contract changed: ' + path
    old = 'artifacts/checkpoints/python-inventory-independent-20261004/'
    new = 'artifacts/checkpoints/python-inventory-p2-delta-independent-20261004/'
    for ref, prefix in [(REVIEW, old), (DELTA_REVIEW, new)]:
        actual = set(v1.git('ls-tree', '-r', '--name-only', ref, '--', prefix).decode().splitlines())
        assert actual == {p for p in registry['records'] if p.startswith(prefix)}, 'review history inventory differs'
    assert (root / (old + 'review_tests.rs')).read_bytes() == (root / (new + 'review_tests.rs')).read_bytes()
    return expected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-version', required=True, choices=[VERSION])
    parser.parse_args()
    expected = approved_union(load_registry())
    tracked = v1.git('ls-files', '--', 'crates', 'Cargo.toml', 'Cargo.lock').decode().splitlines()
    v1.verify_tree(ROOT, expected, tracked)
    print(json.dumps({'status': 'passed', 'source_version': VERSION,
                      'fixed_product': PRODUCT, 'approved_base': BASE,
                      'approved_capture_source': CAPTURE, 'complete_inputs': len(expected),
                      'scope': 'source_integrity_only', 'quality_and_100k': 'not_inherited'}))


if __name__ == '__main__':
    main()
