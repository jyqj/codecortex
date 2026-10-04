#!/usr/bin/env python3
"""Reconstruct v2 + the exact independently accepted bounded owner-context delta.

Source integrity only: historical quality/materialization anchors stay frozen.
No HEAD/latest authorization, manifest refresh, or inherited quality/scale claim.
"""
import argparse
import json
from pathlib import Path

import verify_current_source_v2 as v2

ROOT = Path(__file__).resolve().parents[1]
VERSION = 'query-owner-context-20261004-v3'
REGISTRY = ROOT / 'scripts/current-source-registry-v3.json'
REGISTRY_SHA256 = '8525ded2edbcbf59f8f7965ffa26c684ef6cf838576d61d634efa8408299acec'
BASE = '3c8c204216cb54c41850c6da826a68fec3586430'
SOURCE = 'd32924f0b1b08f55956659df2cb253d0aba8146f'
DELIVERY = '026b566f18525568ae16e505eca13ea5e258dff5'
REVIEW = '9c28792137b89a69d6c0696fa112b7fe54f4c053'
DELTA_PATHS = {
    'crates/cc-search/src/engine_lane_tests.rs',
    'crates/cc-search/src/plan.rs',
    'crates/cc-search/src/query_target.rs',
}


def identities():
    return {
        'schema_version': 3, 'source_version': VERSION,
        'approved_base': BASE, 'previous_source_version': v2.VERSION,
        'previous_registry_sha256': v2.REGISTRY_SHA256,
        'accepted_source': SOURCE, 'accepted_delivery': DELIVERY,
        'independent_acceptance': REVIEW, 'scope': 'source_integrity_only',
        'quality_and_100k': 'not_inherited',
    }


def load_registry(path=REGISTRY):
    raw = path.read_bytes()
    assert v2.v1.sha(raw) == REGISTRY_SHA256, 'stale or altered v3 registry'
    registry = json.loads(raw)
    for key, value in identities().items():
        assert registry[key] == value, 'wrong v3 identity: ' + key
    return registry


def approved_union(registry, root=ROOT):
    for key, value in identities().items():
        assert registry[key] == value, 'wrong v3 identity: ' + key
    v2.v1.ensure_refs([BASE, SOURCE, DELIVERY, REVIEW])
    expected = v2.approved_union(v2.load_registry(), root=root)
    assert set(expected) == v2.v1.inputs(BASE), 'fixed base inventory differs'
    assert all(raw == v2.v1.blob(BASE, p) for p, raw in expected.items()), 'fixed base bytes differ'
    assert set(registry['owner_context_delta']) == DELTA_PATHS, 'owner delta inventory differs'
    assert v2.changed_paths(BASE, SOURCE) == DELTA_PATHS, 'accepted delta inventory differs'
    for path, row in registry['owner_context_delta'].items():
        assert v2.v1.sha(expected[path]) == row['before_sha256'], 'owner base SHA: ' + path
        raw = v2.v1.blob(SOURCE, path)
        assert v2.v1.sha(raw) == row['sha256'], 'owner source SHA: ' + path
        expected[path] = raw
    # Compare the complete reconstructed union to all three fixed identities;
    # these objects are evidence inputs, never the current checkout's authority.
    for ref in [SOURCE, DELIVERY, REVIEW]:
        assert set(expected) == v2.v1.inputs(ref), 'accepted union inventory differs'
        assert all(raw == v2.v1.blob(ref, p) for p, raw in expected.items()), 'accepted union bytes differ'
    assert {p: v2.v1.sha(b) for p, b in expected.items()} == registry['complete_inputs'], 'complete manifest differs'
    preserved = set(v2.v1.git('diff', '--name-only', BASE, REVIEW).decode().splitlines()) - DELTA_PATHS
    assert set(registry['records']) == preserved, 'owner history inventory differs'
    for path, row in registry['records'].items():
        assert row['commit'] == REVIEW, 'owner history pin differs: ' + path
        raw = v2.v1.blob(REVIEW, path)
        assert len(raw) == row['bytes'] and v2.v1.sha(raw) == row['sha256'], 'owner history SHA: ' + path
        assert not (root / path).is_symlink(), 'owner history symlink: ' + path
        assert (root / path).read_bytes() == raw, 'owner history changed: ' + path
    binding_path = 'artifacts/reviews/query-owner-closing-qualifiers-independent-20261004/binding.json'
    binding = json.loads(v2.v1.blob(REVIEW, binding_path))
    assert binding['verdict'] == 'ACCEPT' and binding['tested_sha'] == DELIVERY
    assert binding['product_identical_to_source_sha'] == SOURCE
    return expected


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--source-version', required=True, choices=[VERSION])
    parser.parse_args()
    registry = load_registry()
    expected = approved_union(registry)
    tracked = v2.v1.git('ls-files', '--', 'crates', 'Cargo.toml', 'Cargo.lock').decode().splitlines()
    v2.v1.verify_tree(ROOT, expected, tracked)
    print(json.dumps(dict(identities(), status='passed', complete_inputs=len(expected),
                          preserved_records=len(registry['records']), registry_sha256=REGISTRY_SHA256)))


if __name__ == '__main__':
    main()
